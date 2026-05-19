use crate::domain::CommandStatus;
use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecPlanArtifact {
    pub version: String,
    pub commands: Vec<ExecPlanCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecPlanCommand {
    pub id: String,
    pub run: String,
    pub purpose: String,
    pub cwd: Option<String>,
    pub timeout_sec: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResultArtifact {
    pub version: String,
    pub status: ExecResultStatus,
    pub commands: Vec<ExecCommandResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecResultStatus {
    Success,
    Failure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecCommandResult {
    pub id: String,
    pub exit_code: Option<i32>,
    pub status: CommandStatus,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub duration_ms: Option<u64>,
    pub timed_out: Option<bool>,
    pub stdout_path: Option<String>,
    pub stderr_path: Option<String>,
}

const JSON_ARTIFACT_OUTPUT_SEARCH_LIMIT: usize = 5;

pub fn artifact_uses_json_output(name: &str) -> bool {
    matches!(name, "exec-plan" | "exec-result") || name.ends_with("-result")
}

pub fn json_artifact_text_from_outputs(outputs: &[String], fallback: &str) -> Option<String> {
    outputs
        .iter()
        .rev()
        .filter(|output| !output.trim().is_empty())
        .take(JSON_ARTIFACT_OUTPUT_SEARCH_LIMIT)
        .find_map(|output| json_object_text(output))
        .or_else(|| json_object_text(fallback))
}

pub fn parse_json_artifact<T: DeserializeOwned>(content: &str) -> Result<T> {
    match serde_json::from_str(content) {
        Ok(value) => Ok(value),
        Err(first_error) => {
            let json = json_object_text(content)
                .ok_or_else(|| anyhow!("failed to parse JSON artifact: {first_error}"))?;
            serde_json::from_str(&json).map_err(Into::into)
        }
    }
}

fn json_object_text(content: &str) -> Option<String> {
    if serde_json::from_str::<serde_json::Value>(content).is_ok() {
        return Some(content.to_string());
    }

    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut spans = Vec::new();

    for (index, ch) in content.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(index);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    if let Some(start_index) = start.take() {
                        spans.push((start_index, index + ch.len_utf8()));
                    }
                }
            }
            _ => {}
        }
    }

    spans.into_iter().rev().find_map(|(start, end)| {
        let candidate = &content[start..end];
        serde_json::from_str::<serde_json::Value>(candidate)
            .ok()
            .map(|_| candidate.to_string())
    })
}

pub fn validate_exec_plan(plan: &ExecPlanArtifact) -> Result<()> {
    ensure!(
        plan.version == "0.1",
        "unsupported exec-plan version: {}",
        plan.version
    );
    ensure!(
        !plan.commands.is_empty(),
        "exec-plan commands cannot be empty"
    );

    let mut ids = std::collections::HashSet::new();
    for command in &plan.commands {
        ensure!(!command.id.trim().is_empty(), "command id cannot be empty");
        ensure!(
            ids.insert(command.id.clone()),
            "duplicate command id: {}",
            command.id
        );
        ensure!(
            !command.run.trim().is_empty(),
            "command run cannot be empty"
        );
        ensure!(
            !command.purpose.trim().is_empty(),
            "command purpose cannot be empty"
        );
        if let Some(timeout_sec) = command.timeout_sec {
            ensure!(timeout_sec > 0, "timeoutSec must be positive");
        }
    }

    Ok(())
}

pub fn validate_exec_result(result: &ExecResultArtifact) -> Result<()> {
    ensure!(
        result.version == "0.1",
        "unsupported exec-result version: {}",
        result.version
    );
    ensure!(
        !result.commands.is_empty(),
        "exec-result commands cannot be empty"
    );

    let has_failure = result
        .commands
        .iter()
        .any(|command| command.status == CommandStatus::Failure);
    let expected = if has_failure {
        ExecResultStatus::Failure
    } else {
        ExecResultStatus::Success
    };
    ensure!(
        result.status == expected,
        "exec-result top-level status does not match command aggregation"
    );

    for command in &result.commands {
        ensure!(
            !command.id.trim().is_empty(),
            "exec-result command id cannot be empty"
        );
        if command.status == CommandStatus::Skipped {
            ensure!(
                command.exit_code.is_none(),
                "skipped command must not include exitCode"
            );
        } else {
            ensure!(
                command.exit_code.is_some(),
                "executed command must include exitCode"
            );
            ensure!(
                command
                    .stdout_path
                    .as_deref()
                    .is_some_and(|value| !value.is_empty()),
                "executed command must include stdoutPath"
            );
            ensure!(
                command
                    .stderr_path
                    .as_deref()
                    .is_some_and(|value| !value.is_empty()),
                "executed command must include stderrPath"
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{json_artifact_text_from_outputs, parse_json_artifact};

    #[derive(Debug, serde::Deserialize)]
    struct WorkerResultArtifact {
        result: bool,
        reason: String,
    }

    #[test]
    fn parses_json_artifact_from_text_with_preamble() {
        let artifact: WorkerResultArtifact = parse_json_artifact(
            r#"checking files...
{"result":false,"reason":"missing class"}"#,
        )
        .unwrap();

        assert!(!artifact.result);
        assert_eq!(artifact.reason, "missing class");
    }

    #[test]
    fn selects_json_from_recent_output_segments() {
        let outputs = vec![
            "I will inspect files.".to_string(),
            r#"{"result":false,"reason":"missing class"}"#.to_string(),
            "Ignored trailing explanation.".to_string(),
        ];

        let content = json_artifact_text_from_outputs(&outputs, "").unwrap();
        let artifact: WorkerResultArtifact = parse_json_artifact(&content).unwrap();

        assert_eq!(artifact.reason, "missing class");
    }
}
