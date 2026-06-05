use crate::domain::{NodeType, SessionMode};
use crate::provider::supports_continue_session;
use anyhow::{Result, anyhow, bail, ensure};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;

pub const END_NODE: &str = "$end";
pub const NEW_ROUND_NODE: &str = "$new-round";
const RESERVED_NODE_IDS: &[&str] = &[END_NODE, NEW_ROUND_NODE];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WorkflowValidationError {
    #[error("workflow must include an edge targeting `$end`")]
    MissingEndNode,
    #[error("node `{node_id}` is unreachable from entry")]
    UnreachableNode { node_id: String },
    #[error("edge `{from}` cannot target `$new-round` on success")]
    SuccessNewRoundTarget { from: String },
    #[error("workflow `{workflow_name}` id `{workflow_id}` is duplicated with {conflicts}")]
    DuplicateWorkflowId {
        workflow_name: String,
        workflow_id: String,
        conflicts: String,
    },
    #[error("ai-dynamic node `{node_id}` references invalid workflow `{workflow_name}`: {reason}")]
    AiDynamicInvalidWorkflow {
        node_id: String,
        workflow_name: String,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonPathSegment {
    Key(String),
    Index(usize),
}

pub fn parse_json_path(path: &str) -> Result<Vec<JsonPathSegment>> {
    let mut value = path.trim();
    if let Some(rest) = value.strip_prefix("$.") {
        value = rest;
    } else if value == "$" {
        bail!("json path cannot be root only");
    } else if let Some(rest) = value.strip_prefix('$') {
        value = rest;
    }
    ensure!(!value.is_empty(), "json path cannot be empty");

    let chars = value.chars().collect::<Vec<_>>();
    let mut segments = Vec::new();
    let mut key = String::new();
    let mut index = 0;

    while index < chars.len() {
        match chars[index] {
            '.' => {
                if key.is_empty() {
                    ensure!(
                        matches!(segments.last(), Some(JsonPathSegment::Index(_))),
                        "json path contains an empty segment: {path}"
                    );
                } else {
                    segments.push(JsonPathSegment::Key(std::mem::take(&mut key)));
                }
                index += 1;
            }
            '[' => {
                if !key.is_empty() {
                    segments.push(JsonPathSegment::Key(std::mem::take(&mut key)));
                }
                index += 1;
                let start = index;
                while index < chars.len() && chars[index] != ']' {
                    ensure!(
                        chars[index].is_ascii_digit(),
                        "json path array index must be numeric: {path}"
                    );
                    index += 1;
                }
                ensure!(
                    index < chars.len() && chars[index] == ']',
                    "json path array index is not closed: {path}"
                );
                ensure!(
                    index > start,
                    "json path array index cannot be empty: {path}"
                );
                let array_index = chars[start..index]
                    .iter()
                    .collect::<String>()
                    .parse::<usize>()?;
                segments.push(JsonPathSegment::Index(array_index));
                index += 1;
                if index < chars.len() && chars[index] != '.' && chars[index] != '[' {
                    bail!("json path segment must be separated by `.` or array index: {path}");
                }
            }
            character => {
                key.push(character);
                index += 1;
            }
        }
    }

    ensure!(
        !key.is_empty() || matches!(segments.last(), Some(JsonPathSegment::Index(_))),
        "json path cannot end with an empty segment: {path}"
    );
    if !key.is_empty() {
        segments.push(JsonPathSegment::Key(key));
    }
    ensure!(!segments.is_empty(), "json path cannot be empty");
    Ok(segments)
}

pub fn parse_success_expression_path(expression: &str) -> Result<Vec<JsonPathSegment>> {
    const OPERATORS: [&str; 6] = [">=", "<=", "!=", "==", ">", "<"];
    let trimmed = expression.trim();
    let (_, left, _) = OPERATORS
        .iter()
        .find_map(|operator| {
            trimmed
                .split_once(operator)
                .map(|(left, right)| (*operator, left.trim(), right.trim()))
        })
        .ok_or_else(|| anyhow!("unsupported success expression: {expression}"))?;
    ensure!(
        left.starts_with('$'),
        "success expression left side must start with `$`: {expression}"
    );
    parse_json_path(left)
}

pub fn simple_schema_contains_path(schema: &serde_json::Value, path: &[JsonPathSegment]) -> bool {
    let mut cursor = schema;
    for segment in path {
        match segment {
            JsonPathSegment::Key(key) => {
                let Some(object) = cursor.as_object() else {
                    return false;
                };
                let Some(next) = object.get(key) else {
                    return false;
                };
                cursor = next;
            }
            JsonPathSegment::Index(index) => {
                let Some(array) = cursor.as_array() else {
                    return false;
                };
                let Some(next) = array.get(*index).or_else(|| array.first()) else {
                    return false;
                };
                cursor = next;
            }
        }
    }
    true
}

pub fn looks_like_json_schema(schema: &serde_json::Value) -> bool {
    schema.as_object().is_some_and(|object| {
        [
            "type",
            "properties",
            "required",
            "additionalProperties",
            "items",
        ]
        .iter()
        .any(|key| object.contains_key(*key))
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDsl {
    pub version: String,
    pub id: String,
    pub entry: String,
    #[serde(default)]
    pub control: WorkflowControl,
    pub nodes: Vec<NodeDsl>,
    pub edges: Vec<EdgeDsl>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowControl {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rounds: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum NodeDsl {
    Worker(WorkerNode),
    AiDynamic(AiDynamicNode),
}

impl NodeDsl {
    pub fn id(&self) -> &str {
        match self {
            Self::Worker(node) => &node.id,
            Self::AiDynamic(node) => &node.id,
        }
    }

    pub fn node_type(&self) -> NodeType {
        match self {
            Self::Worker(_) => NodeType::Worker,
            Self::AiDynamic(_) => NodeType::AiDynamic,
        }
    }

    pub fn provider(&self) -> Option<&str> {
        match self {
            Self::Worker(node) => node.provider.as_deref(),
            Self::AiDynamic(node) => node.bootstrap_provider(),
        }
    }

    pub fn profile(&self) -> Option<&str> {
        match self {
            Self::Worker(node) => node.profile.as_deref(),
            Self::AiDynamic(_) => None,
        }
    }

    pub fn manual_check_enabled(&self) -> bool {
        match self {
            Self::Worker(node) => node.manual_check.unwrap_or(false),
            Self::AiDynamic(_) => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerNode {
    pub id: String,
    pub provider: Option<String>,
    pub profile: Option<String>,
    pub goal: Option<String>,
    pub output: Option<OutputContractDsl>,
    pub success_condition: Option<JsonConditionDsl>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub manual_check: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDynamicNode {
    pub id: String,
    pub agent_strategy: AiDynamicAgentStrategy,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "permission_mode",
        alias = "permissionMode"
    )]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_profiles: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_goal: Option<String>,
    #[serde(default)]
    pub control: DynamicControlDsl,
    #[serde(default)]
    pub allowed_workflows: Vec<AllowedWorkflowRefDsl>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiDynamicNodeCompat {
    pub id: String,
    #[serde(default)]
    pub agent_strategy: Option<AiDynamicAgentStrategy>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default, rename = "permission_mode", alias = "permissionMode")]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub allowed_profiles: Vec<String>,
    #[serde(default)]
    pub global_goal: Option<String>,
    #[serde(default)]
    pub control: DynamicControlDsl,
    #[serde(default)]
    pub allowed_workflows: Vec<AllowedWorkflowRefDsl>,
}

impl<'de> Deserialize<'de> for AiDynamicNode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = AiDynamicNodeCompat::deserialize(deserializer)?;
        let agent_strategy = match raw.agent_strategy {
            Some(strategy) => strategy,
            None => {
                let provider = raw
                    .provider
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| serde::de::Error::missing_field("agentStrategy"))?;
                AiDynamicAgentStrategy::Fixed { provider }
            }
        };
        Ok(Self {
            id: raw.id,
            agent_strategy,
            permission_mode: raw
                .permission_mode
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            allowed_profiles: raw
                .allowed_profiles
                .into_iter()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect(),
            global_goal: raw
                .global_goal
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            control: raw.control,
            allowed_workflows: raw.allowed_workflows,
        })
    }
}

impl AiDynamicNode {
    pub fn bootstrap_provider(&self) -> Option<&str> {
        match &self.agent_strategy {
            AiDynamicAgentStrategy::Fixed { provider } => Some(provider.as_str()),
            AiDynamicAgentStrategy::Dynamic {
                bootstrap_provider, ..
            } => Some(bootstrap_provider.as_str()),
        }
    }

    pub fn permission_mode(&self) -> Option<&str> {
        self.permission_mode.as_deref()
    }

    pub fn global_goal(&self) -> Option<&str> {
        self.global_goal.as_deref()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum AiDynamicAgentStrategy {
    #[serde(rename_all = "camelCase")]
    Fixed {
        provider: String,
    },
    #[serde(rename_all = "camelCase")]
    Dynamic {
        bootstrap_provider: String,
        routing_prompt: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicControlDsl {
    #[serde(default = "default_max_dynamic_nodes")]
    pub max_dynamic_nodes: u32,
    #[serde(default = "default_max_fanout")]
    pub max_fanout: u32,
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,
    #[serde(default = "default_max_parallel")]
    pub max_parallel: u32,
    #[serde(default = "default_max_group_depth")]
    pub max_group_depth: u32,
    #[serde(default = "default_max_workflow_invocations")]
    pub max_workflow_invocations: u32,
    #[serde(default)]
    pub allow_nested_dynamic: bool,
}

impl Default for DynamicControlDsl {
    fn default() -> Self {
        Self {
            max_dynamic_nodes: default_max_dynamic_nodes(),
            max_fanout: default_max_fanout(),
            max_depth: default_max_depth(),
            max_parallel: default_max_parallel(),
            max_group_depth: default_max_group_depth(),
            max_workflow_invocations: default_max_workflow_invocations(),
            allow_nested_dynamic: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowedWorkflowRefDsl {
    pub workflow_id: String,
}

fn default_max_dynamic_nodes() -> u32 {
    20
}
fn default_max_fanout() -> u32 {
    5
}
fn default_max_depth() -> u32 {
    6
}
fn default_max_parallel() -> u32 {
    3
}
fn default_max_group_depth() -> u32 {
    1
}
fn default_max_workflow_invocations() -> u32 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputContractDsl {
    pub kind: OutputKind,
    pub artifact: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputKind {
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonConditionDsl {
    Expression {
        expression: String,
    },
    PathEquals {
        path: String,
        equals: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeDsl {
    pub from: String,
    pub to: String,
    pub on: EdgeOutcome,
    pub session: Option<SessionMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EdgeOutcome {
    Success,
    Failure,
}

#[derive(Debug, Clone)]
pub struct ValidatedWorkflow {
    pub raw: WorkflowDsl,
    pub nodes_by_id: IndexMap<String, NodeDsl>,
}

pub fn workflow_contains_ai_dynamic(workflow: &WorkflowDsl) -> bool {
    workflow
        .nodes
        .iter()
        .any(|node| matches!(node, NodeDsl::AiDynamic(_)))
}

impl ValidatedWorkflow {
    pub fn get_node(&self, id: &str) -> Option<&NodeDsl> {
        self.nodes_by_id.get(id)
    }
}

fn validate_worker_node(worker: &WorkerNode, id: &str) -> Result<()> {
    let provider = worker
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("worker node `{id}` provider cannot be blank"))?;
    ensure!(
        !provider.is_empty(),
        "worker node `{id}` provider cannot be blank"
    );
    if let Some(profile) = &worker.profile {
        ensure!(
            !profile.trim().is_empty(),
            "worker node `{id}` profile cannot be blank"
        );
    }
    if let Some(permission_mode) = &worker.permission_mode {
        ensure!(
            !permission_mode.trim().is_empty(),
            "worker node `{id}` permission_mode cannot be blank"
        );
    }
    ensure!(
        !worker.manual_check.unwrap_or(false)
            || (worker.output.is_none() && worker.success_condition.is_none()),
        "worker node `{id}` cannot enable manual_check together with output validation"
    );
    if let Some(output) = &worker.output {
        ensure!(
            !output.artifact.trim().is_empty(),
            "worker node `{id}` output artifact cannot be blank"
        );
        if let Some(schema) = &output.schema {
            ensure!(
                !looks_like_json_schema(schema),
                "worker node `{id}` output schema must use simplified output shape instead of JSON Schema"
            );
        }
    }
    if let Some(condition) = &worker.success_condition {
        ensure!(
            worker
                .output
                .as_ref()
                .is_some_and(|output| output.kind == OutputKind::Json),
            "worker node `{id}` success_condition requires json output"
        );
        let path = match condition {
            JsonConditionDsl::Expression { expression } => {
                ensure!(
                    !expression.trim().is_empty(),
                    "worker node `{id}` success_condition expression cannot be blank"
                );
                parse_success_expression_path(expression)?
            }
            JsonConditionDsl::PathEquals { path, .. } => {
                ensure!(
                    !path.trim().is_empty(),
                    "worker node `{id}` success_condition path cannot be blank"
                );
                parse_json_path(path)?
            }
        };
        if let Some(schema) = worker
            .output
            .as_ref()
            .and_then(|output| output.schema.as_ref())
        {
            ensure!(
                simple_schema_contains_path(schema, &path),
                "worker node `{id}` success_condition path is not declared in output schema"
            );
        }
    }
    Ok(())
}

fn validate_ai_dynamic_node(node: &AiDynamicNode, id: &str) -> Result<()> {
    match &node.agent_strategy {
        AiDynamicAgentStrategy::Fixed { provider } => {
            ensure!(
                !provider.trim().is_empty(),
                "ai-dynamic node `{id}` fixed provider cannot be blank"
            );
        }
        AiDynamicAgentStrategy::Dynamic {
            bootstrap_provider,
            routing_prompt,
        } => {
            ensure!(
                !bootstrap_provider.trim().is_empty(),
                "ai-dynamic node `{id}` bootstrapProvider cannot be blank"
            );
            ensure!(
                !routing_prompt.trim().is_empty(),
                "ai-dynamic node `{id}` routingPrompt cannot be blank"
            );
        }
    }
    if let Some(permission_mode) = &node.permission_mode {
        ensure!(
            !permission_mode.trim().is_empty(),
            "ai-dynamic node `{id}` permissionMode cannot be blank"
        );
    }
    ensure!(
        node.control.max_dynamic_nodes > 0,
        "ai-dynamic node `{id}` maxDynamicNodes must be positive"
    );
    ensure!(
        node.control.max_fanout > 0,
        "ai-dynamic node `{id}` maxFanout must be positive"
    );
    ensure!(
        node.control.max_depth > 0,
        "ai-dynamic node `{id}` maxDepth must be positive"
    );
    ensure!(
        node.control.max_parallel > 0,
        "ai-dynamic node `{id}` maxParallel must be positive"
    );
    ensure!(
        node.control.max_group_depth > 0,
        "ai-dynamic node `{id}` maxGroupDepth must be positive"
    );
    ensure!(
        node.control.max_workflow_invocations > 0,
        "ai-dynamic node `{id}` maxWorkflowInvocations must be positive"
    );
    let mut profiles = HashSet::new();
    for profile in &node.allowed_profiles {
        let profile_id = profile.trim();
        ensure!(
            !profile_id.is_empty(),
            "ai-dynamic node `{id}` allowed profile cannot be blank"
        );
        ensure!(
            profiles.insert(profile_id.to_string()),
            "ai-dynamic node `{id}` allowed profile `{profile_id}` is duplicated"
        );
    }
    if let Some(global_goal) = &node.global_goal {
        ensure!(
            !global_goal.trim().is_empty(),
            "ai-dynamic node `{id}` globalGoal cannot be blank"
        );
    }
    let mut workflows = HashSet::new();
    for allowed in &node.allowed_workflows {
        let workflow_id = allowed.workflow_id.trim();
        ensure!(
            !workflow_id.is_empty(),
            "ai-dynamic node `{id}` allowed workflow id cannot be blank"
        );
        ensure!(
            workflows.insert(workflow_id.to_string()),
            "ai-dynamic node `{id}` allowed workflow `{workflow_id}` is duplicated"
        );
    }
    Ok(())
}

pub fn validate_workflow(workflow: WorkflowDsl) -> Result<ValidatedWorkflow> {
    ensure!(
        workflow.version == "0.1",
        "unsupported workflow version: {}",
        workflow.version
    );
    ensure!(
        !workflow.id.trim().is_empty(),
        "workflow id cannot be empty"
    );
    ensure!(
        !workflow.entry.trim().is_empty(),
        "workflow entry cannot be empty"
    );
    ensure!(
        !workflow.nodes.is_empty(),
        "workflow must contain at least one node"
    );
    if let Some(max_attempts) = workflow.control.max_attempts {
        ensure!(max_attempts > 0, "max_attempts must be a positive integer");
    }
    if let Some(max_rounds) = workflow.control.max_rounds {
        ensure!(max_rounds > 0, "max_rounds must be a positive integer");
    }

    let mut nodes_by_id = IndexMap::new();
    let mut seen_ids = HashSet::new();

    for node in &workflow.nodes {
        let id = node.id();
        ensure!(!id.trim().is_empty(), "node id cannot be empty");
        ensure!(seen_ids.insert(id.to_string()), "duplicate node id: {id}");
        ensure!(
            !RESERVED_NODE_IDS.contains(&id),
            "node id `{id}` is reserved and cannot be used"
        );

        match node {
            NodeDsl::Worker(worker) => validate_worker_node(worker, id)?,
            NodeDsl::AiDynamic(dynamic) => validate_ai_dynamic_node(dynamic, id)?,
        }

        nodes_by_id.insert(id.to_string(), node.clone());
    }

    ensure!(
        nodes_by_id.contains_key(&workflow.entry),
        "entry node not found: {}",
        workflow.entry
    );
    let mut edge_outcomes_by_source = HashSet::new();
    let mut has_end_target = false;
    for edge in &workflow.edges {
        ensure!(
            nodes_by_id.contains_key(&edge.from),
            "edge source not found: {}",
            edge.from
        );
        ensure!(
            edge.to == END_NODE || edge.to == NEW_ROUND_NODE || nodes_by_id.contains_key(&edge.to),
            "edge target not found: {}",
            edge.to
        );
        if edge.to == END_NODE {
            has_end_target = true;
        }
        if edge.to == NEW_ROUND_NODE && edge.on == EdgeOutcome::Success {
            return Err(WorkflowValidationError::SuccessNewRoundTarget {
                from: edge.from.clone(),
            }
            .into());
        }
        ensure!(
            edge.from != END_NODE && edge.from != NEW_ROUND_NODE,
            "edge source cannot be a terminal target: {}",
            edge.from
        );
        ensure!(
            edge_outcomes_by_source.insert((edge.from.clone(), edge.on)),
            "node `{}` already has a {:?} edge",
            edge.from,
            edge.on
        );

        if matches!(edge.session, Some(SessionMode::Continue)) {
            ensure!(
                edge.to != END_NODE && edge.to != NEW_ROUND_NODE,
                "session=continue requires a real node target"
            );
            let target = nodes_by_id
                .get(&edge.to)
                .ok_or_else(|| anyhow!("edge target not found: {}", edge.to))?;
            let provider = target
                .provider()
                .ok_or_else(|| anyhow!("target node `{}` provider cannot be blank", edge.to))?;
            ensure!(
                supports_continue_session(provider)?,
                "session=continue currently only supports agents with continue-session capability"
            );
        }
    }

    if !has_end_target {
        return Err(WorkflowValidationError::MissingEndNode.into());
    }

    let mut reachable = HashSet::new();
    let mut pending = vec![workflow.entry.clone()];
    while let Some(node_id) = pending.pop() {
        if !reachable.insert(node_id.clone()) {
            continue;
        }
        workflow
            .edges
            .iter()
            .filter(|edge| edge.from == node_id)
            .filter(|edge| edge.to != END_NODE && edge.to != NEW_ROUND_NODE)
            .for_each(|edge| pending.push(edge.to.clone()));
    }
    if let Some(node_id) = nodes_by_id.keys().find(|node_id| !reachable.contains(*node_id)) {
        return Err(WorkflowValidationError::UnreachableNode {
            node_id: node_id.clone(),
        }
        .into());
    }

    Ok(ValidatedWorkflow {
        raw: workflow,
        nodes_by_id,
    })
}
