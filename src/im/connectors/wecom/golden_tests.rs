//! Golden wire tests that freeze the observable WeCom behaviour (§11.1 of the
//! Feishu remote-intervention design) across the channel-capability refactor.
//!
//! The expected values live in `golden/wecom_wire.json` and must never be
//! edited to make a refactor pass. Fixture builders and the interaction
//! pipeline helpers in this file may be migrated to new internal types.
//! Regenerate only for an intentional, documented WeCom behaviour change:
//! `GOLD_BAND_BLESS_GOLDEN=1 cargo test --lib wecom::golden_tests`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use camino::Utf8PathBuf;
use serde_json::{Value, json};

use super::*;
use crate::app::intervention::{
    ElicitationQuestion, ElicitationQuestionOption, InterventionAction, InterventionAllowedAction,
    InterventionCommandResult, InterventionCommandStatus, InterventionLocator,
    InterventionRequestIdentity, MAX_REMOTE_TEXT_CHARS, PermissionActionKind,
    RemoteElicitationForm, RemoteQuestion, RemoteQuestionConstraints,
};
use crate::im::{
    IM_PAYLOAD_VERSION, ImActionTokenCodec, ImDestination, ImInboundActionService,
    ImNavigationLocator, ImRepository, ImResolutionOutcome, ImResolvedBy,
    InformationalNotification, InterventionPresentation, InterventionQuestion,
    InterventionQuestionKind, InterventionQuestionOption, InterventionRef,
    InterventionResolutionNotification, resolution_event_id,
};

const GOLDEN_PATH: &str = "src/im/connectors/wecom/golden/wecom_wire.json";
const BLESS_ENV: &str = "GOLD_BAND_BLESS_GOLDEN";
const ACTOR: &str = "single-user";

fn codec() -> ImActionTokenCodec {
    ImActionTokenCodec::new(vec![7; 32]).unwrap()
}

fn locator() -> InterventionLocator {
    InterventionLocator {
        project_id: "p".into(),
        task_id: "t".into(),
        run_id: "r".into(),
        round_id: "round".into(),
        node_id: "n".into(),
        attempt_id: "a".into(),
        outer_node_id: None,
        outer_attempt_id: None,
    }
}

fn navigation_locator() -> ImNavigationLocator {
    ImNavigationLocator {
        project_id: "p".into(),
        task_id: Some("t".into()),
        run_id: Some("r".into()),
        round_id: Some("round".into()),
        node_id: Some("n".into()),
        attempt_id: Some("a".into()),
        outer_node_id: None,
        outer_attempt_id: None,
        scheduled_occurrence_id: None,
    }
}

fn delivery(
    payload: ImDeliveryPayload,
    kind: ImNotificationKind,
    display_ref: Option<u16>,
) -> ImDelivery {
    ImDelivery {
        delivery_id: "delivery-1".into(),
        channel: ImChannelKind::WeCom,
        destination: ImDestination {
            destination_id: ACTOR.into(),
            conversation_id: ACTOR.into(),
            authorized_actor_id: ACTOR.into(),
        },
        notification_kind: kind,
        canonical_event_id: "event-1".into(),
        payload,
        expires_at_ms: i64::MAX,
        display_ref,
    }
}

fn presentation(
    kind: &str,
    body: &str,
    context: Option<&str>,
    fields: &[(&str, &str)],
    questions: Vec<InterventionQuestion>,
) -> InterventionPresentation {
    InterventionPresentation {
        title_key: format!("im.notification.{kind}.title"),
        summary_key: format!("im.notification.{kind}.summary"),
        title: None,
        summary: None,
        body: Some(body.into()),
        context: context.map(Into::into),
        fields: fields
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
        questions,
    }
}

fn intervention_delivery(
    request: InterventionRequestIdentity,
    allowed_actions: Vec<InterventionAllowedAction>,
    presentation: InterventionPresentation,
    kind: ImNotificationKind,
    display_ref: u16,
) -> ImDelivery {
    delivery(
        ImDeliveryPayload::Intervention {
            version: IM_PAYLOAD_VERSION,
            reference: InterventionRef {
                locator: locator(),
                request,
                expected_state: "state".into(),
                allowed_actions,
                expires_at_ms: None,
            },
            presentation,
        },
        kind,
        Some(display_ref),
    )
}

