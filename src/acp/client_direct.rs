//! Exclusive ownership of a retained Direct session's consumer between prompts.
use super::*;
use crate::acp::retention::{DirectSessionRegistration, RetentionCandidate, eviction_candidates};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

static PENDING: LazyLock<Mutex<HashMap<String, DirectSessionRegistration>>> =
    LazyLock::new(Mutex::default);
static MAINTENANCE_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectBackgroundControl {
    pub session_id: String,
    pub connection_generation: u64,
    pub active_tools: usize,
    pub expires_at_ms: u64,
}

pub fn direct_background_control(attempt: &Utf8Path) -> Option<DirectBackgroundControl> {
    let entry = AcpSessionRuntimeRegistry::shared().sessions.lock().ok()?.get(attempt.as_str())?.clone();
    let direct = entry.direct.as_ref()?;
    if entry.active || direct.abort_requested() || entry.connection.is_transport_closed() { return None; }
    let last = direct.background_activity_window()?;
    let active_tools = direct.active_tools.lock().ok()?.len();
    let remaining = Duration::from_secs(direct.registration.policy.background_stop_grace_secs).saturating_sub(last.elapsed());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;
    Some(DirectBackgroundControl { session_id: entry.session_id.clone(), connection_generation: entry.connection_generation, active_tools, expires_at_ms: now + remaining.as_millis() as u64 })
}

/// Background facts for any retained Direct session under `root` (a task dir).
/// Prefers running tools, then the latest grace window; judging "busy" is the
/// consumer's single `isDirectBackgroundActive`.
pub fn direct_background_control_under(root: &Utf8Path) -> Option<DirectBackgroundControl> {
    let attempts = AcpSessionRuntimeRegistry::shared()
        .sessions
        .lock()
        .ok()?
        .iter()
        .filter(|(key, entry)| entry.direct.is_some() && Utf8Path::new(key).starts_with(root))
        .map(|(key, _)| Utf8PathBuf::from(key.as_str()))
        .collect::<Vec<_>>();
    attempts
        .iter()
        .filter_map(|attempt| direct_background_control(attempt))
        .max_by_key(|control| (control.active_tools > 0, control.expires_at_ms))
}

/// Serialize against prompt admission; never cancel a newer foreground prompt
/// through the background control path or fabricate a prompt terminal result.
pub fn cancel_retained_direct_session(attempt: &Utf8Path, expected: &DirectBackgroundControl, expected_turn_id: Option<&str>) -> Result<bool> {
    let registry = AcpSessionRuntimeRegistry::shared();
    let lock = registry.prompt_lock(attempt);
    let Ok(_guard) = lock.try_lock() else { return Ok(false); };
    let entry = registry.sessions.lock().map_err(|_| anyhow!("ACP registry lock poisoned"))?.get(attempt.as_str()).cloned();
    let Some(entry) = entry else { return Ok(false); };
    if entry.active || entry.direct.is_none() || entry.connection.is_transport_closed()
        || entry.session_id != expected.session_id || entry.connection_generation != expected.connection_generation
        || prompt_activity(attempt).is_some() { return Ok(false); }
    let path = AcpAttemptPaths::from_attempt_dir(attempt.to_owned()).snapshot;
    let header = crate::acp::events::read_lifecycle_header_snapshot(&path)?;
    if header.as_ref().and_then(|h| h.turn_id.as_deref()) != expected_turn_id { return Ok(false); }
    if header.as_ref().is_some_and(|h| h.live_turn_activity != crate::acp::events::AcpLiveTurnActivity::Idle) { return Ok(false); }
    let accepted = AdapterConnectionManager::shared().cancel_attempt_prompt(attempt)?;
    if accepted && let Some(direct) = entry.direct.as_ref() {
        direct.background_cancel_requested.store(true, Ordering::Release);
        // The consumer records the stop after every frame received before it.
        *direct.pending_cancel_marker.lock().expect("Direct cancel marker lock") =
            Some(entry.connection.session_route_watermark(&entry.session_id));
    }
    Ok(accepted)
}

