pub mod feishu;
pub mod wecom;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::intervention::{
    InterventionAction, InterventionAllowedAction, PermissionActionKind, PermissionActionQualifier,
    RemoteElicitationAnswer, RemoteElicitationForm, RemoteQuestion,
    intervention_action_for_allowed, permission_action_qualifier, remote_question_answer,
};

use super::{
    INTERVENTION_NODE_FIELD, INTERVENTION_TASK_FIELD, INTERVENTION_WORKSPACE_FIELD,
    ImActionTokenClaims, ImActionTokenCodec, ImDelivery, ImDeliveryPayload, ImInboundError,
    ImLocale, InterventionPresentation, InterventionQuestionKind, is_intervention_context_field,
};

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlatformActionValue {
    pub delivery_id: String,
    pub action_token: String,
    pub action: InterventionAction,
}

#[derive(Clone, PartialEq)]
pub(crate) struct RenderedAction {
    pub label: String,
    pub permission_kind: Option<PermissionActionKind>,
    pub permission_qualifier: Option<PermissionActionQualifier>,
    pub action_index: usize,
    pub value: PlatformActionValue,
}

pub(crate) fn intervention_actions(
    delivery: &ImDelivery,
    codec: &ImActionTokenCodec,
    locale: ImLocale,
) -> Result<Vec<RenderedAction>, ImInboundError> {
    let ImDeliveryPayload::Intervention { reference, .. } = &delivery.payload else {
        return Ok(Vec::new());
    };
    let action_token = codec.encode(&ImActionTokenClaims {
        version: 1,
        delivery_id: delivery.delivery_id.clone(),
        canonical_event_id: delivery.canonical_event_id.clone(),
        channel: delivery.channel,
        destination_id: delivery.destination.destination_id.clone(),
        expires_at_ms: delivery.expires_at_ms,
    })?;
    Ok(reference
        .allowed_actions
        .iter()
        .enumerate()
        .map(|(action_index, allowed)| RenderedAction {
            action_index,
            label: action_label(locale, allowed),
            permission_kind: match allowed {
                InterventionAllowedAction::PermissionOption {
                    permission_kind, ..
                } => Some(*permission_kind),
                _ => None,
            },
            permission_qualifier: match allowed {
                InterventionAllowedAction::PermissionOption {
                    option_id, name, ..
                } => Some(permission_action_qualifier(option_id, name)),
                _ => None,
            },
            value: PlatformActionValue {
                delivery_id: delivery.delivery_id.clone(),
                action_token: action_token.clone(),
                action: intervention_action_for_allowed(allowed),
            },
        })
        .collect())
}

pub(crate) fn notification_markdown(delivery: &ImDelivery, locale: ImLocale) -> String {
    notification_markdown_impl(delivery, locale, true)
}

pub(crate) fn notification_markdown_without_context(
    delivery: &ImDelivery,
    locale: ImLocale,
) -> String {
    notification_markdown_impl(delivery, locale, false)
}

fn notification_markdown_impl(
    delivery: &ImDelivery,
    locale: ImLocale,
    include_context_fields: bool,
) -> String {
    match &delivery.payload {
        ImDeliveryPayload::Intervention { presentation, .. } => {
            let mut text = format!(
                "**{}**\n\n{}",
                localized(locale, &presentation.title_key),
                localized(locale, &presentation.summary_key)
            );
            let details = presentation_detail_text(presentation, locale);
            if !details.is_empty() {
                text.push_str(&format!("\n\n{details}"));
            }
            for (key, value) in &presentation.fields {
                if !include_context_fields && is_intervention_context_field(key) {
                    continue;
                }
                if !value.trim().is_empty() {
                    text.push_str(&format!("\n\n- {}: {value}", localized(locale, key)));
                }
            }
            text
        }
        ImDeliveryPayload::Information { notification, .. } => {
            let mut text = format!("**{}**", localized(locale, &notification.summary_key));
            for (key, value) in &notification.parameters {
                if !value.trim().is_empty() {
                    text.push_str(&format!("\n\n- {}: {value}", localized(locale, key)));
                }
            }
            text
        }
        ImDeliveryPayload::InterventionResolution { resolution, .. } => {
            let display_ref = resolution
                .display_ref
                .or(delivery.display_ref)
                .map(|value| format!("{value:04}"))
                .unwrap_or_else(|| "----".to_string());
            if resolution.outcome.is_cancelled() {
                return localized(locale, "im.intervention.cancelledRef")
                    .replace("{ref}", &display_ref);
            }
            localized(locale, "im.intervention.handledOn")
                .replace("{ref}", &display_ref)
                .replace("{source}", resolved_by_copy(resolution.resolved_by, locale))
        }
    }
}

