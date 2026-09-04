pub mod wecom;

use serde::{Deserialize, Serialize};

use crate::app::intervention::{
    InterventionAction, InterventionAllowedAction, PermissionActionKind, PermissionActionQualifier,
    intervention_action_for_allowed, permission_action_qualifier,
};

use super::{
    ImActionTokenClaims, ImActionTokenCodec, ImDelivery, ImDeliveryPayload, ImInboundError,
    ImLocale, InterventionPresentation, InterventionQuestionKind,
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
            localized(locale, "im.intervention.handledOnDesktop").replace("{ref}", &display_ref)
        }
    }
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
    match (locale, state) {
        (ImLocale::ZhCn, super::ImMessageState::Handled) => "已处理",
        (ImLocale::ZhCn, super::ImMessageState::Expired) => "操作已过期",
        (ImLocale::ZhCn, super::ImMessageState::Failed) => "处理失败，请在桌面端查看",
        (ImLocale::En, super::ImMessageState::Handled) => "Handled",
        (ImLocale::En, super::ImMessageState::Expired) => "This action has expired",
        (ImLocale::En, super::ImMessageState::Failed) => "Failed. Open the desktop app for details",
    }
}

fn action_label(locale: ImLocale, action: &InterventionAllowedAction) -> String {
    match action {
        InterventionAllowedAction::PermissionOption {
            option_id,
            permission_kind,
            ..
        } => {
            let qualifier = permission_action_qualifier(option_id, action_name(action));
            match (locale, qualifier) {
                (_, PermissionActionQualifier::BypassPermissions) => {
                    localized(locale, "im.permission.action.bypass").into()
                }
                (_, PermissionActionQualifier::AutoEdit) => {
                    localized(locale, "im.permission.action.autoEdit").into()
                }
                (_, PermissionActionQualifier::AutoMode) => {
                    localized(locale, "im.permission.action.autoMode").into()
                }
                (_, PermissionActionQualifier::ManualApproval) => {
                    localized(locale, "im.permission.action.manualApproval").into()
                }
                (_, PermissionActionQualifier::KeepPlanning) => {
                    localized(locale, "im.permission.action.keepPlanning").into()
                }
                (ImLocale::ZhCn, PermissionActionQualifier::Standard)
                    if *permission_kind == PermissionActionKind::AllowOnce =>
                {
                    "允许一次".into()
                }
                (ImLocale::ZhCn, PermissionActionQualifier::Standard)
                    if *permission_kind == PermissionActionKind::AllowAlways =>
                {
                    "记住选择".into()
                }
                (ImLocale::ZhCn, PermissionActionQualifier::Standard)
                    if *permission_kind == PermissionActionKind::RejectOnce =>
                {
                    "拒绝".into()
                }
                (ImLocale::ZhCn, PermissionActionQualifier::Standard)
                    if *permission_kind == PermissionActionKind::RejectAlways =>
                {
                    "记住拒绝".into()
                }
                (ImLocale::En, PermissionActionQualifier::Standard)
                    if *permission_kind == PermissionActionKind::AllowOnce =>
                {
                    "Allow once".into()
                }
                (ImLocale::En, PermissionActionQualifier::Standard)
                    if *permission_kind == PermissionActionKind::AllowAlways =>
                {
                    "Remember choice".into()
                }
                (ImLocale::En, PermissionActionQualifier::Standard)
                    if *permission_kind == PermissionActionKind::RejectOnce =>
                {
                    "Deny".into()
                }
                (ImLocale::En, PermissionActionQualifier::Standard)
                    if *permission_kind == PermissionActionKind::RejectAlways =>
                {
                    "Deny all".into()
                }
                (ImLocale::ZhCn, _) => "桌面处理".into(),
                (ImLocale::En, _) => "Desktop".into(),
            }
        }
        InterventionAllowedAction::ElicitationFixedForm { .. } => {
            localized(locale, "im.elicitation.form.label").into()
        }
        InterventionAllowedAction::ElicitationAccept => {
            localized(locale, "im.action.accept").into()
        }
        InterventionAllowedAction::ElicitationDecline => {
            localized(locale, "im.action.decline").into()
        }
        InterventionAllowedAction::ManualSuccess => localized(locale, "im.action.success").into(),
        InterventionAllowedAction::ManualFailure => localized(locale, "im.action.failure").into(),
    }
}

pub(crate) fn action_name(action: &InterventionAllowedAction) -> &str {
    match action {
        InterventionAllowedAction::PermissionOption { name, .. } => name,
        _ => "",
    }
}