pub(super) struct DirectRuntime {
    pub registration: DirectSessionRegistration,
    resident: AtomicBool,
    stop: AtomicBool,
    abort: AtomicBool,
    closing: AtomicBool,
    busy: AtomicBool,
    active_tools: Mutex<HashMap<String, String>>,
    last_activity: Mutex<Instant>,
    last_background_activity: Mutex<Option<Instant>>,
    background_cancel_requested: AtomicBool,
    /// Accepted background stop awaiting its canonical timeline record, ordered
    /// after the route frames received before the stop.
    pending_cancel_marker: Mutex<Option<Option<crate::acp::connection::SessionRouteWatermark>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

pub fn register_direct_session(attempt: &Utf8Path, registration: DirectSessionRegistration) {
    PENDING
        .lock()
        .expect("Direct registration lock")
        .insert(attempt.to_string(), registration);
}

pub fn set_direct_session_resident(project_id: &str, task_id: &str, resident: bool) {
    let registry = AcpSessionRuntimeRegistry::shared();
    let sessions = registry.sessions.lock().expect("ACP session registry lock");
    for entry in sessions.values() {
        if let Some(direct) = &entry.direct
            && direct.registration.project_id == project_id
            && direct.registration.task_id == task_id
        {
            direct.resident.store(resident, Ordering::Release);
        }
    }
    for registration in PENDING
        .lock()
        .expect("Direct registration lock")
        .values_mut()
    {
        if registration.project_id == project_id && registration.task_id == task_id {
            registration.resident = resident;
        }
    }
}

/// Explicit conversation deletion releases its readers before removing files.
pub fn close_direct_conversation(project_id: &str, task_id: &str) -> Result<()> {
    let registry = AcpSessionRuntimeRegistry::shared();
    let keys: Vec<_> = registry
        .sessions
        .lock()
        .expect("ACP registry lock")
        .iter()
        .filter(|(_, entry)| {
            entry.direct.as_ref().is_some_and(|d| {
                d.registration.project_id == project_id && d.registration.task_id == task_id
            })
        })
        .map(|(key, _)| key.clone())
        .collect();
    for key in keys {
        let lock = registry.prompt_lock(Utf8Path::new(&key));
        let _guard = lock
            .lock()
            .map_err(|_| anyhow!("ACP prompt lock poisoned"))?;
        let entry = registry
            .sessions
            .lock()
            .expect("ACP registry lock")
            .get(&key)
            .cloned();
        let Some(entry) = entry else {
            continue;
        };
        let direct = entry.direct.as_ref().expect("Direct entry");
        direct.closing.store(true, Ordering::Release);
        match close_retained(&entry) {
            Ok(true) => {}
            result => {
                direct.closing.store(false, Ordering::Release);
                result?;
                bail!("ACP shared connection cannot release this session");
            }
        }
        stop_consumer(direct);
        registry
            .sessions
            .lock()
            .expect("ACP registry lock")
            .remove(&key);
        AdapterConnectionManager::shared().unregister_attempt_session_if_matches(
            &entry.attempt_dir,
            &live_session_binding(&entry),
        );
        entry.event_pump.close();
        entry.connection.unregister_session_route_if_generation(
            &entry.session_id,
            entry.event_pump.route_generation(),
        );
    }
    PENDING
        .lock()
        .expect("Direct registration lock")
        .retain(|_, d| d.project_id != project_id || d.task_id != task_id);
    Ok(())
}

pub(super) fn prepare_direct_session(attempt: &Utf8Path) -> Option<Arc<DirectRuntime>> {
    let existing = {
        let mut sessions = AcpSessionRuntimeRegistry::shared().sessions.lock().ok()?;
        sessions.get_mut(attempt.as_str()).and_then(|entry| {
            let direct = entry.direct.clone()?;
            if entry.connection.is_transport_closed() {
                direct.abort.store(true, Ordering::Release);
            }
            entry.active = true; // Admission protects the entire hand-off.
            Some(direct)
        })
    };
    if let Some(direct) = &existing {
        stop_consumer(direct);
    }
    let mut registration = PENDING.lock().ok()?.get(attempt.as_str()).cloned();
    if let Some(existing) = existing {
        if !existing.abort_requested() {
            existing.touch();
            return Some(existing);
        }
        registration = registration.or_else(|| Some(existing.registration.clone()));
        AcpSessionRuntimeRegistry::shared().invalidate(attempt);
    }
    registration.map(|registration| {
        Arc::new(DirectRuntime {
            resident: AtomicBool::new(registration.resident),
            registration,
            stop: AtomicBool::new(false),
            busy: AtomicBool::new(false),
            abort: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            active_tools: Mutex::new(HashMap::new()),
            last_activity: Mutex::new(Instant::now()),
            last_background_activity: Mutex::new(None),
            background_cancel_requested: AtomicBool::new(false),
            pending_cancel_marker: Mutex::new(None),
            worker: Mutex::new(None),
        })
    })
}

impl DirectRuntime {
    pub(super) fn touch(&self) {
        *self.last_activity.lock().expect("Direct activity lock") = Instant::now();
    }