pub(crate) fn resolved_by_copy(resolved_by: super::ImResolvedBy, locale: ImLocale) -> &'static str {
    use super::{ImChannelKind, ImResolvedBy};
    let key = match resolved_by {
        ImResolvedBy::Desktop => "im.intervention.resolvedBy.desktop",
        ImResolvedBy::Channel(ImChannelKind::WeCom) => "im.intervention.resolvedBy.wecom",
        ImResolvedBy::Channel(ImChannelKind::Feishu) => "im.intervention.resolvedBy.feishu",
        ImResolvedBy::Runtime => "im.intervention.resolvedBy.runtime",
    };
    localized(locale, key)
}

pub(crate) fn notification_context_markdown(delivery: &ImDelivery, locale: ImLocale) -> String {
    let ImDeliveryPayload::Intervention { presentation, .. } = &delivery.payload else {
        return String::new();
    };
    [
        INTERVENTION_NODE_FIELD,
        INTERVENTION_TASK_FIELD,
        INTERVENTION_WORKSPACE_FIELD,
    ]
    .iter()
    .filter_map(|key| {
        let value = presentation.fields.get(*key)?;
        (!value.trim().is_empty()).then(|| format!("- {}: {}", localized(locale, key), value))
    })
    .collect::<Vec<_>>()
    .join("\n")
}

pub(crate) fn presentation_detail_text(
    presentation: &InterventionPresentation,
    locale: ImLocale,
) -> String {
    presentation_detail_text_impl(presentation, locale, true)
}

pub(crate) fn presentation_question_detail_text(
    presentation: &InterventionPresentation,
    locale: ImLocale,
) -> String {
    presentation_detail_text_impl(presentation, locale, false)
}

fn presentation_detail_text_impl(
    presentation: &InterventionPresentation,
    locale: ImLocale,
    include_body: bool,
) -> String {
    let mut sections = Vec::new();
    let body = presentation
        .body
        .as_deref()
        .filter(|body| !body.trim().is_empty())
        .map(str::to_owned);
    if include_body && let Some(body) = &body {
        sections.push(body.clone());
    }
    for (index, question) in presentation.questions.iter().enumerate() {
        let kind = match question.question_kind {
            InterventionQuestionKind::SingleSelect => localized(locale, "im.question.singleSelect"),
            InterventionQuestionKind::MultiSelect => localized(locale, "im.question.multiSelect"),
            InterventionQuestionKind::FreeText => localized(locale, "im.question.freeText"),
        };
        let mut lines = vec![format!("{}. {} ({kind})", index + 1, question.title)];
        if let Some(description) = question
            .description
            .as_deref()
            .filter(|description| !description.trim().is_empty())
            .filter(|description| {
                body.as_deref()
                    .is_none_or(|body| normalized_prose(description) != normalized_prose(body))
            })
        {
            lines.push(description.to_owned());
        }
        for option in &question.options {
            lines.push(
                option
                    .description
                    .as_deref()
                    .filter(|description| !description.trim().is_empty())
                    .map(|description| format!("- {}: {description}", option.label))
                    .unwrap_or_else(|| format!("- {}", option.label)),
            );
        }
        if question.allows_custom_answer {
            lines.push(localized(locale, "im.question.customAnswer").to_owned());
        }
        sections.push(lines.join("\n"));
    }
    sections.join("\n\n")
}

fn normalized_prose(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn message_state_label(locale: ImLocale, state: super::ImMessageState) -> &'static str {
    localized(
        locale,
        match state {
            super::ImMessageState::Handled => "im.state.handled",
            super::ImMessageState::Expired => "im.state.expired",
            super::ImMessageState::Failed => "im.state.failed",
        },
    )
}

