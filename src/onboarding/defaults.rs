use super::{
    environment::{EnvironmentReport, EnvironmentStatus, environment_policy},
    model::{
        DefaultExecutionProfile, GitDecision, ONBOARDING_SCHEMA_VERSION, OnboardingState,
        SetupError, fingerprint,
    },
};
use crate::{
    app::{App, WorkflowTemplateStore},
    config::{ManagedAgentConfig, ProviderDiagnosticSnapshot, SettingsConfig},
    dsl::{validate_authoring_workflow, validate_workflow_snapshot},
    provider::{
        model_bound_catalogs_from_capabilities, select_config_options_from_capabilities,
        supported_models_from_capabilities, supported_modes_from_capabilities,
    },
    storage::configuration::{commit_pair, with_configuration},
    workflow_model_binding::{WorkerModelBinding, validate_and_inject},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultSetupSelection {
    pub profile: DefaultExecutionProfile,
    /// Explicit permission choice when the target supplies no usable session default.
    pub permission_mode_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSetupPreview {
    pub id: String,
    pub name: String,
    pub slot_count: usize,
    pub previous_bindings: Vec<WorkerModelBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultSetupPreview {
    pub fingerprint: String,
    pub templates: Vec<TemplateSetupPreview>,
    pub slot_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyDefaultSetup {
    pub selection: DefaultSetupSelection,
    pub preview_fingerprint: String,
    pub operation_id: String,
    pub git_skipped: bool,
}

fn ready_agent<'a>(
    settings: &'a SettingsConfig,
    profile: &DefaultExecutionProfile,
    diagnostic: &ProviderDiagnosticSnapshot,
) -> Result<&'a ManagedAgentConfig> {
    let config = settings
        .agents
        .as_ref()
        .and_then(|agents| agents.get(&profile.agent_id))
        .ok_or_else(|| SetupError::new("setup.agent-unavailable"))?;
    if !diagnostic.available
        || diagnostic.config_fingerprint.as_deref()
            != Some(config.diagnostic_fingerprint()?.as_str())
    {
        return Err(SetupError::new("setup.agent-unavailable").into());
    }
    Ok(config)
}

fn validate_selection(
    selection: &DefaultSetupSelection,
    diagnostic: &ProviderDiagnosticSnapshot,
) -> Result<Option<String>> {
    let capabilities = diagnostic
        .capabilities
        .as_ref()
        .ok_or_else(|| SetupError::new("setup.model-config-invalid"))?;
    let models = supported_models_from_capabilities(Some(capabilities));
    let profile = &selection.profile;
    match profile.model_selection.model_id() {
        Some(model) if !models.iter().any(|item| item.id == model) => {
            return Err(SetupError::new("setup.model-config-invalid").into());
        }
        None if !models.is_empty() => {
            return Err(SetupError::new("setup.model-selection-required").into());
        }
        _ => {}
    }
    let options = select_config_options_from_capabilities(Some(capabilities));
    let bound_catalogs = model_bound_catalogs_from_capabilities(Some(capabilities));
    let current_model = options
        .iter()
        .find(|option| option.category.as_deref() == Some("model"))
        .and_then(|option| option.current_value.as_deref())
        .or_else(|| {
            capabilities
                .pointer("/models/currentModelId")
                .and_then(serde_json::Value::as_str)
        });
    let target = profile.model_selection.model_id().or(current_model);
    let target_bound = target.and_then(|model| bound_catalogs.get(model));
    for (id, value) in &profile.config_options {
        let option = target_bound
            .into_iter()
            .flatten()
            .chain(options.iter())
            .find(|option| option.id == *id)
            .ok_or_else(|| SetupError::new("setup.model-config-invalid"))?;
        if matches!(option.category.as_deref(), Some("model" | "mode"))
            || !option.options.iter().any(|item| item.value == *value)
            || (option
                .category
                .as_deref()
                .is_some_and(crate::acp::session_config::is_model_bound_config_category)
                && target != current_model
                && target_bound.is_none())
        {
            return Err(SetupError::new("setup.model-config-invalid").into());
        }
    }
    for (model, overrides) in &profile.model_bound_overrides {
        if !models.iter().any(|item| &item.id == model) {
            return Err(SetupError::new("setup.model-config-invalid").into());
        }
        let catalog = bound_catalogs
            .get(model)
            .ok_or_else(|| SetupError::new("setup.model-config-invalid"))?;
        for (id, value) in overrides {
            if !catalog.iter().any(|option| {
                option.id == *id
                    && option
                        .category
                        .as_deref()
                        .is_some_and(crate::acp::session_config::is_model_bound_config_category)
                    && option.options.iter().any(|item| item.value == *value)
            }) {
                return Err(SetupError::new("setup.model-config-invalid").into());
            }
        }
    }
    let modes = supported_modes_from_capabilities(Some(capabilities));
    let permission = selection
        .permission_mode_id
        .clone()
        .or_else(|| {
            options
                .iter()
                .find(|option| option.category.as_deref() == Some("mode"))
                .and_then(|option| option.current_value.clone())
        })
        .or_else(|| {
            capabilities
                .pointer("/modes/currentModeId")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
    if (!modes.is_empty() && permission.is_none())
        || permission
            .as_ref()
            .is_some_and(|mode| !modes.iter().any(|item| &item.id == mode))
    {
        return Err(SetupError::new("setup.permission-selection-required").into());
    }
    Ok(permission)
}

fn project(
    app: &App,
    settings: &SettingsConfig,
    store: &WorkflowTemplateStore,
    selection: &DefaultSetupSelection,
) -> Result<(DefaultSetupPreview, WorkflowTemplateStore)> {
    let diagnostics = app.provider_diagnostics();
    let diagnostic = diagnostics
        .get(selection.profile.agent_id.as_str())
        .ok_or_else(|| SetupError::new("setup.agent-unavailable"))?;
    let config = ready_agent(settings, &selection.profile, diagnostic)?;
    let permission = validate_selection(selection, diagnostic)?;
    let mut next = store.clone();
    let ids = app.built_in_workflow_template_ids();
    let mut templates = Vec::new();
    let mut revisions = Vec::new();
    for template in &mut next.templates {
        if !template.is_built_in || !ids.contains(&template.id.as_str()) {
            continue;
        }
        validate_authoring_workflow(template.workflow.clone())?;
        let slots: Vec<_> = template
            .workflow
            .nodes
            .iter()
            .filter_map(|node| match node {
                crate::dsl::NodeDsl::Worker(worker) => Some(
                    worker
                        .execution_slot_id
                        .clone()
                        .ok_or_else(|| SetupError::new("setup.templates-initialization-required")),
                ),
                _ => None,
            })
            .collect::<Result<_, _>>()?;
        templates.push(TemplateSetupPreview {
            id: template.id.clone(),
            name: template.name.clone(),
            slot_count: slots.len(),
            previous_bindings: template.model_bindings.bindings.clone(),
        });
        revisions.push(serde_json::json!({"id":template.id,"workflow":template.workflow,"bindings":template.model_bindings}));
        let old = template.model_bindings.bindings.clone();
        for slot in slots {
            if template
                .model_bindings
                .bindings
                .iter()
                .any(|binding| binding.execution_slot_id == slot)
            {
                continue;
            }
            template.model_bindings.bindings.push(WorkerModelBinding {
                execution_slot_id: slot,
                agent_id: selection.profile.agent_id.as_str().into(),
                model_id: None,
                permission_mode_id: permission.clone(),
                auto_accept: false,
                config_options: Default::default(),
                model_bound_overrides: Default::default(),
            });
        }
        for binding in &mut template.model_bindings.bindings {
            if binding.agent_id != selection.profile.agent_id.as_str() {
                binding.permission_mode_id = permission.clone();
                binding.auto_accept = false;
            }
            binding.agent_id = selection.profile.agent_id.as_str().to_string();
            binding.model_id = selection
                .profile
                .model_selection
                .model_id()
                .map(str::to_owned);
            binding.config_options = selection.profile.config_options.clone();
            binding.model_bound_overrides = selection.profile.model_bound_overrides.clone();
        }
        template
            .model_bindings
            .bindings
            .sort_by(|left, right| left.execution_slot_id.cmp(&right.execution_slot_id));
        let executable = validate_and_inject(
            &template.workflow,
            &template.model_bindings,
            settings.agents.as_ref().expect("ready agent validated"),
            &diagnostics,
        )?;
        validate_workflow_snapshot(executable)?;
        if template.model_bindings.bindings != old {
            template.model_bindings.binding_revision = template
                .model_bindings
                .binding_revision
                .checked_add(1)
                .ok_or_else(|| SetupError::new("setup.configuration-conflict"))?;
            template.updated_at = chrono::Utc::now().to_rfc3339();
        }
    }
    if templates.len() != ids.len() || templates.iter().any(|template| template.slot_count == 0) {
        return Err(SetupError::new("setup.templates-initialization-required").into());
    }
    let fingerprint = fingerprint(&serde_json::json!({
        "selection": selection, "defaultProfile":settings.default_execution_profile, "agent":config,
        "capabilities":diagnostic.capabilities, "configFingerprint":diagnostic.config_fingerprint,
        "policy":environment_policy(), "templates":revisions,
    }))?;
    Ok((
        DefaultSetupPreview {
            fingerprint,
            slot_count: templates.iter().map(|template| template.slot_count).sum(),
            templates,
        },
        next,
    ))
}

pub fn preview(app: &App, selection: &DefaultSetupSelection) -> Result<DefaultSetupPreview> {
    with_configuration(&app.paths.user_gold_band_dir(), || {
        project(
            app,
            &app.load_settings()?,
            &app.prepared_workflow_templates()?,
            selection,
        )
        .map(|(preview, _)| preview)
    })
}

fn already_applied(settings: &SettingsConfig, request: &ApplyDefaultSetup) -> Result<bool> {
    if let Some(OnboardingState::Completed {
        last_apply_id,
        request_fingerprint: prior,
        ..
    }) = &settings.onboarding
        && last_apply_id == &request.operation_id
    {
        if prior != &fingerprint(request)? {
            return Err(SetupError::new("setup.configuration-conflict").into());
        }
        return Ok(true);
    }
    Ok(false)
}

pub fn applied_settings(app: &App, request: &ApplyDefaultSetup) -> Result<Option<SettingsConfig>> {
    if uuid::Uuid::parse_str(&request.operation_id).is_err() {
        return Err(SetupError::new("setup.operation-id-invalid").into());
    }
    let settings = app.load_settings()?;
    Ok(already_applied(&settings, request)?.then_some(settings))
}

pub fn apply(
    app: &App,
    request: &ApplyDefaultSetup,
    environment: &EnvironmentReport,
) -> Result<SettingsConfig> {
    if let Some(settings) = applied_settings(app, request)? {
        return Ok(settings);
    }
    let request_fingerprint = fingerprint(request)?;
    // The setup controller owns this invocation's completed environment report.
    // Applying defaults does not launch a second round of external probes.
    with_configuration(&app.paths.user_gold_band_dir(), || {
        let mut settings = app.load_settings()?;
        if already_applied(&settings, request)? {
            return Ok(settings);
        }
        let git_ready = environment.checks.iter().any(|check| {
            check.tool == super::environment::EnvironmentTool::Git
                && check.status == EnvironmentStatus::Ready
        });
        let git_skipped = match &settings.onboarding {
            Some(
                OnboardingState::Pending { git_decision }
                | OnboardingState::Deferred { git_decision },
            ) => *git_decision == GitDecision::Skip,
            _ => request.git_skipped,
        };
        if !git_ready && !git_skipped {
            return Err(SetupError::new("setup.git-decision-required").into());
        }
        let (preview, store) = project(
            app,
            &settings,
            &app.prepared_workflow_templates()?,
            &request.selection,
        )?;
        if preview.fingerprint != request.preview_fingerprint {
            return Err(SetupError::new("setup.preview-stale").into());
        }
        settings.default_execution_profile = Some(request.selection.profile.clone());
        settings.onboarding = Some(OnboardingState::Completed {
            schema_version: ONBOARDING_SCHEMA_VERSION,
            completed_at: chrono::Utc::now().to_rfc3339(),
            last_apply_id: request.operation_id.clone(),
            request_fingerprint: request_fingerprint.clone(),
        });
        commit_pair(
            &app.paths.user_gold_band_dir(),
            &request.operation_id,
            &settings,
            &store,
        )
        .map_err(|error| {
            if error.is::<crate::storage::configuration::ConfigurationError>() {
                error
            } else {
                SetupError::new("setup.commit-failed").into()
            }
        })?;
        app.load_settings()
    })
}
