use std::collections::BTreeMap;

use serde_json::Value;

pub const ACP_SESSION_CONFIG_ROLLED_BACK_CODE: &str = "acp.session-config-rolled-back";
pub const ACP_THOUGHT_LEVEL_CATEGORY: &str = "thought_level";
pub const ACP_MODEL_CONFIG_CATEGORY: &str = "model_config";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolledBackSessionConfig {
    pub category: String,
    pub config_id: String,
    pub value: String,
}

pub fn is_model_bound_config_category(category: &str) -> bool {
    category == ACP_THOUGHT_LEVEL_CATEGORY || category == ACP_MODEL_CONFIG_CATEGORY
}

/// After `session/set_config_option(model)`, dependents in the same apply
/// belong to the new catalog. Keep values that still exist; drop thought /
/// Fast entries the new model does not list. Other categories stay so the
/// existing unavailable error can still block the prompt.
pub fn strip_unsupported_model_bound_overrides(
    catalog_before: Option<&Value>,
    catalog_after: Option<&Value>,
    overrides: &mut BTreeMap<String, String>,
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
        let category = option_category(after).or_else(|| option_category(before));
        let Some(category) = category.filter(|item| is_model_bound_config_category(item)) else {
            continue;
        };
        if option_has_value(after, &value) {
            continue;
        }
        overrides.remove(&config_id);
        rolled_back.push(RolledBackSessionConfig {
            category: category.to_string(),
            config_id,
            value,
        });
    }
    rolled_back
}

pub fn rolled_back_session_config_params(items: &[RolledBackSessionConfig]) -> Value {
    serde_json::json!({
        "items": items.iter().map(|item| serde_json::json!({
            "category": item.category,
            "configId": item.config_id,
            "value": item.value,
        })).collect::<Vec<_>>(),
    })
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
            ("theme".into(), "dark".into()),
        ]);

        let rolled_back =
            strip_unsupported_model_bound_overrides(Some(&before), Some(&after), &mut overrides);

        assert!(rolled_back.is_empty());
        assert_eq!(
            overrides,
            BTreeMap::from([
                ("effort".into(), "high".into()),
                ("fast".into(), "true".into()),
                ("theme".into(), "dark".into()),
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
            ("theme".into(), "dark".into()),
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
                },
                RolledBackSessionConfig {
                    category: ACP_MODEL_CONFIG_CATEGORY.into(),
                    config_id: "fast".into(),
                    value: "true".into(),
                },
            ]
        );
        assert_eq!(
            overrides,
            BTreeMap::from([("theme".into(), "dark".into())])
        );
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
            }]
        );
        assert!(overrides.is_empty());
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
        assert_eq!(
            overrides,
            BTreeMap::from([("theme".into(), "dark".into())])
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
}
