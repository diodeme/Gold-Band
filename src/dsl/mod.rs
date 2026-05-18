use crate::domain::{AcceptanceFailurePolicy, NodeType, SessionMode};
use crate::provider::supports_continue_session;
use anyhow::{Result, anyhow, bail, ensure};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const END_NODE: &str = "$end";
pub const NEW_ROUND_NODE: &str = "$new-round";
const RESERVED_NODE_IDS: &[&str] = &["worker", "exec", "verify", END_NODE, NEW_ROUND_NODE];

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
    pub control: WorkflowControl,
    pub nodes: Vec<NodeDsl>,
    pub edges: Vec<EdgeDsl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowControl {
    pub max_repair_loops: u32,
    pub max_acceptance_loops: u32,
    pub on_acceptance_failure: AcceptanceFailurePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum NodeDsl {
    Worker(WorkerNode),
    Exec(ExecNode),
    Verify(VerifyNode),
}

impl NodeDsl {
    pub fn id(&self) -> &str {
        match self {
            Self::Worker(node) => &node.id,
            Self::Exec(node) => &node.id,
            Self::Verify(node) => &node.id,
        }
    }

    pub fn node_type(&self) -> NodeType {
        match self {
            Self::Worker(_) => NodeType::Worker,
            Self::Exec(_) => NodeType::Exec,
            Self::Verify(_) => NodeType::Verify,
        }
    }

    pub fn provider(&self) -> Option<&str> {
        match self {
            Self::Worker(node) => node.provider.as_deref(),
            Self::Verify(node) => node.provider.as_deref(),
            Self::Exec(_) => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerNode {
    pub id: String,
    pub provider: Option<String>,
    pub profile: Option<String>,
    pub goal: Option<String>,
    pub primary_artifact: Option<String>,
    pub output: Option<OutputContractDsl>,
    pub success_condition: Option<JsonConditionDsl>,
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
pub struct ExecNode {
    pub id: String,
    pub plan_from: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyNode {
    pub id: String,
    pub provider: Option<String>,
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeDsl {
    pub from: String,
    pub to: String,
    pub on: EdgeOutcome,
    pub session: Option<SessionMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EdgeOutcome {
    Success,
    Failure,
    Invalid,
}

#[derive(Debug, Clone)]
pub struct ValidatedWorkflow {
    pub raw: WorkflowDsl,
    pub nodes_by_id: IndexMap<String, NodeDsl>,
    pub verify_node_id: Option<String>,
}

impl ValidatedWorkflow {
    pub fn get_node(&self, id: &str) -> Option<&NodeDsl> {
        self.nodes_by_id.get(id)
    }
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
    ensure!(
        workflow.control.max_repair_loops > 0,
        "max_repair_loops must be a positive integer"
    );
    ensure!(
        workflow.control.max_acceptance_loops > 0,
        "max_acceptance_loops must be a positive integer"
    );

    let mut nodes_by_id = IndexMap::new();
    let mut seen_ids = HashSet::new();
    let mut verify_node_id = None;

    for node in &workflow.nodes {
        let id = node.id();
        ensure!(!id.trim().is_empty(), "node id cannot be empty");
        ensure!(seen_ids.insert(id.to_string()), "duplicate node id: {id}");
        ensure!(
            !RESERVED_NODE_IDS.contains(&id),
            "node id `{id}` is reserved and cannot be used"
        );

        match node {
            NodeDsl::Worker(worker) => {
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
                if let Some(output) = &worker.output {
                    ensure!(
                        !output.artifact.trim().is_empty(),
                        "worker node `{id}` output artifact cannot be blank"
                    );
                    ensure!(
                        worker.primary_artifact.as_deref() == Some(output.artifact.as_str()),
                        "worker node `{id}` output artifact must match primary_artifact"
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
            }
            NodeDsl::Verify(verify) => {
                let provider = verify
                    .provider
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| anyhow!("verify node `{id}` provider cannot be blank"))?;
                ensure!(
                    !provider.is_empty(),
                    "verify node `{id}` provider cannot be blank"
                );
                if let Some(profile) = &verify.profile {
                    ensure!(
                        !profile.trim().is_empty(),
                        "verify node `{id}` profile cannot be blank"
                    );
                }
            }
            NodeDsl::Exec(_) => {}
        }

        if let NodeDsl::Verify(_) = node {
            ensure!(
                verify_node_id.is_none(),
                "workflow can contain at most one verify node"
            );
            verify_node_id = Some(id.to_string());
        }

        nodes_by_id.insert(id.to_string(), node.clone());
    }

    ensure!(
        nodes_by_id.contains_key(&workflow.entry),
        "entry node not found: {}",
        workflow.entry
    );
    ensure!(
        verify_node_id.is_some()
            || matches!(
                workflow.control.on_acceptance_failure,
                AcceptanceFailurePolicy::Stop
            ),
        "acceptance failure policy requires a verify node"
    );

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
        ensure!(
            !(edge.to == END_NODE && edge.on == EdgeOutcome::Invalid),
            "edge `{}` cannot target `$end` on invalid",
            edge.from
        );
        ensure!(
            edge.from != END_NODE && edge.from != NEW_ROUND_NODE,
            "edge source cannot be a terminal target: {}",
            edge.from
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

    for node in nodes_by_id.values() {
        if let NodeDsl::Exec(exec) = node {
            let source = nodes_by_id
                .get(&exec.plan_from)
                .ok_or_else(|| anyhow!("exec planFrom not found: {}", exec.plan_from))?;
            match source {
                NodeDsl::Worker(worker) => {
                    ensure!(
                        worker.primary_artifact.as_deref() == Some("exec-plan"),
                        "exec node `{}` requires planFrom worker `{}` to declare primaryArtifact=exec-plan",
                        exec.id,
                        exec.plan_from
                    );
                }
                _ => bail!(
                    "exec node `{}` planFrom must point to a worker node",
                    exec.id
                ),
            }
        }
    }

    Ok(ValidatedWorkflow {
        raw: workflow,
        nodes_by_id,
        verify_node_id,
    })
}
