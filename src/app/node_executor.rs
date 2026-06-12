use anyhow::{Result, anyhow, bail, ensure};
use camino::Utf8PathBuf;

use crate::artifacts::parse_json_artifact;
use crate::domain::{InvocationKind, NodeOutcome, PauseReason, RunStatus, SessionMode, VERSION};
use crate::dsl::{
    JsonConditionDsl, JsonPathSegment, NodeDsl, ValidatedWorkflow, WorkerNode, parse_json_path,
};
use crate::observability::{ProgressStage, progress};
use crate::provider::{
    PromptArtifactRef, PromptOutputContract, PromptPredecessorContext, PromptRuntimeContext,
    PromptVisibility, ProviderRunResult, ProviderRunStatus, StreamMode, WorkerInvocation,
};
use crate::runtime::{
    NodeState, RoundState, RoundTraceStep, WorkerRefState, validate_node_state,
    validate_worker_ref_state,
};
use crate::storage::sqlite::{AttemptIndexContext, index_attempt_with_retry};
use crate::storage::{read_json, write_json};

use super::ids::now_rfc3339_like;
use super::{AcpLiveEventContext, App};

fn worker_task_instruction(worker: &WorkerNode) -> Option<String> {
    worker
        .goal
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn success_condition_text(condition: &JsonConditionDsl) -> String {
    match condition {
        JsonConditionDsl::Expression { expression } => expression.clone(),
        JsonConditionDsl::PathEquals { path, equals } => {
            format!("JSON field `{}` equals `{}`", path, equals)
        }
    }
}

fn worker_output_contract(worker: &WorkerNode) -> Option<PromptOutputContract> {
    worker.output.as_ref().map(|output| PromptOutputContract {
        artifact: output.artifact.clone(),
        kind: format!("{:?}", output.kind).to_ascii_lowercase(),
        schema: output.schema.clone(),
        schema_text: None,
        success_condition: worker
            .success_condition
            .as_ref()
            .map(success_condition_text),
    })
}

fn runtime_prompt_context(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> PromptRuntimeContext {
    let task_inputs_dir = app.paths.task_dir(task_id).join("authoring").join("inputs");
    PromptRuntimeContext {
        project_id: app.paths.project_id.clone(),
        task_id: task_id.to_string(),
        run_id: run_id.to_string(),
        round_id: round_id.to_string(),
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        language: app.config.desktop_language,
        run_dir: app.paths.run_dir(task_id, run_id),
        round_dir: app.paths.round_dir(task_id, run_id, round_id),
        node_dir: app.paths.node_dir(task_id, run_id, round_id, node_id),
        attempt_dir: app
            .paths
            .attempt_dir(task_id, run_id, round_id, node_id, attempt_id),
        attachments_dir: app
            .paths
            .attachments_dir(task_id, run_id, round_id, node_id, attempt_id),
        task_inputs_dir: task_inputs_dir.exists().then_some(task_inputs_dir),
    }
}

#[derive(Clone)]
struct TraceRef {
    round_id: String,
    step: RoundTraceStep,
}

fn load_rounds_through_current(
    app: &App,
    task_id: &str,
    run_id: &str,
    current_round: &RoundState,
) -> Vec<RoundState> {
    let rounds_dir = app.paths.run_dir(task_id, run_id).join("rounds");
    let mut rounds = std::fs::read_dir(rounds_dir.as_std_path())
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(std::result::Result::ok))
        .filter_map(|entry| Utf8PathBuf::from_path_buf(entry.path()).ok())
        .map(|path| path.join("round.json"))
        .filter(|path| path.exists())
        .filter_map(|path| read_json::<RoundState>(&path).ok())
        .filter(|round| round.id != current_round.id)
        .collect::<Vec<_>>();
    rounds.push(current_round.clone());
    rounds.sort_by_key(|round| round.index);
    rounds
}

fn flatten_trace_until_current(
    rounds: &[RoundState],
    current_round_id: &str,
    current_node_id: &str,
    current_attempt_id: &str,
) -> Vec<TraceRef> {
    let mut refs = Vec::new();
    for round in rounds {
        let mut trace = round.trace.clone();
        trace.sort_by_key(|step| step.sequence);
        for step in trace {
            if round.id == current_round_id
                && step.node_id == current_node_id
                && step.attempt_id == current_attempt_id
            {
                return refs;
            }
            refs.push(TraceRef {
                round_id: round.id.clone(),
                step,
            });
        }
    }
    refs
}

fn branch_kind_for_node(node: &NodeDsl) -> String {
    if node.manual_check_enabled() {
        return "人工check".to_string();
    }
    match node {
        NodeDsl::Worker(worker)
            if worker.output.is_some() || worker.success_condition.is_some() =>
        {
            "节点输出检查".to_string()
        }
        _ => "普通".to_string(),
    }
}

fn output_contract_reason(_worker: &WorkerNode) -> Option<String> {
    None
}

fn artifact_preview(path: &Utf8PathBuf) -> Option<String> {
    let content = std::fs::read_to_string(path.as_std_path()).ok()?;
    const LIMIT: usize = 2048;
    if content.len() > LIMIT {
        let preview = content.chars().take(LIMIT).collect::<String>();
        Some(format!(
            "{}\n... preview omitted; read the file if needed",
            preview
        ))
    } else {
        Some(content)
    }
}

fn output_artifact_for_predecessor(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    trace: &RoundTraceStep,
    node_dsl: &NodeDsl,
) -> Option<PromptArtifactRef> {
    let artifact = match node_dsl {
        NodeDsl::Worker(worker) => worker
            .output
            .as_ref()
            .map(|output| output.artifact.as_str()),
        NodeDsl::AiDynamic(_) => None,
    }?;
    let path = app.paths.artifact_file(
        task_id,
        run_id,
        round_id,
        &trace.node_id,
        &trace.attempt_id,
        artifact,
    );
    Some(PromptArtifactRef {
        name: artifact.to_string(),
        preview: path.exists().then(|| artifact_preview(&path)).flatten(),
        path,
    })
}

fn build_predecessor_contexts(
    app: &App,
    task_id: &str,
    run_id: &str,
    current_round: &RoundState,
    current_node_id: &str,
    current_attempt_id: &str,
    workflow: &ValidatedWorkflow,
) -> Vec<PromptPredecessorContext> {
    let rounds = load_rounds_through_current(app, task_id, run_id, current_round);
    let traces = flatten_trace_until_current(
        &rounds,
        &current_round.id,
        current_node_id,
        current_attempt_id,
    );

    traces
        .iter()
        .enumerate()
        .filter_map(|(index, trace_ref)| {
            let node_dsl = workflow.get_node(&trace_ref.step.node_id)?;
            let node = read_json::<NodeState>(&app.paths.node_file(
                task_id,
                run_id,
                &trace_ref.round_id,
                &trace_ref.step.node_id,
                &trace_ref.step.attempt_id,
            ))
            .ok();
            let next = traces.get(index + 1);
            let branch_direction = next
                .and_then(|next| next.step.edge_outcome.clone())
                .or_else(|| {
                    if trace_ref.round_id == current_round.id {
                        current_round
                            .trace
                            .iter()
                            .find(|step| {
                                step.node_id == current_node_id
                                    && step.attempt_id == current_attempt_id
                                    && step.from_node_id.as_deref()
                                        == Some(trace_ref.step.node_id.as_str())
                            })
                            .and_then(|step| step.edge_outcome.clone())
                    } else {
                        None
                    }
                });
            let branch_reason = match node_dsl {
                NodeDsl::Worker(worker) => output_contract_reason(worker),
                NodeDsl::AiDynamic(_) => None,
            };
            Some(PromptPredecessorContext {
                round_id: trace_ref.round_id.clone(),
                node_id: trace_ref.step.node_id.clone(),
                attempt_id: trace_ref.step.attempt_id.clone(),
                node_type: format!("{:?}", node_dsl.node_type()).to_ascii_lowercase(),
                branch_kind: branch_kind_for_node(node_dsl),
                outcome: node
                    .and_then(|node| node.outcome)
                    .map(|outcome| format!("{:?}", outcome).to_ascii_lowercase()),
                branch_direction,
                output_artifact: output_artifact_for_predecessor(
                    app,
                    task_id,
                    run_id,
                    &trace_ref.round_id,
                    &trace_ref.step,
                    node_dsl,
                ),
                branch_reason,
            })
        })
        .collect()
}

pub(crate) fn build_worker_invocation(
    app: &App,
    task_id: &str,
    run_id: &str,
    round: &RoundState,
    attempt_id: &str,
    workflow: &ValidatedWorkflow,
    node_id: &str,
    session_mode: SessionMode,
    continue_ref: Option<serde_json::Value>,
    resume_prompt: Option<String>,
    resume_prompt_id: Option<String>,
    resume_prompt_visibility: PromptVisibility,
) -> Result<WorkerInvocation> {
    let round_id = round.id.as_str();
    let node_dsl = workflow.get_node(node_id).expect("validated node exists");
    let (
        profile,
        permission_mode,
        model,
        output_contract,
        task_instruction,
        invocation_kind,
        cold_artifacts,
        cold_attachments,
    ) = match node_dsl {
        NodeDsl::Worker(worker) => (
            worker.profile.clone(),
            worker.permission_mode.clone(),
            worker.model.clone(),
            worker_output_contract(worker),
            worker_task_instruction(worker),
            InvocationKind::WorkerGeneric,
            Vec::new(),
            Vec::new(),
        ),
        NodeDsl::AiDynamic(_) => {
            bail!("ai-dynamic nodes must be executed by the dynamic orchestrator")
        }
    };

    let profile_content = profile
        .as_deref()
        .map(|id| app.profile_show(id).map(|profile| profile.content))
        .transpose()?;

    let runtime_context =
        runtime_prompt_context(app, task_id, run_id, round_id, node_id, attempt_id);
    let predecessors =
        build_predecessor_contexts(app, task_id, run_id, round, node_id, attempt_id, workflow);

    let input_attachment_paths: Vec<String> = {
        let inputs_dir = app.paths.task_dir(task_id).join("authoring").join("inputs");
        if inputs_dir.exists() {
            std::fs::read_dir(inputs_dir.as_std_path())
                .map(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                        .map(|e| e.path().to_string_lossy().to_string())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    };

    Ok(WorkerInvocation {
        invocation_kind,
        profile,
        profile_content,
        requirement_path: Some(app.paths.requirement_file(task_id)),
        requirement_text: None,
        workspace_dir: app.paths.repo_root.clone(),
        attempt_dir: runtime_context.attempt_dir.clone(),
        output_contract,
        runtime_context,
        predecessors,
        extra_system_sections: Vec::new(),
        task_instruction,
        session_mode,
        permission_mode,
        model,
        continue_ref,
        resume_prompt,
        resume_prompt_id,
        resume_prompt_visibility,
        stream_mode: StreamMode::StreamJson,
        log_prompts: app.config.log_prompts,
        log_provider_command: app.config.log_provider_command,
        attachments_dir: matches!(node_dsl, NodeDsl::Worker(_)).then(|| {
            app.paths
                .attachments_dir(task_id, run_id, round_id, node_id, attempt_id)
        }),
        cold_artifacts,
        cold_attachments,
        input_attachment_paths,
    })
}

pub(crate) fn execute_ai_node(
    app: &App,
    task_id: &str,
    run_id: &str,
    round: &RoundState,
    attempt_id: &str,
    workflow: &ValidatedWorkflow,
    node_id: &str,
    node: NodeState,
    session_mode: SessionMode,
    continue_ref: Option<serde_json::Value>,
    resume_prompt: Option<String>,
    resume_prompt_id: Option<String>,
    resume_prompt_visibility: PromptVisibility,
) -> Result<NodeState> {
    let round_id = round.id.as_str();
    let invocation = build_worker_invocation(
        app,
        task_id,
        run_id,
        round,
        attempt_id,
        workflow,
        node_id,
        session_mode,
        continue_ref,
        resume_prompt,
        resume_prompt_id,
        resume_prompt_visibility,
    )?;

    progress(&format!(
        "calling provider for {}/{}/{}",
        round_id, node_id, attempt_id
    ));
    progress(&format!(
        "raw stream file: {}",
        app.paths
            .raw_stream_file(task_id, run_id, round_id, node_id, attempt_id)
    ));
    let provider_id = node
        .resolved_config
        .get("provider")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow!("node `{node_id}` is missing resolved provider"))?;
    tracing::debug!(task_id, run_id, round_id, node_id, attempt_id, provider_id, stage = ?ProgressStage::CallingProvider, "calling provider");
    let live_update_context = AcpLiveEventContext {
        task_id: task_id.to_string(),
        run_id: run_id.to_string(),
        round_id: round_id.to_string(),
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        outer_node_id: None,
        outer_attempt_id: None,
    };
    let attempt_dir_for_index = invocation.attempt_dir.clone();
    let live_update = app.acp_live_update_for(live_update_context);
    let result = app
        .provider_for_id(provider_id)?
        .run_worker_with_live_update(
            invocation,
            live_update.as_ref().map(|callback| callback as _),
        )?;

    // Fire-and-forget: index this attempt for cross-session search
    let ctx = AttemptIndexContext {
        task_id: task_id.to_string(),
        run_id: run_id.to_string(),
        round_id: round_id.to_string(),
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        outer_node_id: None,
        outer_attempt_id: None,
    };
    std::thread::spawn(move || {
        index_attempt_with_retry(&attempt_dir_for_index, &ctx);
    });

    progress(&format!(
        "normalizing artifact for {}/{}/{}",
        round_id, node_id, attempt_id
    ));
    tracing::debug!(task_id, run_id, round_id, node_id, attempt_id, stage = ?ProgressStage::NormalizingArtifact, "normalizing provider result");
    finalize_ai_attempt(
        app, task_id, run_id, round_id, attempt_id, node_id, node, result,
    )
}

fn evaluate_json_success_condition(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node: &NodeState,
    artifact_name: &str,
) -> Result<Option<NodeOutcome>> {
    let artifact_path = app.paths.artifact_file(
        task_id,
        run_id,
        round_id,
        &node.node_id,
        &node.attempt_id,
        artifact_name,
    );
    let content = std::fs::read_to_string(artifact_path.as_std_path())?;
    let Ok(value) = parse_json_artifact(&content) else {
        return Ok(Some(NodeOutcome::Invalid));
    };

    if let Some(schema) = node.resolved_config.get("outputSchema") {
        if !matches_simple_schema(&value, schema)? {
            return Ok(Some(NodeOutcome::Invalid));
        }
    }

    if let Some(expression) = node
        .resolved_config
        .get("successConditionExpression")
        .and_then(|value| value.as_str())
    {
        return evaluate_json_expression(&value, expression).map(|success| {
            Some(if success {
                NodeOutcome::Success
            } else {
                NodeOutcome::Failure
            })
        });
    }

    let Some(path) = node
        .resolved_config
        .get("successConditionPath")
        .and_then(|value| value.as_str())
    else {
        return Ok(None);
    };
    let Some(expected) = node.resolved_config.get("successConditionEquals") else {
        return Ok(Some(NodeOutcome::Invalid));
    };
    let Some(cursor) = select_json_path(&value, path) else {
        return Ok(Some(NodeOutcome::Invalid));
    };
    Ok(Some(if json_values_equal(cursor, expected) {
        NodeOutcome::Success
    } else {
        NodeOutcome::Failure
    }))
}

fn select_json_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let segments = parse_json_path(path).ok()?;
    let mut cursor = value;
    for segment in segments {
        cursor = match segment {
            JsonPathSegment::Key(key) => cursor.get(key)?,
            JsonPathSegment::Index(index) => cursor.as_array()?.get(index)?,
        };
    }
    Some(cursor)
}

fn evaluate_json_expression(value: &serde_json::Value, expression: &str) -> Result<bool> {
    const OPERATORS: [&str; 6] = [">=", "<=", "!=", "==", ">", "<"];
    let trimmed = expression.trim();
    let (operator, left, right) = OPERATORS
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
    let Some(actual) = select_json_path(value, left) else {
        return Ok(false);
    };
    let expected = parse_expression_value(right)?;
    compare_json_values(actual, &expected, operator)
}

fn parse_expression_value(value: &str) -> Result<serde_json::Value> {
    serde_json::from_str(value)
        .or_else(|_| serde_json::from_str(&format!("\"{}\"", value.trim_matches('"'))))
        .map_err(|error| anyhow!("invalid success expression value `{value}`: {error}"))
}

fn compare_json_values(
    actual: &serde_json::Value,
    expected: &serde_json::Value,
    operator: &str,
) -> Result<bool> {
    Ok(match operator {
        "==" => json_values_equal(actual, expected),
        "!=" => !json_values_equal(actual, expected),
        ">" => json_number(actual)? > json_number(expected)?,
        ">=" => json_number(actual)? >= json_number(expected)?,
        "<" => json_number(actual)? < json_number(expected)?,
        "<=" => json_number(actual)? <= json_number(expected)?,
        _ => bail!("unsupported success expression operator: {operator}"),
    })
}

fn json_values_equal(actual: &serde_json::Value, expected: &serde_json::Value) -> bool {
    if actual == expected {
        return true;
    }
    match (actual, expected) {
        (serde_json::Value::Bool(left), serde_json::Value::String(right))
        | (serde_json::Value::String(right), serde_json::Value::Bool(left)) => {
            right.eq_ignore_ascii_case(&left.to_string())
        }
        (serde_json::Value::Number(_), serde_json::Value::String(_))
        | (serde_json::Value::String(_), serde_json::Value::Number(_)) => json_number(actual)
            .and_then(|left| json_number(expected).map(|right| left == right))
            .unwrap_or(false),
        (serde_json::Value::Null, serde_json::Value::String(right))
        | (serde_json::Value::String(right), serde_json::Value::Null) => {
            right.eq_ignore_ascii_case("null")
        }
        _ => false,
    }
}

fn json_number(value: &serde_json::Value) -> Result<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
        .ok_or_else(|| anyhow!("success expression comparison requires numbers"))
}

