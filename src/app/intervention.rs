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
            elicitation_request_settlement, elicitation_response_file, pending_elicitation_file,
            write_elicitation_response_if_pending,
        },
        events::current_timestamp,
        interaction::{AcpInteractionSettlement, AcpPromptInteractionKind},
        permission::{
            PendingPermissionState, PermissionResponseState, pending_permission_file,
            permission_request_settlement, permission_response_file,
            write_permission_response_if_pending,
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
/// Length cap for remote free text and custom answers when the schema does not
/// declare `maxLength`; channels may lower it further.
pub const MAX_REMOTE_TEXT_CHARS: u32 = 2000;
/// Free-text questions allowing more characters than this render multi-line.
const MAX_REMOTE_SINGLE_LINE_TEXT_CHARS: u32 = 200;
/// Sample text used only by the schema satisfiability probes.
const REMOTE_TEXT_PROBE: &str = "x";
/// Keywords a free-text property may use and still be answerable remotely.
const REMOTE_TEXT_PROPERTY_KEYWORDS: [&str; 9] = [
    "type",
    "title",
    "description",
    "default",
    "examples",
    "maxLength",
    "deprecated",
    "readOnly",
    "_meta",
];
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

/// Transport sentinel for a custom ("other") choice. Domain option ids are
/// decimal indexes, so this value never collides with a listed option.
pub const REMOTE_CUSTOM_ANSWER_OPTION: &str = "other";

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

impl InterventionAllowedAction {
    /// The fixed form published among `actions`, if any.
    pub fn fixed_form(actions: &[Self]) -> Option<&RemoteElicitationForm> {
        actions.iter().find_map(|action| match action {
            Self::ElicitationFixedForm { form } => Some(form),
            _ => None,
        })
    }
}

pub fn intervention_action_for_allowed(action: &InterventionAllowedAction) -> InterventionAction {
    match action {
        InterventionAllowedAction::PermissionOption { option_id, .. } => {
            InterventionAction::PermissionOption {
                option_id: option_id.clone(),
            }
        }
        // Fixed forms are submitted through form answers; a published action
        // index is not a valid transport for them and is rejected inbound.
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

/// Channel-independent remote form for one elicitation. Channels publish a
/// fitted copy (`fit_form`) and inbound submissions are validated against the
/// copy persisted in their delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteElicitationForm {
    pub questions: Vec<RemoteQuestion>,
    pub decline_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteQuestion {
    /// IM callback field name (`q0`, `q1`…); never the schema field name.
    pub selector_key: String,
    pub question: ElicitationQuestion,
    pub constraints: RemoteQuestionConstraints,
    /// Schema field that receives the custom ("other") answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_answer_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteQuestionConstraints {
    pub required: bool,
    /// Only meaningful for multi-select questions.
    pub allows_empty: bool,
    /// Length cap for free text and custom answers, in characters.
    pub max_chars: u32,
    /// Only meaningful for free-text questions.
    pub multiline: bool,
}

/// One submitted answer, keyed by `RemoteQuestion::selector_key`. Option ids
/// are decimal indexes into `ElicitationQuestion::options`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RemoteElicitationAnswer {
    Choice {
        option_ids: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        custom_text: Option<String>,
    },
    Text {
        text: String,
    },
}

/// Structured reason a remote form submission was refused; transports map it
/// to their own error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteFormViolation {
    AnswerMissing,
    OptionInvalid,
    CustomAnswerMissing,
    TextTooLong,
    AnswerInvalid,
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

/// Canonical lifecycle of one intervention request, independent of which
/// surface (desktop, IM, runtime stop) ended it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterventionSettlement {
    Pending,
    /// Answered by a person or an auto-accept policy.
    Answered,
    /// Ended without an answer (stop, timeout, abandoned run).
    Cancelled,
}

impl From<AcpInteractionSettlement> for InterventionSettlement {
    fn from(settlement: AcpInteractionSettlement) -> Self {
        match settlement {
            AcpInteractionSettlement::Pending => Self::Pending,
            AcpInteractionSettlement::Answered => Self::Answered,
            AcpInteractionSettlement::Cancelled => Self::Cancelled,
        }
    }
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

    /// Where the request is in its lifecycle. Unlike [`Self::inspect`] it does
    /// not require the run to still be positioned on the request's attempt, so
    /// it also answers for requests a stopped or retried run left behind.
    pub fn settlement(
        &self,
        locator: &InterventionLocator,
        request: &InterventionRequestIdentity,
    ) -> Result<InterventionSettlement, InterventionError> {
        self.validate_locator(locator)?;
        let attempt_dir = self.attempt_dir(locator);
        let storage_unavailable =
            |_| InterventionError::new(InterventionErrorCode::InterventionStorageUnavailable);
        let settlement = match request {
            InterventionRequestIdentity::Permission { request_id } => {
                permission_request_settlement(&attempt_dir, request_id)
                    .map_err(storage_unavailable)?
                    .map(Into::into)
            }
            InterventionRequestIdentity::Elicitation { elicitation_id } => {
                elicitation_request_settlement(&attempt_dir, elicitation_id)
                    .map_err(storage_unavailable)?
                    .map(Into::into)
            }
            InterventionRequestIdentity::ManualCheck => Some(self.manual_settlement(locator)?),
        };
        settlement.ok_or_else(|| {
            InterventionError::new(InterventionErrorCode::InterventionRequestNotFound)
        })
    }

    fn manual_settlement(
        &self,
        locator: &InterventionLocator,
    ) -> Result<InterventionSettlement, InterventionError> {
        if self
            .app
            .validate_manual_check_submission(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                &locator.node_id,
                &locator.attempt_id,
            )
            .is_ok()
        {
            return Ok(InterventionSettlement::Pending);
        }
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
        Ok(if !node.manual_check_pending && node.outcome.is_some() {
            InterventionSettlement::Answered
        } else {
            InterventionSettlement::Cancelled
        })
    }

    /// A request without its pending file is either settled (reported as
    /// handled) or was never published here.
    fn missing_request_error(
        &self,
        locator: &InterventionLocator,
        request: &InterventionRequestIdentity,
        request_kind: &str,
    ) -> InterventionError {
        match self.settlement(locator, request) {
            Ok(InterventionSettlement::Answered | InterventionSettlement::Cancelled) => {
                InterventionError::new(InterventionErrorCode::InterventionAlreadyHandled)
            }
            _ => InterventionError::new(InterventionErrorCode::InterventionRequestNotFound)
                .with_detail("requestKind", request_kind),
        }
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
            read_json(&pending_permission_file(&attempt_dir, request_id))
                .map_err(|_| self.missing_request_error(&locator, &request, "permission"))?;
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
            read_json(&pending_elicitation_file(&attempt_dir, elicitation_id))
                .map_err(|_| self.missing_request_error(&locator, &request, "elicitation"))?;
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
            .filter(|output| !output.trim().is_empty());
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
            prompt: latest_output.map(|latest_output| InterventionPrompt {
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
        if !canonical_elicitation_response_is_valid(
            &pending.payload.request,
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
        requires_desktop: permission_actions_require_desktop(allowed_actions),
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
    let custom_answer_fields = custom_answer_fields(properties);
    let questions = properties
        .into_iter()
        .flat_map(|properties| properties.iter())
        .filter(|(name, schema)| custom_answer_target(name, schema, properties.unwrap()).is_none())
        .map(|(field_name, schema)| {
            elicitation_question(
                field_name,
                schema,
                custom_answer_fields.contains_key(field_name.as_str()),
            )
        })
        .collect::<Vec<_>>();

    let form = properties.and_then(|_| {
        remote_elicitation_form(&request, &questions, &custom_answer_fields)
            .ok()
            .flatten()
    });
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
    custom_answer_fields: &BTreeMap<String, String>,
) -> Result<Option<RemoteElicitationForm>, InterventionError> {
    if questions.is_empty() || remote_schema_has_unsupported_constraints(request) {
        return Ok(None);
    }
    if questions
        .iter()
        .any(|question| question.field_name.len() > MAX_REMOTE_ELICITATION_FIELD_NAME_BYTES)
    {
        return Ok(None);
    }
    let schema = request
        .get("requestedSchema")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({ "type": "object" }));
    let properties = schema.get("properties").and_then(Value::as_object);
    let property = |name: &str| properties.and_then(|properties| properties.get(name));
    let required = required_fields(request);

    let mut remote_questions = Vec::with_capacity(questions.len());
    for (index, question) in questions.iter().enumerate() {
        let (max_chars, multiline, custom_answer_key) = match question.question_kind {
            ElicitationQuestionKind::FreeText => {
                let schema = property(&question.field_name);
                if !remote_text_property(schema) {
                    return Ok(None);
                }
                let max_chars = remote_text_max_chars(schema);
                if max_chars == 0 {
                    return Ok(None);
                }
                (
                    max_chars,
                    max_chars > MAX_REMOTE_SINGLE_LINE_TEXT_CHARS,
                    None,
                )
            }
            ElicitationQuestionKind::SingleSelect | ElicitationQuestionKind::MultiSelect => {
                if !remote_scalar_options_are_valid(&question.options) {
                    return Ok(None);
                }
                let custom_answer_key = custom_answer_fields
                    .get(&question.field_name)
                    .filter(|key| {
                        key.len() <= MAX_REMOTE_ELICITATION_FIELD_NAME_BYTES
                            && remote_text_property(property(key))
                            && remote_text_max_chars(property(key)) > 0
                    })
                    .cloned();
                let max_chars = custom_answer_key
                    .as_deref()
                    .map(|key| remote_text_max_chars(property(key)))
                    .unwrap_or(MAX_REMOTE_TEXT_CHARS);
                (max_chars, false, custom_answer_key)
            }
        };
        let is_required = required.contains(&question.field_name);
        remote_questions.push(RemoteQuestion {
            selector_key: format!("q{index}"),
            question: question.clone(),
            constraints: RemoteQuestionConstraints {
                required: is_required,
                allows_empty: !is_required,
                max_chars,
                multiline,
            },
            custom_answer_key,
        });
    }

    // Satisfiability probes: the schema must accept the minimal (required
    // only) and the complete answer sets the remote form can produce.
    let mut required_content = serde_json::Map::new();
    let mut complete_content = serde_json::Map::new();
    for question in &remote_questions {
        let sample = remote_sample_answer(&question.question);
        complete_content.insert(question.question.field_name.clone(), sample.clone());
        if question.constraints.required {
            required_content.insert(question.question.field_name.clone(), sample);
        }
    }
    if !elicitation_content_is_valid(&schema, &Value::Object(required_content.clone()))?
        || !elicitation_content_is_valid(&schema, &Value::Object(complete_content))?
    {
        return Ok(None);
    }
    // Custom answer probe: "other" is offered only when the schema accepts it
    // in place of the listed options, required questions included. Otherwise
    // the question keeps its listed options and "other" stays desktop-only.
    for question in &mut remote_questions {
        let Some(custom_key) = question.custom_answer_key.clone() else {
            continue;
        };
        let mut content = required_content.clone();
        content.remove(&question.question.field_name);
        content.insert(custom_key, Value::String(REMOTE_TEXT_PROBE.to_owned()));
        if !elicitation_content_is_valid(&schema, &Value::Object(content))? {
            question.custom_answer_key = None;
            question.constraints.max_chars = MAX_REMOTE_TEXT_CHARS;
        }
    }
    Ok(Some(RemoteElicitationForm {
        questions: remote_questions,
        decline_allowed: true,
    }))
}

fn remote_sample_answer(question: &ElicitationQuestion) -> Value {
    let first = || {
        question
            .options
            .first()
            .map(|option| option.value.clone())
            .unwrap_or(Value::Null)
    };
    match question.question_kind {
        ElicitationQuestionKind::SingleSelect => first(),
        ElicitationQuestionKind::MultiSelect => Value::Array(vec![first()]),
        ElicitationQuestionKind::FreeText => Value::String(REMOTE_TEXT_PROBE.to_owned()),
    }
}

fn remote_text_property(schema: Option<&Value>) -> bool {
    schema.and_then(Value::as_object).is_some_and(|fields| {
        fields.get("type").and_then(Value::as_str) == Some("string")
            && fields
                .keys()
                .all(|key| REMOTE_TEXT_PROPERTY_KEYWORDS.contains(&key.as_str()))
    })
}

fn remote_text_max_chars(schema: Option<&Value>) -> u32 {
    let cap = MAX_INTERVENTION_CONTENT_BYTES as u64;
    schema
        .and_then(|schema| schema.get("maxLength"))
        .and_then(Value::as_u64)
        .unwrap_or(u64::from(MAX_REMOTE_TEXT_CHARS))
        .min(cap) as u32
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

fn remote_scalar_options_are_valid(options: &[ElicitationQuestionOption]) -> bool {
    if options.is_empty() {
        return false;
    }
    let mut values = std::collections::HashSet::new();
    options.iter().all(|option| {
        !(option.value.is_object() || option.value.is_array() || option.value.is_null())
            && serde_json::to_vec(&option.value)
                .map(|encoded| encoded.len())
                .unwrap_or(MAX_ELICITATION_OPTION_VALUE_BYTES + 1)
                <= MAX_ELICITATION_OPTION_VALUE_BYTES
            && values.insert(option.value.clone())
    })
}

fn custom_answer_target<'a>(
    field_name: &'a str,
    schema: &'a Value,
    properties: &'a serde_json::Map<String, Value>,
) -> Option<&'a str> {
    let explicit = schema
        .pointer("/_meta/_askUserQuestionCustomAnswer")
        .filter(|metadata| {
            metadata
                .get("isCustomAnswer")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .and_then(|metadata| metadata.get("questionId"))
        .and_then(Value::as_str);
    if explicit.is_some() {
        return explicit;
    }
    // Plain Claude clients retain the paired field names without AIR metadata.
    let target = field_name.strip_suffix("_custom")?;
    if schema.get("type").and_then(Value::as_str) != Some("string")
        || !elicitation_options(schema).is_empty()
    {
        return None;
    }
    let question = properties.get(target)?;
    let options = if question.get("type").and_then(Value::as_str) == Some("array") {
        elicitation_options(question.get("items")?)
    } else {
        elicitation_options(question)
    };
    (!options.is_empty()).then_some(target)
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

// Transport capabilities are enforced before this canonical boundary. In
// particular, ImInboundActionService validates the action against the exact
// allowed_actions persisted in its delivery before invoking Runtime.
fn canonical_elicitation_response_is_valid(
    request: &agent_client_protocol_schema::v1::CreateElicitationRequest,
    action: &ElicitationAction,
    content: Option<&Value>,
) -> Result<bool, InterventionError> {
    match action {
        ElicitationAction::Decline | ElicitationAction::Cancel => Ok(content.is_none()),
        ElicitationAction::Accept => {
            if content.is_some_and(|content| !content.is_object()) {
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
            elicitation_content_is_valid(&schema, &instance)
        }
    }
}

/// Choice field -> the schema field that receives its custom ("other") answer.
fn custom_answer_fields(
    properties: Option<&serde_json::Map<String, Value>>,
) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for (name, schema) in properties.into_iter().flatten() {
        if let Some(target) = custom_answer_target(name, schema, properties.unwrap()) {
            fields
                .entry(target.to_owned())
                .or_insert_with(|| name.clone());
        }
    }
    fields
}

/// Validates elicitation content against `requestedSchema`. A paired custom
/// answer replaces its choice field ("instead of choosing an option above"),
/// so a non-blank custom answer satisfies the choice field's `required` entry;
/// every other keyword is validated unchanged. Desktop and remote share it.
fn elicitation_content_is_valid(
    schema: &Value,
    content: &Value,
) -> Result<bool, InterventionError> {
    let answered_by_custom =
        custom_answer_fields(schema.get("properties").and_then(Value::as_object))
            .into_iter()
            .filter(|(choice, custom)| {
                content.get(choice).is_none()
                    && content
                        .get(custom)
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.trim().is_empty())
            })
            .map(|(choice, _)| choice)
            .collect::<std::collections::BTreeSet<_>>();
    let mut effective = std::borrow::Cow::Borrowed(schema);
    if !answered_by_custom.is_empty() {
        if let Some(required) = effective
            .to_mut()
            .get_mut("required")
            .and_then(Value::as_array_mut)
        {
            required.retain(|field| {
                field
                    .as_str()
                    .is_none_or(|field| !answered_by_custom.contains(field))
            });
        }
    }
    let compiled = jsonschema::JSONSchema::compile(&effective).map_err(|_| {
        InterventionError::new(InterventionErrorCode::InterventionActionInvalid)
            .with_detail("actionKind", "elicitationSchema")
    })?;
    Ok(compiled.is_valid(content))
}

pub fn intervention_action_for_elicitation_form(
    form: &RemoteElicitationForm,
    answers: &BTreeMap<String, RemoteElicitationAnswer>,
) -> Result<InterventionAction, RemoteFormViolation> {
    Ok(InterventionAction::Elicitation {
        action: ElicitationAction::Accept,
        content: Some(remote_form_content(form, answers)?),
    })
}

/// Builds the elicitation content for remote answers. The encoding is the
/// desktop `buildElicitationContent` contract: unanswered questions, blank
/// text and empty selections are omitted; a custom answer is written only to
/// its custom field, untrimmed; multi choices keep the submitted order.
pub fn remote_form_content(
    form: &RemoteElicitationForm,
    answers: &BTreeMap<String, RemoteElicitationAnswer>,
) -> Result<Value, RemoteFormViolation> {
    if answers.keys().any(|selector| {
        !form
            .questions
            .iter()
            .any(|question| question.selector_key == *selector)
    }) {
        return Err(RemoteFormViolation::AnswerInvalid);
    }
    let mut content = serde_json::Map::new();
    for question in &form.questions {
        let answered = match answers.get(&question.selector_key) {
            Some(answer) => encode_remote_answer(question, answer, &mut content)?,
            None => false,
        };
        if !answered && question.constraints.required {
            return Err(RemoteFormViolation::AnswerMissing);
        }
    }
    Ok(Value::Object(content))
}

fn encode_remote_answer(
    question: &RemoteQuestion,
    answer: &RemoteElicitationAnswer,
    content: &mut serde_json::Map<String, Value>,
) -> Result<bool, RemoteFormViolation> {
    let text_fits = |text: &str| {
        if text.chars().count() > question.constraints.max_chars as usize {
            Err(RemoteFormViolation::TextTooLong)
        } else {
            Ok(())
        }
    };
    let field_name = &question.question.field_name;
    match (&question.question.question_kind, answer) {
        (ElicitationQuestionKind::FreeText, RemoteElicitationAnswer::Text { text }) => {
            if text.trim().is_empty() {
                return Ok(false);
            }
            text_fits(text)?;
            content.insert(field_name.clone(), Value::String(text.clone()));
            Ok(true)
        }
        (
            kind @ (ElicitationQuestionKind::SingleSelect | ElicitationQuestionKind::MultiSelect),
            RemoteElicitationAnswer::Choice {
                option_ids,
                custom_text,
            },
        ) => {
            if let Some(text) = custom_text {
                let Some(custom_key) = question.custom_answer_key.as_ref() else {
                    return Err(RemoteFormViolation::AnswerInvalid);
                };
                if !option_ids.is_empty() {
                    return Err(RemoteFormViolation::AnswerInvalid);
                }
                if text.trim().is_empty() {
                    return Err(RemoteFormViolation::CustomAnswerMissing);
                }
                text_fits(text)?;
                content.insert(custom_key.clone(), Value::String(text.clone()));
                return Ok(true);
            }
            let mut seen = std::collections::BTreeSet::new();
            let mut values = Vec::with_capacity(option_ids.len());
            for option_id in option_ids {
                let index = remote_option_index(option_id, question.question.options.len())
                    .ok_or(RemoteFormViolation::OptionInvalid)?;
                if !seen.insert(index) {
                    return Err(RemoteFormViolation::OptionInvalid);
                }
                values.push(question.question.options[index].value.clone());
            }
            match (kind, values.len()) {
                (_, 0) => Ok(false),
                (ElicitationQuestionKind::SingleSelect, 1) => {
                    content.insert(field_name.clone(), values.remove(0));
                    Ok(true)
                }
                (ElicitationQuestionKind::SingleSelect, _) => {
                    Err(RemoteFormViolation::AnswerInvalid)
                }
                _ => {
                    content.insert(field_name.clone(), Value::Array(values));
                    Ok(true)
                }
            }
        }
        _ => Err(RemoteFormViolation::AnswerInvalid),
    }
}

/// Remote option ids are the canonical decimal option index.
fn remote_option_index(option_id: &str, option_count: usize) -> Option<usize> {
    let index = option_id.parse::<usize>().ok()?;
    (index < option_count && index.to_string() == option_id).then_some(index)
}

/// Decodes one question's answer from canonical elicitation content: the
/// inverse of `encode_remote_answer`. `None` means the question is unanswered.
/// Multi choices keep the content order; option ids are canonical indexes.
pub fn remote_question_answer(
    question: &RemoteQuestion,
    content: &Value,
) -> Result<Option<RemoteElicitationAnswer>, RemoteFormViolation> {
    let option_id = |value: &Value| {
        question
            .question
            .options
            .iter()
            .position(|option| option.value == *value)
            .map(|index| index.to_string())
            .ok_or(RemoteFormViolation::OptionInvalid)
    };
    let value = content.get(&question.question.field_name);
    let custom = question
        .custom_answer_key
        .as_ref()
        .and_then(|key| content.get(key));
    Ok(Some(
        match (&question.question.question_kind, value, custom) {
            (_, None, None) => return Ok(None),
            (_, Some(_), Some(_)) => return Err(RemoteFormViolation::AnswerInvalid),
            (_, None, Some(Value::String(text))) => RemoteElicitationAnswer::Choice {
                option_ids: Vec::new(),
                custom_text: Some(text.clone()),
            },
            (ElicitationQuestionKind::FreeText, Some(Value::String(text)), None) => {
                RemoteElicitationAnswer::Text { text: text.clone() }
            }
            (ElicitationQuestionKind::SingleSelect, Some(value), None) if !value.is_array() => {
                RemoteElicitationAnswer::Choice {
                    option_ids: vec![option_id(value)?],
                    custom_text: None,
                }
            }
            (ElicitationQuestionKind::MultiSelect, Some(Value::Array(values)), None) => {
                RemoteElicitationAnswer::Choice {
                    option_ids: values.iter().map(option_id).collect::<Result<_, _>>()?,
                    custom_text: None,
                }
            }
            _ => return Err(RemoteFormViolation::AnswerInvalid),
        },
    ))
}

/// Validates canonical content against a remote form by decoding it back to
/// answers and re-encoding through `remote_form_content`.
pub fn remote_elicitation_form_allows_content(
    form: &RemoteElicitationForm,
    content: &Value,
) -> Result<(), RemoteFormViolation> {
    let Some(fields) = content.as_object() else {
        return Err(RemoteFormViolation::AnswerInvalid);
    };
    let known = |field: &str| {
        form.questions.iter().any(|question| {
            question.question.field_name == field
                || question.custom_answer_key.as_deref() == Some(field)
        })
    };
    if fields.keys().any(|field| !known(field)) {
        return Err(RemoteFormViolation::AnswerInvalid);
    }
    let mut answers = BTreeMap::new();
    for question in &form.questions {
        if let Some(answer) = remote_question_answer(question, content)? {
            answers.insert(question.selector_key.clone(), answer);
        }
    }
    if remote_form_content(form, &answers)? == *content {
        Ok(())
    } else {
        Err(RemoteFormViolation::AnswerInvalid)
    }
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
            ) => remote_elicitation_form_allows_content(form, content).is_ok(),
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
    fn manual_check_without_timeline_output_remains_actionable() {
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
        let service = InterventionCommandService::new(&app);
        let manual = InterventionRequestIdentity::ManualCheck;
        assert_eq!(
            service.settlement(&locator, &manual),
            Ok(InterventionSettlement::Pending)
        );
        let mut stopped = run.clone();
        stopped.status = RunStatus::Completed;
        stopped.outcome = Some(crate::domain::RunOutcome::Killed);
        write_json(
            &app.paths.run_file(&locator.task_id, &locator.run_id),
            &stopped,
        )
        .unwrap();
        assert_eq!(
            service.settlement(&locator, &manual),
            Ok(InterventionSettlement::Cancelled),
            "a stopped run ends the unanswered manual check"
        );
        write_json(&app.paths.run_file(&locator.task_id, &locator.run_id), &run).unwrap();
        let snapshot = service
            .inspect(locator.clone(), InterventionRequestIdentity::ManualCheck)
            .unwrap();
        assert!(
            snapshot.prompt.is_none(),
            "Timeline output is optional presentation data"
        );
        let answered_locator = locator.clone();
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
        assert_eq!(
            service.settlement(&answered_locator, &manual),
            Ok(InterventionSettlement::Answered)
        );
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
    fn desktop_wecom_and_feishu_concurrent_submissions_have_one_accepted_winner() {
        let fixture = permission_fixture(false);
        let snapshot = InterventionCommandService::new(&fixture.app)
            .inspect(fixture.locator.clone(), permission_request())
            .unwrap();
        let barrier = Arc::new(Barrier::new(4));
        let handles = ["allow", "deny", "allow"].map(|option_id| {
            let app = fixture.app.clone_for_background();
            let command = permission_command(&snapshot, option_id);
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                InterventionCommandService::new(&app).execute(command)
            })
        });
        barrier.wait();
        let results = handles.map(|handle| handle.join().unwrap());
        assert_eq!(
            results.iter().filter(|r| matches!(r, Ok(result) if result.status == InterventionCommandStatus::Accepted)).count(),
            1,
        );
        // An identical losing decision is an idempotent success; a different
        // decision is rejected. Neither may be reported as newly Accepted.
        assert_eq!(
            results
                .iter()
                .filter(|r| match r {
                    Ok(result) => result.status == InterventionCommandStatus::AlreadyApplied,
                    Err(error) => error.code == InterventionErrorCode::InterventionAlreadyHandled,
                })
                .count(),
            2,
        );
    }

    #[test]
    fn upgraded_acp_plain_claude_custom_answer_is_one_question() {
        let request = serde_json::from_value(serde_json::json!({
            "mode":"form", "sessionId":"session-001", "message":"Database?",
            "requestedSchema":{"type":"object","properties":{
                "question_0":{"type":"string","enum":["MySQL","PostgreSQL"]},
                "question_0_custom":{"type":"string"}
            }}
        }))
        .unwrap();
        let (prompt, actions) = super::elicitation_prompt_and_actions(&request).unwrap();
        assert_eq!(prompt.questions.len(), 1);
        assert!(prompt.questions[0].allows_custom_answer);
        assert!(!prompt.requires_desktop);
        assert!(matches!(
            actions[0],
            InterventionAllowedAction::ElicitationFixedForm { .. }
        ));
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
        assert_eq!(form.questions.len(), 1);
        let question = &form.questions[0];
        assert_eq!(question.selector_key, "q0");
        assert_eq!(question.question.field_name, "question_0");
        assert_eq!(question.question.options.len(), 4);
        assert!(matches!(
            intervention_action_for_elicitation_form(form, &choice_answers(&[("q0", &["0"])])),
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
        // Channel limits are applied per target; the domain form stays complete.
        assert!(!prompt.requires_desktop);
        let InterventionAllowedAction::ElicitationFixedForm { form } = &snapshot.allowed_actions[0]
        else {
            panic!("the complete AskUserQuestion form is remotable");
        };
        assert_eq!(
            form.questions
                .iter()
                .map(|question| question.custom_answer_key.as_deref())
                .collect::<Vec<_>>(),
            vec![
                Some("question_0_custom"),
                Some("question_1_custom"),
                Some("question_2_custom"),
                Some("question_3_custom")
            ]
        );
        assert_eq!(
            snapshot.allowed_actions[1],
            InterventionAllowedAction::ElicitationDecline
        );
    }

    #[test]
    fn desktop_elicitation_accepts_schema_valid_answers_outside_the_remote_form() {
        let cases = [
            (
                "constrained free text",
                json!({
                    "mode": "form",
                    "sessionId": "session-001",
                    "message": "Add release notes",
                    "requestedSchema": {
                        "type": "object",
                        "properties": {
                            "notes": { "type": "string", "title": "Release notes", "minLength": 8 }
                        },
                        "required": ["notes"]
                    }
                }),
                json!({ "notes": "Ship after the desktop checks pass." }),
                json!({ "notes": "short" }),
            ),
            (
                "numeric field",
                json!({
                    "mode": "form",
                    "sessionId": "session-001",
                    "message": "Choose a retry count",
                    "requestedSchema": {
                        "type": "object",
                        "properties": {
                            "retries": { "type": "integer", "title": "Retries" }
                        },
                        "required": ["retries"]
                    }
                }),
                json!({ "retries": 3 }),
                json!({ "retries": "three" }),
            ),
        ];

        for (case, request, content, invalid_content) in cases {
            let fixture = permission_fixture(false);
            write_elicitation(&fixture, serde_json::from_value(request).unwrap());
            let service = InterventionCommandService::new(&fixture.app);
            let request = InterventionRequestIdentity::Elicitation {
                elicitation_id: "elicit-001".into(),
            };
            let snapshot = service
                .inspect(fixture.locator.clone(), request.clone())
                .unwrap();
            let action = InterventionAction::Elicitation {
                action: ElicitationAction::Accept,
                content: Some(content),
            };
            assert!(
                !intervention_action_is_allowed(&snapshot.allowed_actions, &action),
                "{case} must remain unavailable to the IM transport"
            );
            let invalid = service.execute(InterventionCommand {
                locator: fixture.locator.clone(),
                request: request.clone(),
                expected_state: snapshot.expected_state.clone(),
                action: InterventionAction::Elicitation {
                    action: ElicitationAction::Accept,
                    content: Some(invalid_content),
                },
                expires_at_ms: snapshot.expires_at_ms,
            });
            assert_eq!(
                invalid.unwrap_err().code,
                InterventionErrorCode::InterventionActionInvalid,
                "{case} must still satisfy the complete ACP schema"
            );

            let result = service.execute(InterventionCommand {
                locator: fixture.locator.clone(),
                request,
                expected_state: snapshot.expected_state,
                action,
                expires_at_ms: snapshot.expires_at_ms,
            });

            assert_eq!(
                result.unwrap().status,
                InterventionCommandStatus::Accepted,
                "{case} must remain actionable on desktop"
            );
        }
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
        assert_eq!(
            form.questions
                .iter()
                .map(|question| (
                    question.selector_key.as_str(),
                    question.constraints.required,
                    question.question.options.len(),
                    question.custom_answer_key.as_deref()
                ))
                .collect::<Vec<_>>(),
            vec![
                ("q0", true, 1, None),
                ("q1", true, 1, None),
                ("q2", true, 1, Some("q2_custom"))
            ]
        );
        assert!(
            intervention_action_for_elicitation_form(
                form,
                &choice_answers(&[("q0", &["0"]), ("q1", &["0"]), ("q2", &["0"])])
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
        assert_eq!(
            form.questions[0].question.question_kind,
            ElicitationQuestionKind::MultiSelect
        );
        // A required multi-select may not be submitted empty.
        assert!(!form.questions[0].constraints.allows_empty);
        assert_eq!(
            intervention_action_for_elicitation_form(form, &choice_answers(&[("q0", &[])])),
            Err(RemoteFormViolation::AnswerMissing)
        );
        assert_eq!(
            intervention_action_for_elicitation_form(form, &choice_answers(&[("q0", &["0", "0"])])),
            Err(RemoteFormViolation::OptionInvalid)
        );

        let request = serde_json::from_value(json!({
            "mode": "form",
            "sessionId": "session-001",
            "message": "four questions",
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
        // Question count is a channel capability, not a domain limit.
        assert!(!snapshot.prompt.unwrap().requires_desktop);
        let InterventionAllowedAction::ElicitationFixedForm { form } = &snapshot.allowed_actions[0]
        else {
            panic!("four scalar questions are still a complete remote form");
        };
        assert_eq!(form.questions.len(), 4);
        assert!(form.decline_allowed);
    }

    fn choice_answers(answers: &[(&str, &[&str])]) -> BTreeMap<String, RemoteElicitationAnswer> {
        answers
            .iter()
            .map(|(selector, option_ids)| {
                (
                    (*selector).to_owned(),
                    RemoteElicitationAnswer::Choice {
                        option_ids: option_ids.iter().map(|id| (*id).to_owned()).collect(),
                        custom_text: None,
                    },
                )
            })
            .collect()
    }

    fn remote_form_for(properties: Value, required: Value) -> Option<RemoteElicitationForm> {
        let request = serde_json::from_value(json!({
            "mode": "form",
            "sessionId": "session-001",
            "message": "answer",
            "requestedSchema": {
                "type": "object",
                "properties": properties,
                "required": required
            }
        }))
        .unwrap();
        let (_, actions) = super::elicitation_prompt_and_actions(&request).unwrap();
        actions.into_iter().find_map(|action| match action {
            InterventionAllowedAction::ElicitationFixedForm { form } => Some(form),
            _ => None,
        })
    }

    fn text_answer(selector: &str, text: &str) -> BTreeMap<String, RemoteElicitationAnswer> {
        BTreeMap::from([(
            selector.to_owned(),
            RemoteElicitationAnswer::Text {
                text: text.to_owned(),
            },
        )])
    }

    #[test]
    fn remote_free_text_derives_length_and_multiline_from_schema() {
        let form = remote_form_for(
            json!({
                "name": {"type": "string", "title": "Name", "maxLength": 5},
                "notes": {"type": "string", "title": "Notes"}
            }),
            json!(["name"]),
        )
        .expect("whitelisted free text is remotable");
        let constraints = |field: &str| {
            form.questions
                .iter()
                .find(|question| question.question.field_name == field)
                .unwrap()
                .constraints
        };
        assert_eq!(
            constraints("name"),
            RemoteQuestionConstraints {
                required: true,
                allows_empty: false,
                max_chars: 5,
                multiline: false
            }
        );
        assert_eq!(constraints("notes").max_chars, MAX_REMOTE_TEXT_CHARS);
        assert!(constraints("notes").multiline);
        let selector = |field: &str| {
            form.questions
                .iter()
                .find(|question| question.question.field_name == field)
                .unwrap()
                .selector_key
                .clone()
        };

        assert_eq!(
            remote_form_content(&form, &text_answer(&selector("name"), "Alice")),
            Ok(json!({"name": "Alice"}))
        );
        assert_eq!(
            remote_form_content(&form, &text_answer(&selector("name"), "Alice!")),
            Err(RemoteFormViolation::TextTooLong)
        );
        // Blank text is unanswered, so the required question is missing.
        assert_eq!(
            remote_form_content(&form, &text_answer(&selector("name"), "   ")),
            Err(RemoteFormViolation::AnswerMissing)
        );
        assert_eq!(
            remote_form_content(&form, &text_answer("q9", "x")),
            Err(RemoteFormViolation::AnswerInvalid)
        );
    }

    #[test]
    fn remote_free_text_with_unlisted_constraint_requires_desktop() {
        for constraint in [
            json!({"type": "string", "minLength": 3}),
            json!({"type": "string", "pattern": "^a"}),
            json!({"type": "string", "format": "email"}),
            json!({"type": "integer"}),
        ] {
            assert!(
                remote_form_for(json!({"value": constraint}), json!([])).is_none(),
                "{constraint} must not be remotable"
            );
        }
    }

    #[test]
    fn remote_custom_answer_writes_only_the_custom_field() {
        let form = remote_form_for(
            json!({
                "database": {"type": "string", "title": "DB", "enum": ["MySQL", "PostgreSQL"]},
                "database_custom": {"type": "string", "title": "Other", "maxLength": 4}
            }),
            json!(["database"]),
        )
        .unwrap();
        let question = &form.questions[0];
        assert_eq!(
            question.custom_answer_key.as_deref(),
            Some("database_custom")
        );
        assert_eq!(question.constraints.max_chars, 4);
        let custom = |text: &str, option_ids: &[&str]| {
            BTreeMap::from([(
                "q0".to_owned(),
                RemoteElicitationAnswer::Choice {
                    option_ids: option_ids.iter().map(|id| (*id).to_owned()).collect(),
                    custom_text: Some(text.to_owned()),
                },
            )])
        };
        let content = remote_form_content(&form, &custom(" H2 ", &[])).unwrap();
        assert_eq!(content, json!({"database_custom": " H2 "}));
        assert_eq!(
            remote_elicitation_form_allows_content(&form, &content),
            Ok(())
        );
        assert_eq!(
            remote_form_content(&form, &custom("  ", &[])),
            Err(RemoteFormViolation::CustomAnswerMissing)
        );
        assert_eq!(
            remote_form_content(&form, &custom("SQLite", &[])),
            Err(RemoteFormViolation::TextTooLong)
        );
        assert_eq!(
            remote_form_content(&form, &custom("H2", &["0"])),
            Err(RemoteFormViolation::AnswerInvalid)
        );
    }

    #[test]
    fn a_custom_answer_satisfies_its_required_choice_field() {
        let schema = json!({
            "type": "object",
            "properties": {
                "database": {"type": "string", "title": "DB", "enum": ["MySQL", "PostgreSQL"]},
                "database_custom": {"type": "string", "title": "Other"},
                "note": {"type": "string", "title": "Note"}
            },
            "required": ["database", "note"]
        });
        let valid = |content: Value| elicitation_content_is_valid(&schema, &content).unwrap();
        assert!(valid(json!({"database_custom": "H2", "note": "n"})));
        assert!(valid(json!({"database": "MySQL", "note": "n"})));
        // Blank custom text does not answer the choice.
        assert!(!valid(json!({"database_custom": "  ", "note": "n"})));
        assert!(!valid(json!({"note": "n"})));
        // Unpaired required fields and option constraints stay enforced.
        assert!(!valid(json!({"database_custom": "H2"})));
        assert!(!valid(json!({"database": "SQLite", "note": "n"})));

        let form = remote_form_for(schema["properties"].clone(), schema["required"].clone())
            .expect("required choice with a custom answer is remotable");
        assert_eq!(
            form.questions[0].custom_answer_key.as_deref(),
            Some("database_custom")
        );
        let mut answers = text_answer("q1", "n");
        answers.insert(
            "q0".to_owned(),
            RemoteElicitationAnswer::Choice {
                option_ids: Vec::new(),
                custom_text: Some("H2".to_owned()),
            },
        );
        let content = remote_form_content(&form, &answers).unwrap();
        assert_eq!(content, json!({"database_custom": "H2", "note": "n"}));
        assert!(valid(content));
    }

    #[test]
    fn remote_custom_answer_failing_whitelist_only_drops_the_custom_entry() {
        let form = remote_form_for(
            json!({
                "database": {"type": "string", "title": "DB", "enum": ["MySQL", "PostgreSQL"]},
                "database_custom": {"type": "string", "title": "Other", "pattern": "^[a-z]+$"}
            }),
            json!(["database"]),
        )
        .expect("the listed options stay remotable");
        assert_eq!(form.questions[0].custom_answer_key, None);
        assert_eq!(
            form.questions[0].constraints.max_chars,
            MAX_REMOTE_TEXT_CHARS
        );
        assert_eq!(
            remote_form_content(&form, &choice_answers(&[("q0", &["1"])])),
            Ok(json!({"database": "PostgreSQL"}))
        );
    }

    #[test]
    fn remote_form_content_round_trips_through_allows_content() {
        let form = remote_form_for(
            json!({
                "database": {"type": "string", "enum": ["MySQL", "PostgreSQL"]},
                "features": {"type": "array", "items": {"type": "string", "enum": ["search", "audit", "sso"]}},
                "notes": {"type": "string"}
            }),
            json!(["database"]),
        )
        .unwrap();
        let selector = |field: &str| {
            form.questions
                .iter()
                .find(|question| question.question.field_name == field)
                .unwrap()
                .selector_key
                .clone()
        };
        let mut answers = choice_answers(&[]);
        answers.insert(
            selector("database"),
            RemoteElicitationAnswer::Choice {
                option_ids: vec!["1".into()],
                custom_text: None,
            },
        );
        answers.insert(
            selector("features"),
            RemoteElicitationAnswer::Choice {
                option_ids: vec!["2".into(), "0".into()],
                custom_text: None,
            },
        );
        answers.insert(
            selector("notes"),
            RemoteElicitationAnswer::Text {
                text: "ship it".into(),
            },
        );
        let content = remote_form_content(&form, &answers).unwrap();
        assert_eq!(
            content,
            json!({"database": "PostgreSQL", "features": ["sso", "search"], "notes": "ship it"})
        );
        assert_eq!(
            remote_elicitation_form_allows_content(&form, &content),
            Ok(())
        );
        // Non-canonical option ids and unknown fields are refused.
        assert_eq!(
            remote_form_content(&form, &choice_answers(&[(&selector("database"), &["01"])])),
            Err(RemoteFormViolation::OptionInvalid)
        );
        assert_eq!(
            remote_elicitation_form_allows_content(
                &form,
                &json!({"database": "MySQL", "extra": "x"})
            ),
            Err(RemoteFormViolation::AnswerInvalid)
        );
        assert_eq!(
            remote_elicitation_form_allows_content(&form, &json!({"database": "Oracle"})),
            Err(RemoteFormViolation::OptionInvalid)
        );
        assert_eq!(
            remote_elicitation_form_allows_content(&form, &json!({"features": ["audit"]})),
            Err(RemoteFormViolation::AnswerMissing)
        );
    }

    fn write_pending_permission_item(attempt_dir: &camino::Utf8Path) {
        let mut pending = crate::acp::events::permission_request_event(
            7,
            "permission-001".into(),
            json!({ "options": [{ "optionId": "allow", "name": "Allow", "kind": "allow_once" }] }),
        );
        pending.id = "permission-permission-001".into();
        pending.started_seq = Some(7);
        pending.ended_seq = Some(7);
        crate::acp::events::write_timeline_items(
            &attempt_dir.join("acp.timeline.jsonl"),
            &[pending],
        )
        .unwrap();
    }

    #[test]
    fn a_permission_the_runtime_cancelled_is_handled_not_missing() {
        let fixture = permission_fixture(false);
        let service = InterventionCommandService::new(&fixture.app);
        let attempt_dir = service.attempt_dir(&fixture.locator);
        write_pending_permission_item(&attempt_dir);
        let request = permission_request();
        assert_eq!(
            service.settlement(&fixture.locator, &request),
            Ok(InterventionSettlement::Pending)
        );

        crate::acp::permission::cancel_pending_permission_requests(&attempt_dir, "2Z".into())
            .unwrap();

        assert_eq!(
            service.settlement(&fixture.locator, &request),
            Ok(InterventionSettlement::Cancelled)
        );
        assert_eq!(
            service
                .inspect(fixture.locator.clone(), request.clone())
                .unwrap_err()
                .code,
            InterventionErrorCode::InterventionAlreadyHandled,
            "a stopped request is handled, not an unknown one"
        );
        crate::acp::permission::remove_permission_signal_files(&attempt_dir, "permission-001")
            .unwrap();
        assert_eq!(
            service.settlement(&fixture.locator, &request),
            Ok(InterventionSettlement::Cancelled),
            "the settled timeline item outlives the signal files"
        );
    }

    #[test]
    fn an_answered_permission_settles_as_answered_after_runtime_cleanup() {
        let fixture = permission_fixture(false);
        let service = InterventionCommandService::new(&fixture.app);
        let attempt_dir = service.attempt_dir(&fixture.locator);
        write_pending_permission_item(&attempt_dir);
        let snapshot = service
            .inspect(fixture.locator.clone(), permission_request())
            .unwrap();
        service
            .execute(permission_command(&snapshot, "allow"))
            .unwrap();
        crate::acp::permission::remove_permission_signal_files(&attempt_dir, "permission-001")
            .unwrap();

        assert_eq!(
            service.settlement(&fixture.locator, &permission_request()),
            Ok(InterventionSettlement::Answered)
        );
        let abandoned = InterventionRequestIdentity::Permission {
            request_id: "never-published".into(),
        };
        assert_eq!(
            service
                .settlement(&fixture.locator, &abandoned)
                .unwrap_err()
                .code,
            InterventionErrorCode::InterventionRequestNotFound
        );
    }

    #[test]
    fn a_cancelled_or_abandoned_elicitation_settles_as_cancelled() {
        let fixture = permission_fixture(false);
        write_choice_elicitation(&fixture, &["PostgreSQL", "MySQL"]);
        let service = InterventionCommandService::new(&fixture.app);
        let attempt_dir = service.attempt_dir(&fixture.locator);
        let request = InterventionRequestIdentity::Elicitation {
            elicitation_id: "elicit-001".into(),
        };
        let pending: PendingElicitationState =
            read_json(&pending_elicitation_file(&attempt_dir, "elicit-001")).unwrap();
        let mut item = crate::acp::events::elicitation_request_event(
            8,
            "elicit-001".into(),
            &pending.payload.request,
        );
        item.started_seq = Some(8);
        item.ended_seq = Some(8);
        crate::acp::events::write_timeline_items(&attempt_dir.join("acp.timeline.jsonl"), &[item])
            .unwrap();
        assert_eq!(
            service.settlement(&fixture.locator, &request),
            Ok(InterventionSettlement::Pending)
        );

        // A runtime that disappears leaves the request item without an answer.
        crate::acp::elicitation::remove_elicitation_signal_files(&attempt_dir, "elicit-001")
            .unwrap();
        assert_eq!(
            service.settlement(&fixture.locator, &request),
            Ok(InterventionSettlement::Cancelled)
        );

        write_choice_elicitation(&fixture, &["PostgreSQL", "MySQL"]);
        crate::acp::elicitation::cancel_pending_elicitation_requests(&attempt_dir, "2Z".into())
            .unwrap();
        crate::acp::elicitation::remove_elicitation_signal_files(&attempt_dir, "elicit-001")
            .unwrap();
        assert_eq!(
            service.settlement(&fixture.locator, &request),
            Ok(InterventionSettlement::Cancelled),
            "the timeline cancel response is the settled fact"
        );
        assert_eq!(
            service
                .inspect(fixture.locator.clone(), request)
                .unwrap_err()
                .code,
            InterventionErrorCode::InterventionAlreadyHandled
        );
    }

    #[test]
    fn an_answered_elicitation_settles_as_answered() {
        let fixture = permission_fixture(false);
        write_choice_elicitation(&fixture, &["PostgreSQL", "MySQL"]);
        let service = InterventionCommandService::new(&fixture.app);
        let attempt_dir = service.attempt_dir(&fixture.locator);
        let request = InterventionRequestIdentity::Elicitation {
            elicitation_id: "elicit-001".into(),
        };
        crate::acp::elicitation::write_elicitation_response_if_pending(
            &attempt_dir,
            "elicit-001",
            ElicitationAction::Decline,
            None,
            "2Z".into(),
        )
        .unwrap();
        assert_eq!(
            service.settlement(&fixture.locator, &request),
            Ok(InterventionSettlement::Answered),
            "a decline is an answer, not a cancellation"
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
