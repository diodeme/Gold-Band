//! Official ACP child-session addressing projected onto existing Agent branches.
//! This is transport binding state, not a second execution lifecycle. It is kept
//! with the attached root runtime and rebuilt only from official replay frames.
use super::*;
use crate::acp::branches::{branch_route_for_event, stable_agent_execution_id};

const MAX_NATIVE_CHILD_SESSIONS: usize = crate::acp::history::MAX_PENDING_AGENT_LAUNCHES;
pub(super) const MAX_PENDING_NATIVE_ANNOUNCEMENTS: usize = 64;
const MAX_NATIVE_ANNOUNCEMENT_BYTES: usize = 16 * 1024;
const OFFICIAL_CLAUDE_ACP_NAME: &str = "@agentclientprotocol/claude-agent-acp";
const MINIMUM_NATIVE_SUBAGENT_VERSION: &str = "0.90.0";
const SUBAGENT_PROTOCOL_UPGRADE_REQUIRED: &str = "acp.subagent-protocol-upgrade-required";

/// Older official adapters advertise the same client capability but speak an
/// incompatible draft that also suppresses parent launch tools. Reject that
/// actual handshake rather than silently losing the canonical transcript.
pub(super) fn validate_initialize_result(result: &Value) -> Result<()> {
    if result.pointer("/agentInfo/name").and_then(Value::as_str) != Some(OFFICIAL_CLAUDE_ACP_NAME) {
        return Ok(());
    }
    let version = result
        .pointer("/agentInfo/version")
        .cloned()
        .unwrap_or(Value::Null);
    let minimum = semver::Version::parse(MINIMUM_NATIVE_SUBAGENT_VERSION)
        .expect("valid minimum native subagent protocol version");
    if version
        .as_str()
        .and_then(|version| semver::Version::parse(version).ok())
        .is_some_and(|version| version >= minimum)
    {
        return Ok(());
    }
    Err(runtime_error(manual_runtime_error_info(
        RuntimeErrorDomain::Config,
        SUBAGENT_PROTOCOL_UPGRADE_REQUIRED,
        "The official Claude ACP adapter does not support the required native subagent protocol",
        json!({"minimumVersion": MINIMUM_NATIVE_SUBAGENT_VERSION, "version": version}),
    )))
}

#[derive(Clone, Default)]
pub(super) struct NativeSubagents {
    bindings: HashMap<String, NativeChildBinding>,
    // Only the first state announcement may precede the binding message. Never
    // buffer arbitrary child content, tool calls, or interactive requests.
    pending: HashMap<String, (String, Value)>,
}

#[derive(Clone)]
struct NativeChildBinding {
    parent_session_id: String,
    launch_tool_call_id: String,
    branch_id: String,
}

impl NativeSubagents {
    pub(super) fn owns_launch(&self, launch: &AcpUiEvent) -> bool {
        launch.tool_call_id.as_deref().is_some_and(|id| {
            self.bindings
                .values()
                .any(|binding| binding.launch_tool_call_id == id)
        })
    }
}

fn native_state_status(update: &Value) -> &'static str {
    match update.pointer("/state/state").and_then(Value::as_str) {
        Some("running") => "running",
        Some("requires_action") => "waiting_permission",
        Some("idle") => match update.pointer("/state/stopReason").and_then(Value::as_str) {
            Some("end_turn") => "completed",
            Some("cancelled") => "interrupted",
            Some("error") => "failed",
            _ => "unknown",
        },
        Some("cancelled") => "interrupted",
        Some("error") => "failed",
        _ => "unknown",
    }
}

fn annotate_native_branch(value: &mut Value, wire_session: &str, branch: &str) {
    if !value.get("_meta").is_some_and(Value::is_object) {
        value["_meta"] = json!({});
    }
    value["_meta"]["goldBandConversation"] = json!({"branchId": branch});
    value["_meta"]["goldBandNativeSessionId"] = json!(wire_session);
}

/// Native tool IDs are scoped to their wire child. Only the canonical item
/// key is scoped here; RPC toolCallId and AgentExecutionId remain unchanged.
pub(super) fn canonical_tool_item_id(tool_id: &str, native_branch: Option<&str>) -> String {
    match native_branch {
        Some(branch) => format!("native-tool-call-{branch}-{tool_id}"),
        None => format!("tool-call-{tool_id}"),
    }
}