pub(crate) fn action_label(locale: ImLocale, action: &InterventionAllowedAction) -> String {
    let key = match action {
        InterventionAllowedAction::PermissionOption {
            option_id,
            permission_kind,
            ..
        } => match (
            permission_action_qualifier(option_id, action_name(action)),
            permission_kind,
        ) {
            (PermissionActionQualifier::BypassPermissions, _) => "im.permission.action.bypass",
            (PermissionActionQualifier::AutoEdit, _) => "im.permission.action.autoEdit",
            (PermissionActionQualifier::AutoMode, _) => "im.permission.action.autoMode",
            (PermissionActionQualifier::ManualApproval, _) => "im.permission.action.manualApproval",
            (PermissionActionQualifier::KeepPlanning, _) => "im.permission.action.keepPlanning",
            (PermissionActionQualifier::Standard, PermissionActionKind::AllowOnce) => {
                "im.permission.action.allowOnce"
            }
            (PermissionActionQualifier::Standard, PermissionActionKind::AllowAlways) => {
                "im.permission.action.allowForSession"
            }
            (PermissionActionQualifier::Standard, PermissionActionKind::RejectOnce) => {
                "im.permission.action.deny"
            }
            (PermissionActionQualifier::Standard, PermissionActionKind::RejectAlways) => {
                "im.permission.action.denyAlways"
            }
            _ => "im.permission.action.desktop",
        },
        InterventionAllowedAction::ElicitationFixedForm { .. } => "im.elicitation.form.label",
        InterventionAllowedAction::ElicitationAccept => "im.action.accept",
        InterventionAllowedAction::ElicitationDecline => "im.action.decline",
        InterventionAllowedAction::ManualSuccess => "im.action.success",
        InterventionAllowedAction::ManualFailure => "im.action.failure",
    };
    localized(locale, key).to_owned()
}

pub(crate) fn action_name(action: &InterventionAllowedAction) -> &str {
    match action {
        InterventionAllowedAction::PermissionOption { name, .. } => name,
        _ => "",
    }
}

/// Joins answer labels with the locale's list separator.
pub(crate) fn join_list(locale: ImLocale, items: &[String]) -> String {
    items.join(localized(locale, "im.list.separator"))
}

/// One published question with the answer accepted for it.
pub(crate) struct AcceptedAnswer<'a> {
    pub question: &'a RemoteQuestion,
    pub answer: Option<RemoteElicitationAnswer>,
}

impl AcceptedAnswer<'_> {
    /// Selected published option ids; empty for text and custom answers.
    pub fn option_ids(&self) -> &[String] {
        match &self.answer {
            Some(RemoteElicitationAnswer::Choice { option_ids, .. }) => option_ids,
            _ => &[],
        }
    }

    pub fn custom_text(&self) -> Option<&str> {
        match &self.answer {
            Some(RemoteElicitationAnswer::Choice {
                custom_text: Some(text),
                ..
            }) => Some(text),
            _ => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match &self.answer {
            Some(RemoteElicitationAnswer::Text { text }) => Some(text),
            _ => None,
        }
    }

    /// The answer as the user sees it: option labels, custom or free text.
    pub fn display(&self, locale: ImLocale) -> Option<String> {
        if let Some(text) = self.custom_text().or(self.text()) {
            return Some(text.to_owned());
        }
        let labels = self
            .option_ids()
            .iter()
            .filter_map(|option_id| option_id.parse::<usize>().ok())
            .filter_map(|index| self.question.question.options.get(index))
            .map(|option| option.label.clone())
            .collect::<Vec<_>>();
        (!labels.is_empty()).then(|| join_list(locale, &labels))
    }
}

/// Projects accepted elicitation content onto a delivery's published form.
/// A question whose content does not decode against that copy (for example a
/// desktop answer outside the fitted form) is shown unanswered.
pub(crate) fn accepted_answers<'a>(
    form: &'a RemoteElicitationForm,
    content: &Value,
) -> Vec<AcceptedAnswer<'a>> {
    form.questions
        .iter()
        .map(|question| AcceptedAnswer {
            question,
            answer: remote_question_answer(question, content).ok().flatten(),
        })
        .collect()
}

type CopyCatalog = std::collections::HashMap<String, String>;

