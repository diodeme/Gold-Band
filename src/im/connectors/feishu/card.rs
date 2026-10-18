//! Feishu Card JSON 2.0 rendering and callback decoding. All functions here
//! are pure; callers provide the authoritative delivery and action token.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};

mod wire;
use wire::{
    Behavior, Button, ButtonType, CallbackIntent, CallbackValue, Card, ControlValue, Element,
    FormAction, Header, HeaderTemplate, Input, InputType, MultiSelect, Reply, Select, SelectOption,
    SingleSelect, Text, ToastType, Width,
};

use crate::acp::elicitation::ElicitationAction;
use crate::app::intervention::{
    ElicitationQuestionKind, InterventionAction, InterventionAllowedAction,
    REMOTE_CUSTOM_ANSWER_OPTION, RemoteElicitationAnswer,
};
use crate::domain::NodeOutcome;
use crate::im::{
    ImActionResponse, ImActionResponseState, ImActionTokenCodec, ImDelivery, ImDeliveryPayload,
    ImErrorCode, ImInboundEnvelope, ImInboundError, ImLocale, ImResolutionOutcome, ImResolvedBy,
    ImSubmittedAction, InterventionPresentation, deterministic_action_id,
};

use super::super::{
    action_label, intervention_actions, localized, message_state_label,
    notification_context_markdown, notification_markdown, notification_markdown_without_context,
    resolved_by_copy,
};
use super::ImChannelKind;

const CARD_ACTION_EVENT: &str = "card.action.trigger";
const PERMISSION_BUTTON_LIMIT: usize = 3;
const FORM_SUBMIT_NAME: &str = "gb_submit";
const FORM_NAME: &str = "gb_form";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeishuCardDecodeError {
    pub(crate) code: ImErrorCode,
}

pub(crate) struct FeishuCardAction {
    pub(crate) app_id: String,
    pub(crate) envelope: ImInboundEnvelope,
}

#[derive(Deserialize)]
struct CardActionWire {
    header: CardActionHeader,
    event: CardActionEvent,
}

#[derive(Deserialize)]
struct CardActionHeader {
    #[serde(default)]
    event_type: String,
    #[serde(default)]
    event_id: String,
    #[serde(default)]
    app_id: String,
}

#[derive(Deserialize)]
struct CardActionEvent {
    operator: CardOperator,
    action: CardAction,
    context: CardContext,
}

#[derive(Deserialize)]
struct CardOperator {
    open_id: String,
}

#[derive(Deserialize)]
struct CardAction {
    value: Value,
    #[serde(default)]
    form_value: Option<Value>,
}

#[derive(Deserialize)]
struct CardContext {
    open_chat_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CardActionValue {
    delivery_id: String,
    action_token: String,
    action_index: Option<usize>,
    #[serde(default)]
    form: bool,
    #[serde(default)]
    permission_select: bool,
    #[serde(default)]
    decline: bool,
}

#[derive(Clone, Copy)]
enum ActionValueMode {
    Decline,
    Index(usize),
    Form,
    PermissionSelect,
}

pub(crate) fn delivery_card_content(
    delivery: &ImDelivery,
    token: &crate::im::ImActionTokenCodec,
    locale: ImLocale,
) -> Result<String, ImInboundError> {
    match &delivery.payload {
        ImDeliveryPayload::Intervention { .. } => {
            let actions = intervention_actions(delivery, token, locale)?;
            Ok(Card::new(
                pending_elements(delivery, &actions, locale)?,
                Some(card_header(
                    &pending_title(delivery, locale),
                    HeaderTemplate::Blue,
                )),
            )
            .content())
        }
        ImDeliveryPayload::Information { .. } => Ok(read_only_card_content(
            &notification_markdown(delivery, locale),
        )),
        ImDeliveryPayload::InterventionResolution { .. } => Ok(resolution_payload_card_content(
            &delivery.payload,
            locale,
            None,
            token,
        )),
    }
}

pub(crate) fn resolution_payload_card_content(
    payload: &ImDeliveryPayload,
    locale: ImLocale,
    source: Option<&ImDelivery>,
    token: &ImActionTokenCodec,
) -> String {
    let ImDeliveryPayload::InterventionResolution { resolution, .. } = payload else {
        return read_only_card_content("");
    };
    let extra = terminal_details(
        source,
        Some(&resolution.outcome),
        Some(resolution.resolved_by),
        locale,
    );
    let title = resolution_title(&resolution.outcome, locale);
    if let Some(source) = source {
        let elements =
            source_terminal_elements(source, token, locale, Some(&resolution.outcome), &extra);
        return terminal_interactive_card(elements, title).content();
    }
    let display_ref = resolution
        .display_ref
        .map(|value| format!("{value:04}"))
        .unwrap_or_else(|| "----".to_owned());
    let mut lines = vec![format!("**id={}**", display_ref)];
    lines.extend(extra);
    terminal_read_only_card(&lines.join("\n\n"), title).content()
}

/// A cancelled request was answered by no one: label it cancelled, not handled.
fn resolution_title(outcome: &ImResolutionOutcome, locale: ImLocale) -> &'static str {
    if outcome.is_cancelled() {
        localized(locale, "im.intervention.cancelled")
    } else {
        message_state_label(locale, crate::im::ImMessageState::Handled)
    }
}

fn pending_elements(
    delivery: &ImDelivery,
    actions: &[crate::im::connectors::RenderedAction],
    locale: ImLocale,
) -> Result<Vec<Element>, ImInboundError> {
    let mut elements = vec![markdown_element(&pending_context(delivery, locale))];
    let permission: Vec<_> = if actions.iter().all(|action| {
        matches!(
            action.value.action,
            InterventionAction::PermissionOption { .. }
        )
    }) {
        actions.to_vec()
    } else {
        Vec::new()
    };
    if !permission.is_empty() {
        if permission.len() <= PERMISSION_BUTTON_LIMIT {
            elements.extend(permission.iter().enumerate().map(|(index, action)| {
                button_element(
                    &action.label,
                    button_type(action.value.action.clone(), index == 0),
                    action_value(
                        &action.value.delivery_id,
                        &action.value.action_token,
                        ActionValueMode::Index(action.action_index),
                    ),
                )
            }));
        } else {
            elements.push(permission_select_form(&permission, locale));
        }
        return Ok(elements);
    }

    let failure = actions.iter().find(|action| {
        matches!(
            action.value.action,
            InterventionAction::ManualCheck {
                outcome: NodeOutcome::Failure
            }
        )
    });
    let success = actions.iter().find(|action| {
        matches!(
            action.value.action,
            InterventionAction::ManualCheck {
                outcome: NodeOutcome::Success
            }
        )
    });
    if let (Some(failure), Some(success)) = (failure, success) {
        elements.push(button_element(
            &failure.label,
            ButtonType::Danger,
            action_value(
                &failure.value.delivery_id,
                &failure.value.action_token,
                ActionValueMode::Index(failure.action_index),
            ),
        ));
        elements.push(button_element(
            &success.label,
            ButtonType::Default,
            action_value(
                &success.value.delivery_id,
                &success.value.action_token,
                ActionValueMode::Index(success.action_index),
            ),
        ));
        return Ok(elements);
    }

    let form = actions.iter().find(|action| {
        matches!(action.value.action, InterventionAction::Elicitation { .. })
            && action.label == localized(locale, "im.elicitation.form.label")
    });
    if let Some(form_action) = form {
        let ImDeliveryPayload::Intervention { reference, .. } = &delivery.payload else {
            return Err(ImInboundError::InformationNotActionable);
        };
        let form = reference
            .allowed_actions
            .iter()
            .find_map(|action| match action {
                InterventionAllowedAction::ElicitationFixedForm { form } => Some(form),
                _ => None,
            });
        if let Some(form) = form {
            let mut form_elements = Vec::new();
            for question in &form.questions {
                form_elements.extend(question_elements(question, locale));
            }
            form_elements.push(submit_button(
                &form_action.value.delivery_id,
                &form_action.value.action_token,
                locale,
            ));
            elements.push(Element::Form {
                name: FORM_NAME.into(),
                elements: form_elements,
            });
            if form.decline_allowed
                && let Some(decline) = actions.iter().find(|action| {
                    matches!(
                        action.value.action,
                        InterventionAction::Elicitation {
                            action: ElicitationAction::Decline,
                            ..
                        }
                    )
                })
            {
                elements.push(button_element(
                    localized(locale, "im.action.decline"),
                    ButtonType::Danger,
                    action_value(
                        &decline.value.delivery_id,
                        &decline.value.action_token,
                        ActionValueMode::Decline,
                    ),
                ));
            }
            return Ok(elements);
        }
    }

    // Unexpected action shapes still render as safe callback buttons. The
    // inbound service rejects any index not present in the delivery.
    elements.extend(actions.iter().enumerate().map(|(index, action)| {
        button_element(
            &action.label,
            button_type(action.value.action.clone(), index == 0),
            action_value(
                &action.value.delivery_id,
                &action.value.action_token,
                ActionValueMode::Index(action.action_index),
            ),
        )
    }));
    Ok(elements)
}