fn permission_option(
    option_id: &str,
    name: &str,
    kind: PermissionActionKind,
) -> InterventionAllowedAction {
    InterventionAllowedAction::PermissionOption {
        option_id: option_id.into(),
        name: name.into(),
        permission_kind: kind,
    }
}

fn permission_presentation() -> InterventionPresentation {
    let mut presentation = presentation(
        "permission",
        "command failed; retry without sandbox?",
        None,
        &[
            ("workspaceLabel", "Gold-Band"),
            ("taskTitle", "Repair download"),
            ("nodeLabel", "Reviewer"),
            ("permissionTitle", "Make edits?"),
            ("permissionTool", "Edit files"),
            ("permissionCommand", "cargo build --release"),
            ("permissionPath", "D:\\work"),
            ("permissionParameter", "fallback"),
        ],
        Vec::new(),
    );
    presentation.title = Some("Make edits?".into());
    presentation.summary = Some("command failed; retry without sandbox?".into());
    presentation
}

fn permission_delivery() -> ImDelivery {
    intervention_delivery(
        InterventionRequestIdentity::Permission {
            request_id: "req".into(),
        },
        vec![
            permission_option("allow", "Allow once", PermissionActionKind::AllowOnce),
            permission_option("always", "Always allow", PermissionActionKind::AllowAlways),
            permission_option("reject", "Reject", PermissionActionKind::RejectOnce),
            permission_option("never", "Always reject", PermissionActionKind::RejectAlways),
        ],
        permission_presentation(),
        ImNotificationKind::Permission,
        7,
    )
}

fn overflowing_permission_delivery() -> ImDelivery {
    let kinds = [
        PermissionActionKind::AllowOnce,
        PermissionActionKind::RejectOnce,
        PermissionActionKind::AllowAlways,
        PermissionActionKind::RejectAlways,
        PermissionActionKind::Unknown,
    ];
    let actions = (0..25)
        .map(|index| {
            permission_option(
                &format!("option-{index}"),
                &format!("Option {index}"),
                kinds[index % kinds.len()],
            )
        })
        .collect();
    intervention_delivery(
        InterventionRequestIdentity::Permission {
            request_id: "req".into(),
        },
        actions,
        permission_presentation(),
        ImNotificationKind::Permission,
        7,
    )
}

fn manual_check_delivery() -> ImDelivery {
    intervention_delivery(
        InterventionRequestIdentity::ManualCheck,
        vec![
            InterventionAllowedAction::ManualSuccess,
            InterventionAllowedAction::ManualFailure,
        ],
        presentation(
            "manualCheck",
            "latest model output",
            None,
            &[
                ("workspaceLabel", "Gold-Band"),
                ("nodeLabel", "Interview"),
                ("taskTitle", "Prepare release"),
            ],
            Vec::new(),
        ),
        ImNotificationKind::ManualCheck,
        123,
    )
}

#[derive(Clone, Copy)]
enum ElicitationShape {
    Single,
    Multi,
    Multiple,
}

