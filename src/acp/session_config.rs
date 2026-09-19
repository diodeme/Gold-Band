use std::collections::BTreeMap;

use serde_json::Value;

pub const ACP_SESSION_CONFIG_ROLLED_BACK_CODE: &str = "acp.session-config-rolled-back";
pub const ACP_THOUGHT_LEVEL_CATEGORY: &str = "thought_level";
pub const ACP_MODEL_CONFIG_CATEGORY: &str = "model_config";
pub const ACP_MODEL_BOUND_CATALOGS_KEY: &str = "modelBoundCatalogs";

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
/// belong to the new catalog. Thought level is a category with a wire id that
/// may change across models; keep the same value by remapping onto the new
/// `thought_level` option. `model_config` knobs stay id-scoped (Fast is not
/// Context). Drop entries the new model does not list. Other categories stay
/// so the existing unavailable error can still block the prompt.
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

/// After `set_config_option(model)` omits the table, restore this model's last
/// observation. No cache means first contact: keep the previous model's bound
/// rows so apply can remap, but do not record them as the requested model.
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
            if let Some(thought_id) =
                remap_thought_level_override(Some(catalog_after), overrides, &config_id, &value)
            {
                if thought_id != config_id {
                    overrides.remove(&config_id);
                    overrides.insert(thought_id, value);
                }
                continue;
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
            if let Some(thought_id) =
                remap_thought_level_override(Some(catalog_after), overrides, &config_id, &value)
            {
                if thought_id != config_id {
                    overrides.remove(&config_id);
                    overrides.insert(thought_id, value);
                }
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

fn remap_thought_level_override(
    catalog_after: Option<&Value>,
    _overrides: &BTreeMap<String, String>,
    _config_id: &str,
    value: &str,
) -> Option<String> {
    let thought = thought_option(catalog_after)?;
    let thought_id = thought.get("id").and_then(Value::as_str)?;
    option_has_value(Some(thought), value).then(|| thought_id.to_string())
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