fn pending_context(delivery: &ImDelivery, locale: ImLocale) -> String {
    let mut lines = Vec::new();
    if let Some(display_ref) = delivery.display_ref {
        lines.push(format!("**id={display_ref:04}**"));
    }
    let context = notification_context_markdown(delivery, locale);
    if !context.is_empty() {
        lines.push(context);
    }
    if let ImDeliveryPayload::Intervention { presentation, .. } = &delivery.payload
        && let Some(context) = presentation
            .context
            .as_deref()
            .filter(|text| !text.trim().is_empty())
    {
        lines.push(format!(
            "**{}**\n\n{context}",
            localized(locale, "im.elicitation.detail.context")
        ));
    }
    let body = notification_markdown_without_context(delivery, locale);
    if !body.is_empty() {
        lines.push(body);
    }
    lines.join("\n\n")
}

fn permission_select_form(
    actions: &[crate::im::connectors::RenderedAction],
    locale: ImLocale,
) -> Element {
    let token = actions
        .first()
        .map(|action| action.value.action_token.clone())
        .unwrap_or_default();
    let delivery_id = actions
        .first()
        .map(|action| action.value.delivery_id.clone())
        .unwrap_or_default();
    let options = actions
        .iter()
        .filter_map(|action| match &action.value.action {
            InterventionAction::PermissionOption { option_id, .. } => Some(SelectOption {
                text: Text::plain(&action.label),
                value: option_id.clone(),
            }),
            _ => None,
        })
        .collect();
    Element::Form {
        name: FORM_NAME.into(),
        elements: vec![
            Element::SelectStatic(SingleSelect {
                select: Select {
                    name: "q0".into(),
                    required: true,
                    width: Width::Fill,
                    placeholder: Text::plain(localized(locale, "im.feishu.form.select")),
                    options,
                    disabled: None,
                },
                initial_index: None,
            }),
            form_submit_button(
                &delivery_id,
                &token,
                locale,
                ActionValueMode::PermissionSelect,
            ),
        ],
    }
}

fn question_elements(
    question: &crate::app::intervention::RemoteQuestion,
    locale: ImLocale,
) -> Vec<Element> {
    let mut elements = vec![markdown_element(&format!(
        "**{}**{}",
        question.question.title,
        question
            .question
            .description
            .as_deref()
            .filter(|description| !description.trim().is_empty())
            .map(|description| format!("\n{description}"))
            .unwrap_or_default()
    ))];
    match question.question.question_kind {
        ElicitationQuestionKind::FreeText => elements.push(input_element(
            &question.selector_key,
            question.constraints.required,
            question.constraints.max_chars,
            question.constraints.multiline,
            locale,
        )),
        ElicitationQuestionKind::SingleSelect => {
            elements.push(select_element(question, false, locale));
            if question.custom_answer_key.is_some() {
                elements.push(input_element(
                    &format!("{}_other", question.selector_key),
                    false,
                    question.constraints.max_chars,
                    false,
                    locale,
                ));
            }
        }
        ElicitationQuestionKind::MultiSelect => {
            elements.push(select_element(question, true, locale));
            if question.custom_answer_key.is_some() {
                elements.push(input_element(
                    &format!("{}_other", question.selector_key),
                    false,
                    question.constraints.max_chars,
                    false,
                    locale,
                ));
            }
        }
    }
    elements
}

fn select_element(
    question: &crate::app::intervention::RemoteQuestion,
    multi: bool,
    locale: ImLocale,
) -> Element {
    let mut options: Vec<SelectOption> = question
        .question
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| SelectOption {
            text: Text::plain(&option.label),
            value: index.to_string(),
        })
        .collect();
    if question.custom_answer_key.is_some() {
        options.push(SelectOption {
            text: Text::plain(localized(locale, "im.feishu.form.other")),
            value: REMOTE_CUSTOM_ANSWER_OPTION.into(),
        });
    }
    let select = Select {
        name: question.selector_key.clone(),
        required: question.constraints.required,
        width: Width::Fill,
        placeholder: Text::plain(if multi {
            localized(locale, "im.feishu.form.multiSelect")
        } else {
            localized(locale, "im.feishu.form.select")
        }),
        options,
        disabled: None,
    };
    if multi {
        Element::MultiSelectStatic(MultiSelect {
            select,
            selected_values: None,
        })
    } else {
        Element::SelectStatic(SingleSelect {
            select,
            initial_index: None,
        })
    }
}

fn input_element(
    name: &str,
    required: bool,
    max_chars: u32,
    multiline: bool,
    locale: ImLocale,
) -> Element {
    Element::Input(Input {
        name: name.into(),
        required,
        max_length: max_chars,
        width: Width::Fill,
        placeholder: Text::plain(if required {
            localized(locale, "im.feishu.form.input")
        } else {
            localized(locale, "im.feishu.form.optional")
        }),
        input_type: multiline.then_some(InputType::MultilineText),
        rows: multiline.then_some(3),
        auto_resize: multiline.then_some(true),
        disabled: None,
        default_value: None,
    })
}

fn submit_button(delivery_id: &str, token: &str, locale: ImLocale) -> Element {
    form_submit_button(delivery_id, token, locale, ActionValueMode::Form)
}

fn form_submit_button(
    delivery_id: &str,
    token: &str,
    locale: ImLocale,
    mode: ActionValueMode,
) -> Element {
    Element::Button(Button {
        text: Text::plain(localized(locale, "im.feishu.form.submit")),
        button_type: ButtonType::Primary,
        behaviors: vec![Behavior::Callback {
            value: action_value(delivery_id, token, mode),
        }],
        name: Some(FORM_SUBMIT_NAME.into()),
        form_action_type: Some(FormAction::Submit),
        disabled: None,
    })
}

fn action_value(delivery_id: &str, token: &str, mode: ActionValueMode) -> CallbackValue {
    CallbackValue {
        delivery_id: delivery_id.into(),
        action_token: token.into(),
        intent: match mode {
            ActionValueMode::Decline => CallbackIntent::Decline { decline: true },
            ActionValueMode::Index(action_index) => CallbackIntent::Index { action_index },
            ActionValueMode::Form => CallbackIntent::Form { form: true },
            ActionValueMode::PermissionSelect => CallbackIntent::PermissionSelect {
                permission_select: true,
            },
        },
    }
}

fn button_element(label: &str, button_type: ButtonType, value: CallbackValue) -> Element {
    Element::Button(Button {
        text: Text::plain(label),
        button_type,
        behaviors: vec![Behavior::Callback { value }],
        name: None,
        form_action_type: None,
        disabled: None,
    })
}

fn button_type(action: InterventionAction, primary: bool) -> ButtonType {
    match action {
        InterventionAction::PermissionOption { .. } => {
            if primary {
                ButtonType::Primary
            } else {
                ButtonType::Default
            }
        }
        InterventionAction::Elicitation {
            action: ElicitationAction::Decline,
            ..
        } => ButtonType::Danger,
        InterventionAction::ManualCheck {
            outcome: NodeOutcome::Failure,
        } => ButtonType::Danger,
        _ => {
            if primary {
                ButtonType::Primary
            } else {
                ButtonType::Default
            }
        }
    }
}