    /// Background Stop window: open from the latest background activity unless an
    /// accepted stop has ended that episode.
    fn background_activity_window(&self) -> Option<Instant> {
        if self.background_cancel_requested.load(Ordering::Acquire) {
            return None;
        }
        *self.last_background_activity.lock().ok()?
    }

    pub(super) fn abort_requested(&self) -> bool {
        self.abort.load(Ordering::Acquire) || self.closing.load(Ordering::Acquire)
    }

    pub(super) fn protected(&self) -> bool {
        self.resident.load(Ordering::Acquire)
            || self.busy.load(Ordering::Acquire)
            || self.closing.load(Ordering::Acquire)
            || !self
                .active_tools
                .lock()
                .expect("Direct tools lock")
                .is_empty()
    }

    // Only accepted Live updates reach here; replay and foreground leases never renew TTL.
    pub(super) fn observe_update(&self, update: &Value) {
        let kind = update.get("sessionUpdate").and_then(Value::as_str);
        let mut settles = false;
        let changed = match kind {
            Some("agent_message_chunk" | "agent_thought_chunk" | "user_message_chunk") => update
                .get("content")
                .is_some_and(|content| content.get("text").and_then(Value::as_str) != Some("")),
            Some("tool_call" | "tool_call_update") => {
                let mut tools = self.active_tools.lock().expect("Direct tools lock");
                let id = update
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match update.get("status").and_then(Value::as_str) {
                    Some(status @ ("pending" | "in_progress")) => {
                        tools.insert(id.into(), status.into()).as_deref() != Some(status)
                            || update.get("content").is_some()
                            || update.get("rawOutput").is_some()
                    }
                    Some("completed" | "failed" | "cancelled" | "canceled") => {
                        settles = true;
                        tools.remove(id).is_some()
                    }
                    _ => {
                        update.get("content").is_some()
                            || update.get("rawOutput").is_some()
                            || update.pointer("/_meta/terminal_output_delta").is_some()
                    }
                }
            }
            _ => false,
        };
        if changed {
            if update.pointer("/_meta/goldBandBackground").and_then(Value::as_bool) == Some(true) {
                *self.last_background_activity.lock().expect("Direct activity lock") = Some(Instant::now());
            }
            // Settling tools after a stop belongs to the stopped episode; only new work reopens it.
            if !settles {
                self.background_cancel_requested.store(false, Ordering::Release);
            }
            self.touch();
        }
    }
}

/// Failed setup must return the retained consumer to service as well.
pub(super) struct PromptHandoff {
    pub attempt: Utf8PathBuf,
    pub policy: AcpRuntimePolicy,
    pub raw_max: u64,
    pub raw_target: u64,
}

impl Drop for PromptHandoff {
    fn drop(&mut self) {
        let resume = {
            let mut sessions = AcpSessionRuntimeRegistry::shared()
                .sessions
                .lock()
                .expect("ACP registry lock");
            if let Some(entry) = sessions.get_mut(self.attempt.as_str())
                && entry.direct.is_some()
                && entry.active
                && !entry.connection.is_transport_closed()
            {
                entry.active = false;
                true
            } else {
                false
            }
        };
        if resume {
            start_consumer(&self.attempt, self.policy, self.raw_max, self.raw_target);
        }
        PENDING
            .lock()
            .expect("Direct registration lock")
            .remove(self.attempt.as_str());
    }
}

pub(super) fn abort_consumer(direct: &DirectRuntime) {
    direct.abort.store(true, Ordering::Release);
    stop_consumer(direct);
}

pub(super) fn stop_consumer(direct: &DirectRuntime) {
    direct.stop.store(true, Ordering::Release);
    let worker = direct.worker.lock().expect("Direct worker lock").take();
    if let Some(worker) = worker {
        let _ = worker.join();
    }
}

pub(super) fn start_consumer(
    attempt: &Utf8Path,
    policy: AcpRuntimePolicy,
    raw_max: u64,
    raw_target: u64,
) {
    let entry = AcpSessionRuntimeRegistry::shared()
        .sessions
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(attempt.as_str()).cloned());
    let Some(entry) = entry else {
        return;
    };
    let Some(direct) = entry.direct.clone() else {
        return;
    };
    if let Some(registration) = PENDING
        .lock()
        .expect("Direct registration lock")
        .remove(attempt.as_str())
    {
        direct
            .resident
            .store(registration.resident, Ordering::Release);
    }
    direct.stop.store(false, Ordering::Release);
    let mut handle = direct.worker.lock().expect("Direct worker lock");
    if handle.is_some() {
        return;
    }
    let state = direct.clone();
    let connection_use = AdapterConnectionUse::new(entry.connection.clone());
    *handle = Some(std::thread::spawn(move || {
        let result = consume(
            entry.clone(),
            state.clone(),
            connection_use,
            policy,
            raw_max,
            raw_target,
        );
        if let Err(error) = result {
            state.abort.store(true, Ordering::Release);
            state
                .active_tools
                .lock()
                .expect("Direct tools lock")
                .clear();
            entry.event_pump.close();
            entry.connection.unregister_session_route_if_generation(
                &entry.session_id,
                entry.event_pump.route_generation(),
            );
            append_diagnostic_best_effort(
                &AcpAttemptPaths::from_attempt_dir(entry.attempt_dir.clone()).diagnostics,
                "error",
                error.to_string(),
                Some(json!({"code":"acp.direct-consumer.failed"})),
            );
            tracing::warn!(code = "acp.direct-consumer.failed", %error);
        }
    }));
    drop(handle);
    start_maintenance(policy, direct.registration.policy.sweep_interval_secs);
}