pub(crate) fn localized(locale: ImLocale, key: &str) -> &str {
    match (locale, key) {
        (ImLocale::ZhCn, "im.notification.permission.title") => "权限请求",
        (ImLocale::ZhCn, "im.notification.permission.summary") => "任务正在等待你的权限决定。",
        (ImLocale::ZhCn, "im.notification.elicitation.title") => "需要补充信息",
        (ImLocale::ZhCn, "im.notification.elicitation.summary") => "任务正在等待你的回答。",
        (ImLocale::ZhCn, "im.notification.manualCheck.title") => "需要人工检查",
        (ImLocale::ZhCn, "im.notification.manualCheck.summary") => "任务正在等待人工检查结果。",
        (ImLocale::ZhCn, "im.notification.run_success.summary") => "Run 已成功完成",
        (ImLocale::ZhCn, "im.notification.run_failure.summary") => "Run 执行失败",
        (ImLocale::ZhCn, "im.notification.acp_turn_finished.summary") => "ACP 回合已结束",
        (ImLocale::ZhCn, "nodeLabel") => "节点",
        (ImLocale::ZhCn, "taskTitle") => "任务",
        (ImLocale::ZhCn, "permissionTool") => "工具",
        (ImLocale::ZhCn, "permissionPath") => "路径",
        (ImLocale::ZhCn, "permissionParameter") => "参数",
        (ImLocale::ZhCn, "permissionCommand") => "命令",
        (ImLocale::ZhCn, "im.action.accept") => "接受",
        (ImLocale::ZhCn, "im.action.decline") => "拒绝",
        (ImLocale::ZhCn, "im.action.success") => "成功",
        (ImLocale::ZhCn, "im.action.failure") => "失败",
        (ImLocale::ZhCn, "im.question.singleSelect") => "单选",
        (ImLocale::ZhCn, "im.question.multiSelect") => "多选",
        (ImLocale::ZhCn, "im.question.freeText") => "文本回答，请在桌面端填写",
        (ImLocale::ZhCn, "im.question.customAnswer") => "- 其他答案请在桌面端填写",
        (ImLocale::ZhCn, "im.permission.vote.subtitle") => "请选择授权范围后提交",
        (ImLocale::ZhCn, "im.permission.vote.linkedSubtitle") => {
            "请求详情见上一条消息，请选择授权范围后提交"
        }
        (ImLocale::ZhCn, "im.permission.card.title") => "权限审批",
        (ImLocale::ZhCn, "im.permission.card.titleWithRef") => "权限审批（id={ref}）",
        (ImLocale::ZhCn, "im.permission.card.summary") => "Agent 请求执行命令",
        (ImLocale::ZhCn, "im.permission.detail.description") => "说明",
        (ImLocale::ZhCn, "im.permission.detail.link") => {
            "请求详情见本消息，请在下一条权限审批卡片中选择。"
        }
        (ImLocale::ZhCn, "im.manualCheck.card.titleWithRef") => "人工检查（id={ref}）",
        (ImLocale::ZhCn, "im.manualCheck.vote.linkedSubtitle") => {
            "模型输出见上一条消息，请选择检查结果后提交"
        }
        (ImLocale::ZhCn, "im.manualCheck.detail.output") => "最后一轮模型输出",
        (ImLocale::ZhCn, "im.manualCheck.detail.link") => {
            "模型输出见本消息，请在下一条人工检查卡片中选择。"
        }
        (ImLocale::ZhCn, "im.permission.desktopGuidance") => "请在桌面端处理此权限请求",
        (ImLocale::ZhCn, "im.permission.submit") => "提交",
        (ImLocale::ZhCn, "im.permission.submitted") => "已提交",
        (ImLocale::ZhCn, "im.permission.action.bypass") => "跳过权限",
        (ImLocale::ZhCn, "im.permission.action.autoEdit") => "自动编辑",
        (ImLocale::ZhCn, "im.permission.action.autoMode") => "自动模式",
        (ImLocale::ZhCn, "im.permission.action.manualApproval") => "手动确认",
        (ImLocale::ZhCn, "im.permission.action.keepPlanning") => "保持计划",
        (ImLocale::ZhCn, "im.elicitation.form.label") => "远程表单",
        (ImLocale::ZhCn, "im.elicitation.card.titleWithRef") => "补充信息（id={ref}）",
        (ImLocale::ZhCn, "im.elicitation.vote.linkedSubtitle") => {
            "问题详情见上一条消息，请完成选择后提交"
        }
        (ImLocale::ZhCn, "im.elicitation.detail.link") => {
            "问题详情见本消息，请在下一条补充信息卡片中选择。"
        }
        (ImLocale::ZhCn, "im.elicitation.detail.context") => "前序模型输出",
        (ImLocale::ZhCn, "im.intervention.handledOnDesktop") => "**id={ref} 已在桌面端处理**",
        (ImLocale::En, "im.notification.permission.title") => "Permission request",
        (ImLocale::En, "im.notification.permission.summary") => {
            "A task is waiting for your permission decision."
        }
        (ImLocale::En, "im.notification.elicitation.title") => "More information required",
        (ImLocale::En, "im.notification.elicitation.summary") => {
            "A task is waiting for your answer."
        }
        (ImLocale::En, "im.notification.manualCheck.title") => "Manual check required",
        (ImLocale::En, "im.notification.manualCheck.summary") => {
            "A task is waiting for a manual check result."
        }
        (ImLocale::En, "im.notification.run_success.summary") => "Run succeeded",
        (ImLocale::En, "im.notification.run_failure.summary") => "Run failed",
        (ImLocale::En, "im.notification.acp_turn_finished.summary") => "ACP turn finished",
        (ImLocale::En, "nodeLabel") => "Node",
        (ImLocale::En, "taskTitle") => "Task",
        (ImLocale::En, "permissionTool") => "Tool",
        (ImLocale::En, "permissionPath") => "Path",
        (ImLocale::En, "permissionParameter") => "Parameter",
        (ImLocale::En, "permissionCommand") => "Command",
        (ImLocale::En, "im.action.accept") => "Accept",
        (ImLocale::En, "im.action.decline") => "Decline",
        (ImLocale::En, "im.action.success") => "Success",
        (ImLocale::En, "im.action.failure") => "Failure",
        (ImLocale::En, "im.question.singleSelect") => "Single choice",
        (ImLocale::En, "im.question.multiSelect") => "Multiple choice",
        (ImLocale::En, "im.question.freeText") => "Text answer; enter it in the desktop app",
        (ImLocale::En, "im.question.customAnswer") => "- Enter another answer in the desktop app",
        (ImLocale::En, "im.permission.vote.subtitle") => "Choose a scope and submit",
        (ImLocale::En, "im.permission.vote.linkedSubtitle") => {
            "See the preceding message for details, then choose a scope and submit"
        }
        (ImLocale::En, "im.permission.card.title") => "Permission approval",
        (ImLocale::En, "im.permission.card.titleWithRef") => "Permission (id={ref})",
        (ImLocale::En, "im.permission.card.summary") => "Agent requests command execution",
        (ImLocale::En, "im.permission.detail.description") => "Description",
        (ImLocale::En, "im.permission.detail.link") => {
            "This message contains the request details. Use the next permission card to decide."
        }
        (ImLocale::En, "im.manualCheck.card.titleWithRef") => "Manual check (id={ref})",
        (ImLocale::En, "im.manualCheck.vote.linkedSubtitle") => {
            "See the preceding message for the model output, choose a result, then submit"
        }
        (ImLocale::En, "im.manualCheck.detail.output") => "Latest model output",
        (ImLocale::En, "im.manualCheck.detail.link") => {
            "This message contains the model output. Use the next manual-check card to decide."
        }
        (ImLocale::En, "im.permission.desktopGuidance") => {
            "Handle this permission request in the desktop app"
        }
        (ImLocale::En, "im.permission.submit") => "Submit",
        (ImLocale::En, "im.permission.submitted") => "Submitted",
        (ImLocale::En, "im.permission.action.bypass") => "Bypass",
        (ImLocale::En, "im.permission.action.autoEdit") => "Auto edits",
        (ImLocale::En, "im.permission.action.autoMode") => "Auto mode",
        (ImLocale::En, "im.permission.action.manualApproval") => "Manual",
        (ImLocale::En, "im.permission.action.keepPlanning") => "Keep plan",
        (ImLocale::En, "im.elicitation.form.label") => "Remote form",
        (ImLocale::En, "im.elicitation.card.titleWithRef") => "More information (id={ref})",
        (ImLocale::En, "im.elicitation.vote.linkedSubtitle") => {
            "See the preceding message for details, choose an answer, then submit"
        }
        (ImLocale::En, "im.elicitation.detail.link") => {
            "This message contains the questions. Use the next information card to answer."
        }
        (ImLocale::En, "im.elicitation.detail.context") => "Previous model output",
        (ImLocale::En, "im.intervention.handledOnDesktop") => "**id={ref} Handled on desktop**",
        _ => key,
    }
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
            "记住选择"
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
            "记住拒绝"
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
    }
}