pub(super) fn native_tool_branch(raw: Option<&Value>) -> Option<&str> {
    let raw = raw?;
    raw.pointer("/_meta/goldBandNativeSessionId")?.as_str()?;
    raw.pointer("/_meta/goldBandConversation/branchId")?
        .as_str()
        .filter(|branch| *branch != ROOT_BRANCH_ID)
}

impl AcpRuntime<'_> {
    fn native_control_only_restore(&self) -> bool {
        self.session_update_phase == SessionUpdatePhase::RestoringWithoutReplay
            || (self.session_update_phase == SessionUpdatePhase::ReplayingHistory
                && !self.runtime_policy.external_session_sync_enabled)
    }

    fn reject_native_frame(&self, reason: &str, wire_session: Option<&str>, update: &Value) {
        append_structured_diagnostic_best_effort(
            &self.paths.diagnostics,
            "warn",
            "acp.native-subagent-frame-rejected",
            Some(json!({"reason":reason, "sessionId":wire_session, "update":update})),
        );
    }

    fn native_session_branch(&self, wire_session: &str) -> Option<String> {
        if self.session_id.as_deref() == Some(wire_session) {
            Some(ROOT_BRANCH_ID.to_string())
        } else {
            self.native_subagents
                .bindings
                .get(wire_session)
                .map(|binding| binding.branch_id.clone())
        }
    }

    fn native_launch_exists(&mut self, parent: &str, launch_id: &str) -> Result<bool> {
        let Some(branch) = self.native_session_branch(parent) else {
            return Ok(false);
        };
        if self.session_update_phase == SessionUpdatePhase::ReplayingHistory
            && self.runtime_policy.external_session_sync_enabled
            && self
                .provider_history_replay
                .has_pending_agent_launch(&branch, launch_id)
        {
            return Ok(true);
        }
        let item_id = canonical_tool_item_id(
            launch_id,
            (branch != ROOT_BRANCH_ID).then_some(branch.as_str()),
        );
        let launch = if let Some(item) = self.pending_timeline_patches.get(&item_id) {
            Some(item.item.clone())
        } else if let Some(item) = self.timeline_items.get(&item_id) {
            Some(item.clone())
        } else if branch == ROOT_BRANCH_ID {
            self.timeline_store.read_item(&item_id)?
        } else if let Some(store) = self.branch_timeline_stores.get_mut(&branch) {
            store.read_item(&item_id)?
        } else {
            None
        };
        if launch.is_none()
            && self.session_update_phase == SessionUpdatePhase::ReplayingHistory
            && self
                .provider_history_replay
                .pending_agent_launch_capacity_reached()
        {
            self.reject_native_frame(
                "replay-launch-proof-capacity",
                Some(parent),
                &json!({"toolCallId":launch_id}),
            );
        }
        Ok(launch.is_some_and(|event| {
            event.session_id == self.session_id
                && event_branch_id(&event) == branch
                && branch_route_for_event(&event)
                    .launched_agent_execution_id
                    .is_some()
        }))
    }

    fn persist_native_state(&mut self, child: &str, update: &Value) -> Result<()> {
        if self.native_control_only_restore() {
            return Ok(());
        }
        let binding = self
            .native_subagents
            .bindings
            .get(child)
            .expect("validated child binding");
        let branch = binding.branch_id.clone();
        self.seq = self.seq.saturating_add(1);
        let mut event = normalize_session_update(self.seq, self.session_id.clone(), update);
        event.id = format!("native-agent-state-{branch}-{}", self.seq);
        event.kind = "agentState".to_string();
        event.content = None;
        event.status = Some(native_state_status(update).to_string());
        let raw = event.raw.as_mut().expect("normalized event raw");
        raw["source"] = json!("nativeSubagentState");
        annotate_native_branch(raw, child, &branch);
        self.persist_event_inner(
            &event,
            self.session_update_phase != SessionUpdatePhase::ReplayingHistory,
            None,
        )
    }

    /// Returns true for every native/child frame, including rejected ones. A
    /// child that is unknown or conflicting must never fall through to root.
    pub(super) fn handle_native_session_update(
        &mut self,
        wire_session: Option<&str>,
        update: &Value,
    ) -> Result<bool> {
        let kind = update
            .get("sessionUpdate")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let is_child = wire_session
            .is_some_and(|wire| self.session_id.as_deref().is_some_and(|root| root != wire));
        if kind != "subagent_update" && kind != "session_message" && !is_child {
            return Ok(false);
        }
        let Some(wire) = wire_session else {
            self.reject_native_frame("missing-session", wire_session, update);
            return Ok(true);
        };
        if kind == "subagent_update" {
            let Some(child) = update
                .get("sessionId")
                .and_then(Value::as_str)
                .filter(|child| !child.is_empty())
            else {
                self.reject_native_frame("missing-child-session", Some(wire), update);
                return Ok(true);
            };
            if self.native_session_branch(wire).is_none()
                || self.session_id.as_deref() == Some(child)
                || child == wire
            {
                self.reject_native_frame("invalid-parent-session", Some(wire), update);
            } else if let Some(binding) = self.native_subagents.bindings.get(child) {
                if binding.parent_session_id != wire {
                    self.reject_native_frame("conflicting-parent", Some(wire), update);
                } else if update.get("state").is_some() {
                    // Omitted fields are partial metadata updates. Explicit
                    // null/unknown state, unlike omission, clears certainty.
                    self.persist_native_state(child, update)?;
                }
            } else if self.native_subagents.pending.contains_key(child) {
                // The protocol permits one pre-binding announcement, not an
                // unbounded stream of buffered lifecycle transitions.
                self.reject_native_frame("duplicate-unbound-announcement", Some(wire), update);
            } else if self.native_subagents.pending.len() >= MAX_PENDING_NATIVE_ANNOUNCEMENTS
                || update.to_string().len() > MAX_NATIVE_ANNOUNCEMENT_BYTES
            {
                self.reject_native_frame("announcement-capacity", Some(wire), update);
            } else {
                self.native_subagents
                    .pending
                    .insert(child.to_string(), (wire.to_string(), update.clone()));
            }
            return Ok(true);
        }
        if kind == "session_message" {
            let sender = update.get("senderSessionId").and_then(Value::as_str);
            let recipient = update.get("recipientSessionId").and_then(Value::as_str);
            let message_id = update.get("messageId").and_then(Value::as_str);
            let (Some(sender), Some(message_id)) = (sender, message_id) else {
                self.reject_native_frame("missing-message-binding", Some(wire), update);
                return Ok(true);
            };
            if recipient != Some(wire) || self.native_session_branch(sender).is_none() || !is_child
            {
                self.reject_native_frame("invalid-message-route", Some(wire), update);
                return Ok(true);
            }
            if let Some(binding) = self.native_subagents.bindings.get(wire) {
                if message_id == binding.launch_tool_call_id {
                    if sender != binding.parent_session_id {
                        self.reject_native_frame("conflicting-launch-binding", Some(wire), update);
                    } else if self.session_update_phase == SessionUpdatePhase::ReplayingHistory
                        && self.runtime_policy.external_session_sync_enabled
                    {
                        // Official replay starts the child transcript at its
                        // original launch message. Replace identified message
                        // snapshots; do not append them to a restored live stream.
                        self.active_timeline_streams.remove(&binding.branch_id);
                    }
                    return Ok(true); // launch prompt already exists in canonical branch
                }
                if self.native_launch_exists(sender, message_id)? {
                    self.reject_native_frame("conflicting-launch-binding", Some(wire), update);
                    return Ok(true);
                }
                self.persist_native_child_update(wire, update, true)?;
                return Ok(true);
            }
            let pending_parent = self
                .native_subagents
                .pending
                .get(wire)
                .map(|(parent, _)| parent.as_str());
            if pending_parent != Some(sender) || !self.native_launch_exists(sender, message_id)? {
                self.reject_native_frame("unconfirmed-launch-binding", Some(wire), update);
                return Ok(true);
            }
            if self.native_subagents.bindings.len() >= MAX_NATIVE_CHILD_SESSIONS
                || self
                    .native_subagents
                    .bindings
                    .values()
                    .any(|binding| binding.launch_tool_call_id == message_id)
            {
                self.reject_native_frame("binding-capacity-or-conflict", Some(wire), update);
                return Ok(true);
            }
            let branch_id = stable_agent_execution_id(
                self.session_id.as_deref().expect("known root"),
                message_id,
            );
            if self.session_update_phase == SessionUpdatePhase::ReplayingHistory
                && self.runtime_policy.external_session_sync_enabled
            {
                self.active_timeline_streams.remove(&branch_id);
            }
            self.native_subagents.bindings.insert(
                wire.to_string(),
                NativeChildBinding {
                    parent_session_id: sender.to_string(),
                    launch_tool_call_id: message_id.to_string(),
                    branch_id,
                },
            );
            let (_, announcement) = self
                .native_subagents
                .pending
                .remove(wire)
                .expect("validated announcement");
            self.persist_native_state(wire, &announcement)?;
            return Ok(true);
        }
        if !self.native_subagents.bindings.contains_key(wire) {
            self.reject_native_frame("unknown-child-session", Some(wire), update);
            return Ok(true);
        }
        self.persist_native_child_update(wire, update, false)?;
        Ok(true)
    }

    fn persist_native_child_update(
        &mut self,
        wire: &str,
        update: &Value,
        session_message: bool,
    ) -> Result<()> {
        if self.native_control_only_restore() {
            return Ok(());
        }
        let branch = self.native_subagents.bindings[wire].branch_id.clone();
        self.seq = self.seq.saturating_add(1);
        let mut event = normalize_session_update(self.seq, self.session_id.clone(), update);
        if session_message {
            event.kind = "userTextDelta".to_string();
            event.content = update
                .get("content")
                .and_then(Value::as_array)
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|block| block.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n")
                });
            event.id = format!(
                "native-session-message-{branch}-{}",
                update["messageId"].as_str().unwrap_or_default()
            );
            event.status = Some("completed".to_string());
        }
        let raw = event.raw.as_mut().expect("normalized raw");
        annotate_native_branch(raw, wire, &branch);
        if self.session_update_phase != SessionUpdatePhase::ReplayingHistory
            && let Some(direct) = &self.direct
        {
            let background = self.active_prompt_turn.is_none();
            raw["_meta"]["goldBandBackground"] = json!(background);
            direct.observe_update(raw, background);
        }
        // Child usage/config/compaction are branch evidence only: never feed the
        // root prompt accumulator, control latch, context gauge, or config.
        self.persist_projected_event(
            &event,
            self.session_update_phase != SessionUpdatePhase::ReplayingHistory,
            None,
        )?;
        if self.session_update_phase != SessionUpdatePhase::ReplayingHistory {
            self.capture_turn_file_event(&event)?;
        }
        Ok(())
    }

    /// Keep RPC identity untouched; this only annotates the display branch.
    pub(super) fn annotate_native_interaction(&self, params: &mut Value) -> bool {
        let Some(wire) = params
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            return true;
        };
        if self.session_id.as_deref().is_none_or(|root| root == wire) {
            return true;
        }
        let Some(binding) = self.native_subagents.bindings.get(&wire) else {
            self.reject_native_frame("unknown-child-interaction", Some(&wire), params);
            return false;
        };
        annotate_native_branch(params, &wire, &binding.branch_id);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_subagent_initialize_version_boundary_uses_actual_provider_handshake() {
        for version in [
            json!("0.87.0"),
            json!("0.90.0-beta.1"),
            json!("invalid"),
            Value::Null,
        ] {
            let result = json!({"agentInfo":{"name":OFFICIAL_CLAUDE_ACP_NAME,"version":version}});
            let error = validate_initialize_result(&result).unwrap_err();
            let error = error.downcast_ref::<RuntimeError>().unwrap();
            assert_eq!(error.info.code_str(), SUBAGENT_PROTOCOL_UPGRADE_REQUIRED);
            assert_eq!(
                error.info.params["minimumVersion"],
                MINIMUM_NATIVE_SUBAGENT_VERSION
            );
            assert_eq!(error.info.params["version"], version);
        }
        assert!(
            validate_initialize_result(&json!({"agentInfo":{"name":OFFICIAL_CLAUDE_ACP_NAME}}))
                .is_err()
        );
        for version in ["0.90.0", "0.91.0", "1.0.0"] {
            assert!(
                validate_initialize_result(
                    &json!({"agentInfo":{"name":OFFICIAL_CLAUDE_ACP_NAME,"version":version}})
                )
                .is_ok()
            );
        }
        assert!(
            validate_initialize_result(
                &json!({"agentInfo":{"name":"other-provider","version":"0.1.0"}})
            )
            .is_ok()
        );
        assert!(validate_initialize_result(&json!({})).is_ok());
    }
}
