use std::collections::BTreeMap;

use serde_json::Value;

pub const ACP_SESSION_CONFIG_ROLLED_BACK_CODE: &str = "acp.session-config-rolled-back";
pub const ACP_THOUGHT_LEVEL_CATEGORY: &str = "thought_level";
pub const ACP_MODEL_CONFIG_CATEGORY: &str = "model_config";
pub const ACP_MODEL_BOUND_CATALOGS_KEY: &str = "modelBoundCatalogs";
pub const ACP_MODEL_BOUND_OVERRIDES_KEY: &str = "modelBoundOverrides";
pub const ACP_CONFIG_OPTION_OVERRIDES_KEY: &str = "configOptionOverrides";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolledBackSessionConfig {
    pub category: String,
    pub config_id: String,
    pub value: String,
    pub name: Option<String>,
}

pub fn is_model_bound_config_category(category: &str) -> bool {
    category == ACP_THOUGHT_LEVEL_CATEGORY || category == ACP_MODEL_CONFIG_CATEGORY
}

/// After `session/set_config_option(model)`, dependents in the same apply
/// belong to the new catalog. `thought_level` identity is the value, not the
/// option id: remap only when the source is `thought_level` (or an unknown
/// leftover thought wire name) and a target `thought_level` still lists that
/// value. Other categories, including `model_config`, stay option-id scoped:
/// same id keeps a listed value; missing id is dropped and never remapped by
/// value onto a different option. Other non-bound categories stay so the
/// existing unavailable error can still block the prompt.
///
/// Authoring overrides may still use Doctor/previous-model option ids after
/// `session/new` already returned the live catalog. Those missing ids are
/// remapped or rolled back even when Gold Band did not change the model.
pub fn strip_unsupported_model_bound_overrides(
    catalog_before: Option<&Value>,
    catalog_after: Option<&Value>,
    overrides: &mut BTreeMap<String, String>,
) -> Vec<RolledBackSessionConfig> {
    strip_model_bound_overrides(catalog_before, catalog_after, overrides, true)
}

pub fn align_overrides_to_live_catalog(
    catalog_before: Option<&Value>,
    catalog_after: Option<&Value>,
    overrides: &mut BTreeMap<String, String>,
) -> Vec<RolledBackSessionConfig> {
    strip_model_bound_overrides(catalog_before, catalog_after, overrides, false)
}

/// Same-session continue must not re-merge frozen authoring leftovers onto a
/// snapshot that already rolled them back. Direct follow-up avoids this by
/// queuing prompts on the live session; AUTO / workflow continue rebuilds the
/// invocation and must pass snapshot `configOptionOverrides` as the complete
/// authority. New sessions still start from authoring, then silently retain
/// against the selected model's last observed bound catalog so a later Gemini
/// node does not re-notice `reasoning` after bootstrap already observed that
/// the live table omits it.
pub fn invocation_config_option_overrides(
    reuse_session: bool,
    authoring: BTreeMap<String, String>,
    snapshot: BTreeMap<String, String>,
    capabilities: Option<&Value>,
    selected_model: Option<&str>,
) -> BTreeMap<String, String> {
    if reuse_session {
        return snapshot;
    }
    let mut next = authoring;
    retain_authoring_model_bound_overrides(&mut next, capabilities, selected_model);
    next
}

fn authoring_config_options_for_model(
    capabilities: Option<&Value>,
    selected_model: Option<&str>,
) -> Option<Value> {
    let capabilities = capabilities?;
    let current = capabilities.get("configOptions")?;
    let current_options = live_config_option_array(current)?;
    let catalogs = model_bound_catalogs_from_capabilities_value(Some(capabilities));
    let selected = trimmed_model_id(selected_model)
        .or_else(|| catalog_model_current_value(current_options))?;
    if !catalogs.contains_key(&selected) {
        return Some(current.clone());
    }
    let mut next: Vec<Value> = current_options
        .iter()
        .filter(|option| !option_category(Some(option)).is_some_and(is_model_bound_config_category))
        .cloned()
        .collect();
    let bound = catalogs
        .get(&selected)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let insert_at = next
        .iter()
        .position(is_model_select_option)
        .map(|index| index + 1)
        .unwrap_or(next.len());
    for (offset, option) in bound.into_iter().enumerate() {
        next.insert(insert_at + offset, option);
    }
    Some(Value::Array(next))
}

fn retain_authoring_model_bound_overrides(
    overrides: &mut BTreeMap<String, String>,
    capabilities: Option<&Value>,
    selected_model: Option<&str>,
) {
    let Some(projected) = authoring_config_options_for_model(capabilities, selected_model) else {
        return;
    };
    let current = capabilities.and_then(|value| value.get("configOptions"));
    let _ = strip_unsupported_model_bound_overrides(current, Some(&projected), overrides);
}

/// `session/new` and in-session model switches both treat the live catalog as
/// the apply fact source: remap thought by value, unspecify unsupported
/// `thought_level` / `model_config`. Attached reuse without a model RPC still
/// leaves listed-but-invalid values for `acp.session-config-value-unavailable`.
pub fn reconcile_session_config_overrides(
    catalog_before: Option<&Value>,
    catalog_after: Option<&Value>,
    overrides: &mut BTreeMap<String, String>,
    model_applied: bool,
    new_session: bool,
) -> Vec<RolledBackSessionConfig> {
    strip_model_bound_overrides(
        catalog_before,
        catalog_after,
        overrides,
        model_applied || new_session,
    )
}

/// Authoring (home / workflow / run-mode) reads last-observed bound catalogs.
/// `thought_level` / `model_config` belong to `(agent, modelId)`, not to the
/// Agent. Cursor adapters also keep a process-global current model, so a later
/// Doctor or live session can observe Luna after an interactive Luna turn.
/// Remember each model's last bound catalog; never replace Grok's Fast with
/// Luna's Context, and never hide Luna Fast just because Doctor current is Grok.
pub fn merge_doctor_authoring_capabilities(previous: Option<&Value>, incoming: Value) -> Value {
    if !incoming.is_object() {
        return incoming;
    }
    let Some(incoming_options) = incoming.get("configOptions").and_then(Value::as_array) else {
        return incoming;
    };
    let catalogs = collect_model_bound_catalogs(previous, incoming_options, true);
    let mut merged = incoming;
    if let Some(object) = merged.as_object_mut() {
        object.insert(
            ACP_MODEL_BOUND_CATALOGS_KEY.into(),
            Value::Object(catalogs.into_iter().collect()),
        );
    }
    merged
}

/// Session live catalogs feed the authoring cache. They upsert
/// `modelBoundCatalogs[modelId]` and retarget authoring `configOptions`
/// currentValue plus bound rows to the live model. Doctor health, model/mode
/// option lists, and other models' last observations stay; `checked_at` is
/// owned by the diagnostic snapshot, not this merge.
pub fn upsert_session_authoring_model_bound_catalog(
    previous: Option<&Value>,
    live_config_options: &Value,
) -> Option<Value> {
    let previous = previous.filter(|value| value.is_object())?;
    let incoming_options = live_config_option_array(live_config_options)?;
    if catalog_model_current_value(incoming_options).is_none() {
        return None;
    }
    let catalogs = collect_model_bound_catalogs(Some(previous), incoming_options, false);
    let catalogs = Value::Object(catalogs.into_iter().collect());
    let next_options =
        project_authoring_current_table(previous.get("configOptions"), incoming_options);
    if previous.get(ACP_MODEL_BOUND_CATALOGS_KEY) == Some(&catalogs)
        && previous.get("configOptions") == Some(&next_options)
    {
        return None;
    }
    let mut merged = previous.clone();
    let object = merged.as_object_mut()?;
    object.insert(ACP_MODEL_BOUND_CATALOGS_KEY.into(), catalogs);
    object.insert("configOptions".into(), next_options);
    Some(merged)
}

fn project_authoring_current_table(
    previous_options: Option<&Value>,
    incoming_options: &[Value],
) -> Value {
    let incoming_model = catalog_model_current_value(incoming_options);
    let incoming_bound = bound_config_options(incoming_options);
    let previous = previous_options
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut next: Vec<Value> = previous
        .iter()
        .filter(|option| !option_category(Some(option)).is_some_and(is_model_bound_config_category))
        .cloned()
        .collect();
    let mut has_model = false;
    for option in &mut next {
        if !is_model_select_option(option) {
            continue;
        }
        has_model = true;
        if let (Some(model), Some(object)) = (incoming_model.as_ref(), option.as_object_mut()) {
            object.insert("currentValue".into(), Value::String(model.clone()));
        }
    }
    if !has_model {
        if let Some(model_option) = incoming_options
            .iter()
            .find(|option| is_model_select_option(option))
        {
            next.insert(0, model_option.clone());
        }
    }
    let insert_at = next
        .iter()
        .position(is_model_select_option)
        .map(|index| index + 1)
        .unwrap_or(next.len());
    for (offset, option) in incoming_bound.into_iter().enumerate() {
        next.insert(insert_at + offset, option);
    }
    Value::Array(next)
}

fn is_model_select_option(option: &Value) -> bool {
    option.get("id").and_then(Value::as_str) == Some("model")
        || option.get("category").and_then(Value::as_str) == Some("model")
}

fn live_config_option_array(live_config_options: &Value) -> Option<&[Value]> {
    match live_config_options {
        Value::Array(options) => Some(options.as_slice()),
        Value::Object(object) => object.get("configOptions")?.as_array().map(Vec::as_slice),
        _ => None,
    }
}