/// One keyed copy table per desktop language, embedded at compile time.
fn catalog_source(locale: ImLocale) -> &'static str {
    match locale {
        ImLocale::ZhCn => include_str!("../locales/zh-CN.json"),
        ImLocale::ZhTw => include_str!("../locales/zh-TW.json"),
        ImLocale::En => include_str!("../locales/en.json"),
        ImLocale::JaJp => include_str!("../locales/ja-JP.json"),
        ImLocale::KoKr => include_str!("../locales/ko-KR.json"),
        ImLocale::PtBr => include_str!("../locales/pt-BR.json"),
        ImLocale::Es => include_str!("../locales/es.json"),
    }
}

static COPY_CATALOGS: std::sync::LazyLock<Vec<(ImLocale, CopyCatalog)>> =
    std::sync::LazyLock::new(|| {
        ImLocale::ALL
            .iter()
            .map(|locale| {
                let catalog = serde_json::from_str(catalog_source(*locale))
                    .expect("embedded IM copy catalog is valid JSON");
                (*locale, catalog)
            })
            .collect()
    });

fn catalog(locale: ImLocale) -> &'static CopyCatalog {
    COPY_CATALOGS
        .iter()
        .find_map(|(candidate, catalog)| (*candidate == locale).then_some(catalog))
        .expect("every desktop language has an IM copy catalog")
}

