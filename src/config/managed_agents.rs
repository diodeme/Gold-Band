use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{ManagedAgentConfig, ManagedAgentId};

// Settings store user fields only; the in-memory adapter is an executable projection.
pub(super) fn serialize<S: Serializer>(
    agents: &Option<BTreeMap<ManagedAgentId, ManagedAgentConfig>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut value = serde_json::to_value(agents).map_err(serde::ser::Error::custom)?;
    if let Some(entries) = value.as_object_mut() {
        for (id, config) in entries {
            if crate::agent_catalog::builtin_agent(id).is_some() {
                if let Some(adapter) = config
                    .get_mut("adapter")
                    .and_then(serde_json::Value::as_object_mut)
                {
                    adapter.remove("command");
                    adapter.remove("args");
                }
            }
        }
    }
    value.serialize(serializer)
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<BTreeMap<ManagedAgentId, ManagedAgentConfig>>, D::Error> {
    let mut value = serde_json::Value::deserialize(deserializer)?;
    if let Some(entries) = value.as_object_mut() {
        for (id, config) in entries {
            if let Some(entry) = crate::agent_catalog::builtin_agent(id) {
                if let Some(adapter) = config
                    .get_mut("adapter")
                    .and_then(serde_json::Value::as_object_mut)
                {
                    adapter.insert("command".into(), serde_json::json!(entry.command));
                    adapter.insert("args".into(), serde_json::json!(entry.args));
                }
            }
        }
    }
    serde_json::from_value(value).map_err(serde::de::Error::custom)
}