pub(crate) fn decode_card_action(
    payload: &[u8],
) -> Result<FeishuCardAction, FeishuCardDecodeError> {
    let wire: CardActionWire = serde_json::from_slice(payload).map_err(|_| protocol_invalid())?;
    let event = wire.event;
    if wire.header.event_type != CARD_ACTION_EVENT
        || wire.header.event_id.trim().is_empty()
        || wire.header.app_id.trim().is_empty()
    {
        return Err(protocol_invalid());
    }
    let value = parse_action_value(&event.action.value)?;
    let form = event.action.form_value.unwrap_or_else(|| json!({}));
    let action = match value_mode(&value) {
        Some(ActionValueMode::Decline) => ImSubmittedAction::ElicitationDecline,
        Some(ActionValueMode::PermissionSelect) => {
            let option_id = form
                .get("q0")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(protocol_invalid)?;
            ImSubmittedAction::PermissionOption { option_id }
        }
        Some(ActionValueMode::Form) => decode_form_action(&form)?,
        Some(ActionValueMode::Index(index)) => ImSubmittedAction::PublishedAction { index },
        None => return Err(protocol_invalid()),
    };
    let actor_id = non_empty(event.operator.open_id)?;
    let conversation_id = non_empty(event.context.open_chat_id)?;
    Ok(FeishuCardAction {
        app_id: wire.header.app_id,
        envelope: ImInboundEnvelope {
            action_id: deterministic_action_id(ImChannelKind::Feishu, &wire.header.event_id),
            channel: ImChannelKind::Feishu,
            platform_event_id: wire.header.event_id,
            conversation_id,
            actor_id,
            delivery_id: non_empty(value.delivery_id)?,
            action_token: Some(non_empty(value.action_token)?),
            action,
        },
    })
}

fn decode_form_action(form: &Value) -> Result<ImSubmittedAction, FeishuCardDecodeError> {
    let object = form.as_object().ok_or_else(protocol_invalid)?;
    let mut answers = BTreeMap::new();
    for (key, value) in object {
        let answer = match value {
            Value::String(text) => RemoteElicitationAnswer::Text { text: text.clone() },
            Value::Array(options) => RemoteElicitationAnswer::Choice {
                option_ids: options
                    .iter()
                    .map(|option| option.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(protocol_invalid)?,
                custom_text: None,
            },
            _ => return Err(protocol_invalid()),
        };
        answers.insert(key.clone(), answer);
    }
    Ok(ImSubmittedAction::ElicitationForm { answers })
}

fn parse_action_value(value: &Value) -> Result<CardActionValue, FeishuCardDecodeError> {
    let parsed = match value {
        Value::String(raw) => serde_json::from_str(raw),
        Value::Object(_) => serde_json::from_value(value.clone()),
        _ => Err(serde_json::Error::io(std::io::Error::other(
            "invalid card action value",
        ))),
    };
    parsed.map_err(|_| protocol_invalid())
}

fn value_mode(value: &CardActionValue) -> Option<ActionValueMode> {
    let modes = usize::from(value.form)
        + usize::from(value.permission_select)
        + usize::from(value.decline)
        + usize::from(value.action_index.is_some());
    if modes != 1 {
        return None;
    }
    if value.decline {
        Some(ActionValueMode::Decline)
    } else if value.form {
        Some(ActionValueMode::Form)
    } else if value.permission_select {
        Some(ActionValueMode::PermissionSelect)
    } else {
        value.action_index.map(ActionValueMode::Index)
    }
}

fn non_empty(value: String) -> Result<String, FeishuCardDecodeError> {
    let trimmed = value.trim().to_owned();
    if trimmed.is_empty() {
        Err(protocol_invalid())
    } else {
        Ok(trimmed)
    }
}

fn protocol_invalid() -> FeishuCardDecodeError {
    FeishuCardDecodeError {
        code: ImErrorCode::ProtocolInvalid,
    }
}

pub(crate) fn action_reply(
    response: &ImActionResponse,
    locale: ImLocale,
    token: &ImActionTokenCodec,
) -> Value {
    let content = message_state_label(locale, response.state.message_state());
    match response.state {
        ImActionResponseState::AlreadyHandled if response.outcome.is_none() => {
            Reply::new(ToastType::Info, content, None)
        }
        ImActionResponseState::Handled | ImActionResponseState::AlreadyHandled => {
            let toast_type = if response.state == ImActionResponseState::Handled {
                ToastType::Success
            } else {
                ToastType::Info
            };
            Reply::new(
                toast_type,
                content,
                Some(terminal_card(response, locale, token)),
            )
        }
        ImActionResponseState::Rejected(ImErrorCode::ActionExpired) => Reply::new(
            ToastType::Warning,
            content,
            Some(terminal_card(response, locale, token)),
        ),
        ImActionResponseState::Rejected(code) => {
            Reply::new(ToastType::Error, error_copy(code, locale), None)
        }
    }
    .into_value()
}

pub(crate) fn processing_reply(locale: ImLocale) -> Value {
    Reply::new(
        ToastType::Info,
        localized(locale, "im.feishu.processing"),
        None,
    )
    .into_value()
}

pub(crate) fn decode_error_reply(error: FeishuCardDecodeError, locale: ImLocale) -> Value {
    Reply::new(ToastType::Error, error_copy(error.code, locale), None).into_value()
}

fn error_copy(code: ImErrorCode, locale: ImLocale) -> &'static str {
    match code {
        ImErrorCode::FormCustomAnswerMissing => localized(locale, "im.feishu.customAnswerMissing"),
        ImErrorCode::FormTextTooLong => localized(locale, "im.feishu.textTooLong"),
        ImErrorCode::InformationNotActionable => localized(locale, "im.feishu.notActionable"),
        ImErrorCode::FormAnswerMissing
        | ImErrorCode::FormOptionInvalid
        | ImErrorCode::ActionInvalid
        | ImErrorCode::ProtocolInvalid => localized(locale, "im.feishu.invalidAnswer"),
        _ => message_state_label(locale, crate::im::ImMessageState::Failed),
    }
}

fn terminal_card(
    response: &ImActionResponse,
    locale: ImLocale,
    token: &ImActionTokenCodec,
) -> Card {
    let Some(delivery) = &response.delivery else {
        return terminal_read_only_card(
            message_state_label(locale, response.state.message_state()),
            message_state_label(locale, response.state.message_state()),
        );
    };
    let extra = terminal_details(
        Some(delivery),
        response.outcome.as_ref(),
        response.resolved_by,
        locale,
    );
    let elements =
        source_terminal_elements(delivery, token, locale, response.outcome.as_ref(), &extra);
    terminal_interactive_card(
        elements,
        message_state_label(locale, response.state.message_state()),
    )
}

fn terminal_details(
    delivery: Option<&ImDelivery>,
    outcome: Option<&ImResolutionOutcome>,
    resolved_by: Option<ImResolvedBy>,
    locale: ImLocale,
) -> Vec<String> {
    let mut extra = Vec::new();
    if let (Some(delivery), Some(ImResolutionOutcome::ElicitationSubmitted { content })) =
        (delivery, outcome)
    {
        if let Some(submitted) = submitted_answer_lines(delivery, content, locale) {
            extra.push(submitted);
        }
    }
    if let Some(outcome) = outcome {
        extra.push(format!(
            "- {}: {}",
            localized(locale, "im.feishu.result.outcome"),
            outcome_copy(outcome, delivery, locale)
        ));
    }
    // A cancelled request has no handling surface to attribute.
    let resolved_by =
        resolved_by.filter(|_| !outcome.is_some_and(ImResolutionOutcome::is_cancelled));
    if let Some(resolved_by) = resolved_by {
        extra.push(format!(
            "- {}: {}",
            localized(locale, "im.feishu.result.source"),
            resolved_by_copy(resolved_by, locale)
        ));
    }
    extra
}

fn source_terminal_elements(
    delivery: &ImDelivery,
    token: &ImActionTokenCodec,
    locale: ImLocale,
    outcome: Option<&ImResolutionOutcome>,
    extra: &[String],
) -> Vec<Element> {
    let actions = intervention_actions(delivery, token, locale).unwrap_or_default();
    let mut elements = pending_elements(delivery, &actions, locale)
        .unwrap_or_else(|_| vec![markdown_element(&notification_markdown(delivery, locale))]);
    elements.iter_mut().for_each(Element::disable);
    restore_terminal_values(&mut elements, delivery, outcome);
    if !extra.is_empty() {
        elements.push(markdown_element(&extra.join("\n\n")));
    }
    elements
}

