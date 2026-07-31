use super::ScheduledMode;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// The authoring inputs that determine the semantic identity of a scheduled task.
///
/// Execution settings intentionally do not live in this structure. A model,
/// thought level, permission mode, or direct session policy can therefore be
/// changed without changing the content fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskContentInput {
    pub mode: ScheduledMode,
    pub instruction: String,
    #[serde(default)]
    pub attachment_hashes: Vec<String>,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_authoring: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_authoring: Option<AutoAuthoringIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_agent_id: Option<String>,
}

/// Persisted form of [`ScheduledTaskContentInput`].
pub type ScheduledTaskContentSnapshot = ScheduledTaskContentInput;

impl Default for ScheduledTaskContentInput {
    fn default() -> Self {
        Self {
            mode: ScheduledMode::Direct,
            instruction: String::new(),
            attachment_hashes: Vec::new(),
            workspace_id: String::new(),
            workflow_authoring: None,
            auto_authoring: None,
            direct_agent_id: None,
        }
    }
}

impl ScheduledTaskContentInput {
    pub fn new(
        mode: ScheduledMode,
        instruction: impl Into<String>,
        attachment_hashes: impl IntoIterator<Item = impl Into<String>>,
        workspace_id: impl Into<String>,
    ) -> Self {
        Self {
            mode,
            instruction: instruction.into(),
            attachment_hashes: attachment_hashes.into_iter().map(Into::into).collect(),
            workspace_id: workspace_id.into(),
            ..Self::default()
        }
    }

    pub fn direct(
        instruction: impl Into<String>,
        attachment_hashes: impl IntoIterator<Item = impl Into<String>>,
        workspace_id: impl Into<String>,
        direct_agent_id: impl Into<String>,
    ) -> Self {
        let mut input = Self::new(
            ScheduledMode::Direct,
            instruction,
            attachment_hashes,
            workspace_id,
        );
        input.direct_agent_id = Some(direct_agent_id.into());
        input
    }

    pub fn workflow(
        instruction: impl Into<String>,
        attachment_hashes: impl IntoIterator<Item = impl Into<String>>,
        workspace_id: impl Into<String>,
        workflow_authoring: Value,
    ) -> Self {
        let mut input = Self::new(
            ScheduledMode::Workflow,
            instruction,
            attachment_hashes,
            workspace_id,
        );
        input.workflow_authoring = Some(workflow_authoring);
        input
    }