fn matches_simple_schema(value: &serde_json::Value, schema: &serde_json::Value) -> Result<bool> {
    match schema {
        serde_json::Value::String(type_name) => Ok(matches_simple_type(value, type_name)),
        serde_json::Value::Object(schema_object) => {
            let Some(value_object) = value.as_object() else {
                return Ok(false);
            };
            for (key, field_schema) in schema_object {
                let Some(field_value) = value_object.get(key) else {
                    return Ok(false);
                };
                if !matches_simple_schema(field_value, field_schema)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        serde_json::Value::Array(items) => {
            let Some(value_array) = value.as_array() else {
                return Ok(false);
            };
            let Some(item_schema) = items.first() else {
                return Ok(true);
            };
            for item in value_array {
                if !matches_simple_schema(item, item_schema)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        serde_json::Value::Null => Ok(true),
        _ => Ok(true),
    }
}

fn matches_simple_type(value: &serde_json::Value, type_name: &str) -> bool {
    match type_name.trim().to_ascii_lowercase().as_str() {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "bool" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "null" => value.is_null(),
        _ => true,
    }
}

pub(crate) fn finalize_ai_attempt(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    attempt_id: &str,
    node_id: &str,
    mut node: NodeState,
    result: ProviderRunResult,
) -> Result<NodeState> {
    node.finished_at = Some(now_rfc3339_like());
    if let Some(seed) = result.worker_ref_seed.clone() {
        let worker_ref = WorkerRefState {
            version: VERSION.to_string(),
            provider: seed.provider,
            mode: seed.mode,
            supports_open_session: seed.supports_open_session,
            supports_continue_session: seed.supports_continue_session,
            continue_ref: seed.continue_ref,
            open_command: seed.open_command,
        };
        validate_worker_ref_state(&worker_ref)?;
        write_json(
            &app.paths
                .worker_ref_file(task_id, run_id, round_id, node_id, attempt_id),
            &worker_ref,
        )?;
    }

    match result.status {
        ProviderRunStatus::Success => {
            if let Some(payload) = result.result_payload {
                if let Some(output_artifact) = payload.output_artifact {
                    let artifact_path = app.paths.artifact_file(
                        task_id,
                        run_id,
                        round_id,
                        node_id,
                        attempt_id,
                        &output_artifact.name,
                    );
                    std::fs::create_dir_all(
                        app.paths
                            .artifacts_dir(task_id, run_id, round_id, node_id, attempt_id)
                            .as_std_path(),
                    )?;
                    std::fs::write(artifact_path.as_std_path(), output_artifact.content)?;
                }
            }

            let needs_output_artifact = node.resolved_config.contains_key("outputArtifact");
            let expected_artifact = node
                .resolved_config
                .get("outputArtifact")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let has_artifact = expected_artifact.as_ref().is_some_and(|artifact| {
                app.paths
                    .artifact_file(task_id, run_id, round_id, node_id, attempt_id, artifact)
                    .exists()
            });
            node.status = RunStatus::Completed;
            node.outcome = Some(if needs_output_artifact && !has_artifact {
                NodeOutcome::Invalid
            } else {
                expected_artifact
                    .as_deref()
                    .map(|artifact| {
                        evaluate_json_success_condition(
                            app, task_id, run_id, round_id, &node, artifact,
                        )
                    })
                    .transpose()?
                    .flatten()
                    .unwrap_or(NodeOutcome::Success)
            });
        }
        ProviderRunStatus::Failure => {
            node.status = RunStatus::Completed;
            node.outcome = Some(NodeOutcome::Failure);
        }
        ProviderRunStatus::Interrupted => {
            node.status = RunStatus::Paused;
            node.outcome = None;
            node.pause_reason = Some(PauseReason::ProcessInterrupted);
        }
        ProviderRunStatus::WaitingForUserInput => {
            node.status = RunStatus::Paused;
            node.outcome = None;
            node.pause_reason = Some(PauseReason::WaitingForUserInput);
        }
        ProviderRunStatus::PermissionRequested => {
            node.status = RunStatus::Paused;
            node.outcome = None;
            node.pause_reason = Some(PauseReason::PermissionRequested);
        }
    }
    validate_node_state(&node)?;
    Ok(node)
}

pub(crate) fn re_evaluate_attempt(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    mut node: NodeState,
) -> Result<NodeState> {
    let artifact_name = node
        .resolved_config
        .get("outputArtifact")
        .and_then(|value| value.as_str())
        .map(str::to_string);

    if let Some(artifact_name) = artifact_name {
        let path = app.paths.artifact_file(
            task_id,
            run_id,
            round_id,
            &node.node_id,
            &node.attempt_id,
            &artifact_name,
        );
        if !path.exists() {
            node.status = RunStatus::Completed;
            node.outcome = Some(NodeOutcome::Invalid);
            validate_node_state(&node)?;
            write_json(
                &app.paths
                    .node_file(task_id, run_id, round_id, &node.node_id, &node.attempt_id),
                &node,
            )?;
            return Ok(node);
        }

        node.outcome = Some(
            evaluate_json_success_condition(app, task_id, run_id, round_id, &node, &artifact_name)?
                .unwrap_or(NodeOutcome::Success),
        );
    }

    node.status = RunStatus::Completed;
    node.finished_at = Some(now_rfc3339_like());
    validate_node_state(&node)?;
    write_json(
        &app.paths
            .node_file(task_id, run_id, round_id, &node.node_id, &node.attempt_id),
        &node,
    )?;
    Ok(node)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_nested_array_json_path() {
        let value = serde_json::json!({ "xx": { "yy": [{ "zz": true }] } });
        assert_eq!(
            select_json_path(&value, "$.xx.yy[0].zz"),
            Some(&serde_json::Value::Bool(true))
        );
    }

    #[test]
    fn evaluates_no_space_and_quoted_boolean_expressions() {
        let value = serde_json::json!({ "result": true });
        assert!(
            evaluate_json_expression(&value, "$.result==true").expect("expression should evaluate")
        );
        assert!(
            evaluate_json_expression(&value, "$.result == \"true\"")
                .expect("expression should evaluate")
        );
    }

    #[test]
    fn matches_simplified_schema() {
        let value = serde_json::json!({ "reason": "ok", "result": true, "extra": 1 });
        let schema = serde_json::json!({ "reason": "String", "result": "boolean" });
        assert!(matches_simple_schema(&value, &schema).expect("schema should match"));
    }

    #[test]
    fn rejects_missing_simplified_schema_field() {
        let value = serde_json::json!({ "reason": "ok" });
        let schema = serde_json::json!({ "reason": "String", "result": "boolean" });
        assert!(!matches_simple_schema(&value, &schema).expect("schema should not match"));
    }
}