/// Project the accepted schema values back through this delivery's published
/// options. Display labels never identify an answer, and callback drafts are
/// never used for a losing submission.
fn restore_terminal_values(
    elements: &mut [Element],
    delivery: &ImDelivery,
    outcome: Option<&ImResolutionOutcome>,
) {
    let ImDeliveryPayload::Intervention { reference, .. } = &delivery.payload else {
        return;
    };
    let mut values = BTreeMap::<String, ControlValue>::new();
    match outcome {
        Some(ImResolutionOutcome::PermissionOption { option_id }) => {
            values.insert("q0".into(), ControlValue::Single(Some(option_id.clone())));
        }
        Some(ImResolutionOutcome::ElicitationSubmitted { content }) => {
            let Some(form) = reference
                .allowed_actions
                .iter()
                .find_map(|action| match action {
                    InterventionAllowedAction::ElicitationFixedForm { form } => Some(form),
                    _ => None,
                })
            else {
                return;
            };
            for question in &form.questions {
                let answer = content.get(&question.question.field_name);
                let custom = question
                    .custom_answer_key
                    .as_ref()
                    .and_then(|key| content.get(key))
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty());
                let selected: Vec<String> = if custom.is_some() {
                    vec![REMOTE_CUSTOM_ANSWER_OPTION.into()]
                } else {
                    question
                        .question
                        .options
                        .iter()
                        .enumerate()
                        .filter_map(|(index, option)| {
                            let matches = match question.question.question_kind {
                                ElicitationQuestionKind::MultiSelect => answer
                                    .and_then(Value::as_array)
                                    .is_some_and(|values| values.contains(&option.value)),
                                _ => answer == Some(&option.value),
                            };
                            matches.then(|| index.to_string())
                        })
                        .collect()
                };
                let value = match question.question.question_kind {
                    ElicitationQuestionKind::SingleSelect => {
                        ControlValue::Single(selected.first().cloned())
                    }
                    ElicitationQuestionKind::MultiSelect => ControlValue::Multi(selected),
                    ElicitationQuestionKind::FreeText => ControlValue::Text(
                        answer.and_then(Value::as_str).unwrap_or_default().into(),
                    ),
                };
                values.insert(question.selector_key.clone(), value);
                if question.custom_answer_key.is_some() {
                    values.insert(
                        format!("{}_other", question.selector_key),
                        ControlValue::Text(custom.unwrap_or_default().into()),
                    );
                }
            }
        }
        _ => {}
    }
    for element in elements {
        element.restore(&values);
    }
}

fn submitted_answer_lines(
    delivery: &ImDelivery,
    submission: &Value,
    locale: ImLocale,
) -> Option<String> {
    let ImDeliveryPayload::Intervention { reference, .. } = &delivery.payload else {
        return None;
    };
    let form = reference
        .allowed_actions
        .iter()
        .find_map(|action| match action {
            InterventionAllowedAction::ElicitationFixedForm { form } => Some(form),
            _ => None,
        })?;
    let mut lines = Vec::new();
    for question in &form.questions {
        let answered = if let Some(custom_key) = question.custom_answer_key.as_deref() {
            submission
                .get(custom_key)
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(|value| value.to_owned())
        } else {
            None
        };
        let display = answered.or_else(|| {
            let value = submission.get(&question.question.field_name)?;
            match question.question.question_kind {
                ElicitationQuestionKind::FreeText => value.as_str().map(|value| value.to_owned()),
                ElicitationQuestionKind::SingleSelect | ElicitationQuestionKind::MultiSelect => {
                    select_value_label(value, &question.question.options)
                }
            }
        });
        lines.push(format!(
            "- {}: {}",
            question.question.title,
            display.unwrap_or_else(|| localized(locale, "im.feishu.result.empty").to_owned())
        ));
    }
    Some(format!(
        "**{}**\n\n{}",
        localized(locale, "im.feishu.result.submission"),
        lines.join("\n")
    ))
}

fn select_value_label(
    value: &Value,
    options: &[crate::app::intervention::ElicitationQuestionOption],
) -> Option<String> {
    let selected = match value {
        Value::Array(values) => values.to_vec(),
        other => vec![other.clone()],
    };
    let labels = selected
        .iter()
        .map(|value| {
            options
                .iter()
                .find(|option| option.value == *value)
                .map(|option| option.label.clone())
        })
        .collect::<Option<Vec<_>>>()?;
    Some(labels.join("、"))
}

fn outcome_copy(
    outcome: &ImResolutionOutcome,
    delivery: Option<&ImDelivery>,
    locale: ImLocale,
) -> String {
    match outcome {
        ImResolutionOutcome::PermissionOption { option_id } => delivery
            .and_then(|delivery| {
                let ImDeliveryPayload::Intervention { reference, .. } = &delivery.payload else {
                    return None;
                };
                reference
                    .allowed_actions
                    .iter()
                    .find(|action| match action {
                        InterventionAllowedAction::PermissionOption {
                            option_id: candidate,
                            ..
                        } => candidate == option_id,
                        _ => false,
                    })
            })
            .map(|action| action_label(locale, action))
            .unwrap_or_else(|| option_id.clone()),
        ImResolutionOutcome::ElicitationSubmitted { .. } => {
            localized(locale, "im.feishu.result.submitted").to_owned()
        }
        ImResolutionOutcome::ResultUnavailable => {
            localized(locale, "im.feishu.result.unavailable").to_owned()
        }
        ImResolutionOutcome::ElicitationDeclined => {
            localized(locale, "im.action.decline").to_owned()
        }
        ImResolutionOutcome::ElicitationCancelled => {
            localized(locale, "im.intervention.cancelled").to_owned()
        }
        ImResolutionOutcome::ManualCheck {
            outcome: NodeOutcome::Success,
        } => localized(locale, "im.action.success").to_owned(),
        ImResolutionOutcome::ManualCheck {
            outcome: NodeOutcome::Failure,
        } => localized(locale, "im.action.failure").to_owned(),
        ImResolutionOutcome::ManualCheck { outcome } => format!("{outcome:?}"),
    }
}

fn pending_title(delivery: &ImDelivery, locale: ImLocale) -> String {
    let ImDeliveryPayload::Intervention { presentation, .. } = &delivery.payload else {
        return localized(locale, "im.notification.permission.title").to_owned();
    };
    localized_title(presentation, locale)
}

fn localized_title(presentation: &InterventionPresentation, locale: ImLocale) -> String {
    presentation
        .title
        .clone()
        .unwrap_or_else(|| localized(locale, &presentation.title_key).to_owned())
}

fn read_only_card_content(markdown: &str) -> String {
    read_only_card(markdown).content()
}

fn read_only_card(markdown: &str) -> Card {
    Card::new(vec![markdown_element(markdown)], None)
}

fn terminal_read_only_card(markdown: &str, title: &str) -> Card {
    terminal_interactive_card(vec![markdown_element(markdown)], title)
}

fn terminal_interactive_card(elements: Vec<Element>, title: &str) -> Card {
    Card::new(elements, Some(card_header(title, HeaderTemplate::Grey)))
}

fn card_header(title: &str, template: HeaderTemplate) -> Header {
    Header {
        title: Text::plain(title),
        template,
    }
}