fn consume(
    entry: AttachedSessionRuntime,
    direct: Arc<DirectRuntime>,
    connection_use: AdapterConnectionUse,
    policy: AcpRuntimePolicy,
    raw_max: u64,
    raw_target: u64,
) -> Result<()> {
    let control_registration = ProviderControlRegistration::new(entry.attempt_dir.clone());
    let mut runtime = AcpRuntime::from_connection(
        &entry.connection_key.provider_id,
        entry.connection_key.workspace_root.clone(),
        Some(entry.connection_key.clone()),
        connection_use,
        AcpAttemptPaths::from_attempt_dir(entry.attempt_dir.clone()),
        control_registration.control(),
        raw_max,
        raw_target,
        policy,
        None,
        direct.registration.live_update.as_ref().map(|callback| {
            &**callback as &dyn Fn(&AcpUiEvent, AcpLiveTimelinePosition) -> Result<()>
        }),
        None,
        true,
    )?;
    runtime.direct = Some(direct.clone());
    runtime.session_id = Some(entry.session_id.clone());
    runtime.rx = Some(entry.event_pump.clone());
    runtime.retain_session_route = true;
    runtime.session_update_phase = SessionUpdatePhase::Live;
    runtime.usage = entry.usage.clone();
    runtime.attempt_usage_ready = true;
    runtime.models = entry.models.clone();
    runtime.modes = entry.modes.clone();
    runtime.config_options = entry.config_options.clone();
    runtime.control.mark_stopped();
    control_registration.commit();
    while !direct.stop.load(Ordering::Acquire) && !runtime.connection.is_transport_closed() {
        match entry
            .event_pump
            .recv_timeout_observed(Duration::from_millis(100))
        {
            Ok(frame) => {
                // Serialize admission with eviction's closing decision.
                {
                    let _sessions = AcpSessionRuntimeRegistry::shared()
                        .sessions
                        .lock()
                        .expect("ACP registry lock");
                    direct.busy.store(true, Ordering::Release);
                }
                let sequence = frame.sequence;
                let value = frame.value;
                let interaction = matches!(
                    value.get("method").and_then(Value::as_str),
                    Some("session/request_permission" | "elicitation/create")
                );
                runtime.append_inbound_frame(&value);
                let result = runtime.handle_inbound(value);
                if interaction {
                    direct.touch();
                }
                direct.busy.store(false, Ordering::Release);
                result?;
                runtime.flush_background_file_changes(false)?;
                entry.event_pump.acknowledge_consumed(sequence)?;
                record_due_cancel_marker(&direct, &entry, &mut runtime, false)?;
            }
            Err(RecvTimeoutError::Timeout) => {
                record_due_cancel_marker(&direct, &entry, &mut runtime, false)?;
                runtime.flush_background_file_changes(false)?;
                runtime.flush_pending_timeline_patches(None)?;
                runtime.flush_pending_live_updates()?;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    // A stop accepted just before hand-off is still a user fact; record it before release.
    record_due_cancel_marker(&direct, &entry, &mut runtime, true)?;
    runtime.flush_background_file_changes(true)?;
    runtime.flush_pending_timeline_patches(None)?;
    runtime.flush_pending_live_updates()?;
    if let Ok(mut sessions) = AcpSessionRuntimeRegistry::shared().sessions.lock() {
        if runtime.connection.is_transport_closed() {
            if sessions
                .get(entry.attempt_dir.as_str())
                .is_some_and(|retained| {
                    retained.connection_generation == entry.connection_generation
                })
            {
                sessions.remove(entry.attempt_dir.as_str());
                AdapterConnectionManager::shared().unregister_attempt_session_if_matches(
                    &entry.attempt_dir,
                    &live_session_binding(&entry),
                );
            }
        } else if let Some(retained) = sessions.get_mut(entry.attempt_dir.as_str())
            && retained.connection_generation == entry.connection_generation
        {
            retained.usage = runtime.usage.clone();
            retained.models = runtime.models.clone();
            retained.modes = runtime.modes.clone();
            retained.config_options = runtime.config_options.clone();
        }
    }
    Ok(())
}

fn record_due_cancel_marker(
    direct: &DirectRuntime,
    entry: &AttachedSessionRuntime,
    runtime: &mut AcpRuntime,
    force: bool,
) -> Result<()> {
    let mut pending = direct.pending_cancel_marker.lock().expect("Direct cancel marker lock");
    let due = pending.as_ref().is_some_and(|watermark| {
        force || watermark.is_none_or(|mark| entry.event_pump.has_consumed(mark))
    });
    if !due {
        return Ok(());
    }
    *pending = None;
    drop(pending);
    runtime.persist_background_cancel_marker()
}

fn start_maintenance(policy: AcpRuntimePolicy, interval_secs: u64) {
    if MAINTENANCE_RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(interval_secs));
            prune_direct(policy);
            let sessions = AcpSessionRuntimeRegistry::shared()
                .sessions
                .lock()
                .expect("ACP registry lock");
            if !sessions.values().any(|entry| entry.direct.is_some()) {
                MAINTENANCE_RUNNING.store(false, Ordering::Release);
                break;
            }
        }
    });
}