    pub fn auto(
        instruction: impl Into<String>,
        attachment_hashes: impl IntoIterator<Item = impl Into<String>>,
        workspace_id: impl Into<String>,
        auto_authoring: AutoAuthoringIdentity,
    ) -> Self {
        let mut input = Self::new(
            ScheduledMode::Auto,
            instruction,
            attachment_hashes,
            workspace_id,
        );
        input.auto_authoring = Some(auto_authoring);
        input
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoAuthoringIdentity {
    pub agent_strategy: String,
    pub agent_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bootstrap_agent_type: Option<String>,
    #[serde(default)]
    pub available_agent_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_goal: Option<String>,
    #[serde(default)]
    pub allowed_workflow_ids: Vec<String>,
}

impl AutoAuthoringIdentity {
    pub fn new(
        agent_type: impl Into<String>,
        agent_strategy: impl Into<String>,
        bootstrap_agent_type: Option<impl Into<String>>,
        available_agent_types: impl IntoIterator<Item = impl Into<String>>,
        global_goal: Option<impl Into<String>>,
        allowed_workflow_ids: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            agent_strategy: agent_strategy.into(),
            agent_type: agent_type.into(),
            bootstrap_agent_type: bootstrap_agent_type.map(Into::into),
            available_agent_types: available_agent_types.into_iter().map(Into::into).collect(),
            global_goal: global_goal.map(Into::into),
            allowed_workflow_ids: allowed_workflow_ids.into_iter().map(Into::into).collect(),
        }
    }
}

/// Returns the canonical JSON value used for hashing.
pub fn canonical_content_json(input: &ScheduledTaskContentInput) -> Value {
    let mut root = Map::new();
    root.insert(
        "attachmentHashes".to_string(),
        sorted_strings(&input.attachment_hashes),
    );
    root.insert(
        "instruction".to_string(),
        Value::String(input.instruction.clone()),
    );
    root.insert(
        "mode".to_string(),
        serde_json::to_value(input.mode).expect("mode serializes"),
    );
    root.insert(
        "workspaceId".to_string(),
        Value::String(input.workspace_id.clone()),
    );

    match input.mode {
        ScheduledMode::Workflow => {
            if let Some(workflow) = &input.workflow_authoring {
                root.insert(
                    "workflowAuthoring".to_string(),
                    canonical_workflow_authoring(workflow),
                );
            }
        }
        ScheduledMode::Auto => {
            if let Some(auto) = &input.auto_authoring {
                root.insert("autoAuthoring".to_string(), canonical_auto_authoring(auto));
            }
        }
        ScheduledMode::Direct => {
            if let Some(agent) = input.direct_agent_id.as_deref() {
                root.insert(
                    "directAgentId".to_string(),
                    Value::String(agent.trim().to_string()),
                );
            }
        }
    }

    // serde_json's default map is ordered, but constructing a fresh map from a
    // BTreeMap keeps this invariant explicit if the crate is built with its
    // preserve_order feature in another consumer.
    let ordered = root
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    Value::Object(ordered.into_iter().collect())
}

/// Computes a stable SHA-256 identity in the `sha256:<lowercase hex>` format.
pub fn content_fingerprint(input: &ScheduledTaskContentInput) -> anyhow::Result<String> {
    let canonical = canonical_content_json(input);
    let bytes = serde_json::to_vec(&canonical)?;
    let digest = Sha256::digest(bytes);
    Ok(format!("sha256:{digest:x}"))
}

/// Compatibility alias for callers that use an explicit fallible name.
pub fn try_content_fingerprint(input: &ScheduledTaskContentInput) -> anyhow::Result<String> {
    content_fingerprint(input)
}

fn canonical_auto_authoring(identity: &AutoAuthoringIdentity) -> Value {
    let available_agent_types = sorted_strings(&identity.available_agent_types);
    let allowed_workflow_ids = sorted_strings(&identity.allowed_workflow_ids);
    let mut object = Map::new();
    object.insert(
        "agentStrategy".to_string(),
        Value::String(identity.agent_strategy.trim().to_string()),
    );
    object.insert(
        "agentType".to_string(),
        Value::String(identity.agent_type.trim().to_string()),
    );
    object.insert("availableAgentTypes".to_string(), available_agent_types);
    object.insert("allowedWorkflowIds".to_string(), allowed_workflow_ids);
    object.insert(
        "bootstrapAgentType".to_string(),
        identity
            .bootstrap_agent_type
            .as_deref()
            .map(|value| Value::String(value.trim().to_string()))
            .unwrap_or(Value::Null),
    );
    object.insert(
        "globalGoal".to_string(),
        identity
            .global_goal
            .as_deref()
            .map(|value| Value::String(value.trim().to_string()))
            .unwrap_or(Value::Null),
    );
    Value::Object(object)
}

fn sorted_strings(values: &[String]) -> Value {
    let values = values
        .iter()
        .map(|value| value.trim().to_string())
        .collect::<BTreeSet<_>>();
    Value::Array(values.into_iter().map(Value::String).collect())
}

fn canonical_workflow_authoring(value: &Value) -> Value {
    normalize_workflow_value(value, None)
}

fn normalize_workflow_value(value: &Value, parent_key: Option<&str>) -> Value {
    match value {
        Value::Object(object) => {
            let mut normalized = object
                .iter()
                .filter(|(key, _)| !is_execution_option(key))
                .map(|(key, value)| {
                    (
                        key.clone(),
                        normalize_workflow_value(value, Some(key.as_str())),
                    )
                })
                .collect::<Vec<_>>();
            normalized.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(normalized.into_iter().collect())
        }
        Value::Array(values) => {
            let mut normalized = values
                .iter()
                .map(|value| normalize_workflow_value(value, parent_key))
                .collect::<Vec<_>>();
            if matches!(
                parent_key,
                Some("nodes" | "edges" | "allowedWorkflows" | "allowed_workflows")
            ) {
                normalized.sort_by(|left, right| {
                    canonical_json_string(left).cmp(&canonical_json_string(right))
                });
            }
            Value::Array(normalized)
        }
        Value::String(value) => Value::String(value.clone()),
        _ => value.clone(),
    }
}

fn canonical_json_string(value: &Value) -> String {
    serde_json::to_string(value).expect("JSON values serialize")
}

fn is_execution_option(key: &str) -> bool {
    matches!(
        key,
        "model"
            | "modelId"
            | "model_id"
            | "bootstrapModel"
            | "bootstrapModelId"
            | "bootstrap_model"
            | "bootstrap_model_id"
            | "acceptanceModel"
            | "acceptanceModelId"
            | "acceptance_model"
            | "acceptance_model_id"
            | "permission"
            | "permissionMode"
            | "permission_mode"
            | "thoughtLevel"
            | "thought_level"
            | "sessionPolicy"
            | "session_policy"
            | "sessionMode"
            | "session_mode"
            | "configOptions"
            | "config_options"
            | "acpOptions"
            | "acp_options"
            | "executionConfig"
            | "execution_config"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct_input(agent_identity: &str) -> ScheduledTaskContentInput {
        ScheduledTaskContentInput::direct(
            "  inspect the repository  ",
            ["sha256:attachment-a"],
            "workspace-a",
            agent_identity,
        )
    }

    #[test]
    fn workflow_agent_provider_changes_fingerprint() {
        let workflow_a = serde_json::json!({
            "version": "0.1",
            "id": "workflow-a",
            "entry": "worker",
            "nodes": [{
                "type": "worker",
                "id": "worker",
                "provider": "claude-acp",
                "model": "model-a",
                "permissionMode": "ask",
                "profile": "review",
                "goal": "inspect",
                "configOptions": {"temperature": "0"}
            }],
            "edges": [{"from": "worker", "to": "__end__", "on": "success"}],
            "control": {"maxAttempts": 2}
        });
        let workflow_b = {
            let mut value = workflow_a.clone();
            value["nodes"][0]["provider"] = serde_json::json!("codex-acp");
            value
        };
        let input_a = ScheduledTaskContentInput::workflow(
            "run the workflow",
            Vec::<String>::new(),
            "workspace-a",
            workflow_a,
        );
        let input_b = ScheduledTaskContentInput::workflow(
            "run the workflow",
            Vec::<String>::new(),
            "workspace-a",
            workflow_b,
        );

        assert_ne!(
            content_fingerprint(&input_a).unwrap(),
            content_fingerprint(&input_b).unwrap()
        );
    }

    #[test]
    fn auto_strategy_and_available_provider_changes_fingerprint() {
        let input_a = ScheduledTaskContentInput::auto(
            "choose an agent",
            Vec::<String>::new(),
            "workspace-a",
            AutoAuthoringIdentity::new(
                "claude-acp",
                "dynamic",
                Some("codex-acp"),
                ["gemini", "claude-acp", "gemini"],
                Some("finish the goal"),
                ["workflow-b", "workflow-a"],
            ),
        );
        let input_b = ScheduledTaskContentInput::auto(
            "choose an agent",
            Vec::<String>::new(),
            "workspace-a",
            AutoAuthoringIdentity::new(
                "claude-acp",
                "fixed",
                Some("codex-acp"),
                ["gemini", "claude-acp"],
                Some("finish the goal"),
                ["workflow-a", "workflow-b"],
            ),
        );

        assert_ne!(
            content_fingerprint(&input_a).unwrap(),
            content_fingerprint(&input_b).unwrap()
        );
    }

    #[test]
    fn direct_agent_attachment_workspace_and_instruction_changes_fingerprint() {
        let baseline = direct_input("claude-acp");
        let mut changed_agent = baseline.clone();
        changed_agent.direct_agent_id = Some("codex-acp".to_string());
        let mut changed_attachment = baseline.clone();
        changed_attachment.attachment_hashes = vec!["sha256:attachment-b".to_string()];
        let mut changed_workspace = baseline.clone();
        changed_workspace.workspace_id = "workspace-b".to_string();
        let mut changed_instruction = baseline.clone();
        changed_instruction.instruction = "inspect a different thing".to_string();

        let original = content_fingerprint(&baseline).unwrap();
        assert_ne!(original, content_fingerprint(&changed_agent).unwrap());
        assert_ne!(original, content_fingerprint(&changed_attachment).unwrap());
        assert_ne!(original, content_fingerprint(&changed_workspace).unwrap());
        assert_ne!(original, content_fingerprint(&changed_instruction).unwrap());
    }

    #[test]
    fn execution_settings_are_absent_from_canonical_identity() {
        let input_a = direct_input("claude-acp");
        let mut input_b = input_a.clone();
        input_b.workflow_authoring = Some(serde_json::json!({
            "nodes": [{
                "id": "worker",
                "provider": "claude-acp",
                "model": "model-a",
                "thoughtLevel": "high",
                "permissionMode": "bypass",
                "sessionPolicy": "continuous",
                "configOptions": {"foo": "bar"}
            }]
        }));
        input_b.mode = ScheduledMode::Workflow;
        input_b.direct_agent_id = None;

        let mut input_c = input_a.clone();
        input_c.workflow_authoring = Some(serde_json::json!({
            "nodes": [{
                "id": "worker",
                "provider": "claude-acp",
                "model": "model-b",
                "thoughtLevel": "low",
                "permissionMode": "ask",
                "sessionPolicy": "new",
                "configOptions": {"foo": "baz"}
            }]
        }));
        input_c.mode = ScheduledMode::Workflow;
        input_c.direct_agent_id = None;

        assert_eq!(
            content_fingerprint(&input_b).unwrap(),
            content_fingerprint(&input_c).unwrap()
        );
        let canonical = canonical_content_json(&input_b).to_string();
        for excluded in ["model-a", "high", "bypass", "continuous", "foo"] {
            assert!(
                !canonical.contains(excluded),
                "canonical identity leaked {excluded}"
            );
        }
    }

    #[test]
    fn provider_and_workflow_order_is_normalized() {
        let first = ScheduledTaskContentInput::auto(
            "goal",
            ["sha256:z", "sha256:a"],
            "workspace",
            AutoAuthoringIdentity::new(
                "agent",
                "dynamic",
                Some("bootstrap"),
                ["provider-b", "provider-a", "provider-b"],
                Some("goal"),
                ["workflow-b", "workflow-a", "workflow-b"],
            ),
        );
        let second = ScheduledTaskContentInput::auto(
            "goal",
            ["sha256:a", "sha256:z"],
            "workspace",
            AutoAuthoringIdentity::new(
                "agent",
                "dynamic",
                Some("bootstrap"),
                ["provider-a", "provider-b"],
                Some("goal"),
                ["workflow-a", "workflow-b"],
            ),
        );

        assert_eq!(
            canonical_content_json(&first),
            canonical_content_json(&second)
        );
        assert_eq!(
            content_fingerprint(&first).unwrap(),
            content_fingerprint(&second).unwrap()
        );
    }
}
