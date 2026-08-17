use crate::config::{ManagedAgentConfig, ManagedAgentId, ProviderDiagnosticSnapshot};
use crate::dsl::{NodeDsl, WorkflowDsl};
use crate::provider::{
    select_config_options_from_capabilities, supported_models_from_capabilities,
    supported_modes_from_capabilities,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use uuid::Uuid;

pub const WORKFLOW_MODEL_BINDING_MIGRATION_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowModelBindings {
    #[serde(default)]
    pub definition_revision: String,
    #[serde(default)]
    pub binding_revision: u64,
    #[serde(default)]
    pub bindings: Vec<WorkerModelBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAuthoringWorkflow {
    pub workflow: WorkflowDsl,
    #[serde(default)]
    pub model_bindings: WorkflowModelBindings,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum TaskAuthoringWorkflowCompat {
    Current(TaskAuthoringWorkflow),
    Legacy(WorkflowDsl),
}

impl TaskAuthoringWorkflowCompat {
    pub fn into_current(self) -> (TaskAuthoringWorkflow, bool) {
        match self {
            Self::Current(current) => (current, false),
            Self::Legacy(workflow) => (
                TaskAuthoringWorkflow {
                    workflow,
                    model_bindings: WorkflowModelBindings::default(),
                },
                true,
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerModelBinding {
    pub execution_slot_id: String,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config_options: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "code", content = "params")]
pub enum WorkflowModelBindingError {
    #[error("workflow-model-binding.slot-required")]
    #[serde(rename = "workflow-model-binding.slot-required")]
    SlotRequired { node_id: String },
    #[error("workflow-model-binding.slot-duplicate")]
    #[serde(rename = "workflow-model-binding.slot-duplicate")]
    SlotDuplicate {
        execution_slot_id: String,
        node_ids: Vec<String>,
    },
    #[error("workflow-model-binding.binding-duplicate")]
    #[serde(rename = "workflow-model-binding.binding-duplicate")]
    BindingDuplicate { execution_slot_id: String },
    #[error("workflow-model-binding.slot-not-found")]
    #[serde(rename = "workflow-model-binding.slot-not-found")]
    SlotNotFound { execution_slot_id: String },
    #[error("workflow-model-binding.agent-required")]
    #[serde(rename = "workflow-model-binding.agent-required")]
    AgentRequired {
        execution_slot_id: String,
        node_id: String,
    },
    #[error("workflow-model-binding.agent-unavailable")]
    #[serde(rename = "workflow-model-binding.agent-unavailable")]
    AgentUnavailable {
        execution_slot_id: String,
        agent_id: String,
    },
    #[error("workflow-model-binding.model-unsupported")]
    #[serde(rename = "workflow-model-binding.model-unsupported")]
    ModelUnsupported {
        execution_slot_id: String,
        agent_id: String,
        model_id: String,
    },
    #[error("workflow-model-binding.permission-unsupported")]
    #[serde(rename = "workflow-model-binding.permission-unsupported")]
    PermissionUnsupported {
        execution_slot_id: String,
        agent_id: String,
        permission_mode_id: String,
    },
    #[error("workflow-model-binding.option-unsupported")]
    #[serde(rename = "workflow-model-binding.option-unsupported")]
    OptionUnsupported {
        execution_slot_id: String,
        agent_id: String,
        option_id: String,
        value: String,
    },
}

impl WorkflowModelBindingError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::SlotRequired { .. } => "workflow-model-binding.slot-required",
            Self::SlotDuplicate { .. } => "workflow-model-binding.slot-duplicate",
            Self::BindingDuplicate { .. } => "workflow-model-binding.binding-duplicate",
            Self::SlotNotFound { .. } => "workflow-model-binding.slot-not-found",
            Self::AgentRequired { .. } => "workflow-model-binding.agent-required",
            Self::AgentUnavailable { .. } => "workflow-model-binding.agent-unavailable",
            Self::ModelUnsupported { .. } => "workflow-model-binding.model-unsupported",
            Self::PermissionUnsupported { .. } => "workflow-model-binding.permission-unsupported",
            Self::OptionUnsupported { .. } => "workflow-model-binding.option-unsupported",
        }
    }

    pub fn params(&self) -> serde_json::Value {
        match self {
            Self::SlotRequired { node_id } => serde_json::json!({ "nodeId": node_id }),
            Self::SlotDuplicate {
                execution_slot_id,
                node_ids,
            } => serde_json::json!({
                "executionSlotId": execution_slot_id,
                "nodeIds": node_ids,
            }),
            Self::BindingDuplicate { execution_slot_id } => {
                serde_json::json!({ "executionSlotId": execution_slot_id })
            }
            Self::SlotNotFound { execution_slot_id } => {
                serde_json::json!({ "executionSlotId": execution_slot_id })
            }
            Self::AgentRequired {
                execution_slot_id,
                node_id,
            } => serde_json::json!({
                "executionSlotId": execution_slot_id,
                "nodeId": node_id,
            }),
            Self::AgentUnavailable {
                execution_slot_id,
                agent_id,
            } => serde_json::json!({
                "executionSlotId": execution_slot_id,
                "agentId": agent_id,
            }),
            Self::ModelUnsupported {
                execution_slot_id,
                agent_id,
                model_id,
            } => serde_json::json!({
                "executionSlotId": execution_slot_id,
                "agentId": agent_id,
                "modelId": model_id,
            }),
            Self::PermissionUnsupported {
                execution_slot_id,
                agent_id,
                permission_mode_id,
            } => serde_json::json!({
                "executionSlotId": execution_slot_id,
                "agentId": agent_id,
                "permissionModeId": permission_mode_id,
            }),
            Self::OptionUnsupported {
                execution_slot_id,
                agent_id,
                option_id,
                value,
            } => serde_json::json!({
                "executionSlotId": execution_slot_id,
                "agentId": agent_id,
                "optionId": option_id,
                "value": value,
            }),
        }
    }
}

pub fn new_execution_slot_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn built_in_execution_slot_id(template_id: &str, node_id: &str) -> String {
    format!("builtin:{template_id}:{node_id}")
}

pub fn definition_revision(workflow: &WorkflowDsl) -> String {
    let mut value = serde_json::to_value(workflow).expect("WorkflowDsl is serializable");
    if let Some(nodes) = value
        .get_mut("nodes")
        .and_then(serde_json::Value::as_array_mut)
    {
        for node in nodes {
            if node.get("type").and_then(serde_json::Value::as_str) != Some("worker") {
                continue;
            }
            if let Some(object) = node.as_object_mut() {
                object.remove("executionSlotId");
                object.remove("provider");
                object.remove("model");
                object.remove("permission_mode");
                object.remove("config_options");
            }
        }
    }
    let bytes = serde_json::to_vec(&value).expect("canonical workflow JSON is serializable");
    format!("{:x}", Sha256::digest(bytes))
}

pub fn migrate_authoring_workflow(
    workflow: &mut WorkflowDsl,
    model_bindings: &mut WorkflowModelBindings,
    built_in_template_id: Option<&str>,
) -> Result<bool, WorkflowModelBindingError> {
    validate_unique_bindings(model_bindings)?;
    let mut changed = false;
    let previous_bindings = model_bindings.bindings.clone();
    let mut bindings = previous_bindings
        .iter()
        .cloned()
        .map(|binding| (binding.execution_slot_id.clone(), binding))
        .collect::<BTreeMap<_, _>>();

    for node in &mut workflow.nodes {
        let NodeDsl::Worker(worker) = node else {
            continue;
        };
        let expected_built_in_slot = built_in_template_id
            .map(|template_id| built_in_execution_slot_id(template_id, &worker.id));
        let slot = if let Some(expected) = expected_built_in_slot {
            if worker.execution_slot_id.as_deref() != Some(expected.as_str()) {
                worker.execution_slot_id = Some(expected.clone());
                changed = true;
            }
            expected
        } else if let Some(slot) = worker
            .execution_slot_id
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            slot.to_string()
        } else {
            let slot = new_execution_slot_id();
            worker.execution_slot_id = Some(slot.clone());
            changed = true;
            slot
        };

        if let Some(agent_id) = worker
            .provider
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
        {
            bindings
                .entry(slot.clone())
                .or_insert_with(|| WorkerModelBinding {
                    execution_slot_id: slot,
                    agent_id,
                    model_id: worker
                        .model
                        .clone()
                        .filter(|value| !value.trim().is_empty()),
                    permission_mode_id: worker
                        .permission_mode
                        .clone()
                        .filter(|value| !value.trim().is_empty()),
                    config_options: worker.config_options.clone(),
                });
            changed = true;
        }
        let removed_provider = worker.provider.take().is_some();
        let removed_model = worker.model.take().is_some();
        let removed_permission = worker.permission_mode.take().is_some();
        let removed_options = !worker.config_options.is_empty();
        if removed_options {
            worker.config_options.clear();
        }
        if removed_provider || removed_model || removed_permission || removed_options {
            changed = true;
        }
    }

    let active_slots = workflow
        .nodes
        .iter()
        .filter_map(|node| match node {
            NodeDsl::Worker(worker) => worker.execution_slot_id.clone(),
            NodeDsl::AiDynamic(_) => None,
        })
        .collect::<BTreeSet<_>>();
    bindings.retain(|slot, _| active_slots.contains(slot));
    let next_bindings = bindings.into_values().collect::<Vec<_>>();
    let bindings_changed = previous_bindings != next_bindings;
    if bindings_changed {
        changed = true;
    }
    model_bindings.bindings = next_bindings;
    let revision = definition_revision(workflow);
    if model_bindings.definition_revision != revision {
        model_bindings.definition_revision = revision;
        changed = true;
    }
    if bindings_changed {
        model_bindings.binding_revision = model_bindings.binding_revision.saturating_add(1);
    }
    Ok(changed)
}

pub fn reconcile_authoring_workflow_for_save(
    workflow: &mut WorkflowDsl,
    model_bindings: &mut WorkflowModelBindings,
    persisted: Option<&TaskAuthoringWorkflow>,
    built_in_template_id: Option<&str>,
) -> Result<bool, WorkflowModelBindingError> {
    let mut persisted = persisted.cloned();
    if let Some(current) = persisted.as_mut() {
        migrate_authoring_workflow(
            &mut current.workflow,
            &mut current.model_bindings,
            built_in_template_id,
        )?;
    }

    let mut changed = migrate_authoring_workflow(workflow, model_bindings, built_in_template_id)?;
    let next_binding_revision = match persisted.as_ref() {
        Some(current) if current.model_bindings.bindings == model_bindings.bindings => {
            current.model_bindings.binding_revision
        }
        Some(current) => current.model_bindings.binding_revision.saturating_add(1),
        None if model_bindings.bindings.is_empty() => 0,
        None => 1,
    };
    if model_bindings.binding_revision != next_binding_revision {
        model_bindings.binding_revision = next_binding_revision;
        changed = true;
    }
    Ok(changed)
}

fn validate_unique_bindings(
    model_bindings: &WorkflowModelBindings,
) -> Result<(), WorkflowModelBindingError> {
    let mut execution_slot_ids = BTreeSet::new();
    for binding in &model_bindings.bindings {
        if !execution_slot_ids.insert(binding.execution_slot_id.as_str()) {
            return Err(WorkflowModelBindingError::BindingDuplicate {
                execution_slot_id: binding.execution_slot_id.clone(),
            });
        }
    }
    Ok(())
}

pub fn validate_and_inject(
    authoring: &WorkflowDsl,
    model_bindings: &WorkflowModelBindings,
    managed_agents: &BTreeMap<ManagedAgentId, ManagedAgentConfig>,
    diagnostics: &BTreeMap<String, ProviderDiagnosticSnapshot>,
) -> Result<WorkflowDsl, WorkflowModelBindingError> {
    validate_unique_bindings(model_bindings)?;
    let mut nodes_by_slot = BTreeMap::<String, Vec<String>>::new();
    for node in &authoring.nodes {
        let NodeDsl::Worker(worker) = node else {
            continue;
        };
        let slot = worker
            .execution_slot_id
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| WorkflowModelBindingError::SlotRequired {
                node_id: worker.id.clone(),
            })?;
        nodes_by_slot
            .entry(slot.to_string())
            .or_default()
            .push(worker.id.clone());
    }
    if let Some((execution_slot_id, node_ids)) = nodes_by_slot
        .iter()
        .find(|(_, node_ids)| node_ids.len() > 1)
    {
        return Err(WorkflowModelBindingError::SlotDuplicate {
            execution_slot_id: execution_slot_id.clone(),
            node_ids: node_ids.clone(),
        });
    }

    let bindings_by_slot = model_bindings
        .bindings
        .iter()
        .map(|binding| (binding.execution_slot_id.as_str(), binding))
        .collect::<BTreeMap<_, _>>();
    if let Some(binding) = model_bindings
        .bindings
        .iter()
        .find(|binding| !nodes_by_slot.contains_key(&binding.execution_slot_id))
    {
        return Err(WorkflowModelBindingError::SlotNotFound {
            execution_slot_id: binding.execution_slot_id.clone(),
        });
    }

    let mut executable = authoring.clone();
    for node in &mut executable.nodes {
        let NodeDsl::Worker(worker) = node else {
            continue;
        };
        let slot = worker.execution_slot_id.as_deref().expect("slot validated");
        let binding = bindings_by_slot.get(slot).copied().ok_or_else(|| {
            WorkflowModelBindingError::AgentRequired {
                execution_slot_id: slot.to_string(),
                node_id: worker.id.clone(),
            }
        })?;
        let parsed_agent = ManagedAgentId::from_str(&binding.agent_id).ok();
        let diagnostic = diagnostics
            .get(&binding.agent_id)
            .filter(|diagnostic| diagnostic.available);
        if parsed_agent
            .as_ref()
            .is_none_or(|agent_id| !managed_agents.contains_key(agent_id))
            || diagnostic.is_none()
        {
            return Err(WorkflowModelBindingError::AgentUnavailable {
                execution_slot_id: slot.to_string(),
                agent_id: binding.agent_id.clone(),
            });
        }
        let capabilities = diagnostic.and_then(|item| item.capabilities.as_ref());
        if let Some(model_id) = binding.model_id.as_ref() {
            let supported = supported_models_from_capabilities(capabilities);
            if !supported.iter().any(|option| option.id == *model_id) {
                return Err(WorkflowModelBindingError::ModelUnsupported {
                    execution_slot_id: slot.to_string(),
                    agent_id: binding.agent_id.clone(),
                    model_id: model_id.clone(),
                });
            }
        }
        if let Some(permission_mode_id) = binding.permission_mode_id.as_ref() {
            let supported = supported_modes_from_capabilities(capabilities);
            if !supported
                .iter()
                .any(|option| option.id == *permission_mode_id)
            {
                return Err(WorkflowModelBindingError::PermissionUnsupported {
                    execution_slot_id: slot.to_string(),
                    agent_id: binding.agent_id.clone(),
                    permission_mode_id: permission_mode_id.clone(),
                });
            }
        }
        let options = select_config_options_from_capabilities(capabilities);
        for (option_id, value) in &binding.config_options {
            if !options.iter().any(|option| {
                option.id == *option_id && option.options.iter().any(|item| item.value == *value)
            }) {
                return Err(WorkflowModelBindingError::OptionUnsupported {
                    execution_slot_id: slot.to_string(),
                    agent_id: binding.agent_id.clone(),
                    option_id: option_id.clone(),
                    value: value.clone(),
                });
            }
        }
        worker.provider = Some(binding.agent_id.clone());
        worker.model = binding.model_id.clone();
        worker.permission_mode = binding.permission_mode_id.clone();
        worker.config_options = binding.config_options.clone();
    }
    Ok(executable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::{NodeDsl, WorkerNode};

    fn workflow(worker: WorkerNode) -> WorkflowDsl {
        WorkflowDsl {
            version: "0.1".into(),
            id: "workflow".into(),
            entry: worker.id.clone(),
            control: Default::default(),
            nodes: vec![NodeDsl::Worker(worker)],
            edges: Vec::new(),
        }
    }

    fn worker() -> WorkerNode {
        WorkerNode {
            id: "dev".into(),
            execution_slot_id: None,
            provider: Some("agent-a".into()),
            model: Some("model-a".into()),
            profile: None,
            goal: None,
            output: None,
            success_condition: None,
            permission_mode: Some("mode-a".into()),
            config_options: BTreeMap::from([("thought".into(), "high".into())]),
            manual_check: None,
            prompt_envelope: Default::default(),
        }
    }

    #[test]
    fn migration_extracts_execution_fields_and_is_idempotent() {
        let mut workflow = workflow(worker());
        let mut bindings = WorkflowModelBindings::default();
        assert!(migrate_authoring_workflow(&mut workflow, &mut bindings, Some("default")).unwrap());
        let NodeDsl::Worker(worker) = &workflow.nodes[0] else {
            panic!("worker expected")
        };
        assert_eq!(
            worker.execution_slot_id.as_deref(),
            Some("builtin:default:dev")
        );
        assert!(worker.provider.is_none());
        assert_eq!(bindings.bindings.len(), 1);
        let revision = bindings.binding_revision;
        assert!(
            !migrate_authoring_workflow(&mut workflow, &mut bindings, Some("default")).unwrap()
        );
        assert_eq!(bindings.binding_revision, revision);
    }

    #[test]
    fn migration_rejects_duplicate_binding_slots_without_mutating_bindings() {
        let mut workflow = workflow(worker());
        let duplicate = WorkerModelBinding {
            execution_slot_id: "slot-a".into(),
            agent_id: "agent-a".into(),
            model_id: None,
            permission_mode_id: None,
            config_options: BTreeMap::new(),
        };
        let mut bindings = WorkflowModelBindings {
            definition_revision: String::new(),
            binding_revision: 0,
            bindings: vec![duplicate.clone(), duplicate],
        };
        let original = bindings.clone();

        let error = migrate_authoring_workflow(&mut workflow, &mut bindings, None).unwrap_err();

        assert_eq!(
            error,
            WorkflowModelBindingError::BindingDuplicate {
                execution_slot_id: "slot-a".into(),
            }
        );
        assert_eq!(error.code(), "workflow-model-binding.binding-duplicate");
        assert_eq!(
            error.params(),
            serde_json::json!({ "executionSlotId": "slot-a" })
        );
        assert_eq!(bindings, original);
    }

    #[test]
    fn executable_injection_rejects_duplicate_binding_slots() {
        let mut node = worker();
        node.execution_slot_id = Some("slot-a".into());
        node.provider = None;
        node.model = None;
        node.permission_mode = None;
        node.config_options.clear();
        let workflow = workflow(node);
        let duplicate = WorkerModelBinding {
            execution_slot_id: "slot-a".into(),
            agent_id: "agent-a".into(),
            model_id: None,
            permission_mode_id: None,
            config_options: BTreeMap::new(),
        };
        let bindings = WorkflowModelBindings {
            definition_revision: String::new(),
            binding_revision: 0,
            bindings: vec![duplicate.clone(), duplicate],
        };

        let error = validate_and_inject(&workflow, &bindings, &BTreeMap::new(), &BTreeMap::new())
            .unwrap_err();

        assert_eq!(
            error,
            WorkflowModelBindingError::BindingDuplicate {
                execution_slot_id: "slot-a".into(),
            }
        );
    }

    #[test]
    fn definition_revision_ignores_slot_and_execution_fields() {
        let first = workflow(worker());
        let mut second = first.clone();
        let NodeDsl::Worker(worker) = &mut second.nodes[0] else {
            panic!("worker expected")
        };
        worker.execution_slot_id = Some("another-slot".into());
        worker.provider = Some("another-agent".into());
        worker.model = None;
        assert_eq!(definition_revision(&first), definition_revision(&second));
    }

    #[test]
    fn empty_bindings_serialize_as_an_explicit_array() {
        let value = serde_json::to_value(WorkflowModelBindings::default()).unwrap();

        assert_eq!(value.get("bindings"), Some(&serde_json::json!([])));
    }
}