/// Keys without copy (for example raw presentation field names) render as-is.
pub(crate) fn localized<'a>(locale: ImLocale, key: &'a str) -> &'a str {
    catalog(locale).get(key).map_or(key, String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn customer_copy_and_action_labels_are_localized() {
        assert_eq!(
            localized(ImLocale::ZhCn, "im.notification.permission.title"),
            "权限请求"
        );
        let action = |option_id: &str, name: &str, kind: PermissionActionKind| {
            InterventionAllowedAction::PermissionOption {
                option_id: option_id.into(),
                name: name.into(),
                permission_kind: kind,
            }
        };
        assert_eq!(
            action_label(
                ImLocale::En,
                &action(
                    "allow_once",
                    "Yes, proceed",
                    PermissionActionKind::AllowOnce
                )
            ),
            "Allow once"
        );
        assert_eq!(
            action_label(
                ImLocale::ZhCn,
                &action(
                    "allow_for_session",
                    "Yes, and don't ask again for these files",
                    PermissionActionKind::AllowAlways
                )
            ),
            "本次会话允许"
        );
        assert_eq!(
            action_label(
                ImLocale::En,
                &action(
                    "allow_for_session",
                    "Yes, and don't ask again for these files",
                    PermissionActionKind::AllowAlways
                )
            ),
            "Allow for session"
        );
        assert_eq!(
            action_label(
                ImLocale::En,
                &action("cancel", "No", PermissionActionKind::RejectOnce)
            ),
            "Deny"
        );
        assert_eq!(
            action_label(
                ImLocale::ZhCn,
                &action(
                    "reject_always",
                    "Always deny",
                    PermissionActionKind::RejectAlways
                )
            ),
            "始终拒绝"
        );
        assert_eq!(
            action_label(
                ImLocale::En,
                &action(
                    "reject_always",
                    "Always deny",
                    PermissionActionKind::RejectAlways
                )
            ),
            "Always deny"
        );
        for (option_id, name, expected) in [
            ("auto", "Yes, and use \"auto\" mode", "自动模式"),
            ("acceptEdits", "Yes, and auto-accept edits", "自动编辑"),
            (
                "bypassPermissions",
                "Yes, and bypass permissions",
                "跳过权限",
            ),
            ("default", "Yes, and manually approve edits", "手动确认"),
            ("plan", "No, keep planning", "保持计划"),
        ] {
            assert_eq!(
                action_label(
                    ImLocale::ZhCn,
                    &action(option_id, name, PermissionActionKind::AllowAlways)
                ),
                expected
            );
        }
        assert_eq!(
            action_label(
                ImLocale::ZhCn,
                &action("opaque-secret", "Opaque", PermissionActionKind::Unknown)
            ),
            "桌面处理"
        );
        assert_eq!(
            action_label(
                ImLocale::JaJp,
                &action("allow_once", "Allow", PermissionActionKind::AllowOnce)
            ),
            "今回のみ許可"
        );
        assert_eq!(join_list(ImLocale::ZhTw, &["A".into(), "B".into()]), "A、B");
        assert_eq!(join_list(ImLocale::Es, &["A".into(), "B".into()]), "A, B");
    }

    #[test]
    fn accepted_answers_project_options_custom_text_and_missing_answers() {
        use crate::app::intervention::{
            ElicitationQuestion, ElicitationQuestionKind, ElicitationQuestionOption,
            RemoteQuestionConstraints,
        };
        let question =
            |selector: &str, kind: ElicitationQuestionKind, custom: bool| RemoteQuestion {
                selector_key: selector.into(),
                question: ElicitationQuestion {
                    field_name: format!("{selector}_field"),
                    title: selector.into(),
                    description: None,
                    question_kind: kind,
                    options: ["a", "b", "c"]
                        .into_iter()
                        .map(|value| ElicitationQuestionOption {
                            value: serde_json::json!(value),
                            label: value.to_uppercase(),
                            description: None,
                        })
                        .collect(),
                    allows_custom_answer: custom,
                },
                constraints: RemoteQuestionConstraints {
                    required: false,
                    allows_empty: true,
                    max_chars: 100,
                    multiline: false,
                },
                custom_answer_key: custom.then(|| format!("{selector}_custom")),
            };
        let form = RemoteElicitationForm {
            questions: vec![
                question("multi", ElicitationQuestionKind::MultiSelect, false),
                question("single", ElicitationQuestionKind::SingleSelect, true),
                question("text", ElicitationQuestionKind::FreeText, false),
                question("missing", ElicitationQuestionKind::SingleSelect, false),
            ],
            decline_allowed: false,
        };
        let content = serde_json::json!({
            "multi_field": ["c", "a"],
            "single_custom": "Other answer",
            "text_field": "Free text",
        });
        let answers = accepted_answers(&form, &content);
        let display = |locale| {
            answers
                .iter()
                .map(|answer| answer.display(locale))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            display(ImLocale::ZhCn),
            [
                Some("C、A".to_owned()),
                Some("Other answer".to_owned()),
                Some("Free text".to_owned()),
                None,
            ]
        );
        assert_eq!(display(ImLocale::En)[0].as_deref(), Some("C, A"));
        assert_eq!(answers[0].option_ids(), ["2", "0"]);
        assert!(answers[1].option_ids().is_empty());
        assert_eq!(answers[1].custom_text(), Some("Other answer"));
        assert_eq!(answers[2].text(), Some("Free text"));
    }

    fn placeholders(text: &str) -> std::collections::BTreeSet<&str> {
        text.match_indices('{')
            .filter_map(|(start, _)| {
                let end = text[start..].find('}')?;
                Some(&text[start..=start + end])
            })
            .collect()
    }

    /// zh-CN is the semantic source: every locale carries the same keys,
    /// the same placeholders and the English product words zh-CN keeps.
    #[test]
    fn every_locale_catalog_matches_the_zh_cn_source() {
        const RETAINED_WORDS: [&str; 4] = ["Agent", "ACP", "Run", "id="];
        let source = catalog(ImLocale::ZhCn);
        assert!(!source.is_empty());
        for locale in ImLocale::ALL {
            let catalog = catalog(locale);
            let mut missing: Vec<_> = source
                .keys()
                .filter(|key| !catalog.contains_key(*key))
                .collect();
            let mut extra: Vec<_> = catalog
                .keys()
                .filter(|key| !source.contains_key(*key))
                .collect();
            missing.sort();
            extra.sort();
            assert!(
                missing.is_empty() && extra.is_empty(),
                "{locale:?}: missing {missing:?}, extra {extra:?}"
            );
            for (key, text) in source {
                let translated = &catalog[key];
                assert!(!translated.trim().is_empty(), "{locale:?}: {key} is empty");
                assert_eq!(
                    placeholders(text),
                    placeholders(translated),
                    "{locale:?}: {key} placeholders differ"
                );
                for word in RETAINED_WORDS {
                    assert!(
                        !text.contains(word) || translated.contains(word),
                        "{locale:?}: {key} must keep {word}"
                    );
                }
            }
        }
    }
}
