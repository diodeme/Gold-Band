use crate::domain::{NodeOutcome, PauseReason, RunOutcome, SessionMode, VERSION};
use crate::dsl::{DynamicControlDsl, WorkflowDsl};
use anyhow::{Result, ensure};
use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const DYNAMIC_COMPLETION_ARTIFACT: &str = "dynamic-node-completion";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicRunStatus {
    Running,
    Paused,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicNodeKind {
    Worker,
    WorkflowInvocation,
    Merge,
    Acceptance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicNodeStatus {
    Pending,
    Ready,
    Running,
    Paused,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicGroupStatus {
    Open,
    MergeReady,
    Merging,
    Merged,
    Accepting,
    Accepted,
    Closed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceMode {
    Readonly,
    Worktree,
    Main,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePolicy {
    pub mode: WorkspaceMode,
}

impl Default for WorkspacePolicy {
    fn default() -> Self {
        Self {
            mode: WorkspaceMode::Readonly,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowedWorkflowSnapshot {
    pub workflow_id: String,
    pub snapshot_id: String,
    pub name: String,
    pub contains_ai_dynamic: bool,
    pub workflow: WorkflowDsl,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicRunState {
    pub version: String,
    pub id: String,
    pub parent_run_id: String,
    pub parent_round_id: String,
    pub parent_node_id: String,
    pub parent_attempt_id: String,
    pub status: DynamicRunStatus,
    pub outcome: Option<RunOutcome>,
    #[serde(default)]
    pub pause_reason: Option<PauseReason>,
    pub started_at: String,
    pub updated_at: String,
    pub control: DynamicControlDsl,
    pub allowed_workflow_snapshots: Vec<AllowedWorkflowSnapshot>,
    pub current_node_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicNodeState {
    pub version: String,
    pub id: String,
    pub dynamic_run_id: String,
    pub kind: DynamicNodeKind,
    pub title: String,
    pub task: String,
    pub status: DynamicNodeStatus,
    pub outcome: Option<NodeOutcome>,
    pub group_id: Option<String>,
    pub chain_id: String,
    pub depth: u32,
    pub depends_on: Vec<String>,
    pub workspace: WorkspacePolicy,
    pub workspace_path: Option<Utf8PathBuf>,
    pub provider: Option<String>,
    pub profile: Option<String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    pub session_mode: SessionMode,
    pub continue_from_node_id: Option<String>,
    pub workflow_id: Option<String>,
    pub workflow_snapshot_id: Option<String>,
    pub child_run_id: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicGroupState {
    pub version: String,
    pub id: String,
    pub dynamic_run_id: String,
    pub status: DynamicGroupStatus,
    pub depth: u32,
    pub parent_group_id: Option<String>,
    pub root_node_ids: Vec<String>,
    pub terminal_node_ids: Vec<String>,
    pub merge_node_id: Option<String>,
    pub acceptance_node_id: Option<String>,
    pub created_by_node_id: String,
    pub merge: DynamicAgentTaskSpec,
    pub acceptance: DynamicAgentTaskSpec,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
#[serde(rename_all = "camelCase")]
pub struct DynamicProposalValidationError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_values: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(default)]
    pub params: serde_json::Value,
}

impl DynamicProposalValidationError {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        params: serde_json::Value,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            path: None,
            actual: None,
            expected: None,
            allowed_values: Vec::new(),
            suggestion: None,
            params,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicProposalState {
    pub version: String,
    pub id: String,
    pub dynamic_run_id: String,
    pub source_node_id: String,
    pub artifact_path: Utf8PathBuf,
    pub raw_output_path: Utf8PathBuf,
    pub parsed: serde_json::Value,
    pub validation_status: DynamicProposalValidationStatus,
    pub validation_errors: Vec<DynamicProposalValidationError>,
    pub materialized_event_ids: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicProposalValidationStatus {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicGraphState {
    pub version: String,
    pub run: DynamicRunState,
    pub nodes: Vec<DynamicNodeState>,
    pub groups: Vec<DynamicGroupState>,
    pub proposals: Vec<DynamicProposalState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DynamicNodeSpec {
    pub id: String,
    pub kind: DynamicNodeSpecKind,
    pub title: String,
    pub task: String,
    pub provider: Option<String>,
    pub profile: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub session_mode: SessionMode,
    #[serde(default)]
    pub continue_from_node_id: Option<String>,
    #[serde(default)]
    pub workspace: WorkspacePolicy,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub workflow_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicNodeSpecKind {
    Worker,
    WorkflowInvocation,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DynamicAgentTaskSpec {
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub task: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DynamicNodeCompletion {
    pub version: String,
    pub kind: DynamicNodeCompletionKind,
    pub status: DynamicCompletionStatus,
    pub summary: String,
    pub next: DynamicNext,
    #[serde(default)]
    pub source: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicNodeCompletionKind {
    DynamicNodeCompletion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicCompletionStatus {
    Success,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum DynamicNext {
    End,
    Single {
        node: DynamicNodeSpec,
    },
    Fanout {
        #[serde(rename = "groupId")]
        group_id: String,
        nodes: Vec<DynamicNodeSpec>,
        merge: DynamicAgentTaskSpec,
        acceptance: DynamicAgentTaskSpec,
    },
}

impl Default for SessionMode {
    fn default() -> Self {
        Self::New
    }
}

pub fn dynamic_completion_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(DynamicNodeCompletion))
        .expect("dynamic completion JSON schema serializes")
}

#[derive(Debug, Clone, Default)]
pub struct DynamicCompletionSchemaPolicy {
    pub provider_required: bool,
    pub node_model_required: bool,
    pub agent_task_model_required: bool,
    pub agent_task_model_visible: bool,
    pub provider_ids: Vec<String>,
    pub model_names: Vec<String>,
    pub profile_ids: Vec<String>,
    pub workflow_ids: Vec<String>,
    pub max_fanout: u32,
}

pub fn dynamic_completion_effective_schema(
    policy: &DynamicCompletionSchemaPolicy,
) -> serde_json::Value {
    let mut schema = dynamic_completion_schema();
    patch_dynamic_completion_root_schema(&mut schema);
    reset_schema_definitions(&mut schema);
    set_schema_definition(
        &mut schema,
        "SessionMode",
        enum_string_schema(["new", "continue"]),
    );
    set_schema_definition(
        &mut schema,
        "WorkspaceMode",
        enum_string_schema(["readonly", "worktree", "main"]),
    );
    set_schema_definition(&mut schema, "WorkspacePolicy", workspace_policy_schema());
    set_schema_definition(
        &mut schema,
        "DynamicNodeSpec",
        dynamic_node_spec_schema(policy),
    );
    set_schema_definition(
        &mut schema,
        "DynamicAgentTaskSpec",
        dynamic_agent_task_spec_schema(policy),
    );
    set_schema_definition(&mut schema, "DynamicNext", dynamic_next_schema(policy));
    schema
}

fn patch_dynamic_completion_root_schema(schema: &mut serde_json::Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };
    object.insert(
        "required".to_string(),
        serde_json::json!(["version", "kind", "status", "summary", "next"]),
    );
    object.insert("additionalProperties".to_string(), serde_json::json!(false));
    let Some(properties) = object
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    properties.remove("source");
    properties.insert(
        "version".to_string(),
        enum_string_schema([VERSION.to_string()]),
    );
    properties.insert(
        "kind".to_string(),
        enum_string_schema([DYNAMIC_COMPLETION_ARTIFACT.to_string()]),
    );
    properties.insert("status".to_string(), enum_string_schema(["success"]));
}

fn schema_definitions_mut(
    schema: &mut serde_json::Value,
) -> Option<&mut serde_json::Map<String, serde_json::Value>> {
    if schema.get("definitions").is_some() {
        schema
            .get_mut("definitions")
            .and_then(serde_json::Value::as_object_mut)
    } else {
        schema
            .get_mut("$defs")
            .and_then(serde_json::Value::as_object_mut)
    }
}

fn reset_schema_definitions(schema: &mut serde_json::Value) {
    if let Some(definitions) = schema_definitions_mut(schema) {
        definitions.clear();
    }
}

fn set_schema_definition(schema: &mut serde_json::Value, name: &str, value: serde_json::Value) {
    if let Some(definitions) = schema_definitions_mut(schema) {
        definitions.insert(name.to_string(), value);
    }
}

fn schema_ref(name: &str) -> serde_json::Value {
    serde_json::json!({
        "$ref": format!("#/definitions/{name}")
    })
}

fn string_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "string",
        "minLength": 1
    })
}

fn enum_string_schema(values: impl IntoIterator<Item = impl Into<String>>) -> serde_json::Value {
    let values = values.into_iter().map(Into::into).collect::<Vec<String>>();
    serde_json::json!({
        "type": "string",
        "enum": values
    })
}

fn optional_enum_or_string_schema(values: &[String]) -> serde_json::Value {
    if values.is_empty() {
        string_schema()
    } else {
        enum_string_schema(values.iter().cloned())
    }
}

fn forbidden_properties_schema(fields: &[&str]) -> serde_json::Value {
    let properties = fields
        .iter()
        .map(|field| ((*field).to_string(), serde_json::json!(false)))
        .collect::<serde_json::Map<_, _>>();
    serde_json::json!({ "properties": properties })
}

fn conditional_schema(
    discriminator_field: &str,
    discriminator_value: &str,
    required: &[&str],
    forbidden: &[&str],
) -> serde_json::Value {
    let mut discriminator_properties = serde_json::Map::new();
    discriminator_properties.insert(
        discriminator_field.to_string(),
        serde_json::json!({ "enum": [discriminator_value] }),
    );
    let if_schema = serde_json::json!({
        "required": [discriminator_field],
        "properties": discriminator_properties,
    });
    let mut then_schema = serde_json::Map::new();
    if !required.is_empty() {
        then_schema.insert("required".to_string(), serde_json::json!(required));
    }
    if !forbidden.is_empty() {
        then_schema.insert(
            "properties".to_string(),
            forbidden_properties_schema(forbidden)
                .get("properties")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        );
    }
    serde_json::json!({
        "if": if_schema,
        "then": serde_json::Value::Object(then_schema)
    })
}

fn workspace_policy_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["mode"],
        "additionalProperties": false,
        "properties": {
            "mode": schema_ref("WorkspaceMode")
        }
    })
}

fn dynamic_node_spec_schema(policy: &DynamicCompletionSchemaPolicy) -> serde_json::Value {
    let mut worker_required = vec!["id", "kind", "title", "task"];
    if policy.provider_required {
        worker_required.push("provider");
    }
    if policy.node_model_required {
        worker_required.push("model");
    }
    let mut worker_forbidden = vec!["workflowId", "permissionMode"];
    if !policy.provider_required {
        worker_forbidden.push("provider");
    }
    if !policy.node_model_required {
        worker_forbidden.push("model");
    }
    serde_json::json!({
        "type": "object",
        "required": ["id", "kind", "title", "task"],
        "additionalProperties": false,
        "properties": {
            "id": string_schema(),
            "kind": enum_string_schema(["worker", "workflow-invocation"]),
            "title": string_schema(),
            "task": string_schema(),
            "provider": optional_enum_or_string_schema(&policy.provider_ids),
            "profile": optional_enum_or_string_schema(&policy.profile_ids),
            "model": optional_enum_or_string_schema(&policy.model_names),
            "sessionMode": schema_ref("SessionMode"),
            "continueFromNodeId": string_schema(),
            "workspace": schema_ref("WorkspacePolicy"),
            "dependsOn": {
                "type": "array",
                "items": string_schema()
            },
            "workflowId": optional_enum_or_string_schema(&policy.workflow_ids)
        },
        "allOf": [
            conditional_schema(
                "kind",
                "worker",
                &worker_required,
                &worker_forbidden,
            ),
            conditional_schema(
                "kind",
                "workflow-invocation",
                &["id", "kind", "title", "task", "workflowId"],
                &["provider", "profile", "model", "permissionMode"],
            )
        ]
    })
}

fn dynamic_agent_task_spec_schema(policy: &DynamicCompletionSchemaPolicy) -> serde_json::Value {
    let mut required = vec!["title", "task"];
    if policy.provider_required {
        required.push("provider");
    }
    if policy.agent_task_model_required {
        required.push("model");
    }
    let mut forbidden = Vec::new();
    if !policy.provider_required {
        forbidden.push("provider");
    }
    if policy.agent_task_model_visible && !policy.agent_task_model_required {
        forbidden.push("model");
    }
    let mut properties = serde_json::Map::from_iter([
        ("title".to_string(), string_schema()),
        (
            "provider".to_string(),
            optional_enum_or_string_schema(&policy.provider_ids),
        ),
        ("task".to_string(), string_schema()),
    ]);
    if policy.agent_task_model_visible {
        properties.insert(
            "model".to_string(),
            optional_enum_or_string_schema(&policy.model_names),
        );
    }
    let mut schema = serde_json::json!({
        "type": "object",
        "required": required,
        "additionalProperties": false,
        "properties": properties
    });
    if !forbidden.is_empty() {
        if let Some(object) = schema.as_object_mut() {
            object.insert(
                "allOf".to_string(),
                serde_json::json!([forbidden_properties_schema(&forbidden)]),
            );
        }
    }
    schema
}

fn dynamic_next_schema(policy: &DynamicCompletionSchemaPolicy) -> serde_json::Value {
    let max_items = u64::from(policy.max_fanout.max(1));
    serde_json::json!({
        "type": "object",
        "required": ["type"],
        "additionalProperties": false,
        "properties": {
            "type": enum_string_schema(["end", "single", "fanout"]),
            "node": schema_ref("DynamicNodeSpec"),
            "groupId": string_schema(),
            "nodes": {
                "type": "array",
                "minItems": 1,
                "maxItems": max_items,
                "items": schema_ref("DynamicNodeSpec")
            },
            "merge": schema_ref("DynamicAgentTaskSpec"),
            "acceptance": schema_ref("DynamicAgentTaskSpec")
        },
        "allOf": [
            conditional_schema(
                "type",
                "end",
                &["type"],
                &["node", "groupId", "nodes", "merge", "acceptance"],
            ),
            conditional_schema(
                "type",
                "single",
                &["type", "node"],
                &["groupId", "nodes", "merge", "acceptance"],
            ),
            conditional_schema(
                "type",
                "fanout",
                &["type", "groupId", "nodes", "merge", "acceptance"],
                &["node"],
            )
        ]
    })
}

pub fn validate_dynamic_run_state(state: &DynamicRunState) -> Result<()> {
    ensure!(state.version == VERSION, "unsupported dynamic run version");
    ensure!(
        !state.id.trim().is_empty(),
        "dynamic run id cannot be empty"
    );
    ensure!(
        !state.parent_run_id.trim().is_empty(),
        "dynamic run parentRunId cannot be empty"
    );
    ensure!(
        !(state.status != DynamicRunStatus::Completed && state.outcome.is_some()),
        "non-completed dynamic run cannot have outcome"
    );
    ensure!(
        !(state.status == DynamicRunStatus::Completed && state.outcome.is_none()),
        "completed dynamic run must have outcome"
    );
    ensure!(
        !(state.status != DynamicRunStatus::Paused && state.pause_reason.is_some()),
        "non-paused dynamic run cannot have pauseReason"
    );
    Ok(())
}

pub fn validate_dynamic_node_state(state: &DynamicNodeState) -> Result<()> {
    ensure!(state.version == VERSION, "unsupported dynamic node version");
    ensure!(
        !state.id.trim().is_empty(),
        "dynamic node id cannot be empty"
    );
    ensure!(
        !state.dynamic_run_id.trim().is_empty(),
        "dynamic node dynamicRunId cannot be empty"
    );
    ensure!(
        !state.title.trim().is_empty(),
        "dynamic node title cannot be empty"
    );
    ensure!(
        !state.task.trim().is_empty(),
        "dynamic node task cannot be empty"
    );
    ensure!(
        !(state.status != DynamicNodeStatus::Completed && state.outcome.is_some()),
        "non-completed dynamic node cannot have outcome"
    );
    ensure!(
        !(state.status == DynamicNodeStatus::Completed && state.outcome.is_none()),
        "completed dynamic node must have outcome"
    );
    Ok(())
}

pub fn validate_dynamic_group_state(state: &DynamicGroupState) -> Result<()> {
    ensure!(
        state.version == VERSION,
        "unsupported dynamic group version"
    );
    ensure!(
        !state.id.trim().is_empty(),
        "dynamic group id cannot be empty"
    );
    if let Some(parent_group_id) = state.parent_group_id.as_deref() {
        ensure!(
            !parent_group_id.trim().is_empty(),
            "dynamic group parentGroupId cannot be empty"
        );
        ensure!(
            parent_group_id != state.id,
            "dynamic group cannot reference itself as parent"
        );
    }
    ensure!(
        !state.root_node_ids.is_empty(),
        "dynamic group must have root nodes"
    );
    Ok(())
}