fn collect_model_bound_catalogs(
    previous: Option<&Value>,
    incoming_options: &[Value],
    prune_missing_models: bool,
) -> BTreeMap<String, Value> {
    let incoming_model = catalog_model_current_value(incoming_options);
    let incoming_model_ids = catalog_model_ids(incoming_options);
    let incoming_bound = bound_config_options(incoming_options);
    let mut catalogs = BTreeMap::<String, Value>::new();
    if let Some(previous) = previous.filter(|value| value.is_object()) {
        if let Some(previous_map) = previous
            .get(ACP_MODEL_BOUND_CATALOGS_KEY)
            .and_then(Value::as_object)
        {
            for (model_id, catalog) in previous_map {
                let model_id = model_id.trim();
                if model_id.is_empty() {
                    continue;
                }
                if prune_missing_models
                    && !incoming_model_ids.is_empty()
                    && !incoming_model_ids.iter().any(|id| id == model_id)
                {
                    continue;
                }
                catalogs.insert(model_id.to_string(), catalog.clone());
            }
        }
        if let Some(previous_options) = previous.get("configOptions").and_then(Value::as_array) {
            if let Some(previous_model) = catalog_model_current_value(previous_options) {
                let keep_previous = !prune_missing_models
                    || incoming_model_ids.is_empty()
                    || incoming_model_ids.iter().any(|id| id == &previous_model);
                if keep_previous {
                    catalogs
                        .entry(previous_model)
                        .or_insert_with(|| Value::Array(bound_config_options(previous_options)));
                }
            }
        }
    }
    if let Some(incoming_model) = incoming_model {
        catalogs.insert(incoming_model, Value::Array(incoming_bound));
    }
    catalogs
}

