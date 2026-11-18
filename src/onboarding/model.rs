use crate::config::ManagedAgentId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ONBOARDING_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum OnboardingState {
    Pending {
        git_decision: GitDecision,
    },
    Completed {
        schema_version: u32,
        completed_at: String,
        last_apply_id: String,
        request_fingerprint: String,
    },
    ExistingInstall,
    /// New install where the user chose to enter the app and finish setup later.
    /// This state does not bypass the runtime gate: it can only be written after
    /// the current environment report passes. The Git decision is retained so a
    /// later settings-driven setup can finish without asking the user again.
    Deferred {
        git_decision: GitDecision,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitDecision {
    #[default]
    Undecided,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ModelSelection {
    Explicit { model_id: String },
    ProviderDefault,
}

impl ModelSelection {
    pub fn model_id(&self) -> Option<&str> {
        match self {
            Self::Explicit { model_id } => Some(model_id),
            Self::ProviderDefault => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultExecutionProfile {
    pub agent_id: ManagedAgentId,
    pub model_selection: ModelSelection,
    #[serde(default)]
    pub config_options: BTreeMap<String, String>,
    #[serde(default)]
    pub model_bound_overrides: BTreeMap<String, BTreeMap<String, String>>,
}

impl DefaultExecutionProfile {
    /// A new workspace's authoring defaults. Existing saved choices always win.
    pub fn initial_run_mode(&self) -> crate::config::ConversationRunModeEntry {
        use crate::config::{
            ConversationAutoConfig, ConversationDirectConfig, ConversationRunMode,
            ConversationRunModeEntry,
        };
        ConversationRunModeEntry {
            mode: ConversationRunMode::Auto,
            workflow_template_id: None,
            optional_entry_preferences: Default::default(),
            direct_preferences: Default::default(),
            authoring_revision: 0,
            direct_config: Some(ConversationDirectConfig {
                agent_type: self.agent_id.as_str().into(),
                model_id: self.model_selection.model_id().map(str::to_owned),
                permission_mode: None,
                auto_accept: false,
                config_options: self.config_options.clone(),
                model_bound_overrides: self.model_bound_overrides.clone(),
            }),
            auto_config: Some(ConversationAutoConfig {
                agent_strategy: None,
                agent_type: self.agent_id.as_str().into(),
                model_id: self.model_selection.model_id().map(str::to_owned),
                config_options: self.config_options.clone(),
                model_bound_overrides: self.model_bound_overrides.clone(),
                permission_mode: None,
                auto_accept: false,
                bootstrap_agent_type: None,
                bootstrap_model_id: None,
                bootstrap_config_options: Default::default(),
                bootstrap_model_bound_overrides: Default::default(),
                acceptance_model_id: None,
                acceptance_config_options: Default::default(),
                acceptance_model_bound_overrides: Default::default(),
                available_agents: None,
                routing_prompt: None,
                allowed_workflows: None,
                allowed_profiles: None,
                global_goal: None,
                control: None,
                active_template_id: None,
                active_template_name: None,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{code}")]
pub struct SetupError {
    pub code: &'static str,
    pub params: serde_json::Value,
}

impl SetupError {
    pub fn new(code: &'static str) -> Self {
        Self {
            code,
            params: serde_json::json!({}),
        }
    }
}

pub fn fingerprint(value: &impl Serialize) -> anyhow::Result<String> {
    Ok(blake3::hash(&serde_json::to_vec(value)?)
        .to_hex()
        .to_string())
}