struct GoldenQuestion {
    field_name: &'static str,
    title: &'static str,
    description: Option<&'static str>,
    kind: InterventionQuestionKind,
    required: bool,
    allows_custom_answer: bool,
    options: Vec<(&'static str, &'static str, Option<&'static str>)>,
}

fn golden_questions(shape: ElicitationShape) -> Vec<GoldenQuestion> {
    let databases = vec![
        ("MySQL", "MySQL", Some("默认数据库")),
        ("PostgreSQL", "PostgreSQL", None),
        ("SQLite", "SQLite", None),
    ];
    match shape {
        ElicitationShape::Single => vec![GoldenQuestion {
            field_name: "database",
            title: "数据库选择",
            description: Some("请选择要使用的数据库？"),
            kind: InterventionQuestionKind::SingleSelect,
            required: true,
            allows_custom_answer: true,
            options: databases,
        }],
        ElicitationShape::Multi => vec![GoldenQuestion {
            field_name: "features",
            title: "功能选择",
            description: Some("请选择要启用的功能"),
            kind: InterventionQuestionKind::MultiSelect,
            required: true,
            allows_custom_answer: false,
            options: vec![
                ("search", "搜索", None),
                ("export", "导出", Some("导出为 CSV")),
                ("audit", "审计日志", None),
            ],
        }],
        ElicitationShape::Multiple => vec![
            GoldenQuestion {
                field_name: "database",
                title: "数据库选择",
                description: None,
                kind: InterventionQuestionKind::SingleSelect,
                required: true,
                allows_custom_answer: false,
                options: databases,
            },
            GoldenQuestion {
                field_name: "region",
                title: "部署区域",
                description: Some("可选"),
                kind: InterventionQuestionKind::SingleSelect,
                required: false,
                allows_custom_answer: false,
                options: vec![("cn", "中国大陆", None), ("sg", "新加坡", None)],
            },
        ],
    }
}

/// Migration point: the pre-refactor WeCom forms carried no custom answers and
/// never allowed an empty multi-select, so the fixture keeps both off.
fn remote_question(index: usize, question: &GoldenQuestion) -> RemoteQuestion {
    RemoteQuestion {
        selector_key: format!("q{index}"),
        question: ElicitationQuestion {
            field_name: question.field_name.into(),
            title: question.title.into(),
            description: question.description.map(Into::into),
            question_kind: match &question.kind {
                InterventionQuestionKind::SingleSelect => ElicitationQuestionKind::SingleSelect,
                InterventionQuestionKind::MultiSelect => ElicitationQuestionKind::MultiSelect,
                other => panic!("golden questions are choice questions, got {other:?}"),
            },
            options: question
                .options
                .iter()
                .map(|(value, label, description)| ElicitationQuestionOption {
                    value: json!(value),
                    label: (*label).into(),
                    description: description.map(Into::into),
                })
                .collect(),
            allows_custom_answer: false,
        },
        constraints: RemoteQuestionConstraints {
            required: question.required,
            allows_empty: false,
            max_chars: MAX_REMOTE_TEXT_CHARS,
            multiline: false,
        },
        custom_answer_key: None,
    }
}

fn elicitation_form(shape: ElicitationShape) -> RemoteElicitationForm {
    RemoteElicitationForm {
        questions: golden_questions(shape)
            .iter()
            .enumerate()
            .map(|(index, question)| remote_question(index, question))
            .collect(),
        decline_allowed: false,
    }
}

fn elicitation_delivery(shape: ElicitationShape) -> ImDelivery {
    let questions = golden_questions(shape)
        .into_iter()
        .map(|question| InterventionQuestion {
            field_name: question.field_name.into(),
            title: question.title.into(),
            description: question.description.map(Into::into),
            question_kind: question.kind,
            options: question
                .options
                .iter()
                .map(|(value, label, description)| InterventionQuestionOption {
                    value: json!(value),
                    label: (*label).into(),
                    description: description.map(Into::into),
                })
                .collect(),
            allows_custom_answer: question.allows_custom_answer,
        })
        .collect();
    intervention_delivery(
        InterventionRequestIdentity::Elicitation {
            elicitation_id: "elicit-1".into(),
        },
        vec![
            InterventionAllowedAction::ElicitationFixedForm {
                form: elicitation_form(shape),
            },
            InterventionAllowedAction::ElicitationDecline,
        ],
        presentation(
            "elicitation",
            "请确认部署配置？",
            Some("已有部署包含订单服务与用户服务。"),
            &[
                ("workspaceLabel", "Gold-Band"),
                ("nodeLabel", "Direct agent"),
                ("taskTitle", "Task"),
            ],
            questions,
        ),
        ImNotificationKind::Elicitation,
        80,
    )
}

fn desktop_resolution_delivery(kind: ImNotificationKind) -> ImDelivery {
    let mut delivery = delivery(
        ImDeliveryPayload::InterventionResolution {
            version: IM_PAYLOAD_VERSION,
            resolution: InterventionResolutionNotification {
                canonical_event_id: resolution_event_id("event-1"),
                source_event_id: "event-1".into(),
                notification_kind: kind,
                locator: navigation_locator(),
                display_ref: Some(12),
                resolved_by: ImResolvedBy::Desktop,
                outcome: ImResolutionOutcome::ElicitationSubmitted { content: json!({}) },
            },
        },
        kind,
        Some(12),
    );
    delivery.canonical_event_id = resolution_event_id("event-1");
    delivery
}

fn information_delivery() -> ImDelivery {
    delivery(
        ImDeliveryPayload::Information {
            version: IM_PAYLOAD_VERSION,
            notification: InformationalNotification {
                canonical_event_id: "event-1".into(),
                notification_kind: ImNotificationKind::RunFailure,
                locator: navigation_locator(),
                outcome: "failure".into(),
                summary_key: "im.notification.runFailure.summary".into(),
                parameters: BTreeMap::from([
                    ("taskTitle".to_owned(), "Prepare release".to_owned()),
                    ("workspaceLabel".to_owned(), "Gold-Band".to_owned()),
                ]),
                missed_count: Some(2),
            },
        },
        ImNotificationKind::RunFailure,
        Some(5),
    )
}

fn callback(card_event: Value, req_id: &str) -> Value {
    json!({
        "cmd": "aibot_event_callback",
        "headers": { "req_id": req_id },
        "body": {
            "msgid": "event-1",
            "chattype": "single",
            "from": { "userid": ACTOR },
            "event": {
                "eventtype": "template_card_event",
                "template_card_event": card_event
            }
        }
    })
}

fn button_callback(index: usize) -> Value {
    callback(
        json!({
            "card_type": "button_interaction",
            "event_key": format!("{}:{index}", card_task_id("delivery-1", "req-1")),
            "task_id": card_task_id("delivery-1", "req-1")
        }),
        "callback-req-1",
    )
}

fn selection_callback(card_type: &str, task_id: &str, selections: &[(&str, &[&str])]) -> Value {
    callback(
        json!({
            "card_type": card_type,
            "event_key": format!("{}:submit", card_task_id("delivery-1", "req-1")),
            "task_id": card_task_id(task_id, "req-1"),
            "selected_items": {
                "selected_item": selections.iter().map(|(key, option_ids)| json!({
                    "question_key": key,
                    "option_ids": { "option_id": option_ids }
                })).collect::<Vec<_>>()
            }
        }),
        "callback-req-1",
    )
}

/// Migration point: the canonical InterventionAction applied for one decoded
/// WeCom envelope, or the terminal card state a WeCom user observes when the
/// generic inbound boundary rejects it. Internal error codes are deliberately
/// not frozen; only their user-visible projection is.
fn inbound_outcome(envelope: &ImInboundEnvelope, delivery: &ImDelivery) -> Value {
    let temp = tempfile::TempDir::new().unwrap();
    let repository = Arc::new(ImRepository::new(
        Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
    ));
    // The repository requires the deterministic delivery id; the wire fixtures
    // keep the short `delivery-1` id, so rebind both sides consistently.
    let mut delivery = delivery.clone();
    delivery.delivery_id = ImDelivery::deterministic_id(
        delivery.channel,
        &delivery.destination.destination_id,
        delivery.notification_kind,
        &delivery.canonical_event_id,
    );
    let mut envelope = envelope.clone();
    envelope.delivery_id = delivery.delivery_id.clone();
    repository.enqueue(&delivery, 0).unwrap();
    let service = ImInboundActionService::new(repository, codec());
    let captured = Mutex::new(None::<InterventionAction>);
    let result = service.handle_with_delivery(&envelope, &delivery, 1, |command| {
        *captured.lock().unwrap() = Some(command.action);
        Ok(InterventionCommandResult {
            status: InterventionCommandStatus::Accepted,
            canonical_state: "resolved".into(),
        })
    });
    match result {
        Ok(_) => json!({ "action": captured.into_inner().unwrap() }),
        Err(error) => json!({
            "rejectedCardState": card_state_name(
                ImActionResponseState::Rejected(error.code()).message_state()
            )
        }),
    }
}

/// Migration point: the user-visible card state of one inbound rejection.
fn card_state_name(state: ImMessageState) -> &'static str {
    match state {
        ImMessageState::Expired => "expired",
        ImMessageState::Handled => "handled",
        _ => "failed",
    }
}

