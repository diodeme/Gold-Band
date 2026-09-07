use std::collections::BTreeMap;

use blake3::Hasher;
use camino::Utf8PathBuf;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::acp::timeline::read_indexed_latest_root_agent_output;
use crate::{
    acp::{
        elicitation::{
            ElicitationAction, ElicitationResponseState, PendingElicitationState,
            elicitation_response_file, pending_elicitation_file,
            write_elicitation_response_if_pending,
        },
        events::current_timestamp,
        interaction::AcpPromptInteractionKind,
        permission::{
            PendingPermissionState, PermissionResponseState, pending_permission_file,
            permission_response_file, write_permission_response_if_pending,
        },
    },
    app::{App, ManualCheckSubmissionLease},
    domain::{NodeOutcome, RunStatus},
    runtime::{NodeState, RunState},
    storage::read_json,
};

pub const MAX_INTERVENTION_CONTENT_BYTES: usize = 8 * 1024;
const MAX_ELICITATION_PROMPT_CHARS: usize = 512;
const MAX_ELICITATION_QUESTION_CHARS: usize = 128;
const MAX_ELICITATION_OPTION_LABEL_CHARS: usize = 128;
const MAX_ELICITATION_OPTION_DESCRIPTION_CHARS: usize = 256;
const MAX_ELICITATION_OPTION_VALUE_BYTES: usize = 256;
const MAX_REMOTE_ELICITATION_FIELD_NAME_BYTES: usize = 256;
const MAX_REMOTE_ELICITATION_VOTE_OPTIONS: usize = 20;
const MAX_REMOTE_ELICITATION_SELECTORS: usize = 3;
const MAX_REMOTE_ELICITATION_SELECT_OPTIONS: usize = 10;
const MAX_REMOTE_PERMISSION_OPTIONS: usize = 20;
const MAX_PERMISSION_PROMPT_CHARS: usize = 256;
const MAX_PERMISSION_FIELD_CHARS: usize = 256;
const MAX_PERMISSION_PATHS: usize = 3;
const MAX_MANUAL_CHECK_OUTPUT_CHARS: usize = 4096;
const PERMISSION_TOOL_FIELD: &str = "permissionTool";
const PERMISSION_PATH_FIELD: &str = "permissionPath";
const PERMISSION_COMMAND_FIELD: &str = "permissionCommand";
const PERMISSION_PARAMETER_FIELD: &str = "permissionParameter";
const PERMISSION_PARAMETER_KEYS: [&str; 7] = [
    "command", "path", "pattern", "query", "glob", "skill", "args",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionLocator {
    pub project_id: String,
    pub task_id: String,
    pub run_id: String,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_attempt_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InterventionRequestIdentity {
    Permission { request_id: String },
    Elicitation { elicitation_id: String },
    ManualCheck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionActionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionActionQualifier {
    Standard,
    BypassPermissions,
    AutoEdit,
    AutoMode,
    ManualApproval,
    KeepPlanning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InterventionAllowedAction {
    PermissionOption {
        option_id: String,
        name: String,
        permission_kind: PermissionActionKind,
    },
    ElicitationFixedForm {
        form: RemoteElicitationForm,
    },
    ElicitationAccept,
    ElicitationDecline,
    ManualSuccess,
    ManualFailure,
}

pub fn intervention_action_for_allowed(action: &InterventionAllowedAction) -> InterventionAction {
    match action {
        InterventionAllowedAction::PermissionOption { option_id, .. } => {
            InterventionAction::PermissionOption {
                option_id: option_id.clone(),
            }
        }
        // Fixed forms are submitted through Form selections; LocalIndex is not
        // a valid transport for them and is rejected by the inbound boundary.
        InterventionAllowedAction::ElicitationFixedForm { .. } => InterventionAction::Elicitation {
            action: ElicitationAction::Accept,
            content: None,
        },
        InterventionAllowedAction::ElicitationAccept => InterventionAction::Elicitation {
            action: ElicitationAction::Accept,
            content: None,
        },
        InterventionAllowedAction::ElicitationDecline => InterventionAction::Elicitation {
            action: ElicitationAction::Decline,
            content: None,
        },
        InterventionAllowedAction::ManualSuccess => InterventionAction::ManualCheck {
            outcome: NodeOutcome::Success,
        },
        InterventionAllowedAction::ManualFailure => InterventionAction::ManualCheck {
            outcome: NodeOutcome::Failure,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ElicitationQuestionKind {
    SingleSelect,
    MultiSelect,
    FreeText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationQuestionOption {
    pub value: Value,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteElicitationOption {
    pub value: Value,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteScalarChoiceQuestion {
    pub selector_key: String,
    pub field_name: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub required: bool,
    pub options: Vec<RemoteElicitationOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RemoteElicitationForm {
    SingleScalarChoice {
        question: RemoteScalarChoiceQuestion,
    },
    MultiScalarChoice {
        question: RemoteScalarChoiceQuestion,
        allows_empty: bool,
    },
    ScalarChoiceQuestions {
        questions: Vec<RemoteScalarChoiceQuestion>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteElicitationFormSelection {
    pub selector_key: String,
    pub option_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationQuestion {
    pub field_name: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub question_kind: ElicitationQuestionKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ElicitationQuestionOption>,
    pub allows_custom_answer: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionPrompt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<ElicitationQuestion>,
    pub requires_desktop: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InterventionAction {
    PermissionOption {
        option_id: String,
    },
    Elicitation {
        action: ElicitationAction,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<Value>,
    },
    ManualCheck {
        outcome: NodeOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionSnapshot {
    pub locator: InterventionLocator,
    pub request: InterventionRequestIdentity,
    pub expected_state: String,
    pub allowed_actions: Vec<InterventionAllowedAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<InterventionPrompt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionCommand {
    pub locator: InterventionLocator,
    pub request: InterventionRequestIdentity,
    pub expected_state: String,
    pub action: InterventionAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InterventionCommandStatus {
    Accepted,
    AlreadyApplied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionCommandResult {
    pub status: InterventionCommandStatus,
    pub canonical_state: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InterventionErrorCode {
    InterventionLocatorInvalid,
    InterventionProjectMismatch,
    InterventionRequestNotFound,
    InterventionOwnerMismatch,
    InterventionActionInvalid,
    InterventionExpired,
    InterventionRevisionConflict,
    InterventionAlreadyHandled,
    InterventionRuntimeStateMismatch,
    InterventionContentTooLarge,
    InterventionStorageUnavailable,
}

impl InterventionErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InterventionLocatorInvalid => "INTERVENTION_LOCATOR_INVALID",
            Self::InterventionProjectMismatch => "INTERVENTION_PROJECT_MISMATCH",
            Self::InterventionRequestNotFound => "INTERVENTION_REQUEST_NOT_FOUND",
            Self::InterventionOwnerMismatch => "INTERVENTION_OWNER_MISMATCH",
            Self::InterventionActionInvalid => "INTERVENTION_ACTION_INVALID",
            Self::InterventionExpired => "INTERVENTION_EXPIRED",
            Self::InterventionRevisionConflict => "INTERVENTION_REVISION_CONFLICT",
            Self::InterventionAlreadyHandled => "INTERVENTION_ALREADY_HANDLED",
            Self::InterventionRuntimeStateMismatch => "INTERVENTION_RUNTIME_STATE_MISMATCH",
            Self::InterventionContentTooLarge => "INTERVENTION_CONTENT_TOO_LARGE",
            Self::InterventionStorageUnavailable => "INTERVENTION_STORAGE_UNAVAILABLE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code:?}")]
#[serde(rename_all = "camelCase")]
pub struct InterventionError {
    pub code: InterventionErrorCode,
    pub retryable: bool,
    pub details: BTreeMap<String, String>,
}

impl InterventionError {
    fn new(code: InterventionErrorCode) -> Self {
        Self {
            code,
            retryable: false,
            details: BTreeMap::new(),
        }
    }

    fn with_detail(mut self, key: &str, value: impl Into<String>) -> Self {
        self.details.insert(key.to_string(), value.into());
        self
    }

    /// Bridges a desktop command-layer failure into the shared intervention
    /// error contract without replacing the stable source code.
    pub fn from_command_error(code: impl Into<String>, params: Value) -> Self {
        let mut details = BTreeMap::from([("commandCode".to_string(), code.into())]);
        if let Some(object) = params.as_object() {
            for (key, value) in object {
                if let Some(text) = value.as_str() {
                    details.insert(key.clone(), text.to_string());
                }
            }
        }
        Self {
            code: InterventionErrorCode::InterventionRuntimeStateMismatch,
            retryable: false,
            details,
        }
    }
}

pub struct PreparedManualCheck {
    lease: ManualCheckSubmissionLease,
    command: InterventionCommand,
}

pub struct InterventionCommandService<'a> {
    app: &'a App,
}

impl<'a> InterventionCommandService<'a> {
    pub fn new(app: &'a App) -> Self {
        Self { app }
    }

    pub fn inspect(
        &self,
        locator: InterventionLocator,
        request: InterventionRequestIdentity,
    ) -> Result<InterventionSnapshot, InterventionError> {
        self.validate_locator(&locator)?;
        match request.clone() {
            InterventionRequestIdentity::Permission { request_id } => {
                self.validate_owner(&locator, true)?;
                self.inspect_permission(locator, request, &request_id)
            }
            InterventionRequestIdentity::Elicitation { elicitation_id } => {
                self.validate_owner(&locator, true)?;
                self.inspect_elicitation(locator, request, &elicitation_id)
            }
            InterventionRequestIdentity::ManualCheck => {
                self.validate_owner(&locator, false)?;
                self.inspect_manual(locator, request)
            }
        }
    }

    pub fn inspect_for_command(
        &self,
        command: &InterventionCommand,
    ) -> Result<InterventionSnapshot, InterventionError> {
        let snapshot = self.inspect(command.locator.clone(), command.request.clone())?;
        if !command.expected_state.is_empty() {
            self.validate_expected_state(&snapshot, command)?;
        }
        Ok(snapshot)
    }

    pub fn execute(
        &self,
        command: InterventionCommand,
    ) -> Result<InterventionCommandResult, InterventionError> {
        self.validate_expiry(command.expires_at_ms)?;
        match &command.action {
            InterventionAction::PermissionOption { option_id } => {
                self.execute_permission(&command, option_id)
            }
            InterventionAction::Elicitation { action, content } => {
                self.execute_elicitation(&command, action.clone(), content.clone())
            }
            InterventionAction::ManualCheck { .. } => {
                let prepared = self.prepare_manual_check(command.clone())?;
                self.commit_manual_check_background(prepared)
                    .map(|(result, _)| result)
            }
        }
    }

    pub fn prepare_manual_check(
        &self,
        command: InterventionCommand,
    ) -> Result<PreparedManualCheck, InterventionError> {
        self.validate_expiry(command.expires_at_ms)?;
        let snapshot = self.inspect(command.locator.clone(), command.request.clone())?;
        self.validate_expected_state(&snapshot, &command)?;
        let InterventionAction::ManualCheck { outcome } = command.action else {
            return Err(InterventionError::new(
                InterventionErrorCode::InterventionActionInvalid,
            ));
        };
        if !matches!(outcome, NodeOutcome::Success | NodeOutcome::Failure) {
            return Err(InterventionError::new(
                InterventionErrorCode::InterventionActionInvalid,
            ));
        }
        let lease = self
            .app
            .reserve_manual_check_submission(
                &command.locator.task_id,
                &command.locator.run_id,
                &command.locator.round_id,
                &command.locator.node_id,
                &command.locator.attempt_id,
            )
            .map_err(|_| {
                InterventionError::new(InterventionErrorCode::InterventionAlreadyHandled)
            })?;
        Ok(PreparedManualCheck {
            lease,
            command: InterventionCommand {
                action: InterventionAction::ManualCheck { outcome },
                ..command
            },
        })
    }

    pub fn commit_manual_check_background(
        &self,
        prepared: PreparedManualCheck,
    ) -> Result<(InterventionCommandResult, RunState), InterventionError> {
        let InterventionAction::ManualCheck { outcome } = prepared.command.action else {
            unreachable!("prepared manual check always contains a manual action")
        };
        let locator = prepared.command.locator;
        let run = self
            .app
            .submit_manual_check_background(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                &locator.node_id,
                &locator.attempt_id,
                outcome,
                prepared.lease,
            )
            .map_err(|_| {
                InterventionError::new(InterventionErrorCode::InterventionRuntimeStateMismatch)
            })?;
        Ok((
            InterventionCommandResult {
                status: InterventionCommandStatus::Accepted,
                canonical_state: manual_result_state(&run),
            },
            run,
        ))
    }

    fn inspect_permission(
        &self,
        locator: InterventionLocator,
        request: InterventionRequestIdentity,
        request_id: &str,
    ) -> Result<InterventionSnapshot, InterventionError> {
        let attempt_dir = self.attempt_dir(&locator);
        let pending: PendingPermissionState =
            read_json(&pending_permission_file(&attempt_dir, request_id)).map_err(|_| {
                InterventionError::new(InterventionErrorCode::InterventionRequestNotFound)
                    .with_detail("requestKind", "permission")
            })?;
        if pending.identity.interaction_id != request_id
            || pending.identity.kind != AcpPromptInteractionKind::Permission
            || permission_response_file(&attempt_dir, request_id).exists()
        {
            return Err(InterventionError::new(
                InterventionErrorCode::InterventionAlreadyHandled,
            ));
        }
        let allowed_actions = permission_options(&pending.payload);
        let prompt = permission_prompt(&pending.payload, &allowed_actions);
        let expected_state = permission_expected_state(&locator, &pending, &allowed_actions)?;
        Ok(InterventionSnapshot {
            locator,
            request,
            expected_state,
            allowed_actions,
            prompt: Some(prompt),
            expires_at_ms: None,
        })
    }

    fn inspect_elicitation(
        &self,
        locator: InterventionLocator,
        request: InterventionRequestIdentity,
        elicitation_id: &str,
    ) -> Result<InterventionSnapshot, InterventionError> {
        let attempt_dir = self.attempt_dir(&locator);
        let pending: PendingElicitationState =
            read_json(&pending_elicitation_file(&attempt_dir, elicitation_id)).map_err(|_| {
                InterventionError::new(InterventionErrorCode::InterventionRequestNotFound)
                    .with_detail("requestKind", "elicitation")
            })?;
        if pending.identity.interaction_id != elicitation_id
            || pending.identity.kind != AcpPromptInteractionKind::Elicitation
            || elicitation_response_file(&attempt_dir, elicitation_id).exists()
        {
            return Err(InterventionError::new(
                InterventionErrorCode::InterventionAlreadyHandled,
            ));
        }
        let (mut prompt, allowed_actions) =
            elicitation_prompt_and_actions(&pending.payload.request)?;
        let timeline_path = self.attempt_dir(&locator).join("acp.timeline.jsonl");
        prompt.context = read_indexed_latest_root_agent_output(&timeline_path)
            .ok()
            .flatten()
            .and_then(|output| {
                let output = output.trim();
                (!output.is_empty()
                    && normalized_prose(output) != normalized_prose(&prompt.message))
                .then(|| truncate_chars(output, MAX_MANUAL_CHECK_OUTPUT_CHARS))
            });
        let expected_state = elicitation_expected_state(&locator, &pending, &allowed_actions)?;
        Ok(InterventionSnapshot {
            locator,
            request,
            expected_state,
            allowed_actions,
            prompt: Some(prompt),
            expires_at_ms: None,
        })
    }

    fn inspect_manual(
        &self,
        locator: InterventionLocator,
        request: InterventionRequestIdentity,
    ) -> Result<InterventionSnapshot, InterventionError> {
        if locator.outer_node_id.is_some() {
            return Err(InterventionError::new(
                InterventionErrorCode::InterventionRuntimeStateMismatch,
            )
            .with_detail("requestKind", "manualCheck"));
        }
        self.app
            .validate_manual_check_submission(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                &locator.node_id,
                &locator.attempt_id,
            )
            .map_err(|_| {
                InterventionError::new(InterventionErrorCode::InterventionAlreadyHandled)
            })?;
        let run = self
            .app
            .run_status(&locator.task_id, &locator.run_id)
            .map_err(|_| {
                InterventionError::new(InterventionErrorCode::InterventionStorageUnavailable)
            })?;
        let node: NodeState = read_json(&self.app.paths.node_file(
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
        ))
        .map_err(|_| {
            InterventionError::new(InterventionErrorCode::InterventionStorageUnavailable)
        })?;
        let timeline_path = self.app.paths.acp_timeline_file(
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
        );
        let latest_output = read_indexed_latest_root_agent_output(&timeline_path)
            .map_err(|_| {
                InterventionError::new(InterventionErrorCode::InterventionStorageUnavailable)
            })?
            .filter(|output| !output.trim().is_empty())
            .ok_or_else(|| {
                InterventionError::new(InterventionErrorCode::InterventionRuntimeStateMismatch)
                    .with_detail("requestKind", "manualCheck")
                    .with_detail("latestOutput", "missing")
            })?;
        let allowed_actions = vec![
            InterventionAllowedAction::ManualSuccess,
            InterventionAllowedAction::ManualFailure,
        ];
        let expected_state = state_fingerprint(&serde_json::json!({
            "locator": locator,
            "request": request,
            "runStatus": run.status,
            "pauseReason": run.pause_reason,
            "executionPhase": run.execution.phase,
            "nodeStatus": node.status,
            "manualCheckPending": node.manual_check_pending,
            "nodeOutcome": node.outcome,
            "allowedActions": allowed_actions,
        }))?;
        Ok(InterventionSnapshot {
            locator,
            request,
            expected_state,
            allowed_actions,
            prompt: Some(InterventionPrompt {
                title: None,
                message: truncate_chars(&latest_output, MAX_MANUAL_CHECK_OUTPUT_CHARS),
                context: None,
                fields: BTreeMap::new(),
                questions: Vec::new(),
                requires_desktop: false,
            }),
            expires_at_ms: None,
        })
    }

    fn execute_permission(
        &self,
        command: &InterventionCommand,
        option_id: &str,
    ) -> Result<InterventionCommandResult, InterventionError> {
        let InterventionRequestIdentity::Permission { request_id } = &command.request else {
            return Err(InterventionError::new(
                InterventionErrorCode::InterventionActionInvalid,
            ));
        };
        let attempt_dir = self.attempt_dir(&command.locator);
        let response_path = permission_response_file(&attempt_dir, request_id);
        if response_path.exists() {
            let response: PermissionResponseState = read_json(&response_path).map_err(|_| {
                InterventionError::new(InterventionErrorCode::InterventionStorageUnavailable)
            })?;
            return if response.option_id.as_deref() == Some(option_id) && !response.cancelled {
                Ok(InterventionCommandResult {
                    status: InterventionCommandStatus::AlreadyApplied,
                    canonical_state: state_fingerprint(&response)?,
                })
            } else {
                Err(InterventionError::new(
                    InterventionErrorCode::InterventionAlreadyHandled,
                ))
            };
        }
        let snapshot = self.inspect(command.locator.clone(), command.request.clone())?;
        self.validate_expected_state(&snapshot, command)?;
        if !snapshot.allowed_actions.iter().any(|allowed| {
            matches!(
                allowed,
                InterventionAllowedAction::PermissionOption {
                    option_id: allowed_option_id,
                    ..
                } if allowed_option_id == option_id
            )
        }) {
            return Err(
                InterventionError::new(InterventionErrorCode::InterventionActionInvalid)
                    .with_detail("actionKind", "permissionOption"),
            );
        }
        let response = PermissionResponseState {
            request_id: request_id.clone(),
            option_id: Some(option_id.to_string()),
            cancelled: false,
            decided_at: current_timestamp(),
        };
        if write_permission_response_if_pending(
            &attempt_dir,
            request_id,
            response.option_id.clone(),
            response.cancelled,
            response.decided_at.clone(),
        )
        .map_err(|_| {
            InterventionError::new(InterventionErrorCode::InterventionStorageUnavailable)
        })? {
            return Ok(InterventionCommandResult {
                status: InterventionCommandStatus::Accepted,
                canonical_state: state_fingerprint(&response)?,
            });
        }
        let response: PermissionResponseState =
            read_json(&permission_response_file(&attempt_dir, request_id)).map_err(|_| {
                InterventionError::new(InterventionErrorCode::InterventionAlreadyHandled)
            })?;
        if response.option_id.as_deref() == Some(option_id) && !response.cancelled {
            Ok(InterventionCommandResult {
                status: InterventionCommandStatus::AlreadyApplied,
                canonical_state: state_fingerprint(&response)?,
            })
        } else {
            Err(InterventionError::new(
                InterventionErrorCode::InterventionAlreadyHandled,
            ))
        }
    }

    fn execute_elicitation(
        &self,
        command: &InterventionCommand,
        action: ElicitationAction,
        content: Option<Value>,
    ) -> Result<InterventionCommandResult, InterventionError> {
        let InterventionRequestIdentity::Elicitation { elicitation_id } = &command.request else {
            return Err(InterventionError::new(
                InterventionErrorCode::InterventionActionInvalid,
            ));
        };
        let attempt_dir = self.attempt_dir(&command.locator);
        let response_path = elicitation_response_file(&attempt_dir, elicitation_id);
        if response_path.exists() {
            let response: ElicitationResponseState = read_json(&response_path).map_err(|_| {
                InterventionError::new(InterventionErrorCode::InterventionStorageUnavailable)
            })?;
            return if response.action == action && response.content == content {
                Ok(InterventionCommandResult {
                    status: InterventionCommandStatus::AlreadyApplied,
                    canonical_state: state_fingerprint(&response)?,
                })
            } else {
                Err(InterventionError::new(
                    InterventionErrorCode::InterventionAlreadyHandled,
                ))
            };
        }
        if serde_json::to_vec(&content)
            .map(|value| value.len())
            .unwrap_or(MAX_INTERVENTION_CONTENT_BYTES + 1)
            > MAX_INTERVENTION_CONTENT_BYTES
        {
            return Err(InterventionError::new(
                InterventionErrorCode::InterventionContentTooLarge,
            ));
        }
        let snapshot = self.inspect(command.locator.clone(), command.request.clone())?;
        self.validate_expected_state(&snapshot, command)?;
        let pending: PendingElicitationState =
            read_json(&pending_elicitation_file(&attempt_dir, elicitation_id)).map_err(|_| {
                InterventionError::new(InterventionErrorCode::InterventionStorageUnavailable)
            })?;
        if !elicitation_response_is_valid(
            &pending.payload.request,
            &snapshot.allowed_actions,
            &action,
            content.as_ref(),
        )? {
            return Err(InterventionError::new(
                InterventionErrorCode::InterventionActionInvalid,
            ));
        }
        let response = ElicitationResponseState {
            elicitation_id: elicitation_id.clone(),
            action: action.clone(),
            content: content.clone(),
            decided_at: current_timestamp(),
        };
        if write_elicitation_response_if_pending(
            &attempt_dir,
            elicitation_id,
            action,
            content,
            response.decided_at.clone(),
        )
        .map_err(|_| {
            InterventionError::new(InterventionErrorCode::InterventionStorageUnavailable)
        })? {
            Ok(InterventionCommandResult {
                status: InterventionCommandStatus::Accepted,
                canonical_state: state_fingerprint(&response)?,
            })
        } else {
            Err(InterventionError::new(
                InterventionErrorCode::InterventionAlreadyHandled,
            ))
        }
    }

    fn validate_locator(&self, locator: &InterventionLocator) -> Result<(), InterventionError> {
        if locator.project_id != self.app.paths.project_id {
            return Err(InterventionError::new(
                InterventionErrorCode::InterventionProjectMismatch,
            ));
        }
        let required = [
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
        ];
        if required.iter().any(|value| value.trim().is_empty())
            || locator.outer_node_id.is_some() != locator.outer_attempt_id.is_some()
        {
            return Err(InterventionError::new(
                InterventionErrorCode::InterventionLocatorInvalid,
            ));
        }
        Ok(())
    }

    fn validate_owner(
        &self,
        locator: &InterventionLocator,
        allow_completed_run: bool,
    ) -> Result<(), InterventionError> {
        let run = self
            .app
            .run_status(&locator.task_id, &locator.run_id)
            .map_err(|_| {
                InterventionError::new(InterventionErrorCode::InterventionRequestNotFound)
            })?;
        let (owner_node, owner_attempt) = match (
            locator.outer_node_id.as_deref(),
            locator.outer_attempt_id.as_deref(),
        ) {
            (Some(node), Some(attempt)) => (node, attempt),
            (None, None) => (locator.node_id.as_str(), locator.attempt_id.as_str()),
            _ => unreachable!("locator shape was validated"),
        };
        if run.current_round.as_deref() != Some(locator.round_id.as_str())
            || run.current_node.as_deref() != Some(owner_node)
            || run.current_attempt.as_deref() != Some(owner_attempt)
            || (!allow_completed_run && run.status == RunStatus::Completed)
        {
            return Err(InterventionError::new(
                InterventionErrorCode::InterventionOwnerMismatch,
            ));
        }
        Ok(())
    }

    fn validate_expected_state(
        &self,
        snapshot: &InterventionSnapshot,
        command: &InterventionCommand,
    ) -> Result<(), InterventionError> {
        if snapshot.expected_state != command.expected_state {
            return Err(InterventionError::new(
                InterventionErrorCode::InterventionRevisionConflict,
            ));
        }
        Ok(())
    }

    fn validate_expiry(&self, expires_at_ms: Option<i64>) -> Result<(), InterventionError> {
        if expires_at_ms.is_some_and(|expires_at| Utc::now().timestamp_millis() >= expires_at) {
            return Err(InterventionError::new(
                InterventionErrorCode::InterventionExpired,
            ));
        }
        Ok(())
    }

    fn attempt_dir(&self, locator: &InterventionLocator) -> Utf8PathBuf {
        match (
            locator.outer_node_id.as_deref(),
            locator.outer_attempt_id.as_deref(),
        ) {
            (Some(outer_node), Some(outer_attempt)) => self.app.paths.dynamic_node_attempt_dir(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                outer_node,
                outer_attempt,
                &locator.node_id,
                &locator.attempt_id,
            ),
            _ => self.app.paths.attempt_dir(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                &locator.node_id,
                &locator.attempt_id,
            ),
        }
    }
}

fn permission_options(params: &Value) -> Vec<InterventionAllowedAction> {
    params
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            let option_id = option
                .get("optionId")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())?;
            let name = option
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            let kind = option
                .get("kind")
                .and_then(Value::as_str)
                .map(permission_action_kind)
                .unwrap_or(PermissionActionKind::Unknown);
            Some(InterventionAllowedAction::PermissionOption {
                option_id: option_id.to_string(),
                name,
                permission_kind: kind,
            })
        })
        .collect()
}

fn permission_action_kind(value: &str) -> PermissionActionKind {
    match value {
        "allow_once" => PermissionActionKind::AllowOnce,
        "allow" => PermissionActionKind::AllowOnce,
        "allow_always" => PermissionActionKind::AllowAlways,
        "allow_for_session" => PermissionActionKind::AllowAlways,
        "reject_once" => PermissionActionKind::RejectOnce,
        "reject" => PermissionActionKind::RejectOnce,
        "cancel" => PermissionActionKind::RejectOnce,
        "reject_always" => PermissionActionKind::RejectAlways,
        _ => PermissionActionKind::Unknown,
    }
}

pub fn permission_action_is_safe_remote_reject(action: &InterventionAllowedAction) -> bool {
    matches!(
        action,
        InterventionAllowedAction::PermissionOption {
            permission_kind: PermissionActionKind::RejectOnce | PermissionActionKind::RejectAlways,
            ..
        }
    )
}

pub fn permission_action_qualifier(option_id: &str, name: &str) -> PermissionActionQualifier {
    let identity = format!("{option_id} {name}").to_ascii_lowercase();
    if identity.contains("bypass") {
        PermissionActionQualifier::BypassPermissions
    } else if identity.contains("auto-accept")
        || (identity.contains("accept") && identity.contains("edit"))
    {
        PermissionActionQualifier::AutoEdit
    } else if identity.contains("auto") {
        PermissionActionQualifier::AutoMode
    } else if identity.contains("manual") {
        PermissionActionQualifier::ManualApproval
    } else if identity.contains("keep planning") {
        PermissionActionQualifier::KeepPlanning
    } else {
        PermissionActionQualifier::Standard
    }
}

pub fn permission_actions_require_desktop(actions: &[InterventionAllowedAction]) -> bool {
    let mut allow_once_counts = [0usize; 6];
    let mut allow_always_counts = [0usize; 6];
    for action in actions {
        let InterventionAllowedAction::PermissionOption {
            option_id,
            name,
            permission_kind,
        } = action
        else {
            continue;
        };
        if *permission_kind == PermissionActionKind::Unknown {
            return true;
        }
        if matches!(
            permission_kind,
            PermissionActionKind::AllowOnce | PermissionActionKind::AllowAlways
        ) {
            let qualifier_index = match permission_action_qualifier(option_id, name) {
                PermissionActionQualifier::Standard => 0,
                PermissionActionQualifier::BypassPermissions => 1,
                PermissionActionQualifier::AutoEdit => 2,
                PermissionActionQualifier::AutoMode => 3,
                PermissionActionQualifier::ManualApproval => 4,
                PermissionActionQualifier::KeepPlanning => 5,
            };
            let counts = match permission_kind {
                PermissionActionKind::AllowOnce => &mut allow_once_counts,
                PermissionActionKind::AllowAlways => &mut allow_always_counts,
                PermissionActionKind::RejectOnce
                | PermissionActionKind::RejectAlways
                | PermissionActionKind::Unknown => unreachable!("reject kinds are excluded"),
            };
            if counts[qualifier_index] > 0 {
                return true;
            }
            counts[qualifier_index] += 1;
        }
    }
    false
}

fn permission_prompt(
    params: &Value,
    allowed_actions: &[InterventionAllowedAction],
) -> InterventionPrompt {
    let tool_call = params.get("toolCall").unwrap_or(params);
    let raw_input = tool_call.get("rawInput").unwrap_or(params);
    let permission_meta = params.pointer("/_meta/permission");
    let title = permission_meta
        .and_then(|meta| meta.get("title"))
        .or_else(|| tool_call.get("title"))
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty());
    let tool = tool_call
        .get("title")
        .and_then(Value::as_str)
        .filter(|tool| !tool.trim().is_empty());
    let raw_message = permission_meta
        .and_then(|meta| meta.get("description"))
        .or_else(|| raw_input.get("description"))
        .and_then(Value::as_str)
        .filter(|description| !description.trim().is_empty())
        .map(str::to_string);
    let command = permission_command(raw_input);
    let parameter = if command.is_some() {
        None
    } else {
        permission_parameter(raw_input)
    };
    let message = raw_message
        .or_else(|| parameter.clone())
        .map(|message| truncate_chars(&message, MAX_PERMISSION_PROMPT_CHARS))
        .unwrap_or_default();

    let mut fields = BTreeMap::new();
    if let Some(tool) = tool {
        fields.insert(
            PERMISSION_TOOL_FIELD.into(),
            truncate_chars(tool, MAX_PERMISSION_FIELD_CHARS),
        );
    }
    let paths = permission_paths(tool_call, raw_input);
    if !paths.is_empty() {
        fields.insert(
            PERMISSION_PATH_FIELD.into(),
            truncate_chars(&paths, MAX_PERMISSION_FIELD_CHARS),
        );
    }
    if let Some(command) = command.as_deref() {
        fields.insert(
            PERMISSION_COMMAND_FIELD.into(),
            truncate_chars(command, MAX_PERMISSION_FIELD_CHARS),
        );
    }
    if let Some(parameter) = parameter.as_deref() {
        fields.insert(
            PERMISSION_PARAMETER_FIELD.into(),
            truncate_chars(&parameter, MAX_PERMISSION_FIELD_CHARS),
        );
    }

    InterventionPrompt {
        title: title.map(|title| truncate_chars(title, MAX_PERMISSION_FIELD_CHARS)),
        message,
        context: None,
        fields,
        questions: Vec::new(),
        requires_desktop: allowed_actions.len() > MAX_REMOTE_PERMISSION_OPTIONS
            || permission_actions_require_desktop(allowed_actions),
    }
}

fn permission_paths(tool_call: &Value, raw_input: &Value) -> String {
    let mut paths = tool_call
        .get("locations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|location| location.get("path").and_then(Value::as_str))
        .filter(|path| !path.trim().is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if paths.is_empty()
        && let Some(path) = raw_input.get("path").and_then(Value::as_str)
        && !path.trim().is_empty()
    {
        paths.push(path.to_owned());
    }
    if paths.is_empty()
        && let Some(cwd) = raw_input.get("cwd").and_then(Value::as_str)
        && !cwd.trim().is_empty()
    {
        paths.push(cwd.to_owned());
    }
    let hidden = paths.len().saturating_sub(MAX_PERMISSION_PATHS);
    let mut text = paths
        .into_iter()
        .take(MAX_PERMISSION_PATHS)
        .collect::<Vec<_>>()
        .join("; ");
    if hidden > 0 {
        text.push_str(&format!("; +{hidden}"));
    }
    text
}

fn permission_command(raw_input: &Value) -> Option<String> {
    let command = scalar_text(raw_input.get("command"))?;
    let mut parts = vec![command];
    if let Some(arguments) = raw_input.get("args").and_then(Value::as_array) {
        parts.extend(
            arguments
                .iter()
                .filter_map(|argument| scalar_text(Some(argument))),
        );
    }
    let command = parts.join(" ");
    (!command.trim().is_empty()).then(|| command.trim().to_owned())
}

fn permission_parameter(raw_input: &Value) -> Option<String> {
    PERMISSION_PARAMETER_KEYS
        .into_iter()
        .find_map(|key| scalar_text(raw_input.get(key)).filter(|value| !value.is_empty()))
}

fn scalar_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.trim().to_owned()).filter(|text| !text.is_empty()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Array(values) => {
            let items = values
                .iter()
                .filter_map(|value| scalar_text(Some(value)))
                .collect::<Vec<_>>();
            (!items.is_empty()).then(|| items.join("; "))
        }
        Value::Object(_) | Value::Null => None,
    }
}

fn elicitation_prompt_and_actions(
    request: &agent_client_protocol_schema::v1::CreateElicitationRequest,
) -> Result<(InterventionPrompt, Vec<InterventionAllowedAction>), InterventionError> {
    let request = serde_json::to_value(request).map_err(|_| {
        InterventionError::new(InterventionErrorCode::InterventionStorageUnavailable)
    })?;
    let message = truncate_chars(
        request
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        MAX_ELICITATION_PROMPT_CHARS,
    );
    let properties = request
        .get("requestedSchema")
        .and_then(|schema| schema.get("properties"))
        .and_then(Value::as_object);
    let custom_answer_targets = properties
        .into_iter()
        .flat_map(|properties| properties.values())
        .filter_map(custom_answer_target)
        .collect::<std::collections::BTreeSet<_>>();
    let questions = properties
        .into_iter()
        .flat_map(|properties| properties.iter())
        .filter(|(_, schema)| custom_answer_target(schema).is_none())
        .map(|(field_name, schema)| {
            elicitation_question(
                field_name,
                schema,
                custom_answer_targets.contains(field_name.as_str()),
            )
        })
        .collect::<Vec<_>>();

    let form =
        properties.and_then(|_| remote_elicitation_form(&request, &questions).ok().flatten());
    let mut actions = Vec::new();
    if let Some(form) = form.as_ref() {
        actions.push(InterventionAllowedAction::ElicitationFixedForm { form: form.clone() });
    } else if questions.is_empty() {
        actions.push(InterventionAllowedAction::ElicitationAccept);
    }
    actions.push(InterventionAllowedAction::ElicitationDecline);
    let schema_is_empty = properties.is_some_and(|properties| properties.is_empty());

    Ok((
        InterventionPrompt {
            title: None,
            message,
            context: None,
            fields: BTreeMap::new(),
            requires_desktop: !schema_is_empty && form.is_none(),
            questions,
        },
        actions,
    ))
}

fn remote_elicitation_form(
    request: &Value,
    questions: &[ElicitationQuestion],
) -> Result<Option<RemoteElicitationForm>, InterventionError> {
    if remote_schema_has_unsupported_constraints(request) {
        return Ok(None);
    }
    if questions
        .iter()
        .any(|question| question.field_name.len() > MAX_REMOTE_ELICITATION_FIELD_NAME_BYTES)
    {
        return Ok(None);
    }
    let required = required_fields(request);
    let schema_is_valid = |content: &Value| -> Result<bool, InterventionError> {
        let schema = request
            .get("requestedSchema")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({ "type": "object" }));
        let compiled = jsonschema::JSONSchema::compile(&schema).map_err(|_| {
            InterventionError::new(InterventionErrorCode::InterventionActionInvalid)
                .with_detail("actionKind", "elicitationSchema")
        })?;
        Ok(compiled.is_valid(content))
    };
    let scalar_options = |question: &ElicitationQuestion, capacity: usize| {
        remote_scalar_options(&question.options, capacity)
    };

    if let [question] = questions {
        let selector = "q0".to_owned();
        match question.question_kind {
            ElicitationQuestionKind::SingleSelect => {
                let Some(options) = scalar_options(question, MAX_REMOTE_ELICITATION_VOTE_OPTIONS)
                else {
                    return Ok(None);
                };
                let remote_question = remote_scalar_choice_question(
                    selector,
                    question,
                    required.contains(&question.field_name),
                    options,
                );
                let selected = remote_question
                    .options
                    .first()
                    .map(|option| option.value.clone())
                    .unwrap_or(Value::Null);
                let candidate = elicitation_choice_content(&remote_question.field_name, &selected);
                if !schema_is_valid(&candidate)? {
                    return Ok(None);
                }
                return Ok(Some(RemoteElicitationForm::SingleScalarChoice {
                    question: remote_question,
                }));
            }
            ElicitationQuestionKind::MultiSelect => {
                if question.allows_custom_answer {
                    return Ok(None);
                }
                let Some(options) = scalar_options(question, MAX_REMOTE_ELICITATION_VOTE_OPTIONS)
                else {
                    return Ok(None);
                };
                let remote_question = remote_scalar_choice_question(
                    selector,
                    question,
                    required.contains(&question.field_name),
                    options,
                );
                let field_name = remote_question.field_name.clone();
                let first = remote_question
                    .options
                    .first()
                    .map(|option| option.value.clone())
                    .unwrap_or(Value::Null);
                if !schema_is_valid(&Value::Object(serde_json::Map::from_iter([(
                    field_name.clone(),
                    Value::Array(vec![first]),
                )])))? {
                    return Ok(None);
                }
                let allows_empty =
                    schema_is_valid(&Value::Object(serde_json::Map::from_iter([(
                        field_name,
                        Value::Array(Vec::new()),
                    )])))?;
                return Ok(Some(RemoteElicitationForm::MultiScalarChoice {
                    question: remote_question,
                    allows_empty,
                }));
            }
            ElicitationQuestionKind::FreeText => return Ok(None),
        }
    }

    if questions.len() < 2 || questions.len() > MAX_REMOTE_ELICITATION_SELECTORS {
        return Ok(None);
    }
    let mut remote_questions = Vec::with_capacity(questions.len());
    for (index, question) in questions.iter().enumerate() {
        if question.question_kind != ElicitationQuestionKind::SingleSelect {
            return Ok(None);
        }
        let Some(options) = scalar_options(question, MAX_REMOTE_ELICITATION_SELECT_OPTIONS) else {
            return Ok(None);
        };
        remote_questions.push(remote_scalar_choice_question(
            format!("q{index}"),
            question,
            required.contains(&question.field_name),
            options,
        ));
    }
    let mut required_content = serde_json::Map::new();
    let mut complete_content = serde_json::Map::new();
    for question in &remote_questions {
        let value = question
            .options
            .first()
            .map(|option| option.value.clone())
            .unwrap_or(Value::Null);
        complete_content.insert(question.field_name.clone(), value.clone());
        if question.required {
            required_content.insert(question.field_name.clone(), value);
        }
    }
    if !schema_is_valid(&Value::Object(required_content.clone()))?
        || !schema_is_valid(&Value::Object(complete_content))?
    {
        return Ok(None);
    }
    Ok(Some(RemoteElicitationForm::ScalarChoiceQuestions {
        questions: remote_questions,
    }))
}

fn remote_schema_has_unsupported_constraints(request: &Value) -> bool {
    let Some(schema) = request.get("requestedSchema") else {
        return true;
    };
    let Some(fields) = schema.as_object() else {
        return true;
    };
    let supported = [
        "type",
        "properties",
        "required",
        "additionalProperties",
        "title",
        "description",
        "default",
        "examples",
        "deprecated",
        "readOnly",
        "writeOnly",
    ];
    fields.keys().any(|key| !supported.contains(&key.as_str()))
}

fn required_fields(request: &Value) -> std::collections::BTreeSet<String> {
    request
        .pointer("/requestedSchema/required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn remote_scalar_options(
    options: &[ElicitationQuestionOption],
    capacity: usize,
) -> Option<Vec<RemoteElicitationOption>> {
    if options.is_empty() || options.len() > capacity {
        return None;
    }
    let mut values = std::collections::HashSet::new();
    let mut remote = Vec::with_capacity(options.len());
    for option in options {
        if option.value.is_object()
            || option.value.is_array()
            || option.value.is_null()
            || serde_json::to_vec(&option.value)
                .map(|encoded| encoded.len())
                .unwrap_or(MAX_ELICITATION_OPTION_VALUE_BYTES + 1)
                > MAX_ELICITATION_OPTION_VALUE_BYTES
            || !values.insert(option.value.clone())
        {
            return None;
        }
        remote.push(RemoteElicitationOption {
            value: option.value.clone(),
            label: option.label.clone(),
            description: option.description.clone(),
        });
    }
    Some(remote)
}

fn remote_scalar_choice_question(
    selector_key: String,
    question: &ElicitationQuestion,
    required: bool,
    options: Vec<RemoteElicitationOption>,
) -> RemoteScalarChoiceQuestion {
    RemoteScalarChoiceQuestion {
        selector_key,
        field_name: question.field_name.clone(),
        title: question.title.clone(),
        description: question.description.clone(),
        required,
        options,
    }
}

fn custom_answer_target(schema: &Value) -> Option<&str> {
    schema
        .pointer("/_meta/_askUserQuestionCustomAnswer")
        .filter(|metadata| {
            metadata
                .get("isCustomAnswer")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })?
        .get("questionId")
        .and_then(Value::as_str)
}

fn elicitation_question(
    field_name: &str,
    schema: &Value,
    allows_custom_answer: bool,
) -> ElicitationQuestion {
    let (question_kind, options) = if schema.get("type").and_then(Value::as_str) == Some("array") {
        let items = schema.get("items").unwrap_or(&Value::Null);
        (
            ElicitationQuestionKind::MultiSelect,
            elicitation_options(items),
        )
    } else {
        let options = elicitation_options(schema);
        let question_kind = if options.is_empty() {
            ElicitationQuestionKind::FreeText
        } else {
            ElicitationQuestionKind::SingleSelect
        };
        (question_kind, options)
    };
    let title = schema
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or(field_name);
    ElicitationQuestion {
        field_name: field_name.to_owned(),
        title: truncate_chars(title, MAX_ELICITATION_QUESTION_CHARS),
        description: schema
            .get("description")
            .and_then(Value::as_str)
            .filter(|description| !description.trim().is_empty())
            .map(|description| {
                truncate_chars(description, MAX_ELICITATION_OPTION_DESCRIPTION_CHARS)
            }),
        question_kind,
        options,
        allows_custom_answer,
    }
}

fn elicitation_options(schema: &Value) -> Vec<ElicitationQuestionOption> {
    if let Some(variants) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(Value::as_array)
    {
        return variants
            .iter()
            .filter_map(|variant| {
                let value = variant.get("const")?.clone();
                Some(ElicitationQuestionOption {
                    label: option_label(variant.get("title").and_then(Value::as_str), &value),
                    description: variant
                        .get("description")
                        .and_then(Value::as_str)
                        .filter(|description| !description.trim().is_empty())
                        .map(|description| {
                            truncate_chars(description, MAX_ELICITATION_OPTION_DESCRIPTION_CHARS)
                        }),
                    value,
                })
            })
            .collect();
    }
    schema
        .get("enum")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .cloned()
        .map(|value| ElicitationQuestionOption {
            label: option_label(None, &value),
            description: None,
            value,
        })
        .collect()
}

fn option_label(title: Option<&str>, value: &Value) -> String {
    let label = title
        .filter(|title| !title.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| value.to_string());
    truncate_chars(&label, MAX_ELICITATION_OPTION_LABEL_CHARS)
}

fn elicitation_response_is_valid(
    request: &agent_client_protocol_schema::v1::CreateElicitationRequest,
    allowed_actions: &[InterventionAllowedAction],
    action: &ElicitationAction,
    content: Option<&Value>,
) -> Result<bool, InterventionError> {
    match action {
        ElicitationAction::Decline => Ok(content.is_none()
            && allowed_actions.contains(&InterventionAllowedAction::ElicitationDecline)),
        ElicitationAction::Accept => {
            let published_action = match content {
                Some(content) => {
                    content.is_object()
                        && (allowed_actions.iter().any(|allowed| {
                            matches!(
                                allowed,
                                InterventionAllowedAction::ElicitationFixedForm { .. }
                            )
                        }) || allowed_actions
                            .contains(&InterventionAllowedAction::ElicitationAccept))
                }
                None => allowed_actions.contains(&InterventionAllowedAction::ElicitationAccept),
            };
            if !published_action {
                return Ok(false);
            }
            let request = serde_json::to_value(request).map_err(|_| {
                InterventionError::new(InterventionErrorCode::InterventionStorageUnavailable)
            })?;
            let schema = request
                .get("requestedSchema")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({ "type": "object" }));
            let instance = content.cloned().unwrap_or_else(|| serde_json::json!({}));
            let compiled = jsonschema::JSONSchema::compile(&schema).map_err(|_| {
                InterventionError::new(InterventionErrorCode::InterventionActionInvalid)
                    .with_detail("actionKind", "elicitationSchema")
            })?;
            Ok(compiled.is_valid(&instance))
        }
    }
}

pub fn intervention_action_for_elicitation_form(
    form: &RemoteElicitationForm,
    selections: &[RemoteElicitationFormSelection],
) -> Result<InterventionAction, InterventionError> {
    let invalid = || {
        InterventionError::new(InterventionErrorCode::InterventionActionInvalid)
            .with_detail("actionKind", "elicitationFormSelection")
    };
    let option_value = |question: &RemoteScalarChoiceQuestion,
                        option_id: &str|
     -> Result<Value, InterventionError> {
        if option_id.len() > 3 {
            return Err(invalid());
        }
        let index = option_id.parse::<usize>().map_err(|_| invalid())?;
        question
            .options
            .get(index)
            .map(|option| option.value.clone())
            .ok_or_else(invalid)
    };

    let content = match form {
        RemoteElicitationForm::SingleScalarChoice { question } => {
            let [selection] = selections else {
                return Err(invalid());
            };
            if selection.selector_key != question.selector_key || selection.option_ids.len() != 1 {
                return Err(invalid());
            }
            elicitation_choice_content(
                &question.field_name,
                &option_value(question, &selection.option_ids[0])?,
            )
        }
        RemoteElicitationForm::MultiScalarChoice {
            question,
            allows_empty,
        } => {
            let [selection] = selections else {
                return Err(invalid());
            };
            if selection.selector_key != question.selector_key
                || (selection.option_ids.is_empty() && !allows_empty)
            {
                return Err(invalid());
            }
            let mut seen = std::collections::HashSet::new();
            let mut values = Vec::with_capacity(selection.option_ids.len());
            for option_id in &selection.option_ids {
                let value = option_value(question, option_id)?;
                if !seen.insert(value.clone()) {
                    return Err(invalid());
                }
                values.push(value);
            }
            elicitation_choice_content(&question.field_name, &Value::Array(values))
        }
        RemoteElicitationForm::ScalarChoiceQuestions { questions } => {
            if selections.len() > questions.len() {
                return Err(invalid());
            }
            let mut content = serde_json::Map::new();
            let mut selectors = std::collections::BTreeSet::new();
            for selection in selections {
                if !selectors.insert(selection.selector_key.clone()) {
                    return Err(invalid());
                }
                let Some(question) = questions
                    .iter()
                    .find(|question| question.selector_key == selection.selector_key)
                else {
                    return Err(invalid());
                };
                if selection.option_ids.len() != 1 {
                    return Err(invalid());
                }
                content.insert(
                    question.field_name.clone(),
                    option_value(question, &selection.option_ids[0])?,
                );
            }
            if questions
                .iter()
                .any(|question| question.required && !selectors.contains(&question.selector_key))
            {
                return Err(invalid());
            }
            Value::Object(content)
        }
    };
    Ok(InterventionAction::Elicitation {
        action: ElicitationAction::Accept,
        content: Some(content),
    })
}

fn remote_elicitation_form_allows_content(form: &RemoteElicitationForm, content: &Value) -> bool {
    let Some(fields) = content.as_object() else {
        return false;
    };
    let questions = match form {
        RemoteElicitationForm::SingleScalarChoice { question }
        | RemoteElicitationForm::MultiScalarChoice { question, .. } => {
            std::slice::from_ref(question)
        }
        RemoteElicitationForm::ScalarChoiceQuestions { questions } => questions,
    };
    let field_names = questions
        .iter()
        .map(|question| question.field_name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if fields
        .keys()
        .any(|field| !field_names.contains(field.as_str()))
    {
        return false;
    }
    questions
        .iter()
        .all(|question| match fields.get(&question.field_name) {
            None => !question.required,
            Some(Value::Array(values)) => {
                matches!(form, RemoteElicitationForm::MultiScalarChoice { .. })
                    && values.len() <= question.options.len()
                    && values
                        .iter()
                        .all(|value| question.options.iter().any(|option| option.value == *value))
            }
            Some(value) => {
                matches!(
                    form,
                    RemoteElicitationForm::SingleScalarChoice { .. }
                        | RemoteElicitationForm::ScalarChoiceQuestions { .. }
                ) && question.options.iter().any(|option| option.value == *value)
            }
        })
}

pub fn intervention_action_is_allowed(
    allowed_actions: &[InterventionAllowedAction],
    action: &InterventionAction,
) -> bool {
    allowed_actions
        .iter()
        .any(|allowed| match (allowed, action) {
            (
                InterventionAllowedAction::PermissionOption {
                    option_id: allowed, ..
                },
                InterventionAction::PermissionOption { option_id: actual },
            ) => allowed == actual,
            (
                InterventionAllowedAction::ElicitationFixedForm { form },
                InterventionAction::Elicitation {
                    action: ElicitationAction::Accept,
                    content: Some(content),
                },
            ) => remote_elicitation_form_allows_content(form, content),
            (
                InterventionAllowedAction::ElicitationAccept,
                InterventionAction::Elicitation {
                    action: ElicitationAction::Accept,
                    content: None,
                },
            ) => true,
            (
                InterventionAllowedAction::ElicitationDecline,
                InterventionAction::Elicitation {
                    action: ElicitationAction::Decline,
                    content: None,
                },
            ) => true,
            (
                InterventionAllowedAction::ManualSuccess,
                InterventionAction::ManualCheck {
                    outcome: NodeOutcome::Success,
                },
            )
            | (
                InterventionAllowedAction::ManualFailure,
                InterventionAction::ManualCheck {
                    outcome: NodeOutcome::Failure,
                },
            ) => true,
            _ => false,
        })
}

pub fn elicitation_choice_content(field_name: &str, value: &Value) -> Value {
    Value::Object(serde_json::Map::from_iter([(
        field_name.to_string(),
        value.clone(),
    )]))
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_none() {
        return prefix;
    }
    let keep = max_chars.saturating_sub(3);
    format!("{}...", value.chars().take(keep).collect::<String>())
}

fn normalized_prose(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn permission_expected_state(
    locator: &InterventionLocator,
    pending: &PendingPermissionState,
    allowed_actions: &[InterventionAllowedAction],
) -> Result<String, InterventionError> {
    state_fingerprint(&serde_json::json!({
        "locator": locator,
        "request": {
            "identity": &pending.identity,
            "params": &pending.payload,
            "createdAt": &pending.created_at,
        },
        "allowedActions": allowed_actions,
    }))
}

fn elicitation_expected_state(
    locator: &InterventionLocator,
    pending: &PendingElicitationState,
    allowed_actions: &[InterventionAllowedAction],
) -> Result<String, InterventionError> {
    state_fingerprint(&serde_json::json!({
        "locator": locator,
        "request": {
            "identity": &pending.identity,
            "jsonrpcId": &pending.payload.jsonrpc_id,
            "request": &pending.payload.request,
            "createdAt": &pending.created_at,
        },
        "allowedActions": allowed_actions,
    }))
}

fn state_fingerprint(value: &impl Serialize) -> Result<String, InterventionError> {
    let bytes = serde_json::to_vec(value).map_err(|_| {
        InterventionError::new(InterventionErrorCode::InterventionStorageUnavailable)
    })?;
    let mut hasher = Hasher::new();
    hasher.update(&bytes);
    Ok(hasher.finalize().to_hex().to_string())
}

fn manual_result_state(run: &RunState) -> String {
    format!("{}:{}", run.execution.revision, run.updated_at)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use camino::Utf8PathBuf;
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::{
        acp::{
            elicitation::{bind_pending_elicitation_timeline_identity, write_pending_elicitation},
            permission::{bind_pending_permission_timeline_identity, write_pending_permission},
            timeline::TimelineItemIdentity,
        },
        domain::{NodeType, PauseReason, VERSION},
        runtime::{
            CURRENT_ACP_STORAGE_SCHEMA_VERSION, RunState, RuntimeExecutionPhase,
            RuntimeExecutionState,
        },
        storage::write_json,
    };

    struct Fixture {
        _temp: TempDir,
        app: App,
        locator: InterventionLocator,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.app.paths.runtime_root.as_std_path());
        }
    }

    fn permission_fixture(outer: bool) -> Fixture {
        let temp = tempfile::tempdir().expect("temp fixture");
        let repo = Utf8PathBuf::from_path_buf(temp.path().join("repo")).expect("utf8 path");
        let app = App::new(repo);
        let locator = InterventionLocator {
            project_id: app.paths.project_id.clone(),
            task_id: "task-001".into(),
            run_id: "run-001".into(),
            round_id: "round-001".into(),
            node_id: if outer { "leaf" } else { "worker" }.into(),
            attempt_id: if outer { "leaf-attempt" } else { "attempt-001" }.into(),
            outer_node_id: outer.then(|| "dynamic".into()),
            outer_attempt_id: outer.then(|| "dynamic-attempt".into()),
        };
        let run = RunState {
            version: VERSION.to_string(),
            id: locator.run_id.clone(),
            task_id: locator.task_id.clone(),
            task_uuid: None,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "2026-08-30T00:00:00Z".into(),
            updated_at: "2026-08-30T00:00:01Z".into(),
            workflow_snapshot: "workflow.snapshot.json".into(),
            current_round: Some(locator.round_id.clone()),
            current_node: Some(
                locator
                    .outer_node_id
                    .clone()
                    .unwrap_or_else(|| locator.node_id.clone()),
            ),
            current_attempt: Some(
                locator
                    .outer_attempt_id
                    .clone()
                    .unwrap_or_else(|| locator.attempt_id.clone()),
            ),
            new_rounds_opened: 0,
            pause_reason: Some(PauseReason::PermissionRequested),
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: Default::default(),
        };
        write_json(&app.paths.run_file(&locator.task_id, &locator.run_id), &run)
            .expect("run fixture");
        let attempt_dir = InterventionCommandService::new(&app).attempt_dir(&locator);
        write_json(
            &attempt_dir.join("node.json"),
            &NodeState {
                version: VERSION.to_string(),
                acp_storage_schema_version: CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                node_id: locator.node_id.clone(),
                node_type: NodeType::Worker,
                run_id: locator.run_id.clone(),
                round_id: locator.round_id.clone(),
                attempt_id: locator.attempt_id.clone(),
                status: RunStatus::Paused,
                outcome: None,
                started_at: "2026-08-30T00:00:00Z".into(),
                finished_at: None,
                manual_check_pending: false,
                runtime_execution_id: None,
                resolved_config: Default::default(),
                uuid: None,
            },
        )
        .expect("node fixture");
        write_pending_permission(
            &attempt_dir,
            "permission-001",
            "turn-1",
            "prompt-event-1",
            json!({
                "sessionId": "session-001",
                "options": [
                    { "optionId": "allow", "name": "Allow", "kind": "allow_once" },
                    { "optionId": "deny", "name": "Deny", "kind": "reject_once" }
                ]
            }),
            "2026-08-30T00:00:01Z".into(),
        )
        .expect("pending permission fixture");
        Fixture {
            _temp: temp,
            app,
            locator,
        }
    }

    fn permission_request() -> InterventionRequestIdentity {
        InterventionRequestIdentity::Permission {
            request_id: "permission-001".into(),
        }
    }

    fn permission_command(snapshot: &InterventionSnapshot, option_id: &str) -> InterventionCommand {
        InterventionCommand {
            locator: snapshot.locator.clone(),
            request: snapshot.request.clone(),
            expected_state: snapshot.expected_state.clone(),
            action: InterventionAction::PermissionOption {
                option_id: option_id.into(),
            },
            expires_at_ms: snapshot.expires_at_ms,
        }
    }

    fn write_choice_elicitation(fixture: &Fixture, option_values: &[&str]) {
        let request = serde_json::from_value(serde_json::json!({
            "mode": "form",
            "sessionId": "session-001",
            "message": "请选择要使用的数据库？",
            "requestedSchema": {
                "type": "object",
                "properties": {
                    "question_0": {
                        "type": "string",
                        "title": "数据库选择",
                        "oneOf": option_values.iter().map(|value| serde_json::json!({
                            "const": value,
                            "title": value,
                            "description": format!("使用 {value}")
                        })).collect::<Vec<_>>()
                    },
                    "question_0_custom": {
                        "type": "string",
                        "title": "Other",
                        "_meta": {
                            "_askUserQuestionCustomAnswer": {
                                "questionId": "question_0",
                                "isCustomAnswer": true
                            }
                        }
                    }
                }
            }
        }))
        .unwrap();
        write_elicitation(fixture, request);
    }

    fn write_elicitation(
        fixture: &Fixture,
        request: agent_client_protocol_schema::v1::CreateElicitationRequest,
    ) {
        let attempt_dir =
            InterventionCommandService::new(&fixture.app).attempt_dir(&fixture.locator);
        write_pending_elicitation(
            &attempt_dir,
            &crate::acp::elicitation::pending_elicitation_state(
                "elicit-001",
                "turn-1",
                "prompt-event-1",
                serde_json::json!(7),
                request,
                "2026-08-30T00:00:01Z".into(),
            ),
        )
        .unwrap();
    }

    #[test]
    fn inspect_requires_complete_project_scoped_locator_and_outer_owner() {
        let fixture = permission_fixture(true);
        let service = InterventionCommandService::new(&fixture.app);
        let snapshot = service
            .inspect(fixture.locator.clone(), permission_request())
            .expect("outer locator is valid");
        assert_eq!(snapshot.locator.outer_node_id.as_deref(), Some("dynamic"));
        assert_eq!(snapshot.allowed_actions.len(), 2);

        let mut foreign = fixture.locator.clone();
        foreign.project_id = "foreign-project".into();
        assert_eq!(
            service
                .inspect(foreign, permission_request())
                .unwrap_err()
                .code,
            InterventionErrorCode::InterventionProjectMismatch
        );

        let mut incomplete = fixture.locator.clone();
        incomplete.outer_attempt_id = None;
        assert_eq!(
            service
                .inspect(incomplete, permission_request())
                .unwrap_err()
                .code,
            InterventionErrorCode::InterventionLocatorInvalid
        );

        let mut historical = fixture.locator.clone();
        historical.outer_attempt_id = Some("old-attempt".into());
        assert_eq!(
            service
                .inspect(historical, permission_request())
                .unwrap_err()
                .code,
            InterventionErrorCode::InterventionOwnerMismatch
        );
    }

    #[test]
    fn expected_state_allowed_action_and_expiry_are_revalidated() {
        let fixture = permission_fixture(false);
        let service = InterventionCommandService::new(&fixture.app);
        let snapshot = service
            .inspect(fixture.locator.clone(), permission_request())
            .unwrap();
        let invalid = service
            .execute(permission_command(&snapshot, "not-allowed"))
            .unwrap_err();
        assert_eq!(
            invalid.code,
            InterventionErrorCode::InterventionActionInvalid
        );

        let mut expired = permission_command(&snapshot, "allow");
        expired.expires_at_ms = Some(0);
        assert_eq!(
            service.execute(expired).unwrap_err().code,
            InterventionErrorCode::InterventionExpired
        );

        let attempt_dir = service.attempt_dir(&fixture.locator);
        write_pending_permission(
            &attempt_dir,
            "permission-001",
            "turn-1",
            "prompt-event-1",
            json!({ "options": [{ "optionId": "allow", "name": "Allow", "kind": "allow_once" }] }),
            "2026-08-30T00:00:02Z".into(),
        )
        .unwrap();
        assert_eq!(
            service
                .execute(permission_command(&snapshot, "allow"))
                .unwrap_err()
                .code,
            InterventionErrorCode::InterventionRevisionConflict
        );
    }

    #[test]
    fn permission_timeline_binding_does_not_invalidate_im_snapshot() {
        let fixture = permission_fixture(false);
        let service = InterventionCommandService::new(&fixture.app);
        let snapshot = service
            .inspect(fixture.locator.clone(), permission_request())
            .unwrap();
        bind_pending_permission_timeline_identity(
            &service.attempt_dir(&fixture.locator),
            "permission-001",
            TimelineItemIdentity {
                branch_id: "root".into(),
                item_id: "permission-0".into(),
                revision: 78,
            },
        )
        .unwrap();

        let result = service.execute(permission_command(&snapshot, "allow"));
        assert_eq!(
            result
                .expect("timeline metadata is not decision state")
                .status,
            InterventionCommandStatus::Accepted
        );
    }

    #[test]
    fn permission_snapshot_projects_structured_details_and_safe_capacity() {
        let fixture = permission_fixture(false);
        let service = InterventionCommandService::new(&fixture.app);
        let attempt_dir = service.attempt_dir(&fixture.locator);
        write_pending_permission(
            &attempt_dir,
            "permission-001",
            "turn-1",
            "prompt-event-1",
            json!({
                "sessionId": "session-001",
                "toolCall": {
                    "toolCallId": "call-001",
                    "kind": "edit",
                    "status": "pending",
                    "title": "Edit files",
                    "locations": [
                        { "path": "D:\\Downloads\\one.txt" },
                        { "path": "D:\\Downloads\\two.txt" },
                        { "path": "D:\\Downloads\\three.txt" },
                        { "path": "D:\\Downloads\\four.txt" }
                    ],
                    "rawInput": {
                        "command": "git status",
                        "args": ["--short", "--branch"],
                        "cwd": "D:\\Test"
                    }
                },
                "options": [
                    { "optionId": "allow_1", "name": "Allow 1", "kind": "allow_once" },
                    { "optionId": "allow_2", "name": "Allow 2", "kind": "allow_once" },
                    { "optionId": "allow_3", "name": "Allow 3", "kind": "allow_once" },
                    { "optionId": "allow_4", "name": "Allow 4", "kind": "allow_once" },
                    { "optionId": "allow_5", "name": "Allow 5", "kind": "allow_once" },
                    { "optionId": "allow_6", "name": "Allow 6", "kind": "allow_once" },
                    { "optionId": "cancel", "name": "Cancel", "kind": "reject_once" }
                ],
                "_meta": {
                    "permission": {
                        "version": 1,
                        "title": "Make edits?",
                        "description": "command failed; retry without sandbox?"
                    }
                }
            }),
            "2026-08-30T00:00:02Z".into(),
        )
        .unwrap();

        let snapshot = service
            .inspect(fixture.locator.clone(), permission_request())
            .unwrap();
        let prompt = snapshot.prompt.expect("permission prompt");
        assert_eq!(prompt.title.as_deref(), Some("Make edits?"));
        assert_eq!(prompt.message, "command failed; retry without sandbox?");
        assert!(!prompt.fields.contains_key("permissionTitle"));
        assert_eq!(
            prompt.fields.get("permissionTool").map(String::as_str),
            Some("Edit files")
        );
        assert_eq!(
            prompt.fields.get("permissionPath").map(String::as_str),
            Some("D:\\Downloads\\one.txt; D:\\Downloads\\two.txt; D:\\Downloads\\three.txt; +1")
        );
        assert_eq!(
            prompt.fields.get("permissionCommand").map(String::as_str),
            Some("git status --short --branch")
        );
        assert!(!prompt.fields.contains_key("permissionParameter"));
        assert!(
            prompt.requires_desktop,
            "duplicate standard permissions are ambiguous"
        );
        assert_eq!(snapshot.allowed_actions.len(), 7);
    }

    #[test]
    fn permission_command_cwd_is_projected_as_the_execution_path() {
        let fixture = permission_fixture(false);
        let service = InterventionCommandService::new(&fixture.app);
        write_pending_permission(
            &service.attempt_dir(&fixture.locator),
            "permission-001",
            "turn-1",
            "prompt-event-1",
            json!({
                "toolCall": {
                    "title": "Run command",
                    "rawInput": {
                        "command": "powershell.exe -NoProfile -Command Set-Content -LiteralPath new.txt",
                        "cwd": "D:\\Test"
                    }
                },
                "options": [
                    { "optionId": "cancel", "name": "No", "kind": "reject_once" }
                ]
            }),
            "2026-08-30T00:00:02Z".into(),
        )
        .unwrap();

        let snapshot = service
            .inspect(fixture.locator.clone(), permission_request())
            .unwrap();
        let prompt = snapshot.prompt.expect("permission prompt");
        assert_eq!(
            prompt.fields.get("permissionPath").map(String::as_str),
            Some("D:\\Test")
        );
        assert_eq!(
            prompt.fields.get("permissionCommand").map(String::as_str),
            Some("powershell.exe -NoProfile -Command Set-Content -LiteralPath new.txt")
        );
    }

    #[test]
    fn permission_option_kinds_cover_codex_claude_and_exit_plan_shapes() {
        let cases = [
            ("allow_once", PermissionActionKind::AllowOnce),
            ("allow", PermissionActionKind::AllowOnce),
            ("allow_for_session", PermissionActionKind::AllowAlways),
            ("allow_always", PermissionActionKind::AllowAlways),
            ("cancel", PermissionActionKind::RejectOnce),
            ("reject", PermissionActionKind::RejectOnce),
            ("reject_always", PermissionActionKind::RejectAlways),
        ];
        for (kind, expected) in cases {
            let fixture = permission_fixture(false);
            let service = InterventionCommandService::new(&fixture.app);
            write_pending_permission(
                &service.attempt_dir(&fixture.locator),
                "permission-001",
                "turn-1",
                "prompt-event-1",
                json!({
                    "options": [
                        { "optionId": kind, "name": kind, "kind": kind },
                        { "optionId": "other", "name": "Other", "kind": "unknown-kind" }
                    ]
                }),
                "2026-08-30T00:00:02Z".into(),
            )
            .unwrap();
            let snapshot = service
                .inspect(fixture.locator.clone(), permission_request())
                .unwrap();
            assert_eq!(
                snapshot.allowed_actions[0],
                InterventionAllowedAction::PermissionOption {
                    option_id: kind.into(),
                    name: kind.into(),
                    permission_kind: expected,
                },
                "kind {kind} must use the typed permission semantic"
            );
            assert!(snapshot.prompt.unwrap().requires_desktop);
        }
    }

    #[test]
    fn duplicate_permission_qualifiers_require_desktop() {
        let actions = [
            InterventionAllowedAction::PermissionOption {
                option_id: "auto_1".into(),
                name: "Use automatic mode".into(),
                permission_kind: PermissionActionKind::AllowAlways,
            },
            InterventionAllowedAction::PermissionOption {
                option_id: "auto_2".into(),
                name: "Use automatic mode again".into(),
                permission_kind: PermissionActionKind::AllowAlways,
            },
        ];
        assert!(permission_actions_require_desktop(&actions));
    }

    #[test]
    fn acp_pending_request_remains_actionable_after_direct_run_completion() {
        let fixture = permission_fixture(false);
        let mut run = fixture
            .app
            .run_status(&fixture.locator.task_id, &fixture.locator.run_id)
            .unwrap();
        run.status = RunStatus::Completed;
        run.outcome = Some(crate::domain::RunOutcome::Success);
        run.pause_reason = None;
        run.execution.phase = RuntimeExecutionPhase::Terminal;
        write_json(
            &fixture
                .app
                .paths
                .run_file(&fixture.locator.task_id, &fixture.locator.run_id),
            &run,
        )
        .unwrap();

        let service = InterventionCommandService::new(&fixture.app);
        let inspection = service.inspect(fixture.locator.clone(), permission_request());
        assert!(inspection.is_ok(), "inspection failed: {inspection:?}");
        assert_eq!(
            service
                .inspect(
                    fixture.locator.clone(),
                    InterventionRequestIdentity::ManualCheck,
                )
                .unwrap_err()
                .code,
            InterventionErrorCode::InterventionOwnerMismatch
        );
    }

    #[test]
    fn same_action_replay_is_idempotent() {
        let fixture = permission_fixture(false);
        let service = InterventionCommandService::new(&fixture.app);
        let snapshot = service
            .inspect(fixture.locator.clone(), permission_request())
            .unwrap();
        let command = permission_command(&snapshot, "allow");
        assert_eq!(
            service.execute(command.clone()).unwrap().status,
            InterventionCommandStatus::Accepted
        );
        assert_eq!(
            service.execute(command).unwrap().status,
            InterventionCommandStatus::AlreadyApplied
        );
    }

    #[test]
    fn execute_dispatches_manual_check_through_background_resume() {
        let temp = tempfile::tempdir().expect("temp fixture");
        let repo = Utf8PathBuf::from_path_buf(temp.path().join("repo")).expect("utf8 path");
        let app = App::new(repo);
        let locator = InterventionLocator {
            project_id: app.paths.project_id.clone(),
            task_id: "task-manual".into(),
            run_id: "run-manual".into(),
            round_id: "round-manual".into(),
            node_id: "worker".into(),
            attempt_id: "attempt-001".into(),
            outer_node_id: None,
            outer_attempt_id: None,
        };
        let workflow = serde_json::from_value::<crate::dsl::WorkflowDsl>(serde_json::json!({
            "version": VERSION,
            "id": "manual-workflow",
            "entry": "worker",
            "nodes": [{
                "id": "worker",
                "type": "worker",
                "provider": "claude-acp",
                "manual_check": true,
                "prompt_envelope": "raw-agent"
            }],
            "edges": [{ "from": "worker", "to": "$end", "on": "success" }]
        }))
        .unwrap();
        write_json(
            &app.paths
                .workflow_snapshot_file(&locator.task_id, &locator.run_id),
            &workflow,
        )
        .unwrap();
        let run = RunState {
            version: VERSION.to_string(),
            id: locator.run_id.clone(),
            task_id: locator.task_id.clone(),
            task_uuid: None,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "2026-09-02T00:00:00Z".into(),
            updated_at: "2026-09-02T00:00:01Z".into(),
            workflow_snapshot: "workflow.snapshot.json".into(),
            current_round: Some(locator.round_id.clone()),
            current_node: Some(locator.node_id.clone()),
            current_attempt: Some(locator.attempt_id.clone()),
            new_rounds_opened: 0,
            pause_reason: Some(PauseReason::WaitingForUserInput),
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: RuntimeExecutionState::new(
                RuntimeExecutionPhase::AwaitingManualCheck,
                None,
                "2026-09-02T00:00:01Z".to_string(),
            ),
        };
        let round = crate::runtime::RoundState {
            version: VERSION.to_string(),
            id: locator.round_id.clone(),
            run_id: locator.run_id.clone(),
            index: 1,
            status: RunStatus::Paused,
            outcome: None,
            trigger: crate::domain::RoundTrigger::Initial,
            started_at: "2026-09-02T00:00:00Z".into(),
            trace: Vec::new(),
            uuid: None,
        };
        let node = NodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: locator.node_id.clone(),
            node_type: NodeType::Worker,
            run_id: locator.run_id.clone(),
            round_id: locator.round_id.clone(),
            attempt_id: locator.attempt_id.clone(),
            status: RunStatus::Paused,
            outcome: None,
            started_at: "2026-09-02T00:00:00Z".into(),
            finished_at: Some("2026-09-02T00:00:01Z".into()),
            manual_check_pending: true,
            runtime_execution_id: None,
            resolved_config: Default::default(),
            uuid: None,
        };
        write_json(&app.paths.run_file(&locator.task_id, &locator.run_id), &run).unwrap();
        write_json(
            &app.paths
                .round_file(&locator.task_id, &locator.run_id, &locator.round_id),
            &round,
        )
        .unwrap();
        write_json(
            &app.paths.node_file(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                &locator.node_id,
                &locator.attempt_id,
            ),
            &node,
        )
        .unwrap();
        crate::acp::events::write_timeline_items(
            &app.paths.acp_timeline_file(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                &locator.node_id,
                &locator.attempt_id,
            ),
            &[crate::acp::events::AcpUiEvent {
                id: "assistant-message-1".into(),
                seq: 1,
                timestamp: "1Z".into(),
                kind: "textDelta".into(),
                session_id: Some("session-1".into()),
                content: Some("latest output".into()),
                title: None,
                tool_call_id: None,
                status: Some("completed".into()),
                started_seq: Some(1),
                ended_seq: Some(1),
                started_at: Some("1Z".into()),
                ended_at: Some("1Z".into()),
                timing: None,
                raw: None,
            }],
        )
        .unwrap();

        let service = InterventionCommandService::new(&app);
        let snapshot = service
            .inspect(locator.clone(), InterventionRequestIdentity::ManualCheck)
            .unwrap();
        let result = service
            .execute(InterventionCommand {
                locator,
                request: snapshot.request.clone(),
                expected_state: snapshot.expected_state.clone(),
                action: InterventionAction::ManualCheck {
                    outcome: NodeOutcome::Success,
                },
                expires_at_ms: snapshot.expires_at_ms,
            })
            .unwrap();

        assert_eq!(result.status, InterventionCommandStatus::Accepted);
        let completed_run = app.run_status("task-manual", "run-manual").unwrap();
        assert_eq!(completed_run.status, RunStatus::Completed);
        assert_eq!(
            completed_run.outcome,
            Some(crate::domain::RunOutcome::Success)
        );
        let completed_node: NodeState = read_json(&app.paths.node_file(
            "task-manual",
            "run-manual",
            "round-manual",
            "worker",
            "attempt-001",
        ))
        .unwrap();
        assert_eq!(completed_node.status, RunStatus::Completed);
        assert_eq!(completed_node.outcome, Some(NodeOutcome::Success));
        assert!(!completed_node.manual_check_pending);
    }

    #[test]
    fn concurrent_desktop_and_im_actions_are_first_writer_wins() {
        let fixture = permission_fixture(false);
        let snapshot = InterventionCommandService::new(&fixture.app)
            .inspect(fixture.locator.clone(), permission_request())
            .unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();
        for option_id in ["allow", "deny"] {
            let app = fixture.app.clone_for_background();
            let command = permission_command(&snapshot, option_id);
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                InterventionCommandService::new(&app).execute(command)
            }));
        }
        barrier.wait();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("writer thread"))
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter_map(|result| result.as_ref().err())
                .filter(|error| error.code == InterventionErrorCode::InterventionAlreadyHandled)
                .count(),
            1
        );
    }

    #[test]
    fn elicitation_snapshot_preserves_question_options_and_submits_schema_shaped_content() {
        let fixture = permission_fixture(false);
        write_choice_elicitation(&fixture, &["MySQL", "PostgreSQL", "H2", "SQL Server"]);
        let service = InterventionCommandService::new(&fixture.app);
        let request = InterventionRequestIdentity::Elicitation {
            elicitation_id: "elicit-001".into(),
        };
        let snapshot = service
            .inspect(fixture.locator.clone(), request.clone())
            .unwrap();

        let prompt = snapshot.prompt.as_ref().unwrap();
        assert_eq!(prompt.message, "请选择要使用的数据库？");
        assert_eq!(prompt.questions.len(), 1);
        assert_eq!(prompt.questions[0].title, "数据库选择");
        assert_eq!(prompt.questions[0].options.len(), 4);
        assert!(prompt.questions[0].allows_custom_answer);
        assert!(!prompt.requires_desktop);
        assert_eq!(snapshot.allowed_actions.len(), 2);
        assert!(
            !snapshot
                .allowed_actions
                .contains(&InterventionAllowedAction::ElicitationAccept)
        );
        let InterventionAllowedAction::ElicitationFixedForm { form } = &snapshot.allowed_actions[0]
        else {
            panic!("expected a fixed elicitation form");
        };
        let RemoteElicitationForm::SingleScalarChoice { question } = form else {
            panic!("expected a single scalar choice form");
        };
        assert_eq!(question.selector_key, "q0");
        assert_eq!(question.field_name, "question_0");
        assert_eq!(question.options.len(), 4);
        assert!(matches!(
            intervention_action_for_elicitation_form(
                form,
                &[RemoteElicitationFormSelection {
                    selector_key: "q0".into(),
                    option_ids: vec!["0".into()],
                }],
            ),
            Ok(InterventionAction::Elicitation {
                action: ElicitationAction::Accept,
                content: Some(content),
            }) if content == elicitation_choice_content(
                "question_0",
                &serde_json::json!("MySQL")
            )
        ));

        let content = elicitation_choice_content("question_0", &serde_json::json!("MySQL"));
        let result = service
            .execute(InterventionCommand {
                locator: snapshot.locator,
                request,
                expected_state: snapshot.expected_state,
                action: InterventionAction::Elicitation {
                    action: ElicitationAction::Accept,
                    content: Some(content.clone()),
                },
                expires_at_ms: snapshot.expires_at_ms,
            })
            .unwrap();
        assert_eq!(result.status, InterventionCommandStatus::Accepted);
        let response: ElicitationResponseState = read_json(&elicitation_response_file(
            &service.attempt_dir(&fixture.locator),
            "elicit-001",
        ))
        .unwrap();
        assert_eq!(response.content, Some(content));
    }

    #[test]
    fn elicitation_snapshot_includes_distinct_prior_agent_output() {
        let fixture = permission_fixture(false);
        write_choice_elicitation(&fixture, &["MySQL", "PostgreSQL"]);
        let attempt_dir =
            InterventionCommandService::new(&fixture.app).attempt_dir(&fixture.locator);
        crate::acp::events::write_timeline_items(
            &attempt_dir.join("acp.timeline.jsonl"),
            &[crate::acp::events::AcpUiEvent {
                id: "assistant-message-1".into(),
                seq: 1,
                timestamp: "1Z".into(),
                kind: "textDelta".into(),
                session_id: Some("session-1".into()),
                content: Some("已形成三个可执行决策分支。".into()),
                title: None,
                tool_call_id: None,
                status: Some("completed".into()),
                started_seq: Some(1),
                ended_seq: Some(1),
                started_at: Some("1Z".into()),
                ended_at: Some("1Z".into()),
                timing: None,
                raw: None,
            }],
        )
        .unwrap();

        let snapshot = InterventionCommandService::new(&fixture.app)
            .inspect(
                fixture.locator.clone(),
                InterventionRequestIdentity::Elicitation {
                    elicitation_id: "elicit-001".into(),
                },
            )
            .unwrap();
        assert_eq!(
            snapshot.prompt.and_then(|prompt| prompt.context).as_deref(),
            Some("已形成三个可执行决策分支。")
        );
    }

    #[test]
    fn elicitation_schema_change_invalidates_an_existing_choice_action() {
        let fixture = permission_fixture(false);
        write_choice_elicitation(&fixture, &["MySQL", "PostgreSQL"]);
        let service = InterventionCommandService::new(&fixture.app);
        let request = InterventionRequestIdentity::Elicitation {
            elicitation_id: "elicit-001".into(),
        };
        let snapshot = service
            .inspect(fixture.locator.clone(), request.clone())
            .unwrap();
        write_choice_elicitation(&fixture, &["SQLite", "PostgreSQL"]);

        let error = service
            .execute(InterventionCommand {
                locator: snapshot.locator,
                request,
                expected_state: snapshot.expected_state,
                action: InterventionAction::Elicitation {
                    action: ElicitationAction::Accept,
                    content: Some(elicitation_choice_content(
                        "question_0",
                        &serde_json::json!("MySQL"),
                    )),
                },
                expires_at_ms: snapshot.expires_at_ms,
            })
            .unwrap_err();
        assert_eq!(
            error.code,
            InterventionErrorCode::InterventionRevisionConflict
        );
    }

    #[test]
    fn elicitation_timeline_binding_does_not_invalidate_im_snapshot() {
        let fixture = permission_fixture(false);
        write_choice_elicitation(&fixture, &["MySQL", "PostgreSQL"]);
        let service = InterventionCommandService::new(&fixture.app);
        let request = InterventionRequestIdentity::Elicitation {
            elicitation_id: "elicit-001".into(),
        };
        let snapshot = service
            .inspect(fixture.locator.clone(), request.clone())
            .unwrap();
        bind_pending_elicitation_timeline_identity(
            &service.attempt_dir(&fixture.locator),
            "elicit-001",
            TimelineItemIdentity {
                branch_id: "root".into(),
                item_id: "elicit-001".into(),
                revision: 79,
            },
        )
        .unwrap();

        let result = service.execute(InterventionCommand {
            locator: snapshot.locator,
            request,
            expected_state: snapshot.expected_state,
            action: InterventionAction::Elicitation {
                action: ElicitationAction::Accept,
                content: Some(elicitation_choice_content(
                    "question_0",
                    &serde_json::json!("MySQL"),
                )),
            },
            expires_at_ms: snapshot.expires_at_ms,
        });
        assert_eq!(
            result
                .expect("timeline metadata is not decision state")
                .status,
            InterventionCommandStatus::Accepted
        );
    }

    #[test]
    fn multi_question_ask_user_question_is_fully_projected_but_not_partially_actionable() {
        let fixture = permission_fixture(false);
        let request = serde_json::from_value(json!({
            "mode": "form",
            "sessionId": "session-001",
            "message": "Please answer the following questions.",
            "requestedSchema": {
                "type": "object",
                "properties": {
                    "question_0": {
                        "type": "string",
                        "title": "预算范围",
                        "description": "你的预算大概是多少？",
                        "oneOf": [
                            {"const":"2000 元以内","title":"2000 元以内","description":"入门级 / 千元机，满足基本使用"},
                            {"const":"2000–4000 元","title":"2000–4000 元","description":"中端机型，性价比高的主流选择"},
                            {"const":"4000–6000 元","title":"4000–6000 元","description":"旗舰级配置，各方面体验均衡出色"},
                            {"const":"6000 元以上","title":"6000 元以上","description":"各家顶级旗舰 / 折叠屏"}
                        ]
                    },
                    "question_0_custom": custom_answer_schema("question_0"),
                    "question_1": {
                        "type": "array",
                        "title": "主要用途",
                        "description": "你主要用手机做什么？（可多选）",
                        "items": {"anyOf": [
                            {"const":"拍照摄影","title":"拍照摄影","description":"看重相机素质、成像风格"},
                            {"const":"游戏娱乐","title":"游戏娱乐","description":"看重性能、散热、高刷新率屏幕"},
                            {"const":"日常通勤续航","title":"日常通勤续航","description":"看重电池容量、快充、耐用性"},
                            {"const":"办公商务","title":"办公商务","description":"看重生态联动、商务功能、信号"}
                        ]}
                    },
                    "question_1_custom": custom_answer_schema("question_1"),
                    "question_2": {
                        "type": "string",
                        "title": "系统偏好",
                        "description": "你对手机系统 / 品牌有偏好吗？",
                        "enum": ["iOS (苹果)", "国产安卓", "原生系安卓", "无所谓"]
                    },
                    "question_2_custom": custom_answer_schema("question_2"),
                    "question_3": {
                        "type": "string",
                        "title": "换机周期",
                        "description": "这部手机打算用多久？",
                        "oneOf": [
                            {"const":"用 4 年以上","title":"用 4 年以上"},
                            {"const":"2–3 年","title":"2–3 年"},
                            {"const":"1–2 年就换","title":"1–2 年就换"}
                        ]
                    },
                    "question_3_custom": custom_answer_schema("question_3")
                }
            }
        }))
        .unwrap();
        write_elicitation(&fixture, request);

        let snapshot = InterventionCommandService::new(&fixture.app)
            .inspect(
                fixture.locator.clone(),
                InterventionRequestIdentity::Elicitation {
                    elicitation_id: "elicit-001".into(),
                },
            )
            .unwrap();
        let prompt = snapshot.prompt.unwrap();
        assert_eq!(prompt.questions.len(), 4);
        assert_eq!(prompt.questions[0].options.len(), 4);
        assert_eq!(
            prompt.questions[1].question_kind,
            ElicitationQuestionKind::MultiSelect
        );
        assert_eq!(prompt.questions[1].options.len(), 4);
        assert_eq!(prompt.questions[2].options[0].label, "iOS (苹果)");
        assert!(
            prompt
                .questions
                .iter()
                .all(|question| question.allows_custom_answer)
        );
        assert!(prompt.requires_desktop);
        assert_eq!(
            snapshot.allowed_actions,
            vec![InterventionAllowedAction::ElicitationDecline]
        );
    }

    #[test]
    fn remote_elicitation_forms_cover_only_fixed_scalar_shapes() {
        let fixture = permission_fixture(false);
        let scalar = |field: &str, value: &str| {
            json!({
                "type": "string",
                "title": field,
                "oneOf": [{"const": value, "title": value}]
            })
        };
        let multi = json!({
            "type": "array",
            "title": "features",
            "items": {"anyOf": [
                {"const": "auth", "title": "auth"},
                {"const": "logging", "title": "logging"}
            ]}
        });
        let request = serde_json::from_value(json!({
            "mode": "form",
            "sessionId": "session-001",
            "message": "answer",
            "requestedSchema": {
                "type": "object",
                "properties": {
                    "q0": scalar("q0", "a"),
                    "q1": scalar("q1", "b"),
                    "q2": scalar("q2", "c"),
                    "q2_custom": custom_answer_schema("q2")
                },
                "required": ["q0", "q1", "q2"]
            }
        }))
        .unwrap();
        write_elicitation(&fixture, request);
        let snapshot = InterventionCommandService::new(&fixture.app)
            .inspect(
                fixture.locator.clone(),
                InterventionRequestIdentity::Elicitation {
                    elicitation_id: "elicit-001".into(),
                },
            )
            .unwrap();
        let InterventionAllowedAction::ElicitationFixedForm { form } = &snapshot.allowed_actions[0]
        else {
            panic!("three scalar questions should use a fixed form");
        };
        let RemoteElicitationForm::ScalarChoiceQuestions { questions } = form else {
            panic!("expected multiple scalar questions");
        };
        assert_eq!(
            questions
                .iter()
                .map(|question| (
                    question.selector_key.as_str(),
                    question.required,
                    question.options.len()
                ))
                .collect::<Vec<_>>(),
            vec![("q0", true, 1), ("q1", true, 1), ("q2", true, 1)]
        );
        assert!(
            intervention_action_for_elicitation_form(
                form,
                &[
                    RemoteElicitationFormSelection {
                        selector_key: "q0".into(),
                        option_ids: vec!["0".into()]
                    },
                    RemoteElicitationFormSelection {
                        selector_key: "q1".into(),
                        option_ids: vec!["0".into()]
                    },
                    RemoteElicitationFormSelection {
                        selector_key: "q2".into(),
                        option_ids: vec!["0".into()]
                    }
                ]
            )
            .is_ok()
        );

        let request = serde_json::from_value(json!({
            "mode": "form",
            "sessionId": "session-001",
            "message": "multi",
            "requestedSchema": {
                "type": "object",
                "properties": {"features": multi},
                "required": ["features"]
            }
        }))
        .unwrap();
        write_elicitation(&fixture, request);
        let snapshot = InterventionCommandService::new(&fixture.app)
            .inspect(
                fixture.locator.clone(),
                InterventionRequestIdentity::Elicitation {
                    elicitation_id: "elicit-001".into(),
                },
            )
            .unwrap();
        let InterventionAllowedAction::ElicitationFixedForm { form } = &snapshot.allowed_actions[0]
        else {
            panic!("multi scalar choice should use a fixed form");
        };
        let RemoteElicitationForm::MultiScalarChoice { allows_empty, .. } = form else {
            panic!("expected multi scalar choice");
        };
        assert!(allows_empty);
        let invalid_duplicate = intervention_action_for_elicitation_form(
            form,
            &[RemoteElicitationFormSelection {
                selector_key: "q0".into(),
                option_ids: vec!["0".into(), "0".into()],
            }],
        );
        assert_eq!(
            invalid_duplicate.unwrap_err().code,
            InterventionErrorCode::InterventionActionInvalid
        );

        let request = serde_json::from_value(json!({
            "mode": "form",
            "sessionId": "session-001",
            "message": "too many",
            "requestedSchema": {
                "type": "object",
                "properties": {
                    "q0": scalar("q0", "a"),
                    "q1": scalar("q1", "b"),
                    "q2": scalar("q2", "c"),
                    "q3": scalar("q3", "d")
                }
            }
        }))
        .unwrap();
        write_elicitation(&fixture, request);
        let snapshot = InterventionCommandService::new(&fixture.app)
            .inspect(
                fixture.locator.clone(),
                InterventionRequestIdentity::Elicitation {
                    elicitation_id: "elicit-001".into(),
                },
            )
            .unwrap();
        assert!(snapshot.prompt.unwrap().requires_desktop);
        assert_eq!(
            snapshot.allowed_actions,
            vec![InterventionAllowedAction::ElicitationDecline]
        );
    }

    fn custom_answer_schema(question_id: &str) -> Value {
        json!({
            "type": "string",
            "title": "Other",
            "_meta": {
                "_askUserQuestionCustomAnswer": {
                    "questionId": question_id,
                    "isCustomAnswer": true
                }
            }
        })
    }
}