fn selected_candidates(
    sessions: &HashMap<String, AttachedSessionRuntime>,
    now: Instant,
) -> Vec<String> {
    let Some(config) = sessions
        .values()
        .find_map(|entry| entry.direct.as_ref().map(|d| d.registration.policy))
    else {
        return Vec::new();
    };
    let mut unique = HashMap::<(u64, String), RetentionCandidate<String>>::new();
    let mut references = HashMap::<(u64, String), usize>::new();
    for entry in sessions.values() {
        *references
            .entry((entry.connection_generation, entry.session_id.clone()))
            .or_default() += 1;
    }
    for (key, entry) in sessions {
        let Some(direct) = &entry.direct else {
            continue;
        };
        let candidate = RetentionCandidate {
            key: key.clone(),
            last_activity: *direct.last_activity.lock().expect("Direct activity lock"),
            protected: entry.active
                || now < entry.foreground_lease_until
                || direct.protected()
                || references[&(entry.connection_generation, entry.session_id.clone())] > 1,
        };
        unique
            .entry((entry.connection_generation, entry.session_id.clone()))
            .and_modify(|existing| {
                existing.protected = true;
            }) // Never close a shared alias.
            .or_insert(candidate);
    }
    eviction_candidates(config, now, unique.into_values().collect())
}