/// Migration point: the card update WeCom sends for one callback outcome.
fn terminal_response(
    context: &ImActionResponseContext,
    delivery: Option<&ImDelivery>,
    state: ImActionResponseState,
    outcome: Option<ImResolutionOutcome>,
) -> Value {
    let handle = context
        .downcast_ref::<WeComResponseHandle>()
        .expect("WeCom response context");
    action_response_request(
        handle,
        &ImActionResponse {
            state,
            resolved_by: None,
            outcome,
            delivery: delivery.cloned(),
        },
        ImLocale::ZhCn,
    )
}

const TERMINAL_STATES: [(&str, ImActionResponseState); 3] = [
    ("handled", ImActionResponseState::Handled),
    (
        "expired",
        ImActionResponseState::Rejected(ImErrorCode::ActionExpired),
    ),
    (
        "failed",
        ImActionResponseState::Rejected(ImErrorCode::ActionProcessingFailed),
    ),
];

fn interaction_case(callback: &Value, delivery: &ImDelivery) -> Value {
    let (envelope, context) = action_envelope(callback).expect("golden callback must decode");
    let inbound = inbound_outcome(&envelope, delivery);
    let accepted = inbound
        .get("action")
        .and_then(|action| serde_json::from_value::<InterventionAction>(action.clone()).ok())
        .map(|action| ImResolutionOutcome::from(&action));
    let responses = TERMINAL_STATES
        .iter()
        .map(|(name, state)| {
            (
                (*name).to_owned(),
                terminal_response(
                    &context,
                    Some(delivery),
                    *state,
                    if *state == ImActionResponseState::Handled {
                        accepted.clone()
                    } else {
                        None
                    },
                ),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json!({
        "envelope": {
            "actionId": envelope.action_id,
            "channel": envelope.channel,
            "platformEventId": envelope.platform_event_id,
            "conversationId": envelope.conversation_id,
            "actorId": envelope.actor_id,
            "deliveryId": envelope.delivery_id,
        },
        "inbound": inbound,
        "responses": responses,
        "missingDeliveryFailure": terminal_response(
            &context,
            None,
            ImActionResponseState::Rejected(ImErrorCode::ActionProcessingFailed),
            None,
        ),
    })
}

fn invalid_case(callback: &Value) -> Value {
    let failure = action_envelope(callback).expect_err("golden callback must be rejected");
    let failed_response = failure.response_context.as_ref().map(|context| {
        terminal_response(
            context,
            None,
            ImActionResponseState::Rejected(ImErrorCode::ProtocolInvalid),
            None,
        )
    });
    // Only private callbacks receive a failed-card update; anything else is
    // silently dropped. The internal parse-failure reason is not frozen.
    json!({ "sentResponse": failed_response.filter(|_| failure.private_candidate) })
}

fn send_plan(delivery: &ImDelivery) -> Value {
    let plan = linked_detail_send_plan(
        "detail-req-1",
        "card-req-1",
        delivery,
        &codec(),
        ImLocale::ZhCn,
    )
    .unwrap();
    json!({ "detail": plan.detail_request, "card": plan.card_request })
}

fn golden_cases() -> BTreeMap<String, Value> {
    let mut cases = BTreeMap::new();
    let mut add = |name: &str, value: Value| {
        assert!(cases.insert(name.to_owned(), value).is_none(), "{name}");
    };

    add(
        "send.information",
        send_request("req-1", &information_delivery(), &codec(), ImLocale::ZhCn).unwrap(),
    );
    for (name, kind) in [
        ("permission", ImNotificationKind::Permission),
        ("elicitation", ImNotificationKind::Elicitation),
        ("manualCheck", ImNotificationKind::ManualCheck),
    ] {
        add(
            &format!("send.desktopResolution.{name}"),
            send_request(
                "req-1",
                &desktop_resolution_delivery(kind),
                &codec(),
                ImLocale::ZhCn,
            )
            .unwrap(),
        );
    }
    add("plan.permission", send_plan(&permission_delivery()));
    add(
        "plan.permission.overflow",
        send_plan(&overflowing_permission_delivery()),
    );
    add("plan.manualCheck", send_plan(&manual_check_delivery()));
    add(
        "plan.elicitation.single",
        send_plan(&elicitation_delivery(ElicitationShape::Single)),
    );
    add(
        "plan.elicitation.multi",
        send_plan(&elicitation_delivery(ElicitationShape::Multi)),
    );
    add(
        "plan.elicitation.multiple",
        send_plan(&elicitation_delivery(ElicitationShape::Multiple)),
    );

    add(
        "interaction.permission.button",
        interaction_case(&button_callback(1), &permission_delivery()),
    );
    add(
        "interaction.permission.vote",
        interaction_case(
            &selection_callback(
                "vote_interaction",
                "delivery-1",
                &[(WECOM_VOTE_QUESTION_KEY, &["2"])],
            ),
            &permission_delivery(),
        ),
    );
    add(
        "interaction.manualCheck.vote",
        interaction_case(
            &selection_callback(
                "vote_interaction",
                "delivery-1",
                &[(WECOM_VOTE_QUESTION_KEY, &["1"])],
            ),
            &manual_check_delivery(),
        ),
    );
    add(
        "interaction.elicitation.single",
        interaction_case(
            &selection_callback("vote_interaction", "delivery-1", &[("q0", &["1"])]),
            &elicitation_delivery(ElicitationShape::Single),
        ),
    );
    add(
        "interaction.elicitation.multi",
        interaction_case(
            &selection_callback("vote_interaction", "delivery-1", &[("q0", &["0", "2"])]),
            &elicitation_delivery(ElicitationShape::Multi),
        ),
    );
    add(
        "interaction.elicitation.multiEmpty",
        interaction_case(
            &selection_callback("vote_interaction", "delivery-1", &[("q0", &[])]),
            &elicitation_delivery(ElicitationShape::Multi),
        ),
    );
    add(
        "interaction.elicitation.multiple",
        interaction_case(
            &selection_callback(
                "multiple_interaction",
                "delivery-1",
                &[("q0", &["2"]), ("q1", &["1"])],
            ),
            &elicitation_delivery(ElicitationShape::Multiple),
        ),
    );
    add(
        "interaction.elicitation.multipleOptionalOmitted",
        interaction_case(
            &selection_callback("multiple_interaction", "delivery-1", &[("q0", &["0"])]),
            &elicitation_delivery(ElicitationShape::Multiple),
        ),
    );
    add(
        "interaction.elicitation.multipleRequiredOmitted",
        interaction_case(
            &selection_callback("multiple_interaction", "delivery-1", &[("q1", &["0"])]),
            &elicitation_delivery(ElicitationShape::Multiple),
        ),
    );
    add(
        "interaction.elicitation.indexOnFixedForm",
        interaction_case(
            &button_callback(0),
            &elicitation_delivery(ElicitationShape::Single),
        ),
    );

    add(
        "invalid.vote.taskMismatch",
        invalid_case(&selection_callback(
            "vote_interaction",
            "wrong-delivery",
            &[(WECOM_VOTE_QUESTION_KEY, &["0"])],
        )),
    );
    add(
        "invalid.multiple.duplicateQuestion",
        invalid_case(&selection_callback(
            "multiple_interaction",
            "delivery-1",
            &[("q0", &["0"]), ("q0", &["1"])],
        )),
    );
    add(
        "invalid.vote.optionIdNotNumeric",
        invalid_case(&selection_callback(
            "vote_interaction",
            "delivery-1",
            &[("q0", &["x"])],
        )),
    );
    add(
        "invalid.button.unknownCardType",
        invalid_case(&callback(
            json!({
                "card_type": "text_notice",
                "event_key": "delivery-1:0",
                "task_id": card_task_id("delivery-1", "req-1")
            }),
            "callback-req-1",
        )),
    );
    cases
}

#[test]
fn wecom_wire_behaviour_matches_golden() {
    let actual = golden_cases();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN_PATH);
    if std::env::var_os(BLESS_ENV).is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut encoded = serde_json::to_string_pretty(&actual).unwrap();
        encoded.push('\n');
        std::fs::write(&path, encoded).unwrap();
        return;
    }
    let expected: BTreeMap<String, Value> =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden file missing"))
            .unwrap();
    assert_eq!(
        expected.keys().collect::<Vec<_>>(),
        actual.keys().collect::<Vec<_>>(),
        "golden case set changed"
    );
    for (name, expected) in &expected {
        assert_eq!(
            &actual[name],
            expected,
            "WeCom golden case `{name}` changed:\n{}",
            serde_json::to_string_pretty(&actual[name]).unwrap()
        );
    }
}