fn catalog_model_current_value(options: &[Value]) -> Option<String> {
    options
        .iter()
        .find(|option| {
            option.get("id").and_then(Value::as_str) == Some("model")
                || option.get("category").and_then(Value::as_str) == Some("model")
        })
        .and_then(|option| option.get("currentValue"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn catalog_model_ids(options: &[Value]) -> Vec<String> {
    options
        .iter()
        .find(|option| {
            option.get("id").and_then(Value::as_str) == Some("model")
                || option.get("category").and_then(Value::as_str) == Some("model")
        })
        .and_then(|option| option.get("options"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| option.get("value").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn bound_config_options(options: &[Value]) -> Vec<Value> {
    options
        .iter()
        .filter(|option| option_category(Some(option)).is_some_and(is_model_bound_config_category))
        .cloned()
        .collect()
}

pub fn live_catalog_model_id(live_config_options: Option<&Value>) -> Option<String> {
    live_config_options
        .and_then(live_config_option_array)
        .and_then(catalog_model_current_value)
}

/// Remember bound options under the catalog's own current model. Fragments
/// without a model currentValue must not replace an earlier observation.
pub fn observe_session_model_bound_catalog(
    catalogs: &mut BTreeMap<String, Value>,
    live_config_options: Option<&Value>,
) -> bool {
    let Some(options) = live_config_options.and_then(live_config_option_array) else {
        return false;
    };
    let Some(owner) = catalog_model_current_value(options) else {
        return false;
    };
    let bound = Value::Array(bound_config_options(options));
    if catalogs.get(&owner) == Some(&bound) {
        return false;
    }
    catalogs.insert(owner, bound);
    true
}

/// Save the leaving model's applied overrides, restore the target model's last
/// applied set, then retain against that model's catalog. Mini rollback must
/// not overwrite Grok's slot.
pub fn switch_model_bound_overrides(
    remembered: &BTreeMap<String, BTreeMap<String, String>>,
    previous_model: Option<&str>,
    next_model: Option<&str>,
    current_overrides: &BTreeMap<String, String>,
    live_config_options: Option<&Value>,
    model_bound_catalogs: &BTreeMap<String, Value>,
) -> (
    BTreeMap<String, String>,
    BTreeMap<String, BTreeMap<String, String>>,
) {
    let mut remembered = remembered.clone();
    if let Some(previous) = trimmed_model_id(previous_model) {
        remembered.insert(previous, current_overrides.clone());
    }
    let mut pending = trimmed_model_id(next_model)
        .and_then(|model_id| remembered.get(&model_id).cloned())
        .unwrap_or_else(|| current_overrides.clone());
    let mut catalog_before = live_config_options.cloned();
    if let Some(previous) = trimmed_model_id(previous_model) {
        restore_session_model_bound_options(model_bound_catalogs, &mut catalog_before, &previous);
    }
    let mut catalog = live_config_options.cloned();
    if let Some(next) = trimmed_model_id(next_model) {
        restore_session_model_bound_options(model_bound_catalogs, &mut catalog, &next);
    }
    let _ = strip_unsupported_model_bound_overrides(
        catalog_before.as_ref(),
        catalog.as_ref(),
        &mut pending,
    );
    if let Some(next) = trimmed_model_id(next_model) {
        remembered.insert(next, pending.clone());
    }
    (pending, remembered)
}

/// Snapshot-side model switch: remember the leaving model's applied map, restore
/// the target model's map, and retain against this session's catalogs. Does not
/// write Direct/home authoring memory.
pub fn apply_session_snapshot_model_switch(session: &mut Value, next_model: Option<&str>) {
    apply_session_snapshot_model_switch_with_authoring(session, next_model, None);
}

/// Same as [`apply_session_snapshot_model_switch`], with the shared Agent
/// capability cache. Authoring catalogs are not written into the session map.
pub fn apply_session_snapshot_model_switch_with_authoring(
    session: &mut Value,
    next_model: Option<&str>,
    authoring_catalogs: Option<&BTreeMap<String, Value>>,
) {
    let previous = session_selected_model_id(session);
    let current_overrides = session_string_map(session, ACP_CONFIG_OPTION_OVERRIDES_KEY);
    let remembered = session_remembered_override_map(session);
    let catalogs = merge_model_bound_catalogs(
        authoring_catalogs,
        &session_model_bound_catalogs(session),
    );
    let live = session.get("configOptions").cloned();
    let (applied, remembered) = switch_model_bound_overrides(
        &remembered,
        previous.as_deref(),
        next_model,
        &current_overrides,
        live.as_ref(),
        &catalogs,
    );
    write_session_string_map(session, ACP_CONFIG_OPTION_OVERRIDES_KEY, &applied);
    write_session_remembered_override_map(session, &remembered);
    write_session_model_override(session, next_model);
}

/// Thought / model_config edits belong to the currently selected model.
pub fn remember_session_snapshot_applied_overrides(session: &mut Value) {
    let Some(model_id) = session_selected_model_id(session) else {
        return;
    };
    let applied = session_string_map(session, ACP_CONFIG_OPTION_OVERRIDES_KEY);
    let mut remembered = session_remembered_override_map(session);
    remembered.insert(model_id, applied);
    write_session_remembered_override_map(session, &remembered);
}

fn trimmed_model_id(model_id: Option<&str>) -> Option<String> {
    model_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn session_selected_model_id(session: &Value) -> Option<String> {
    session
        .get("modelOverride")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            session
                .get("models")
                .and_then(|models| models.get("currentModelId"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| live_catalog_model_id(session.get("configOptions")))
}

fn session_string_map(session: &Value, key: &str) -> BTreeMap<String, String> {
    session
        .get(key)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

fn session_remembered_override_map(session: &Value) -> BTreeMap<String, BTreeMap<String, String>> {
    session
        .get(ACP_MODEL_BOUND_OVERRIDES_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

fn session_model_bound_catalogs(session: &Value) -> BTreeMap<String, Value> {
    session
        .get(ACP_MODEL_BOUND_CATALOGS_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

pub fn merge_model_bound_catalogs(
    authoring: Option<&BTreeMap<String, Value>>,
    session: &BTreeMap<String, Value>,
) -> BTreeMap<String, Value> {
    let mut merged = authoring.cloned().unwrap_or_default();
    merged.extend(session.iter().map(|(key, value)| (key.clone(), value.clone())));
    merged
}

pub fn model_bound_catalogs_from_capabilities_value(
    capabilities: Option<&Value>,
) -> BTreeMap<String, Value> {
    capabilities
        .and_then(|value| value.get(ACP_MODEL_BOUND_CATALOGS_KEY))
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(model_id, catalog)| {
            let model_id = model_id.trim();
            (!model_id.is_empty()).then(|| (model_id.to_string(), catalog.clone()))
        })
        .collect()
}

fn write_session_string_map(session: &mut Value, key: &str, map: &BTreeMap<String, String>) {
    let Some(object) = session.as_object_mut() else {
        return;
    };
    if map.is_empty() {
        object.remove(key);
        return;
    }
    object.insert(
        key.to_string(),
        serde_json::to_value(map).unwrap_or(Value::Object(Default::default())),
    );
}

fn write_session_remembered_override_map(
    session: &mut Value,
    remembered: &BTreeMap<String, BTreeMap<String, String>>,
) {
    let Some(object) = session.as_object_mut() else {
        return;
    };
    if remembered.is_empty() {
        object.remove(ACP_MODEL_BOUND_OVERRIDES_KEY);
        return;
    }
    object.insert(
        ACP_MODEL_BOUND_OVERRIDES_KEY.to_string(),
        serde_json::to_value(remembered).unwrap_or(Value::Object(Default::default())),
    );
}

fn write_session_model_override(session: &mut Value, next_model: Option<&str>) {
    let Some(object) = session.as_object_mut() else {
        return;
    };
    if let Some(model) = trimmed_model_id(next_model) {
        object.insert("modelOverride".into(), Value::String(model));
    } else {
        object.remove("modelOverride");
    }
}

/// After `set_config_option(model)` omits the table, restore this model's last
/// observation. No cache means first contact: keep the previous model's bound
/// rows so apply can remap, but do not record them as the requested model.
/// Authoring catalogs are a read-through for models this session has not
/// observed; they are never stamped into `session_catalogs`.
pub fn retarget_live_model_bound_catalog(
    live: &mut Option<Value>,
    session_catalogs: &mut BTreeMap<String, Value>,
    authoring_catalogs: &BTreeMap<String, Value>,
    requested_model: &str,
    catalog_returned: bool,
) -> bool {
    let requested = requested_model.trim();
    if requested.is_empty() {
        return false;
    }
    if catalog_returned {
        let observed = observe_session_model_bound_catalog(session_catalogs, live.as_ref());
        set_live_catalog_model(live, requested);
        return observed;
    }
    set_live_catalog_model(live, requested);
    if restore_session_model_bound_options(session_catalogs, live, requested) {
        return true;
    }
    restore_session_model_bound_options(authoring_catalogs, live, requested)
}

fn set_live_catalog_model(live: &mut Option<Value>, model: &str) {
    let Some(options) = live.as_mut().and_then(Value::as_array_mut) else {
        return;
    };
    if let Some(option) = options.iter_mut().find(|option| {
        option.get("id").and_then(Value::as_str) == Some("model")
            || option.get("category").and_then(Value::as_str) == Some("model")
    }) {
        if let Some(object) = option.as_object_mut() {
            object.insert("currentValue".into(), Value::String(model.to_string()));
        }
    }
}

/// Bound option the session composer is allowed to edit for the selected model.
/// Live rows are the fact source only when they belong to that model; otherwise
/// this session's cache, then the shared authoring catalog.
pub fn projected_bound_option<'a>(
    session: &'a Value,
    authoring_catalogs: &'a BTreeMap<String, Value>,
    option_id: &str,
) -> Option<&'a Value> {
    let selected = session_selected_model_id(session)?;
    let live = session.get("configOptions");
    let source = if live_catalog_model_id(live).as_deref() == Some(selected.as_str())
        && live_has_model_bound_rows(live)
    {
        live
    } else {
        session
            .get(ACP_MODEL_BOUND_CATALOGS_KEY)
            .and_then(|catalogs| catalogs.get(selected.as_str()))
            .or_else(|| authoring_catalogs.get(&selected))
    };
    config_option_by_id(source, option_id).filter(|option| {
        option_category(Some(option)).is_some_and(is_model_bound_config_category)
    })
}

fn live_has_model_bound_rows(live: Option<&Value>) -> bool {
    live.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|option| option_category(Some(option)).is_some_and(is_model_bound_config_category))
}

pub fn option_lists_value(option: &Value, value: &str) -> bool {
    option_has_value(Some(option), value)
}

pub fn option_listed_values(option: &Value) -> Vec<String> {
    option
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("value").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn option_category_name(option: &Value) -> &str {
    option_category(Some(option)).unwrap_or("config")
}

pub fn restore_session_model_bound_options(
    catalogs: &BTreeMap<String, Value>,
    live_config_options: &mut Option<Value>,
    requested_model: &str,
) -> bool {
    let requested = requested_model.trim();
    if requested.is_empty() {
        return false;
    }
    let Some(restored) = catalogs.get(requested) else {
        return false;
    };
    replace_live_bound_options(live_config_options, restored)
}

/// Authoring upsert may only consume a session catalog that this session
/// actually observed for the live current model. A retargeted leftover table
/// must not stamp B's Context as A.
pub fn authoring_upsert_payload_from_session_catalog(
    live_config_options: Option<&Value>,
    session_catalogs: &BTreeMap<String, Value>,
    current_model_id: Option<&str>,
) -> Option<Value> {
    let current = current_model_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| live_catalog_model_id(live_config_options))?;
    let bound = session_catalogs.get(&current)?;
    let mut options = Vec::new();
    if let Some(live) = live_config_options.and_then(live_config_option_array) {
        for option in live {
            if option_category(Some(option)).is_some_and(is_model_bound_config_category) {
                continue;
            }
            let mut option = option.clone();
            if option.get("id").and_then(Value::as_str) == Some("model")
                || option.get("category").and_then(Value::as_str) == Some("model")
            {
                if let Some(object) = option.as_object_mut() {
                    object.insert("currentValue".into(), Value::String(current.clone()));
                }
            }
            options.push(option);
        }
    }
    if options.iter().all(|option| {
        option.get("id").and_then(Value::as_str) != Some("model")
            && option.get("category").and_then(Value::as_str) != Some("model")
    }) {
        options.insert(
            0,
            serde_json::json!({
                "id": "model",
                "category": "model",
                "currentValue": current,
            }),
        );
    }
    if let Some(bound) = bound.as_array() {
        let insert_at = options
            .iter()
            .position(|option| {
                option.get("id").and_then(Value::as_str) == Some("model")
                    || option.get("category").and_then(Value::as_str) == Some("model")
            })
            .map(|index| index + 1)
            .unwrap_or(options.len());
        for (offset, option) in bound.iter().cloned().enumerate() {
            options.insert(insert_at + offset, option);
        }
    }
    Some(Value::Array(options))
}

fn replace_live_bound_options(live_config_options: &mut Option<Value>, restored: &Value) -> bool {
    let Some(options) = live_config_options.as_mut().and_then(Value::as_array_mut) else {
        return false;
    };
    let restored = restored.as_array().cloned().unwrap_or_default();
    let previous: Vec<Value> = options
        .iter()
        .filter(|option| option_category(Some(option)).is_some_and(is_model_bound_config_category))
        .cloned()
        .collect();
    if previous == restored {
        return false;
    }
    options.retain(|option| {
        !option_category(Some(option)).is_some_and(is_model_bound_config_category)
    });
    let insert_at = options
        .iter()
        .position(|option| {
            option.get("id").and_then(Value::as_str) == Some("model")
                || option.get("category").and_then(Value::as_str) == Some("model")
        })
        .map(|index| index + 1)
        .unwrap_or(options.len());
    for (offset, option) in restored.into_iter().enumerate() {
        options.insert(insert_at + offset, option);
    }
    true
}

fn strip_model_bound_overrides(
    catalog_before: Option<&Value>,
    catalog_after: Option<&Value>,
    overrides: &mut BTreeMap<String, String>,
    rollback_invalid_listed_values: bool,
) -> Vec<RolledBackSessionConfig> {
    let Some(catalog_after) = catalog_after else {
        return Vec::new();
    };
    let ids = overrides.keys().cloned().collect::<Vec<_>>();
    let mut rolled_back = Vec::new();
    for config_id in ids {
        let Some(value) = overrides.get(&config_id).cloned() else {
            continue;
        };
        let after = config_option_by_id(Some(catalog_after), &config_id);
        let before = config_option_by_id(catalog_before, &config_id);
        if after.is_none() {
            let source_is_thought_level = option_category(before)
                .map(|category| category == ACP_THOUGHT_LEVEL_CATEGORY)
                .unwrap_or(true);
            if source_is_thought_level {
                if apply_thought_level_remap(
                    Some(catalog_after),
                    overrides,
                    &config_id,
                    &value,
                ) {
                    continue;
                }
            }
            if option_category(before).is_some_and(|item| !is_model_bound_config_category(item)) {
                continue;
            }
            let category = option_category(before)
                .filter(|item| is_model_bound_config_category(item))
                .map(str::to_string)
                .unwrap_or_else(|| ACP_MODEL_CONFIG_CATEGORY.to_string());
            overrides.remove(&config_id);
            rolled_back.push(RolledBackSessionConfig {
                name: rollback_display_name(before, Some(catalog_after), &category),
                category,
                config_id,
                value,
            });
            continue;
        }
        let category = option_category(after).or_else(|| option_category(before));
        let Some(category) = category.filter(|item| is_model_bound_config_category(item)) else {
            continue;
        };
        if option_has_value(after, &value) {
            continue;
        }
        if !rollback_invalid_listed_values {
            continue;
        }
        if category == ACP_THOUGHT_LEVEL_CATEGORY {
            if apply_thought_level_remap(Some(catalog_after), overrides, &config_id, &value) {
                continue;
            }
        }
        overrides.remove(&config_id);
        rolled_back.push(RolledBackSessionConfig {
            name: rollback_display_name(before.or(after), Some(catalog_after), category),
            category: category.to_string(),
            config_id,
            value,
        });
    }
    rolled_back
}

pub fn rolled_back_session_config_params(items: &[RolledBackSessionConfig]) -> Value {
    serde_json::json!({
        "items": items.iter().map(|item| {
            let mut value = serde_json::json!({
                "category": item.category,
                "configId": item.config_id,
                "value": item.value,
            });
            if let Some(name) = item.name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
                value["name"] = serde_json::Value::String(name.to_string());
            }
            value
        }).collect::<Vec<_>>(),
    })
}

fn apply_thought_level_remap(
    catalog_after: Option<&Value>,
    overrides: &mut BTreeMap<String, String>,
    config_id: &str,
    value: &str,
) -> bool {
    let Some(thought_id) =
        remap_thought_level_override(catalog_after, overrides, config_id, value)
    else {
        return false;
    };
    if thought_id != config_id {
        overrides.remove(config_id);
        overrides.insert(thought_id, value.to_string());
    }
    true
}

fn remap_thought_level_override(
    catalog_after: Option<&Value>,
    overrides: &BTreeMap<String, String>,
    config_id: &str,
    value: &str,
) -> Option<String> {
    let thought = thought_option_with_value(catalog_after, value)?;
    let thought_id = thought.get("id").and_then(Value::as_str)?;
    if thought_id != config_id {
        if let Some(existing) = overrides.get(thought_id) {
            if option_has_value(Some(thought), existing) {
                return None;
            }
        }
    }
    Some(thought_id.to_string())
}

fn thought_option_with_value<'a>(catalog: Option<&'a Value>, value: &str) -> Option<&'a Value> {
    catalog.and_then(Value::as_array).and_then(|options| {
        options.iter().find(|option| {
            option.get("category").and_then(Value::as_str) == Some(ACP_THOUGHT_LEVEL_CATEGORY)
                && option_has_value(Some(option), value)
        })
    })
}

fn thought_option(catalog: Option<&Value>) -> Option<&Value> {
    catalog.and_then(Value::as_array).and_then(|options| {
        options.iter().find(|option| {
            option.get("category").and_then(Value::as_str) == Some(ACP_THOUGHT_LEVEL_CATEGORY)
        })
    })
}

fn rollback_display_name(
    option: Option<&Value>,
    catalog_after: Option<&Value>,
    category: &str,
) -> Option<String> {
    option_display_name(option).or_else(|| {
        if category == ACP_THOUGHT_LEVEL_CATEGORY {
            option_display_name(thought_option(catalog_after))
        } else {
            None
        }
    })
}

fn option_display_name(option: Option<&Value>) -> Option<String> {
    option
        .and_then(|item| item.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn config_option_by_id<'a>(catalog: Option<&'a Value>, config_id: &str) -> Option<&'a Value> {
    catalog.and_then(Value::as_array).and_then(|options| {
        options
            .iter()
            .find(|option| option.get("id").and_then(Value::as_str) == Some(config_id))
    })
}

fn option_category(option: Option<&Value>) -> Option<&str> {
    option.and_then(|option| option.get("category").and_then(Value::as_str))
}

fn option_has_value(option: Option<&Value>, value: &str) -> bool {
    option
        .and_then(|option| option.get("options").and_then(Value::as_array))
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("value").and_then(Value::as_str))
        .any(|item| item == value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fable_catalog() -> Value {
        json!([
            {
                "id": "model",
                "category": "model",
                "currentValue": "claude-fable",
            },
            {
                "id": "thinking",
                "category": "thought_level",
                "options": [{ "value": "false" }, { "value": "true" }],
            },
            {
                "id": "effort",
                "category": "thought_level",
                "options": [{ "value": "high" }, { "value": "extra-high" }],
            },
            {
                "id": "context",
                "category": "model_config",
                "options": [{ "value": "1m" }],
            },
        ])
    }

    fn catalog(current_model: &str, thought_values: &[&str], fast: bool) -> Value {
        let mut options = vec![json!({
            "id": "model",
            "category": "model",
            "currentValue": current_model,
            "options": [
                { "value": "sol", "name": "Sol" },
                { "value": "terra", "name": "Terra" },
            ],
        })];
        if !thought_values.is_empty() {
            options.push(json!({
                "id": "effort",
                "category": "thought_level",
                "options": thought_values.iter().map(|value| json!({ "value": *value })).collect::<Vec<_>>(),
            }));
        }
        if fast {
            options.push(json!({
                "id": "fast",
                "category": "model_config",
                "options": [
                    { "value": "true" },
                    { "value": "false" },
                ],
            }));
        }
        Value::Array(options)
    }

    #[test]
    fn keeps_thought_and_fast_when_the_new_catalog_still_lists_them() {
        let before = catalog("sol", &["low", "high"], true);
        let after = catalog("terra", &["low", "high"], true);
        let mut overrides = BTreeMap::from([
            ("effort".into(), "high".into()),
            ("fast".into(), "true".into()),
        ]);

        let rolled_back =
            strip_unsupported_model_bound_overrides(Some(&before), Some(&after), &mut overrides);

        assert!(rolled_back.is_empty());
        assert_eq!(
            overrides,
            BTreeMap::from([
                ("effort".into(), "high".into()),
                ("fast".into(), "true".into()),
            ])
        );
    }

    #[test]
    fn drops_thought_and_fast_that_the_new_model_does_not_list() {
        let before = catalog("sol", &["low", "high"], true);
        let after = catalog("terra", &["low"], false);
        let mut overrides = BTreeMap::from([
            ("effort".into(), "high".into()),
            ("fast".into(), "true".into()),
        ]);

        let rolled_back =
            strip_unsupported_model_bound_overrides(Some(&before), Some(&after), &mut overrides);

        assert_eq!(
            rolled_back,
            vec![
                RolledBackSessionConfig {
                    category: ACP_THOUGHT_LEVEL_CATEGORY.into(),
                    config_id: "effort".into(),
                    value: "high".into(),
                    name: None,
                },
                RolledBackSessionConfig {
                    category: ACP_MODEL_CONFIG_CATEGORY.into(),
                    config_id: "fast".into(),
                    value: "true".into(),
                    name: None,
                },
            ]
        );
        assert!(overrides.is_empty());
    }

    #[test]
    fn uses_the_previous_catalog_category_when_the_option_id_disappears() {
        let before = catalog("sol", &["high"], false);
        let after = catalog("terra", &[], false);
        let mut overrides = BTreeMap::from([("effort".into(), "high".into())]);

        let rolled_back =
            strip_unsupported_model_bound_overrides(Some(&before), Some(&after), &mut overrides);

        assert_eq!(
            rolled_back,
            vec![RolledBackSessionConfig {
                category: ACP_THOUGHT_LEVEL_CATEGORY.into(),
                config_id: "effort".into(),
                value: "high".into(),
                name: None,
            }]
        );
        assert!(overrides.is_empty());
    }

    fn luna_catalog(thought_values: &[&str], context: bool) -> Value {
        let mut options = vec![json!({
            "id": "model",
            "category": "model",
            "currentValue": "gpt-5.6-luna",
            "options": [
                { "value": "grok-4.6", "name": "Grok" },
                { "value": "gpt-5.6-luna", "name": "Luna" },
            ],
        })];
        if context {
            options.push(json!({
                "id": "context",
                "category": "model_config",
                "name": "Context",
                "options": [
                    { "value": "272k" },
                    { "value": "1m" },
                ],
            }));
        }
        if !thought_values.is_empty() {
            options.push(json!({
                "id": "reasoning",
                "category": "thought_level",
                "options": thought_values.iter().map(|value| json!({ "value": *value })).collect::<Vec<_>>(),
            }));
        }
        options.push(json!({
            "id": "fast",
            "category": "model_config",
            "options": [
                { "value": "true" },
                { "value": "false" },
            ],
        }));
        Value::Array(options)
    }

    #[test]
    fn remaps_thought_level_across_option_ids_when_the_value_still_exists() {
        let before = catalog("grok-4.6", &["low", "medium", "high", "xhigh"], true);
        let after = luna_catalog(&["none", "low", "medium", "high", "xhigh", "max"], true);
        let mut overrides = BTreeMap::from([
            ("effort".into(), "high".into()),
            ("fast".into(), "false".into()),
        ]);

        let rolled_back =
            strip_unsupported_model_bound_overrides(Some(&before), Some(&after), &mut overrides);

        assert!(rolled_back.is_empty());
        assert_eq!(
            overrides,
            BTreeMap::from([
                ("reasoning".into(), "high".into()),
                ("fast".into(), "false".into()),
            ])
        );
    }

    #[test]
    fn does_not_remap_fast_off_onto_thinking_off() {
        let before = catalog("grok-4.6", &["high", "extra-high"], true);
        let after = json!([
            {
                "id": "model",
                "category": "model",
                "currentValue": "claude-fable",
            },
            {
                "id": "thinking",
                "category": "thought_level",
                "options": [{ "value": "false" }, { "value": "true" }],
            },
            {
                "id": "effort",
                "category": "thought_level",
                "options": [{ "value": "high" }, { "value": "extra-high" }],
            },
            {
                "id": "context",
                "category": "model_config",
                "options": [{ "value": "1m" }],
            },
        ]);
        let mut overrides = BTreeMap::from([
            ("effort".into(), "high".into()),
            ("fast".into(), "false".into()),
        ]);

        let rolled_back =
            strip_unsupported_model_bound_overrides(Some(&before), Some(&after), &mut overrides);

        assert_eq!(
            rolled_back,
            vec![RolledBackSessionConfig {
                category: ACP_MODEL_CONFIG_CATEGORY.into(),
                config_id: "fast".into(),
                value: "false".into(),
                name: None,
            }]
        );
        assert_eq!(
            overrides,
            BTreeMap::from([("effort".into(), "high".into())])
        );
        assert!(!overrides.contains_key("thinking"));
    }

    #[test]
    fn remaps_reasoning_high_onto_fable_effort_not_the_first_thought_option() {
        let before = luna_catalog(&["medium", "high"], true);
        let after = json!([
            {
                "id": "model",
                "category": "model",
                "currentValue": "claude-fable",
            },
            {
                "id": "thinking",
                "category": "thought_level",
                "options": [{ "value": "false" }, { "value": "true" }],
            },
            {
                "id": "effort",
                "category": "thought_level",
                "options": [{ "value": "high" }, { "value": "extra-high" }],
            },
            {
                "id": "context",
                "category": "model_config",
                "options": [{ "value": "1m" }],
            },
        ]);
        let mut overrides = BTreeMap::from([
            ("reasoning".into(), "high".into()),
            ("fast".into(), "false".into()),
        ]);

        let rolled_back =
            strip_unsupported_model_bound_overrides(Some(&before), Some(&after), &mut overrides);

        assert_eq!(
            rolled_back,
            vec![RolledBackSessionConfig {
                category: ACP_MODEL_CONFIG_CATEGORY.into(),
                config_id: "fast".into(),
                value: "false".into(),
                name: None,
            }]
        );
        assert_eq!(
            overrides,
            BTreeMap::from([("effort".into(), "high".into())])
        );
        assert!(!overrides.contains_key("thinking"));
    }

    #[test]
    fn remaps_authoring_thought_id_when_live_catalog_already_uses_another_id() {
        let live = luna_catalog(&["none", "low", "medium", "high", "xhigh", "max"], true);
        let mut overrides = BTreeMap::from([
            ("effort".into(), "high".into()),
            ("fast".into(), "false".into()),
        ]);

        let rolled_back =
            strip_unsupported_model_bound_overrides(Some(&live), Some(&live), &mut overrides);

        assert!(rolled_back.is_empty());
        assert_eq!(
            overrides,
            BTreeMap::from([
                ("reasoning".into(), "high".into()),
                ("fast".into(), "false".into()),
            ])
        );
    }

    #[test]
    fn aligns_authoring_thought_id_to_the_live_catalog_without_a_model_change() {
        let live = luna_catalog(&["none", "low", "medium", "high", "xhigh", "max"], true);
        let mut overrides = BTreeMap::from([
            ("effort".into(), "high".into()),
            ("fast".into(), "false".into()),
        ]);

        let rolled_back = align_overrides_to_live_catalog(Some(&live), Some(&live), &mut overrides);

        assert!(rolled_back.is_empty());
        assert_eq!(
            overrides,
            BTreeMap::from([
                ("reasoning".into(), "high".into()),
                ("fast".into(), "false".into()),
            ])
        );
    }

    #[test]
    fn rolls_back_authoring_option_ids_missing_from_the_live_catalog() {
        let before = catalog("grok-4.6", &["low", "medium", "high"], true);
        let after = luna_catalog(&["none", "low", "medium", "high"], false);
        let mut overrides = BTreeMap::from([
            ("effort".into(), "max".into()),
            ("context".into(), "1m".into()),
            ("fast".into(), "false".into()),
        ]);

        let rolled_back =
            strip_unsupported_model_bound_overrides(Some(&before), Some(&after), &mut overrides);

        assert_eq!(
            rolled_back,
            vec![
                RolledBackSessionConfig {
                    category: ACP_MODEL_CONFIG_CATEGORY.into(),
                    config_id: "context".into(),
                    value: "1m".into(),
                    name: None,
                },
                RolledBackSessionConfig {
                    category: ACP_THOUGHT_LEVEL_CATEGORY.into(),
                    config_id: "effort".into(),
                    value: "max".into(),
                    name: None,
                },
            ]
        );
        assert_eq!(overrides, BTreeMap::from([("fast".into(), "false".into())]));
    }

    #[test]
    fn aligns_missing_authoring_option_ids_to_unspecified() {
        let live = luna_catalog(&["none", "low", "medium", "high"], false);
        let mut overrides = BTreeMap::from([
            ("effort".into(), "max".into()),
            ("context".into(), "1m".into()),
            ("fast".into(), "false".into()),
        ]);

        let rolled_back = align_overrides_to_live_catalog(Some(&live), Some(&live), &mut overrides);

        assert_eq!(
            rolled_back,
            vec![
                RolledBackSessionConfig {
                    category: ACP_MODEL_CONFIG_CATEGORY.into(),
                    config_id: "context".into(),
                    value: "1m".into(),
                    name: None,
                },
                RolledBackSessionConfig {
                    category: ACP_MODEL_CONFIG_CATEGORY.into(),
                    config_id: "effort".into(),
                    value: "max".into(),
                    name: None,
                },
            ]
        );
        assert_eq!(overrides, BTreeMap::from([("fast".into(), "false".into())]));
    }

    #[test]
    fn rolled_back_params_include_protocol_name() {
        let params = rolled_back_session_config_params(&[RolledBackSessionConfig {
            category: ACP_MODEL_CONFIG_CATEGORY.into(),
            config_id: "context".into(),
            value: "1m".into(),
            name: Some("Context".into()),
        }]);

        assert_eq!(
            params,
            json!({
                "items": [{
                    "category": ACP_MODEL_CONFIG_CATEGORY,
                    "configId": "context",
                    "value": "1m",
                    "name": "Context",
                }],
            })
        );
    }

    #[test]
    fn does_not_map_fast_onto_a_different_model_config_option() {
        let before = catalog("grok-4.6", &["high"], true);
        let after = luna_catalog(&["high"], true);
        let mut overrides = BTreeMap::from([
            ("effort".into(), "high".into()),
            ("fast".into(), "false".into()),
        ]);

        let rolled_back =
            strip_unsupported_model_bound_overrides(Some(&before), Some(&after), &mut overrides);

        assert!(rolled_back.is_empty());
        assert_eq!(
            overrides,
            BTreeMap::from([
                ("reasoning".into(), "high".into()),
                ("fast".into(), "false".into()),
            ])
        );
        assert!(!overrides.contains_key("context"));
    }

    #[test]
    fn rolls_back_context_when_the_new_model_does_not_list_it() {
        let before = luna_catalog(&["high"], true);
        let after = catalog("grok-4.6", &["low", "medium", "high", "xhigh"], true);
        let mut overrides = BTreeMap::from([
            ("reasoning".into(), "high".into()),
            ("fast".into(), "false".into()),
            ("context".into(), "1m".into()),
        ]);

        let rolled_back =
            strip_unsupported_model_bound_overrides(Some(&before), Some(&after), &mut overrides);

        assert_eq!(
            rolled_back,
            vec![RolledBackSessionConfig {
                category: ACP_MODEL_CONFIG_CATEGORY.into(),
                config_id: "context".into(),
                value: "1m".into(),
                name: Some("Context".into()),
            }]
        );
        assert_eq!(
            overrides,
            BTreeMap::from([
                ("effort".into(), "high".into()),
                ("fast".into(), "false".into()),
            ])
        );
    }

    #[test]
    fn drops_fast_when_the_new_model_has_no_model_config_option() {
        let before = catalog("sol", &["high"], true);
        let after = catalog("terra", &["high"], false);
        let mut overrides = BTreeMap::from([
            ("effort".into(), "high".into()),
            ("fast".into(), "true".into()),
        ]);

        let rolled_back =
            strip_unsupported_model_bound_overrides(Some(&before), Some(&after), &mut overrides);

        assert_eq!(
            rolled_back,
            vec![RolledBackSessionConfig {
                category: ACP_MODEL_CONFIG_CATEGORY.into(),
                config_id: "fast".into(),
                value: "true".into(),
                name: None,
            }]
        );
        assert_eq!(
            overrides,
            BTreeMap::from([("effort".into(), "high".into())])
        );
    }

    #[test]
    fn leaves_non_model_bound_overrides_for_the_unavailable_error_path() {
        let before = json!([{
            "id": "theme",
            "category": "appearance",
            "options": [{ "value": "dark" }],
        }]);
        let after = catalog("terra", &["low"], false);
        let mut overrides = BTreeMap::from([("theme".into(), "dark".into())]);

        let rolled_back =
            strip_unsupported_model_bound_overrides(Some(&before), Some(&after), &mut overrides);

        assert!(rolled_back.is_empty());
        assert_eq!(overrides, BTreeMap::from([("theme".into(), "dark".into())]));
    }

    #[test]
    fn keeps_listed_invalid_values_when_aligning_without_a_model_change() {
        let live = catalog("grok-4.6", &["low", "medium"], true);
        let mut overrides = BTreeMap::from([("effort".into(), "high".into())]);

        let rolled_back = align_overrides_to_live_catalog(Some(&live), Some(&live), &mut overrides);

        assert!(rolled_back.is_empty());
        assert_eq!(
            overrides,
            BTreeMap::from([("effort".into(), "high".into())])
        );
    }

    #[test]
    fn new_session_rolls_back_listed_invalid_values_without_a_model_rpc() {
        let live = catalog("grok-4.6", &["low", "medium"], true);
        let mut overrides = BTreeMap::from([("effort".into(), "high".into())]);

        let rolled_back = reconcile_session_config_overrides(
            Some(&live),
            Some(&live),
            &mut overrides,
            false,
            true,
        );

        assert_eq!(
            rolled_back,
            vec![RolledBackSessionConfig {
                category: ACP_THOUGHT_LEVEL_CATEGORY.into(),
                config_id: "effort".into(),
                value: "high".into(),
                name: None,
            }]
        );
        assert!(overrides.is_empty());
    }

    #[test]
    fn reused_session_keeps_listed_invalid_values_without_a_model_rpc() {
        let live = catalog("grok-4.6", &["low", "medium"], true);
        let mut overrides = BTreeMap::from([("effort".into(), "high".into())]);

        let rolled_back = reconcile_session_config_overrides(
            Some(&live),
            Some(&live),
            &mut overrides,
            false,
            false,
        );

        assert!(rolled_back.is_empty());
        assert_eq!(
            overrides,
            BTreeMap::from([("effort".into(), "high".into())])
        );
    }

    #[test]
    fn does_not_invent_rollbacks_without_a_new_catalog() {
        let before = catalog("sol", &["high"], true);
        let mut overrides = BTreeMap::from([("effort".into(), "high".into())]);

        let rolled_back =
            strip_unsupported_model_bound_overrides(Some(&before), None, &mut overrides);

        assert!(rolled_back.is_empty());
        assert_eq!(
            overrides,
            BTreeMap::from([("effort".into(), "high".into())])
        );
    }

    fn capabilities(config_options: Value) -> Value {
        json!({ "configOptions": config_options })
    }

    #[test]
    fn doctor_keeps_previous_and_incoming_model_bound_catalogs() {
        let previous = capabilities(catalog("grok-4.6", &["low", "high"], true));
        let incoming = capabilities(luna_catalog(&["none", "low", "medium", "high"], true));

        let merged = merge_doctor_authoring_capabilities(Some(&previous), incoming);
        let grok = catalog_ids(merged["modelBoundCatalogs"]["grok-4.6"].as_array().unwrap());
        let luna = catalog_ids(
            merged["modelBoundCatalogs"]["gpt-5.6-luna"]
                .as_array()
                .unwrap(),
        );
        let latest = catalog_ids(merged["configOptions"].as_array().unwrap());

        assert_eq!(grok, vec!["effort", "fast"]);
        assert!(luna.contains(&"reasoning"));
        assert!(luna.contains(&"context"));
        assert!(luna.contains(&"fast"));
        assert!(latest.contains(&"context"));
        assert_eq!(
            catalog_model_current_value(merged["configOptions"].as_array().unwrap()).as_deref(),
            Some("gpt-5.6-luna")
        );
    }

    #[test]
    fn doctor_stamps_the_incoming_model_bound_catalog_without_previous() {
        let incoming = capabilities(luna_catalog(&["high"], true));
        let merged = merge_doctor_authoring_capabilities(None, incoming);
        let luna = catalog_ids(
            merged["modelBoundCatalogs"]["gpt-5.6-luna"]
                .as_array()
                .unwrap(),
        );
        assert!(luna.contains(&"context"));
        assert!(luna.contains(&"fast"));
        assert!(luna.contains(&"reasoning"));
    }

    #[test]
    fn doctor_replaces_model_bound_options_when_the_previous_model_is_gone() {
        let previous = capabilities(catalog("grok-4.6", &["high"], true));
        let incoming = json!({
            "configOptions": [{
                "id": "model",
                "category": "model",
                "currentValue": "deepseek-v4-pro",
                "options": [
                    { "value": "deepseek-v4-pro", "name": "DeepSeek V4 Pro" },
                    { "value": "deepseek-flash", "name": "DeepSeek Flash" },
                ],
            }]
        });

        let merged = merge_doctor_authoring_capabilities(Some(&previous), incoming);
        assert!(merged["modelBoundCatalogs"].get("grok-4.6").is_none());
        assert_eq!(merged["modelBoundCatalogs"]["deepseek-v4-pro"], json!([]));
        assert_eq!(
            catalog_model_current_value(merged["configOptions"].as_array().unwrap()).as_deref(),
            Some("deepseek-v4-pro")
        );
    }

    #[test]
    fn doctor_refreshes_model_bound_options_when_the_current_model_is_unchanged() {
        let previous = capabilities(catalog("grok-4.6", &["low"], true));
        let incoming = capabilities(catalog("grok-4.6", &["low", "high"], true));

        let merged = merge_doctor_authoring_capabilities(Some(&previous), incoming.clone());
        assert_eq!(
            catalog_ids(merged["modelBoundCatalogs"]["grok-4.6"].as_array().unwrap()),
            vec!["effort", "fast"]
        );
        assert_eq!(
            catalog_ids(incoming["configOptions"].as_array().unwrap())
                .into_iter()
                .filter(|id| *id != "model")
                .collect::<Vec<_>>(),
            vec!["effort", "fast"]
        );
    }

    fn doctor_authoring_capabilities() -> Value {
        merge_doctor_authoring_capabilities(
            None,
            json!({
                "configOptions": [
                    {
                        "id": "model",
                        "category": "model",
                        "currentValue": "grok-4.6",
                        "options": [
                            { "value": "grok-4.6", "name": "Grok" },
                            { "value": "gpt-5.6-luna", "name": "Luna" },
                            { "value": "gpt-5.2", "name": "GPT-5.2" }
                        ]
                    },
                    {
                        "id": "mode",
                        "category": "mode",
                        "currentValue": "agent",
                        "options": [{ "value": "agent", "name": "Agent" }]
                    },
                    {
                        "id": "effort",
                        "category": "thought_level",
                        "options": [{ "value": "low" }, { "value": "high" }]
                    },
                    {
                        "id": "fast",
                        "category": "model_config",
                        "options": [{ "value": "false" }, { "value": "true" }]
                    }
                ]
            }),
        )
    }

    fn luna_session_live_catalog() -> Value {
        json!([
            {
                "id": "model",
                "category": "model",
                "currentValue": "gpt-5.6-luna",
                "options": [{ "value": "gpt-5.6-luna", "name": "Luna" }],
            },
            {
                "id": "context",
                "category": "model_config",
                "name": "Context",
                "options": [{ "value": "272k" }, { "value": "1m" }],
            },
            {
                "id": "reasoning",
                "category": "thought_level",
                "options": [{ "value": "high" }],
            }
        ])
    }

    fn authoring_model_option_values(options: &[Value]) -> Vec<&str> {
        options
            .iter()
            .find(|option| {
                option.get("id").and_then(Value::as_str) == Some("model")
                    || option.get("category").and_then(Value::as_str) == Some("model")
            })
            .and_then(|option| option.get("options").and_then(Value::as_array))
            .into_iter()
            .flatten()
            .filter_map(|option| option.get("value").and_then(Value::as_str))
            .collect()
    }

    #[test]
    fn session_live_catalog_upserts_authoring_cache_and_current_table() {
        let previous = doctor_authoring_capabilities();
        let live = luna_session_live_catalog();

        let merged = upsert_session_authoring_model_bound_catalog(Some(&previous), &live).unwrap();
        let grok = catalog_ids(merged["modelBoundCatalogs"]["grok-4.6"].as_array().unwrap());
        let luna = catalog_ids(
            merged["modelBoundCatalogs"]["gpt-5.6-luna"]
                .as_array()
                .unwrap(),
        );
        let current = merged["configOptions"].as_array().unwrap();

        assert_eq!(grok, vec!["effort", "fast"]);
        assert!(luna.contains(&"context"));
        assert!(luna.contains(&"reasoning"));
        assert_eq!(
            catalog_model_current_value(current).as_deref(),
            Some("gpt-5.6-luna")
        );
        assert_eq!(
            authoring_model_option_values(current),
            vec!["grok-4.6", "gpt-5.6-luna", "gpt-5.2"]
        );
        assert_eq!(
            catalog_ids(current)
                .into_iter()
                .filter(|id| *id != "model")
                .collect::<Vec<_>>(),
            vec!["context", "reasoning", "mode"]
        );
        assert_eq!(
            current
                .iter()
                .find(|option| option.get("id").and_then(Value::as_str) == Some("mode"))
                .and_then(|option| option.get("currentValue").and_then(Value::as_str)),
            Some("agent")
        );
        assert!(upsert_session_authoring_model_bound_catalog(Some(&merged), &live).is_none());
        assert!(upsert_session_authoring_model_bound_catalog(None, &live).is_none());
    }

    #[test]
    fn session_live_catalog_refreshes_authoring_current_table_when_catalogs_already_cached() {
        let previous = doctor_authoring_capabilities();
        let live = luna_session_live_catalog();
        let mut cached = upsert_session_authoring_model_bound_catalog(Some(&previous), &live)
            .unwrap();
        cached["configOptions"] = previous["configOptions"].clone();

        let merged = upsert_session_authoring_model_bound_catalog(Some(&cached), &live).unwrap();
        let current = merged["configOptions"].as_array().unwrap();

        assert_eq!(
            catalog_model_current_value(current).as_deref(),
            Some("gpt-5.6-luna")
        );
        assert_eq!(
            catalog_ids(current)
                .into_iter()
                .filter(|id| *id != "model")
                .collect::<Vec<_>>(),
            vec!["context", "reasoning", "mode"]
        );
        assert_eq!(
            authoring_model_option_values(current),
            vec!["grok-4.6", "gpt-5.6-luna", "gpt-5.2"]
        );
    }

    #[test]
    fn session_omitted_catalog_restores_cached_bound_options_without_stamping_the_new_model() {
        let mut catalogs = BTreeMap::new();
        observe_session_model_bound_catalog(
            &mut catalogs,
            Some(&catalog("sol", &["low", "high"], true)),
        );
        let mut live = Some(luna_catalog(&["high"], true));
        observe_session_model_bound_catalog(&mut catalogs, live.as_ref());

        retarget_current_model(live.as_mut().unwrap(), "sol");
        assert!(restore_session_model_bound_options(
            &catalogs, &mut live, "sol"
        ));

        let ids = catalog_ids(live.as_ref().unwrap().as_array().unwrap());
        assert_eq!(
            ids.into_iter()
                .filter(|id| *id != "model")
                .collect::<Vec<_>>(),
            vec!["effort", "fast"]
        );
        assert_eq!(live_catalog_model_id(live.as_ref()).as_deref(), Some("sol"));
        assert_eq!(
            catalog_ids(catalogs["sol"].as_array().unwrap()),
            vec!["effort", "fast"]
        );
        assert!(catalogs["gpt-5.6-luna"]
            .as_array()
            .unwrap()
            .iter()
            .any(|option| option.get("id").and_then(Value::as_str) == Some("context")));
    }

    #[test]
    fn session_first_contact_keeps_previous_bound_options_and_skips_authoring_upsert() {
        let mut catalogs = BTreeMap::new();
        let mut live = Some(luna_catalog(&["high"], true));
        observe_session_model_bound_catalog(&mut catalogs, live.as_ref());
        retarget_current_model(live.as_mut().unwrap(), "sol");
        assert!(!restore_session_model_bound_options(
            &catalogs, &mut live, "sol"
        ));

        let ids = catalog_ids(live.as_ref().unwrap().as_array().unwrap());
        assert!(ids.contains(&"context"));
        assert!(catalogs.get("sol").is_none());
        assert!(authoring_upsert_payload_from_session_catalog(
            live.as_ref(),
            &catalogs,
            Some("sol")
        )
        .is_none());
    }

    #[test]
    fn session_live_catalog_for_the_same_model_refreshes_the_cache() {
        let mut catalogs = BTreeMap::new();
        observe_session_model_bound_catalog(&mut catalogs, Some(&catalog("sol", &["low"], true)));
        observe_session_model_bound_catalog(
            &mut catalogs,
            Some(&catalog("sol", &["low", "high"], true)),
        );
        assert_eq!(
            catalog_ids(catalogs["sol"].as_array().unwrap()),
            vec!["effort", "fast"]
        );
    }

    #[test]
    fn restores_grok_extra_high_after_switching_to_mini_whose_catalog_cannot_keep_it() {
        let grok = catalog("grok-4.6", &["high", "extra-high"], true);
        let mini = catalog("gpt-5-mini", &["low", "high"], false);
        let mut catalogs = BTreeMap::new();
        observe_session_model_bound_catalog(&mut catalogs, Some(&grok));
        observe_session_model_bound_catalog(&mut catalogs, Some(&mini));
        let current = BTreeMap::from([
            ("effort".into(), "extra-high".into()),
            ("fast".into(), "true".into()),
        ]);

        let (mini_applied, remembered) = switch_model_bound_overrides(
            &BTreeMap::new(),
            Some("grok-4.6"),
            Some("gpt-5-mini"),
            &current,
            Some(&mini),
            &catalogs,
        );
        assert!(mini_applied.is_empty());
        assert_eq!(
            remembered.get("grok-4.6"),
            Some(&current)
        );
        assert_eq!(remembered.get("gpt-5-mini"), Some(&BTreeMap::new()));

        let (grok_applied, _) = switch_model_bound_overrides(
            &remembered,
            Some("gpt-5-mini"),
            Some("grok-4.6"),
            &mini_applied,
            Some(&grok),
            &catalogs,
        );
        assert_eq!(grok_applied, current);
    }

    #[test]
    fn session_snapshot_model_switch_remembers_overrides_without_writing_mini_rollback_over_grok() {
        let grok = catalog("grok-4.6", &["high", "extra-high"], true);
        let mini = catalog("gpt-5-mini", &["low", "high"], false);
        let mut catalogs = BTreeMap::new();
        observe_session_model_bound_catalog(&mut catalogs, Some(&grok));
        observe_session_model_bound_catalog(&mut catalogs, Some(&mini));
        let mut session = json!({
            "modelOverride": "grok-4.6",
            "configOptions": grok,
            "modelBoundCatalogs": catalogs,
            "configOptionOverrides": {
                "effort": "extra-high",
                "fast": "true",
            },
        });

        apply_session_snapshot_model_switch(&mut session, Some("gpt-5-mini"));
        assert_eq!(
            session["modelBoundOverrides"]["grok-4.6"],
            json!({ "effort": "extra-high", "fast": "true" })
        );
        assert_eq!(session["modelBoundOverrides"]["gpt-5-mini"], json!({}));
        assert!(
            session
                .get("configOptionOverrides")
                .and_then(Value::as_object)
                .map(|value| value.is_empty())
                .unwrap_or(true)
        );

        apply_session_snapshot_model_switch(&mut session, Some("grok-4.6"));
        assert_eq!(
            session["configOptionOverrides"],
            json!({ "effort": "extra-high", "fast": "true" })
        );
        assert_eq!(
            session["modelBoundOverrides"]["grok-4.6"],
            json!({ "effort": "extra-high", "fast": "true" })
        );
    }

    #[test]
    fn session_model_switch_retains_against_authoring_catalogs_when_this_session_has_not_observed_the_model() {
        let grok = catalog("grok-4.6", &["high", "extra-high"], true);
        let mini = catalog("gpt-5-mini", &["low", "high"], false);
        let mut session_catalogs = BTreeMap::new();
        observe_session_model_bound_catalog(&mut session_catalogs, Some(&mini));
        let mut authoring_catalogs = BTreeMap::new();
        observe_session_model_bound_catalog(&mut authoring_catalogs, Some(&grok));
        let mut session = json!({
            "modelOverride": "gpt-5-mini",
            "configOptions": mini,
            "modelBoundCatalogs": session_catalogs,
            "configOptionOverrides": {},
            "modelBoundOverrides": {
                "grok-4.6": { "effort": "extra-high", "fast": "true" },
                "gpt-5-mini": {},
            },
        });

        apply_session_snapshot_model_switch_with_authoring(
            &mut session,
            Some("grok-4.6"),
            Some(&authoring_catalogs),
        );
        assert_eq!(
            session["configOptionOverrides"],
            json!({ "effort": "extra-high", "fast": "true" })
        );
        assert_eq!(
            session["modelBoundOverrides"]["grok-4.6"],
            json!({ "effort": "extra-high", "fast": "true" })
        );
        assert!(
            session["modelBoundCatalogs"].get("grok-4.6").is_none(),
            "authoring catalogs must not be stamped as this session's observation",
        );
    }

    #[test]
    fn does_not_remap_fast_off_when_previous_model_catalog_is_not_the_live_table() {
        let grok = catalog("grok-4.6", &["high", "extra-high"], true);
        let fable = fable_catalog();
        let mut catalogs = BTreeMap::new();
        observe_session_model_bound_catalog(&mut catalogs, Some(&grok));
        observe_session_model_bound_catalog(&mut catalogs, Some(&fable));
        let current = BTreeMap::from([
            ("effort".into(), "high".into()),
            ("fast".into(), "false".into()),
        ]);

        let (applied, _) = switch_model_bound_overrides(
            &BTreeMap::new(),
            Some("grok-4.6"),
            Some("claude-fable"),
            &current,
            Some(&fable),
            &catalogs,
        );
        assert_eq!(applied, BTreeMap::from([("effort".into(), "high".into())]));
        assert!(!applied.contains_key("thinking"));

        let (applied_from_stale_live, _) = switch_model_bound_overrides(
            &BTreeMap::new(),
            Some("grok-4.6"),
            Some("claude-fable"),
            &current,
            Some(&grok),
            &catalogs,
        );
        assert_eq!(
            applied_from_stale_live,
            BTreeMap::from([("effort".into(), "high".into())])
        );
        assert!(!applied_from_stale_live.contains_key("thinking"));
    }

    #[test]
    fn does_not_overwrite_an_already_valid_thought_value_when_remapping() {
        let after = json!([{
            "id": "effort",
            "category": "thought_level",
            "options": [{ "value": "medium" }, { "value": "high" }],
        }]);
        let before = json!([{
            "id": "reasoning",
            "category": "thought_level",
            "options": [{ "value": "medium" }, { "value": "high" }],
        }]);
        let mut overrides = BTreeMap::from([
            ("effort".into(), "high".into()),
            ("reasoning".into(), "medium".into()),
        ]);

        let rolled_back =
            strip_unsupported_model_bound_overrides(Some(&before), Some(&after), &mut overrides);

        assert!(rolled_back.is_empty() || rolled_back.iter().all(|item| item.config_id == "reasoning"));
        assert_eq!(overrides.get("effort").map(String::as_str), Some("high"));
        assert!(!overrides.contains_key("reasoning"));
    }

    #[test]
    fn first_visit_seeds_from_current_overrides_that_the_target_catalog_can_keep() {
        let grok = catalog("grok-4.6", &["high", "extra-high"], true);
        let mini = catalog("gpt-5-mini", &["low", "high"], false);
        let mut catalogs = BTreeMap::new();
        observe_session_model_bound_catalog(&mut catalogs, Some(&grok));
        observe_session_model_bound_catalog(&mut catalogs, Some(&mini));
        let current = BTreeMap::from([("effort".into(), "high".into())]);

        let (applied, remembered) = switch_model_bound_overrides(
            &BTreeMap::new(),
            Some("grok-4.6"),
            Some("gpt-5-mini"),
            &current,
            Some(&mini),
            &catalogs,
        );
        assert_eq!(applied, current);
        assert_eq!(remembered.get("gpt-5-mini"), Some(&current));
    }

    #[test]
    fn restores_empty_remembered_slot_instead_of_seeding_from_current() {
        let grok = catalog("grok-4.6", &["high", "extra-high"], true);
        let mini = catalog("gpt-5-mini", &["low", "high"], false);
        let mut catalogs = BTreeMap::new();
        observe_session_model_bound_catalog(&mut catalogs, Some(&grok));
        observe_session_model_bound_catalog(&mut catalogs, Some(&mini));
        let remembered = BTreeMap::from([
            (
                "grok-4.6".into(),
                BTreeMap::from([("effort".into(), "extra-high".into())]),
            ),
            ("gpt-5-mini".into(), BTreeMap::new()),
        ]);
        let current = BTreeMap::from([("effort".into(), "extra-high".into())]);

        let (applied, _) = switch_model_bound_overrides(
            &remembered,
            Some("grok-4.6"),
            Some("gpt-5-mini"),
            &current,
            Some(&mini),
            &catalogs,
        );
        assert!(applied.is_empty());
    }

    #[test]
    fn overwrites_a_model_slot_with_the_state_at_leave() {
        let grok = catalog("grok-4.6", &["high", "extra-high"], true);
        let mini = catalog("gpt-5-mini", &["low", "high"], false);
        let mut catalogs = BTreeMap::new();
        observe_session_model_bound_catalog(&mut catalogs, Some(&grok));
        observe_session_model_bound_catalog(&mut catalogs, Some(&mini));

        let (mini_applied, remembered) = switch_model_bound_overrides(
            &BTreeMap::new(),
            Some("grok-4.6"),
            Some("gpt-5-mini"),
            &BTreeMap::from([("effort".into(), "extra-high".into())]),
            Some(&mini),
            &catalogs,
        );
        let (_, remembered) = switch_model_bound_overrides(
            &remembered,
            Some("gpt-5-mini"),
            Some("grok-4.6"),
            &mini_applied,
            Some(&grok),
            &catalogs,
        );
        let (mini_again, remembered) = switch_model_bound_overrides(
            &remembered,
            Some("grok-4.6"),
            Some("gpt-5-mini"),
            &BTreeMap::from([("effort".into(), "high".into())]),
            Some(&mini),
            &catalogs,
        );
        let (grok_applied, _) = switch_model_bound_overrides(
            &remembered,
            Some("gpt-5-mini"),
            Some("grok-4.6"),
            &mini_again,
            Some(&grok),
            &catalogs,
        );
        assert_eq!(
            grok_applied,
            BTreeMap::from([("effort".into(), "high".into())])
        );
    }

    #[test]
    fn retarget_restores_authoring_catalog_without_stamping_session_observation() {
        let grok = catalog("grok-4.6", &["high", "extra-high"], true);
        let luna_bound = json!([
            {
                "id": "reasoning",
                "category": "thought_level",
                "options": [{ "value": "high" }],
            },
            {
                "id": "context",
                "category": "model_config",
                "options": [{ "value": "1m" }],
            },
        ]);
        let mut live = Some(json!([
            {
                "id": "model",
                "category": "model",
                "currentValue": "gpt-5.6-luna",
            },
            {
                "id": "effort",
                "category": "thought_level",
                "options": [{ "value": "extra-high" }],
            },
            {
                "id": "fast",
                "category": "model_config",
                "options": [{ "value": "false" }, { "value": "true" }],
            },
        ]));
        let mut session_catalogs = BTreeMap::new();
        observe_session_model_bound_catalog(&mut session_catalogs, Some(&grok));
        let mut authoring = BTreeMap::new();
        authoring.insert("gpt-5.6-luna".into(), luna_bound);

        assert!(retarget_live_model_bound_catalog(
            &mut live,
            &mut session_catalogs,
            &authoring,
            "gpt-5.6-luna",
            false,
        ));
        let ids = catalog_ids(live.as_ref().and_then(Value::as_array).unwrap());
        assert_eq!(ids, vec!["model", "reasoning", "context"]);
        assert!(session_catalogs.get("gpt-5.6-luna").is_none());
    }

    #[test]
    fn selected_luna_context_is_available_from_authoring_while_live_table_is_still_grok() {
        let grok = catalog("grok-4.6", &["high", "extra-high"], true);
        let luna_bound = json!([
            {
                "id": "reasoning",
                "category": "thought_level",
                "options": [{ "value": "high" }, { "value": "extra-high" }],
            },
            {
                "id": "context",
                "category": "model_config",
                "options": [{ "value": "272k" }, { "value": "1m" }],
            },
            {
                "id": "fast",
                "category": "model_config",
                "options": [{ "value": "false" }, { "value": "true" }],
            },
        ]);
        let session = json!({
            "modelOverride": "gpt-5.6-luna",
            "models": { "currentModelId": "gpt-5.6-luna" },
            "configOptions": grok,
            "modelBoundCatalogs": {
                "grok-4.6": bound_config_options(grok.as_array().unwrap()),
            },
        });
        let mut authoring = BTreeMap::new();
        authoring.insert("gpt-5.6-luna".into(), luna_bound);

        let context = projected_bound_option(&session, &authoring, "context")
            .expect("Luna Context must be selectable from the authoring catalog");
        assert!(option_lists_value(context, "1m"));
        assert!(option_lists_value(context, "272k"));
        assert!(!option_lists_value(context, "2m"));
        assert!(projected_bound_option(&session, &authoring, "fast").is_some());
        assert!(projected_bound_option(&session, &authoring, "effort").is_none());
    }

    #[test]
    fn doctor_current_table_context_is_not_projected_onto_live_grok() {
        let grok = catalog("grok-4.6", &["high", "extra-high"], true);
        let session = json!({
            "configOptions": grok,
        });
        let mut authoring = BTreeMap::new();
        authoring.insert(
            "gpt-5.6-luna".into(),
            json!([{
                "id": "context",
                "category": "model_config",
                "options": [{ "value": "1m" }],
            }]),
        );
        assert!(projected_bound_option(&session, &authoring, "context").is_none());
        assert!(projected_bound_option(&session, &authoring, "fast").is_some());
    }

    #[test]
    fn continue_does_not_reapply_authoring_bound_options_after_snapshot_rollback() {
        let authoring = BTreeMap::from([("reasoning".into(), "xhigh".into())]);
        let snapshot = BTreeMap::new();
        assert!(
            invocation_config_option_overrides(true, authoring, snapshot, None, Some("gemini"))
                .is_empty()
        );
    }

    #[test]
    fn continue_keeps_snapshot_overrides_instead_of_authoring_leftovers() {
        let authoring = BTreeMap::from([("reasoning".into(), "xhigh".into())]);
        let snapshot = BTreeMap::from([("effort".into(), "high".into())]);
        assert_eq!(
            invocation_config_option_overrides(true, authoring, snapshot, None, Some("gemini"),),
            BTreeMap::from([("effort".into(), "high".into())])
        );
    }

    #[test]
    fn new_session_keeps_authoring_when_selected_model_catalog_is_unobserved() {
        let authoring = BTreeMap::from([("reasoning".into(), "xhigh".into())]);
        let capabilities = capabilities(json!([
            {
                "id": "model",
                "category": "model",
                "currentValue": "grok-4.6",
                "options": [
                    { "value": "grok-4.6" },
                    { "value": "gemini-3.5-flash" },
                ],
            },
            {
                "id": "reasoning",
                "category": "thought_level",
                "options": [{ "value": "xhigh" }, { "value": "high" }],
            },
        ]));
        assert_eq!(
            invocation_config_option_overrides(
                false,
                authoring,
                BTreeMap::new(),
                Some(&capabilities),
                Some("gemini-3.5-flash"),
            )
            .get("reasoning")
            .map(String::as_str),
            Some("xhigh")
        );
    }

    #[test]
    fn new_session_silently_drops_bound_options_omitted_from_observed_model_catalog() {
        let authoring = BTreeMap::from([("reasoning".into(), "xhigh".into())]);
        let mut capabilities = capabilities(catalog("grok-4.6", &["xhigh", "high"], true));
        capabilities.as_object_mut().unwrap().insert(
            ACP_MODEL_BOUND_CATALOGS_KEY.into(),
            json!({ "gemini-3.5-flash": [] }),
        );
        assert!(
            invocation_config_option_overrides(
                false,
                authoring,
                BTreeMap::new(),
                Some(&capabilities),
                Some("gemini-3.5-flash"),
            )
            .is_empty()
        );
    }

    #[test]
    fn session_config_fragment_without_model_does_not_replace_the_cache() {
        let mut catalogs = BTreeMap::new();
        observe_session_model_bound_catalog(&mut catalogs, Some(&catalog("sol", &["high"], true)));
        let fragment = json!([{
            "id": "fast",
            "category": "model_config",
            "options": [{ "value": "true" }],
        }]);
        assert!(!observe_session_model_bound_catalog(
            &mut catalogs,
            Some(&fragment)
        ));
        assert_eq!(
            catalog_ids(catalogs["sol"].as_array().unwrap()),
            vec!["effort", "fast"]
        );
    }

    fn retarget_current_model(catalog: &mut Value, model: &str) {
        if let Some(options) = catalog.as_array_mut() {
            for option in options {
                if option.get("id").and_then(Value::as_str) == Some("model")
                    || option.get("category").and_then(Value::as_str) == Some("model")
                {
                    if let Some(object) = option.as_object_mut() {
                        object.insert("currentValue".into(), json!(model));
                    }
                }
            }
        }
    }

    fn catalog_ids(options: &[Value]) -> Vec<&str> {
        options
            .iter()
            .filter_map(|option| option.get("id").and_then(Value::as_str))
            .collect()
    }
}