fn markdown_element(content: &str) -> Element {
    Element::Markdown {
        content: content.to_owned(),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::app::intervention::{
        ElicitationQuestion, ElicitationQuestionKind, ElicitationQuestionOption,
        InterventionLocator, InterventionRequestIdentity, PermissionActionKind,
        RemoteElicitationForm, RemoteQuestion, RemoteQuestionConstraints,
    };
    use crate::im::{
        IM_PAYLOAD_VERSION, ImActionTokenCodec, ImDestination, ImNotificationKind,
        ImSubmittedAction, InterventionRef,
    };

    fn codec() -> ImActionTokenCodec {
        ImActionTokenCodec::new(vec![7; 32]).unwrap()
    }

    pub(crate) fn delivery(
        kind: ImNotificationKind,
        actions: Vec<InterventionAllowedAction>,
    ) -> ImDelivery {
        ImDelivery {
            delivery_id: "delivery-1".into(),
            channel: ImChannelKind::Feishu,
            destination: ImDestination {
                destination_id: "ou_owner".into(),
                conversation_id: "oc_private".into(),
                authorized_actor_id: "ou_owner".into(),
            },
            notification_kind: kind,
            canonical_event_id: "event-1".into(),
            payload: ImDeliveryPayload::Intervention {
                version: IM_PAYLOAD_VERSION,
                reference: InterventionRef {
                    locator: InterventionLocator {
                        project_id: "p".into(),
                        task_id: "t".into(),
                        run_id: "r".into(),
                        round_id: "round".into(),
                        node_id: "n".into(),
                        attempt_id: "a".into(),
                        outer_node_id: None,
                        outer_attempt_id: None,
                    },
                    request: match kind {
                        ImNotificationKind::Permission => InterventionRequestIdentity::Permission {
                            request_id: "request-1".into(),
                        },
                        ImNotificationKind::Elicitation => {
                            InterventionRequestIdentity::Elicitation {
                                elicitation_id: "elicit-1".into(),
                            }
                        }
                        ImNotificationKind::ManualCheck => InterventionRequestIdentity::ManualCheck,
                        _ => unreachable!(),
                    },
                    expected_state: "pending".into(),
                    allowed_actions: actions,
                    expires_at_ms: None,
                },
                presentation: InterventionPresentation {
                    title_key: match kind {
                        ImNotificationKind::Permission => "im.notification.permission.title",
                        ImNotificationKind::Elicitation => "im.notification.elicitation.title",
                        _ => "im.notification.manualCheck.title",
                    }
                    .into(),
                    summary_key: "im.notification.permission.summary".into(),
                    title: Some("Intervention".into()),
                    summary: Some("Waiting for a decision".into()),
                    body: None,
                    context: None,
                    fields: BTreeMap::new(),
                    questions: Vec::new(),
                },
            },
            expires_at_ms: 10_000,
            display_ref: Some(7),
        }
    }

    fn permission_action(index: usize) -> InterventionAllowedAction {
        InterventionAllowedAction::PermissionOption {
            option_id: format!("opt_{index}"),
            name: format!("action_{index}"),
            permission_kind: PermissionActionKind::AllowOnce,
        }
    }

    fn card(delivery: &ImDelivery) -> Value {
        serde_json::from_str(&delivery_card_content(delivery, &codec(), ImLocale::ZhCn).unwrap())
            .unwrap()
    }

    /// Captured from the pre-refactor renderer. Bless only for a documented
    /// platform wire change, never to accommodate an internal refactor.
    #[test]
    fn published_cards_match_the_pre_refactor_wire_contract() {
        let mut cases = BTreeMap::new();
        for (language, locale) in [("zh-CN", ImLocale::ZhCn), ("en", ImLocale::En)] {
            for count in [1, 3, 4, 20] {
                let source = delivery(
                    ImNotificationKind::Permission,
                    (0..count).map(permission_action).collect(),
                );
                let rendered: Value = serde_json::from_str(
                    &delivery_card_content(&source, &codec(), locale).unwrap(),
                )
                .unwrap();
                cases.insert(format!("{language}/permission-{count}"), rendered);
                let response = ImActionResponse {
                    state: ImActionResponseState::Handled,
                    resolved_by: Some(ImResolvedBy::Desktop),
                    outcome: Some(ImResolutionOutcome::PermissionOption {
                        option_id: "opt_0".into(),
                    }),
                    delivery: Some(source),
                };
                cases.insert(
                    format!("{language}/permission-{count}-terminal"),
                    action_reply(&response, locale, &codec()),
                );
            }
            let manual = delivery(
                ImNotificationKind::ManualCheck,
                vec![
                    InterventionAllowedAction::ManualSuccess,
                    InterventionAllowedAction::ManualFailure,
                ],
            );
            cases.insert(
                format!("{language}/manual"),
                serde_json::from_str::<Value>(
                    &delivery_card_content(&manual, &codec(), locale).unwrap(),
                )
                .unwrap(),
            );
            let mut response = submitted_mixed_form_response();
            if let ImDeliveryPayload::Intervention {
                presentation,
                reference,
                ..
            } = &mut response.delivery.as_mut().unwrap().payload
            {
                presentation.context = Some("Previous **output**".into());
                if let InterventionAllowedAction::ElicitationFixedForm { form } =
                    &mut reference.allowed_actions[0]
                {
                    form.decline_allowed = true;
                    form.questions[2].constraints.multiline = true;
                }
                reference
                    .allowed_actions
                    .push(InterventionAllowedAction::ElicitationDecline);
            }
            cases.insert(
                format!("{language}/mixed-pending"),
                serde_json::from_str::<Value>(
                    &delivery_card_content(response.delivery.as_ref().unwrap(), &codec(), locale)
                        .unwrap(),
                )
                .unwrap(),
            );
            cases.insert(
                format!("{language}/mixed-terminal"),
                action_reply(&response, locale, &codec()),
            );
            response.state = ImActionResponseState::AlreadyHandled;
            cases.insert(
                format!("{language}/mixed-already-handled"),
                action_reply(&response, locale, &codec()),
            );
            response.outcome =
                Some(ImResolutionOutcome::ElicitationSubmitted { content: json!({}) });
            cases.insert(
                format!("{language}/mixed-empty"),
                action_reply(&response, locale, &codec()),
            );
            response.state = ImActionResponseState::Rejected(ImErrorCode::ActionExpired);
            cases.insert(
                format!("{language}/expired"),
                action_reply(&response, locale, &codec()),
            );
            response.state = ImActionResponseState::AlreadyHandled;
            response.outcome = None;
            cases.insert(
                format!("{language}/unavailable"),
                action_reply(&response, locale, &codec()),
            );
            cases.insert(
                format!("{language}/read-only"),
                serde_json::to_value(read_only_card("**Information**\n\nDetails")).unwrap(),
            );
            cases.insert(format!("{language}/processing"), processing_reply(locale));
            cases.insert(
                format!("{language}/invalid"),
                decode_error_reply(protocol_invalid(), locale),
            );
        }
        let actual = serde_json::to_value(cases).unwrap();
        if std::env::var_os("GOLD_BAND_BLESS_GOLDEN").is_some() {
            std::fs::write(
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/src/im/connectors/feishu/fixtures/card_wire.json"
                ),
                serde_json::to_string_pretty(&actual).unwrap() + "\n",
            )
            .unwrap();
            return;
        }
        let expected: Value =
            serde_json::from_str(include_str!("fixtures/card_wire.json")).unwrap();
        assert_eq!(
            actual, expected,
            "card JSON must remain unchanged across internal refactors"
        );
    }

    #[test]
    fn permission_uses_buttons_below_the_limit_and_select_above_it() {
        let buttons = card(&delivery(
            ImNotificationKind::Permission,
            (0..3).map(permission_action).collect(),
        ));
        let button_labels = buttons["body"]["elements"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|element| element["tag"] == "button")
            .collect::<Vec<_>>();
        assert_eq!(button_labels.len(), 3);

        let select = card(&delivery(
            ImNotificationKind::Permission,
            (0..20).map(permission_action).collect(),
        ));
        let form = &select["body"]["elements"][1];
        assert_eq!(form["tag"], "form");
        assert_eq!(form["elements"][0]["options"].as_array().unwrap().len(), 20);
        assert_eq!(form["elements"][1]["form_action_type"], "submit");
        assert_eq!(
            form["elements"][1]["behaviors"][0]["value"]["permissionSelect"],
            true
        );
    }

    #[test]
    fn manual_check_renders_failure_before_success() {
        let manual = card(&delivery(
            ImNotificationKind::ManualCheck,
            vec![
                InterventionAllowedAction::ManualSuccess,
                InterventionAllowedAction::ManualFailure,
            ],
        ));
        let buttons = manual["body"]["elements"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|element| element["tag"] == "button")
            .map(|element| element["text"]["content"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(buttons, vec!["失败", "成功"]);
    }

    #[test]
    fn elicitation_uses_one_form_custom_inputs_and_external_decline() {
        let question = |selector: &str, kind: ElicitationQuestionKind| RemoteQuestion {
            selector_key: selector.into(),
            question: ElicitationQuestion {
                field_name: format!("field_{selector}"),
                title: format!("Question {selector}"),
                description: None,
                question_kind: kind.clone(),
                options: Vec::new(),
                allows_custom_answer: matches!(
                    kind,
                    ElicitationQuestionKind::SingleSelect | ElicitationQuestionKind::MultiSelect
                ),
            },
            constraints: RemoteQuestionConstraints {
                required: true,
                allows_empty: false,
                max_chars: 100,
                multiline: false,
            },
            custom_answer_key: matches!(
                kind,
                ElicitationQuestionKind::SingleSelect | ElicitationQuestionKind::MultiSelect
            )
            .then(|| format!("field_{selector}_custom")),
        };
        let form = RemoteElicitationForm {
            questions: vec![
                question("q0", ElicitationQuestionKind::SingleSelect),
                question("q1", ElicitationQuestionKind::MultiSelect),
                question("q2", ElicitationQuestionKind::FreeText),
            ],
            decline_allowed: true,
        };
        let content = card(&delivery(
            ImNotificationKind::Elicitation,
            vec![
                InterventionAllowedAction::ElicitationFixedForm { form },
                InterventionAllowedAction::ElicitationDecline,
            ],
        ));
        let form_element = &content["body"]["elements"][1];
        assert_eq!(form_element["tag"], "form");
        assert_eq!(form_element["elements"][1]["tag"], "select_static");
        assert_eq!(form_element["elements"][4]["tag"], "multi_select_static");
        assert_eq!(form_element["elements"][5]["tag"], "input");
        assert_eq!(form_element["elements"][7]["name"], "q2");
        assert_eq!(form_element["elements"][8]["form_action_type"], "submit");
        assert_eq!(
            form_element["elements"][8]["behaviors"][0]["value"]["form"],
            true
        );
        assert_eq!(content["body"]["elements"][2]["text"]["content"], "拒绝");
        let decline = decode_card_action(&callback_payload(
            content["body"]["elements"][2]["behaviors"][0]["value"].clone(),
            None,
        ))
        .unwrap();
        assert_eq!(
            decline.envelope.action,
            ImSubmittedAction::ElicitationDecline
        );
    }

    #[test]
    fn elicitation_context_survives_submission_and_durable_update() {
        let mut response = submitted_single_select_response();
        let source = response.delivery.as_mut().unwrap();
        let ImDeliveryPayload::Intervention { presentation, .. } = &mut source.payload else {
            unreachable!()
        };
        presentation.context = Some("Previous **model output**".into());
        let pending = card(source);
        let text = pending["body"]["elements"][0]["content"].as_str().unwrap();
        assert!(text.contains("前序模型输出"));
        assert!(text.contains("Previous **model output**"));
        let terminal = action_reply(&response, ImLocale::ZhCn, &codec());
        assert!(terminal.to_string().contains("Previous **model output**"));
        assert_callback_matches_patch(&response);
    }

    #[test]
    fn empty_context_is_omitted_and_full_snapshot_keeps_form_controls() {
        let mut response = submitted_single_select_response();
        let source = response.delivery.as_mut().unwrap();
        let empty = card(source);
        assert!(!empty.to_string().contains("前序模型输出"));
        let ImDeliveryPayload::Intervention { presentation, .. } = &mut source.payload else {
            unreachable!()
        };
        presentation.context = Some("测".repeat(4096));
        let content = card(source);
        assert!(
            content["body"]["elements"][0]["content"]
                .as_str()
                .unwrap()
                .contains(&"测".repeat(4096))
        );
        assert_eq!(content["body"]["elements"][1]["tag"], "form");
    }

    #[test]
    fn ambiguous_callback_action_modes_are_rejected() {
        let value = json!({"deliveryId":"d", "actionToken":"t", "decline":true, "form":true});
        let error = decode_card_action(&callback_payload(value, None))
            .err()
            .unwrap();
        assert_eq!(error.code, ImErrorCode::ProtocolInvalid);
    }

    fn callback_payload(value: Value, form_value: Option<Value>) -> Vec<u8> {
        json!({
            "schema": "2.0",
            "header": {
                "event_id": "event-callback-1",
                "event_type": "card.action.trigger",
                "create_time": "1791959702123",
                "app_id": "cli_fixture_app",
            },
            "event": {
                "operator": { "open_id": "ou_owner" },
                "action": {
                    "value": value,
                    "tag": "button",
                    "name": "gb_submit",
                    "form_value": form_value,
                },
                "context": {
                    "open_message_id": "om_message",
                    "open_chat_id": "oc_private",
                },
            },
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn callback_decodes_the_official_header_event_wire_shape() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event-wire-1",
                "event_type": "card.action.trigger",
                "create_time": "1791959702123",
                "app_id": "cli_fixture_app",
                "tenant_key": "tenant_fixture",
            },
            "event": {
                "operator": { "open_id": "ou_owner" },
                "action": {
                    "value": {
                        "deliveryId": "delivery-1",
                        "actionToken": "signed-token",
                        "actionIndex": 0,
                    },
                    "tag": "button",
                    "name": "gb_action",
                },
                "context": {
                    "open_message_id": "om_message",
                    "open_chat_id": "oc_private",
                },
            },
        })
        .to_string()
        .into_bytes();

        let action = decode_card_action(&payload).unwrap();
        let envelope = action.envelope;
        assert_eq!(
            envelope.action_id,
            deterministic_action_id(ImChannelKind::Feishu, "event-wire-1")
        );
        assert_eq!(
            envelope.action,
            ImSubmittedAction::PublishedAction { index: 0 }
        );
    }

    #[test]
    fn callback_decodes_index_select_form_and_json_string_values() {
        let base = json!({
            "deliveryId": "delivery-1",
            "actionToken": "signed-token",
        });
        let mut indexed = base.clone();
        indexed["actionIndex"] = json!(0);
        let action = decode_card_action(&callback_payload(indexed, None)).unwrap();
        let envelope = action.envelope;
        assert_eq!(
            envelope.action_id,
            deterministic_action_id(ImChannelKind::Feishu, "event-callback-1")
        );
        assert_eq!(
            envelope.action,
            ImSubmittedAction::PublishedAction { index: 0 }
        );
        assert_eq!(envelope.action_token.as_deref(), Some("signed-token"));
        let debug = format!("{envelope:?}");
        for forbidden in ["signed-token", "ou_owner", "oc_private"] {
            assert!(
                !debug.contains(forbidden),
                "card callback debug leaked {forbidden}"
            );
        }

        let mut select = base.clone();
        select["permissionSelect"] = json!(true);
        let action =
            decode_card_action(&callback_payload(select, Some(json!({ "q0": "opt_12" })))).unwrap();
        let envelope = action.envelope;
        assert_eq!(
            envelope.action,
            ImSubmittedAction::PermissionOption {
                option_id: "opt_12".into()
            }
        );

        let mut form = base;
        form["form"] = json!(true);
        let raw = serde_json::to_string(&form).unwrap();
        let action = decode_card_action(&callback_payload(
            Value::String(raw),
            Some(json!({
                "q0": "0",
                "q0_other": "ignored",
                "q1": ["0", REMOTE_CUSTOM_ANSWER_OPTION],
                "q1_other": "custom",
                "q2": "",
            })),
        ))
        .unwrap();
        let envelope = action.envelope;
        assert_eq!(
            envelope.action,
            ImSubmittedAction::ElicitationForm {
                answers: BTreeMap::from([
                    (
                        "q0".into(),
                        RemoteElicitationAnswer::Text { text: "0".into() }
                    ),
                    (
                        "q0_other".into(),
                        RemoteElicitationAnswer::Text {
                            text: "ignored".into()
                        }
                    ),
                    (
                        "q1".into(),
                        RemoteElicitationAnswer::Choice {
                            option_ids: vec!["0".into(), REMOTE_CUSTOM_ANSWER_OPTION.into()],
                            custom_text: None,
                        }
                    ),
                    (
                        "q1_other".into(),
                        RemoteElicitationAnswer::Text {
                            text: "custom".into()
                        }
                    ),
                    (
                        "q2".into(),
                        RemoteElicitationAnswer::Text {
                            text: String::new()
                        }
                    ),
                ]),
            }
        );
    }

    #[test]
    fn replies_preserve_the_card_on_expected_validation_failures() {
        let processing = processing_reply(ImLocale::ZhCn);
        assert_eq!(processing["toast"]["content"], "处理中");
        assert!(processing.get("card").is_none());

        let validation = action_reply(
            &crate::im::ImActionResponse {
                state: ImActionResponseState::Rejected(ImErrorCode::FormCustomAnswerMissing),
                resolved_by: None,
                outcome: None,
                delivery: None,
            },
            ImLocale::ZhCn,
            &codec(),
        );
        assert_eq!(validation["toast"]["content"], "请填写“其他”的内容");
        assert!(validation.get("card").is_none());

        let handled = action_reply(
            &crate::im::ImActionResponse {
                state: ImActionResponseState::Handled,
                resolved_by: Some(ImResolvedBy::Channel(ImChannelKind::Feishu)),
                outcome: Some(ImResolutionOutcome::ElicitationSubmitted { content: json!({}) }),
                delivery: Some(delivery(ImNotificationKind::Elicitation, Vec::new())),
            },
            ImLocale::ZhCn,
            &codec(),
        );
        assert_eq!(handled["card"]["type"], "raw");
        assert!(handled["card"]["data"]["config"]["update_multi"].is_boolean());
    }

    fn submitted_single_select_response() -> crate::im::ImActionResponse {
        let question = RemoteQuestion {
            selector_key: "q0".into(),
            question: ElicitationQuestion {
                field_name: "environment".into(),
                title: "部署环境".into(),
                description: None,
                question_kind: ElicitationQuestionKind::SingleSelect,
                options: vec![
                    ElicitationQuestionOption {
                        value: json!("development"),
                        label: "开发".into(),
                        description: None,
                    },
                    ElicitationQuestionOption {
                        value: json!("production"),
                        label: "生产".into(),
                        description: None,
                    },
                ],
                allows_custom_answer: true,
            },
            constraints: RemoteQuestionConstraints {
                required: true,
                allows_empty: false,
                max_chars: 100,
                multiline: false,
            },
            custom_answer_key: Some("environment_custom".into()),
        };
        let form = RemoteElicitationForm {
            questions: vec![question],
            decline_allowed: false,
        };
        let delivery = delivery(
            ImNotificationKind::Elicitation,
            vec![InterventionAllowedAction::ElicitationFixedForm { form }],
        );
        crate::im::ImActionResponse {
            state: ImActionResponseState::Handled,
            resolved_by: Some(ImResolvedBy::Channel(ImChannelKind::Feishu)),
            outcome: Some(ImResolutionOutcome::ElicitationSubmitted {
                content: json!({ "environment": "production" }),
            }),
            delivery: Some(delivery),
        }
    }

    #[test]
    fn terminal_single_select_preserves_the_accepted_choice_in_the_control() {
        let reply = action_reply(
            &submitted_single_select_response(),
            ImLocale::ZhCn,
            &codec(),
        );
        let select = &reply["card"]["data"]["body"]["elements"][1]["elements"][1];
        assert_eq!(select["tag"], "select_static");
        assert_eq!(select["disabled"], true);
        assert_eq!(
            select["initial_index"], 2,
            "terminal control must select the published option, not just append an answer summary"
        );
        assert_eq!(select["options"][1]["value"], "1");
    }

    #[test]
    fn terminal_patch_matches_callback_after_resolution_serialization() {
        let response = submitted_single_select_response();
        assert_callback_matches_patch(&response);
    }

    #[test]
    fn terminal_optional_answers_remain_empty_instead_of_selecting_a_default() {
        let mut response = submitted_single_select_response();
        let ImDeliveryPayload::Intervention { reference, .. } =
            &mut response.delivery.as_mut().unwrap().payload
        else {
            unreachable!()
        };
        let InterventionAllowedAction::ElicitationFixedForm { form } =
            &mut reference.allowed_actions[0]
        else {
            unreachable!()
        };
        form.questions[0].constraints.required = false;
        form.questions[0].constraints.allows_empty = true;
        let template = form.questions[0].clone();
        for (selector, kind) in [
            ("q1", ElicitationQuestionKind::MultiSelect),
            ("q2", ElicitationQuestionKind::FreeText),
        ] {
            let mut question = template.clone();
            question.selector_key = selector.into();
            question.question.field_name = selector.into();
            question.question.question_kind = kind;
            question.custom_answer_key = None;
            form.questions.push(question);
        }
        response.outcome = Some(ImResolutionOutcome::ElicitationSubmitted { content: json!({}) });
        let reply = action_reply(&response, ImLocale::ZhCn, &codec());
        let controls = reply["card"]["data"]["body"]["elements"][1]["elements"]
            .as_array()
            .unwrap();
        let control = |name: &str| controls.iter().find(|value| value["name"] == name).unwrap();
        assert_eq!(control("q0")["initial_index"], 0);
        assert_eq!(control("q0_other")["default_value"], "");
        assert_eq!(control("q1")["selected_values"], json!([]));
        assert_eq!(control("q2")["default_value"], "");
        for name in ["q0", "q0_other", "q1", "q2"] {
            assert_eq!(control(name)["disabled"], true);
        }
        assert_callback_matches_patch(&response);
    }

    fn assert_callback_matches_patch(response: &crate::im::ImActionResponse) {
        let source = response.delivery.as_ref().unwrap();
        let ImDeliveryPayload::Intervention { reference, .. } = &source.payload else {
            unreachable!()
        };
        let payload = ImDeliveryPayload::InterventionResolution {
            version: IM_PAYLOAD_VERSION,
            resolution: crate::im::InterventionResolutionNotification {
                canonical_event_id: crate::im::resolution_event_id(&source.canonical_event_id),
                source_event_id: source.canonical_event_id.clone(),
                notification_kind: source.notification_kind,
                locator: crate::im::ImNavigationLocator {
                    project_id: reference.locator.project_id.clone(),
                    task_id: None,
                    run_id: None,
                    round_id: None,
                    node_id: None,
                    attempt_id: None,
                    outer_node_id: None,
                    outer_attempt_id: None,
                    scheduled_occurrence_id: None,
                },
                display_ref: source.display_ref,
                resolved_by: response.resolved_by.unwrap(),
                outcome: response.outcome.clone().unwrap(),
            },
        };
        let restored = serde_json::from_str(&serde_json::to_string(&payload).unwrap()).unwrap();
        let patch: Value = serde_json::from_str(&resolution_payload_card_content(
            &restored,
            ImLocale::ZhCn,
            Some(source),
            &codec(),
        ))
        .unwrap();
        let callback = action_reply(response, ImLocale::ZhCn, &codec());
        assert_eq!(
            patch, callback["card"]["data"],
            "durable PATCH must not erase the callback's accepted answers"
        );
    }

    fn submitted_mixed_form_response() -> ImActionResponse {
        let mut response = submitted_single_select_response();
        let ImDeliveryPayload::Intervention { reference, .. } =
            &mut response.delivery.as_mut().unwrap().payload
        else {
            unreachable!()
        };
        let InterventionAllowedAction::ElicitationFixedForm { form } =
            &mut reference.allowed_actions[0]
        else {
            unreachable!()
        };
        let template = form.questions[0].clone();
        for (selector, field, kind) in [
            ("q1", "features", ElicitationQuestionKind::MultiSelect),
            ("q2", "notes", ElicitationQuestionKind::FreeText),
            ("q3", "custom_single", ElicitationQuestionKind::SingleSelect),
            ("q4", "custom_multi", ElicitationQuestionKind::MultiSelect),
        ] {
            let mut question = template.clone();
            question.selector_key = selector.into();
            question.question.field_name = field.into();
            question.question.question_kind = kind;
            question.custom_answer_key =
                (selector == "q3" || selector == "q4").then(|| format!("{field}_custom"));
            question.question.options = [json!(1), json!("1"), json!(true)]
                .into_iter()
                .map(|value| ElicitationQuestionOption {
                    value,
                    label: "同名选项".into(),
                    description: None,
                })
                .collect();
            form.questions.push(question);
        }
        response.outcome = Some(ImResolutionOutcome::ElicitationSubmitted {
            content: json!({
                "environment": "production",
                "features": [true, 1],
                "notes": "第一行\n第二行",
                "custom_single_custom": "自定义单选",
                "custom_multi_custom": "自定义多选",
            }),
        });
        response
    }

    #[test]
    fn terminal_mixed_form_restores_typed_values_custom_answers_and_text() {
        let mut response = submitted_mixed_form_response();
        let reply = action_reply(&response, ImLocale::ZhCn, &codec());
        let controls = reply["card"]["data"]["body"]["elements"][1]["elements"]
            .as_array()
            .unwrap();
        let control = |name: &str| controls.iter().find(|value| value["name"] == name).unwrap();
        assert_eq!(control("q0")["initial_index"], 2);
        assert_eq!(control("q1")["selected_values"], json!(["0", "2"]));
        assert_eq!(control("q2")["default_value"], "第一行\n第二行");
        assert_eq!(control("q3")["initial_index"], 4);
        assert_eq!(control("q3_other")["default_value"], "自定义单选");
        assert_eq!(control("q4")["selected_values"], json!(["other"]));
        assert_eq!(control("q4_other")["default_value"], "自定义多选");
        for name in ["q0", "q1", "q2", "q3", "q3_other", "q4", "q4_other"] {
            assert_eq!(control(name)["disabled"], true);
        }
        assert_callback_matches_patch(&response);

        response.state = ImActionResponseState::AlreadyHandled;
        assert_callback_matches_patch(&response);
    }

    #[test]
    fn late_click_without_a_settled_result_does_not_erase_the_card() {
        let mut response = submitted_single_select_response();
        response.state = ImActionResponseState::AlreadyHandled;
        response.outcome = None;
        response.resolved_by = None;
        let reply = action_reply(&response, ImLocale::ZhCn, &codec());
        assert!(reply.get("toast").is_some());
        assert!(reply.get("card").is_none());
    }

    #[test]
    fn terminal_permission_select_restores_the_winning_option() {
        let response = crate::im::ImActionResponse {
            state: ImActionResponseState::Handled,
            resolved_by: Some(ImResolvedBy::Desktop),
            outcome: Some(ImResolutionOutcome::PermissionOption {
                option_id: "opt_3".into(),
            }),
            delivery: Some(delivery(
                ImNotificationKind::Permission,
                (0..5).map(permission_action).collect(),
            )),
        };
        let reply = action_reply(&response, ImLocale::ZhCn, &codec());
        assert_eq!(
            reply["card"]["data"]["body"]["elements"][1]["elements"][0]["initial_index"],
            4
        );
        assert_callback_matches_patch(&response);
    }

    #[test]
    fn handled_reply_renders_the_submitted_form_content() {
        let reply = action_reply(
            &submitted_single_select_response(),
            ImLocale::ZhCn,
            &codec(),
        );
        let content = reply["card"]["data"].to_string();
        assert!(
            content.contains("部署环境: 生产"),
            "submitted values must be rendered with their published labels: {content}"
        );
        let body = reply["card"]["data"]["body"].to_string();
        assert!(
            body.contains("\"tag\":\"select_static\"") && body.contains("\"disabled\":true"),
            "terminal card must keep and disable the original controls: {body}"
        );
    }

    #[test]
    fn resolution_patch_preserves_the_source_card_details() {
        let question = RemoteQuestion {
            selector_key: "q0".into(),
            question: ElicitationQuestion {
                field_name: "environment".into(),
                title: "部署环境".into(),
                description: Some("选择目标环境".into()),
                question_kind: ElicitationQuestionKind::SingleSelect,
                options: vec![ElicitationQuestionOption {
                    value: json!("production"),
                    label: "生产".into(),
                    description: None,
                }],
                allows_custom_answer: false,
            },
            constraints: RemoteQuestionConstraints {
                required: true,
                allows_empty: false,
                max_chars: 100,
                multiline: false,
            },
            custom_answer_key: None,
        };
        let mut source = delivery(
            ImNotificationKind::Elicitation,
            vec![InterventionAllowedAction::ElicitationFixedForm {
                form: RemoteElicitationForm {
                    questions: vec![question],
                    decline_allowed: false,
                },
            }],
        );
        if let ImDeliveryPayload::Intervention { presentation, .. } = &mut source.payload {
            presentation.title = Some("部署环境".into());
            presentation.fields = BTreeMap::from([
                ("workspaceLabel".into(), "Gold Band".into()),
                ("taskTitle".into(), "数据库选型".into()),
                ("nodeLabel".into(), "direct-agent".into()),
            ]);
        }
        let payload = ImDeliveryPayload::InterventionResolution {
            version: crate::im::IM_PAYLOAD_VERSION,
            resolution: crate::im::InterventionResolutionNotification {
                canonical_event_id: crate::im::resolution_event_id("event-1"),
                source_event_id: "event-1".into(),
                notification_kind: ImNotificationKind::Elicitation,
                locator: crate::im::ImNavigationLocator {
                    project_id: "p".into(),
                    task_id: None,
                    run_id: None,
                    round_id: None,
                    node_id: None,
                    attempt_id: None,
                    outer_node_id: None,
                    outer_attempt_id: None,
                    scheduled_occurrence_id: None,
                },
                display_ref: Some(31),
                resolved_by: ImResolvedBy::Channel(ImChannelKind::Feishu),
                outcome: ImResolutionOutcome::ElicitationSubmitted {
                    content: json!({ "environment": "production" }),
                },
            },
        };
        let content =
            resolution_payload_card_content(&payload, ImLocale::ZhCn, Some(&source), &codec());
        assert!(
            content.contains("\"tag\":\"select_static\"") && content.contains("\"disabled\":true"),
            "resolution patch must keep and disable the original controls: {content}"
        );
        assert!(
            content.contains("需要补充信息"),
            "resolution patch must keep the source form details: {content}"
        );
        for detail in ["部署环境", "选择目标环境", "生产"] {
            assert!(content.contains(detail), "source detail missing: {detail}");
        }
        let id = content.find("id=0007").unwrap();
        let node = content.find("节点: direct-agent").unwrap();
        let task = content.find("任务: 数据库选型").unwrap();
        let workspace = content.find("工作空间: Gold Band").unwrap();
        let body = content.find("需要补充信息").unwrap();
        assert!(
            id < node && node < task && task < workspace && workspace < body,
            "context fields must sit directly under the id: {content}"
        );
    }

    #[test]
    fn cancelled_resolution_is_not_rendered_as_a_decline_or_a_handled_answer() {
        let source = delivery(
            ImNotificationKind::Elicitation,
            vec![
                InterventionAllowedAction::ElicitationAccept,
                InterventionAllowedAction::ElicitationDecline,
            ],
        );
        let resolution = |outcome| ImDeliveryPayload::InterventionResolution {
            version: crate::im::IM_PAYLOAD_VERSION,
            resolution: crate::im::InterventionResolutionNotification {
                canonical_event_id: crate::im::resolution_event_id("event-1"),
                source_event_id: "event-1".into(),
                notification_kind: ImNotificationKind::Elicitation,
                locator: crate::im::ImNavigationLocator {
                    project_id: "p".into(),
                    task_id: None,
                    run_id: None,
                    round_id: None,
                    node_id: None,
                    attempt_id: None,
                    outer_node_id: None,
                    outer_attempt_id: None,
                    scheduled_occurrence_id: None,
                },
                display_ref: Some(12),
                resolved_by: ImResolvedBy::Desktop,
                outcome,
            },
        };
        for (locale, cancelled, declined, handled, source_label) in [
            (ImLocale::ZhCn, "已取消", "拒绝", "已处理", "处理端"),
            (
                ImLocale::En,
                "Cancelled",
                "Decline",
                "Handled",
                "Handled on",
            ),
        ] {
            for source in [Some(&source), None] {
                let content = resolution_payload_card_content(
                    &resolution(ImResolutionOutcome::ElicitationCancelled),
                    locale,
                    source,
                    &codec(),
                );
                let card: Value = serde_json::from_str(&content).unwrap();
                assert_eq!(card["header"]["title"]["content"], cancelled, "{content}");
                assert!(content.contains(cancelled), "{content}");
                assert!(!content.contains(&format!(": {declined}")), "{content}");
                assert!(!content.contains(handled), "{content}");
                assert!(
                    !content.contains(source_label),
                    "nobody handled a cancelled request: {content}"
                );

                let declined_content = resolution_payload_card_content(
                    &resolution(ImResolutionOutcome::ElicitationDeclined),
                    locale,
                    source,
                    &codec(),
                );
                let card: Value = serde_json::from_str(&declined_content).unwrap();
                assert_eq!(card["header"]["title"]["content"], handled);
                assert!(declined_content.contains(&format!(": {declined}")));
                assert!(declined_content.contains(source_label));
            }
        }
    }

    #[test]
    fn option_values_are_schema_indexes_and_custom_is_a_sentinel() {
        let option = ElicitationQuestionOption {
            value: json!("semantic-value"),
            label: "Choice".into(),
            description: None,
        };
        let question = RemoteQuestion {
            selector_key: "q0".into(),
            question: ElicitationQuestion {
                field_name: "field".into(),
                title: "Choice".into(),
                description: None,
                question_kind: ElicitationQuestionKind::SingleSelect,
                options: vec![option],
                allows_custom_answer: true,
            },
            constraints: RemoteQuestionConstraints {
                required: true,
                allows_empty: false,
                max_chars: 100,
                multiline: false,
            },
            custom_answer_key: Some("field_custom".into()),
        };
        let form = RemoteElicitationForm {
            questions: vec![question],
            decline_allowed: false,
        };
        let content = card(&delivery(
            ImNotificationKind::Elicitation,
            vec![InterventionAllowedAction::ElicitationFixedForm { form }],
        ));
        let options = &content["body"]["elements"][1]["elements"][1]["options"];
        assert_eq!(options[0]["value"], "0");
        assert_eq!(options[1]["value"], REMOTE_CUSTOM_ANSWER_OPTION);
    }
}