fn close_retained(entry: &AttachedSessionRuntime) -> Result<bool> {
    if entry.connection.is_transport_closed() {
        return Ok(true);
    }
    if !entry
        .connection
        .initialized_capabilities()
        .is_some_and(|c| {
            c.pointer("/sessionCapabilities/close")
                .is_some_and(Value::is_object)
        })
    {
        let reader_finished = entry.direct.as_ref().is_some_and(|d| {
            d.worker
                .lock()
                .expect("Direct worker lock")
                .as_ref()
                .is_some_and(JoinHandle::is_finished)
        });
        return AdapterConnectionManager::shared().release_unshared_retained_connection(
            &live_session_binding(entry),
            usize::from(!reader_finished),
        );
    }
    // Keep the route and its consumer until every frame preceding close's
    // response is persisted. Connection close helpers unregister too early.
    let request = entry
        .connection
        .begin_direct_request("session/close", json!({"sessionId":entry.session_id}))?;
    let response =
        match request.recv_timeout_with_session_route_watermark(SESSION_EVICTION_CLOSE_TIMEOUT) {
            Ok(response) => response,
            Err(error) => {
                entry.connection.cancel_pending(request.id);
                return Err(error.into());
            }
        };
    if let Some(error) = response.frame.get("error") {
        bail!("ACP session close failed: {error}");
    }
    let deadline = Instant::now() + SESSION_EVICTION_CLOSE_TIMEOUT;
    while !entry
        .direct
        .as_ref()
        .is_some_and(|d| d.abort.load(Ordering::Acquire))
        && response
            .session_route_watermark
            .is_some_and(|mark| !entry.event_pump.has_consumed(mark))
    {
        if Instant::now() >= deadline {
            bail!("ACP close drain timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(true)
}

pub(super) fn prune_direct(policy: AcpRuntimePolicy) {
    let registry = AcpSessionRuntimeRegistry::shared();
    let candidates = {
        let sessions = registry.sessions.lock().expect("ACP registry lock");
        let mut selected = selected_candidates(&sessions, Instant::now());
        selected.extend(
            sessions
                .iter()
                .filter(|(_, e)| {
                    !e.active
                        && e.direct
                            .as_ref()
                            .is_some_and(|d| d.abort.load(Ordering::Acquire))
                })
                .map(|(k, _)| k.clone()),
        );
        let mut seen = HashSet::new();
        selected.retain(|key| seen.insert(key.clone()));
        selected
    };
    for key in candidates {
        let prompt_lock = registry.prompt_lock(Utf8Path::new(&key));
        let Ok(_guard) = prompt_lock.try_lock() else {
            continue;
        };
        let entry = {
            let sessions = registry.sessions.lock().expect("ACP registry lock");
            // Recompute count, TTL and protections after every close; activity
            // and pin operations serialize with this closing decision.
            let Some(entry) = sessions.get(&key) else {
                continue;
            };
            let Some(direct) = &entry.direct else {
                continue;
            };
            if entry.active
                || (!direct.abort.load(Ordering::Acquire)
                    && !selected_candidates(&sessions, Instant::now()).contains(&key))
            {
                continue;
            }
            direct.closing.store(true, Ordering::Release);
            entry.clone()
        };
        match close_retained(&entry) {
            Ok(true) => {
                if let Some(direct) = &entry.direct {
                    stop_consumer(direct);
                }
                registry
                    .sessions
                    .lock()
                    .expect("ACP registry lock")
                    .remove(&key);
                AdapterConnectionManager::shared().unregister_attempt_session_if_matches(
                    &entry.attempt_dir,
                    &live_session_binding(&entry),
                );
                entry.event_pump.close();
                entry.connection.unregister_session_route_if_generation(
                    &entry.session_id,
                    entry.event_pump.route_generation(),
                );
            }
            result => {
                if let Some(direct) = &entry.direct {
                    direct.closing.store(false, Ordering::Release);
                }
                tracing::debug!(code = "acp.direct-retention.close-deferred", ?result, session_id = %entry.session_id);
            }
        }
    }
    AdapterConnectionManager::shared().prune_idle_connections(
        policy.adapter_connection_idle_ttl,
        policy.max_idle_adapter_connections,
    );
}

#[cfg(test)]
pub(super) fn assert_registry_retention_scope(prototype: &AttachedSessionRuntime) {
    let now = Instant::now();
    let mut entries = HashMap::new();
    let direct = prototype.direct.as_ref().unwrap();
    let before = *direct.last_activity.lock().unwrap();
    *direct.last_activity.lock().unwrap() =
        now - Duration::from_secs(direct.registration.policy.idle_ttl_secs + 1);
    for index in 0..8 {
        let mut entry = prototype.clone();
        entry.active = false;
        entry.foreground_lease_until = now;
        entry.session_id = format!("session-{index}");
        entries.insert(format!("project-{index}/attempt"), entry);
    }
    let mut alias = entries.values().next().unwrap().clone();
    entries.insert("alias".into(), alias.clone());
    assert!(
        selected_candidates(&entries, now).is_empty(),
        "aliases count once across projects"
    );
    alias.direct = None;
    for index in 0..20 {
        entries.insert(format!("workflow-{index}"), alias.clone());
    }
    assert!(
        selected_candidates(&entries, now).is_empty(),
        "Workflow sessions do not raise Direct pressure"
    );
    let mut ninth = prototype.clone();
    ninth.active = false;
    ninth.foreground_lease_until = now;
    ninth.session_id = "ninth".into();
    entries.insert("other-project/attempt".into(), ninth);
    assert_eq!(
        selected_candidates(&entries, now).len(),
        1,
        "the ninth distinct Direct session creates TTL pressure"
    );
    *direct.last_activity.lock().unwrap() = before;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_registration_recreates_direct_continuation_without_a_cached_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = crate::app::App::with_config(root, crate::config::RuntimeConfig::default());
        let context = crate::app::AcpLiveEventContext {
            task_id: "direct-followup".into(),
            task_uuid: None,
            run_id: "run".into(),
            round_id: "round".into(),
            node_id: "agent".into(),
            attempt_id: "attempt".into(),
            outer_node_id: None,
            outer_attempt_id: None,
        };
        let metadata_path = app
            .paths
            .task_dir(&context.task_id)
            .join("authoring/conversation.json");
        let attempt = app.paths.attempt_dir(
            &context.task_id,
            &context.run_id,
            &context.round_id,
            &context.node_id,
            &context.attempt_id,
        );
        for mode in ["workflow", "auto", "direct", "direct"] {
            crate::storage::write_json(
                &metadata_path,
                &json!({
                    "version":"3", "source":"conversation-ui", "runMode":mode,
                    "titleAutoGenerated":false, "createdAt":"2026-10-09T00:00:00Z", "resident":true,
                }),
            )
            .unwrap();
            let callback: crate::acp::retention::SessionLiveUpdate = Arc::new(|_, _| Ok(()));
            app.register_acp_direct_session(&context, Some(callback.clone()));
            let runtime = prepare_direct_session(&attempt);
            assert_eq!(runtime.is_some(), mode == "direct");
            if let Some(runtime) = runtime {
                assert!(runtime.resident.load(Ordering::Acquire));
                assert!(Arc::ptr_eq(
                    runtime.registration.live_update.as_ref().unwrap(),
                    &callback
                ));
                assert_eq!(runtime.registration.project_id, app.paths.project_id);
            }
            PENDING.lock().unwrap().remove(attempt.as_str());
        }
    }

    fn state() -> Arc<DirectRuntime> {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("attempt")).unwrap();
        register_direct_session(
            &path,
            DirectSessionRegistration {
                project_id: "retention-test".into(),
                task_id: "direct".into(),
                resident: false,
                policy: Default::default(),
                live_update: None,
            },
        );
        let direct = prepare_direct_session(&path).unwrap();
        PENDING.lock().unwrap().remove(path.as_str());
        direct
    }

    #[test]
    fn background_stop_activity_tracks_parallel_tools_and_ignores_usage() {
        let direct = state();
        let background = |mut value: Value| {
            value["_meta"] = json!({"goldBandBackground":true});
            direct.observe_update(&value);
        };
        background(json!({"sessionUpdate":"usage_update","used":1}));
        assert!(direct.last_background_activity.lock().unwrap().is_none());
        background(json!({"sessionUpdate":"tool_call","toolCallId":"a","status":"pending"}));
        background(json!({"sessionUpdate":"tool_call","toolCallId":"b","status":"in_progress"}));
        background(json!({"sessionUpdate":"tool_call_update","toolCallId":"a","status":"completed"}));
        assert_eq!(direct.active_tools.lock().unwrap().len(), 1);
        let before = *direct.last_background_activity.lock().unwrap();
        background(json!({"sessionUpdate":"usage_update","used":2}));
        assert_eq!(*direct.last_background_activity.lock().unwrap(), before);
        background(json!({"sessionUpdate":"tool_call_update","toolCallId":"b","status":"cancelled"}));
        assert!(direct.active_tools.lock().unwrap().is_empty());
        assert!(direct.last_background_activity.lock().unwrap().is_some());
    }

    #[test]
    fn accepted_background_stop_closes_the_window_until_new_work_starts() {
        let direct = state();
        let background = |mut value: Value| {
            value["_meta"] = json!({"goldBandBackground":true});
            direct.observe_update(&value);
        };
        background(json!({"sessionUpdate":"tool_call","toolCallId":"a","status":"in_progress"}));
        background(json!({"sessionUpdate":"tool_call","toolCallId":"b","status":"in_progress"}));
        assert!(direct.background_activity_window().is_some());
        direct.background_cancel_requested.store(true, Ordering::Release);
        assert!(direct.background_activity_window().is_none());
        // Cancellation settles the stopped tools; that must not reopen the Stop window.
        background(json!({"sessionUpdate":"tool_call_update","toolCallId":"a","status":"cancelled"}));
        background(json!({"sessionUpdate":"tool_call_update","toolCallId":"b","status":"failed"}));
        background(json!({"sessionUpdate":"usage_update","used":3}));
        assert!(direct.background_activity_window().is_none());
        assert!(direct.active_tools.lock().unwrap().is_empty());
        // The Agent ignoring the stop, or a later timer, starts new work and reopens it.
        background(json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"woke"}}));
        assert!(direct.background_activity_window().is_some());
    }

    #[test]
    fn known_tools_protect_and_completion_starts_a_fresh_window() {
        let direct = state();
        let old = Instant::now() - Duration::from_secs(30000);
        *direct.last_activity.lock().unwrap() = old;
        let running = json!({"sessionUpdate":"tool_call","toolCallId":"bg","status":"in_progress"});
        direct.observe_update(&running);
        assert!(direct.protected());
        let progress_time = *direct.last_activity.lock().unwrap();
        direct.observe_update(&running);
        assert_eq!(
            *direct.last_activity.lock().unwrap(),
            progress_time,
            "duplicate status is not activity"
        );
        direct.observe_update(
            &json!({"sessionUpdate":"tool_call_update","toolCallId":"bg","status":"completed"}),
        );
        assert!(!direct.protected());
        assert!(*direct.last_activity.lock().unwrap() > old);
    }

    #[test]
    fn usage_heartbeats_do_not_extend_retention_and_pin_is_independent_of_work() {
        let direct = state();
        let before = *direct.last_activity.lock().unwrap();
        direct.observe_update(&json!({"sessionUpdate":"usage_update","used":123}));
        direct.observe_update(
            &json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":""}}),
        );
        assert_eq!(*direct.last_activity.lock().unwrap(), before);
        direct.resident.store(true, Ordering::Release);
        assert!(direct.protected());
        direct.busy.store(true, Ordering::Release);
        direct.resident.store(false, Ordering::Release);
        assert!(direct.protected(), "unpin never interrupts an interaction");
        direct.busy.store(false, Ordering::Release);
        assert!(!direct.protected());
    }
}
