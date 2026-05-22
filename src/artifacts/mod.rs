use anyhow::{Result, anyhow};
use serde::de::DeserializeOwned;

const JSON_ARTIFACT_OUTPUT_SEARCH_LIMIT: usize = 5;

pub fn artifact_uses_json_output(name: &str) -> bool {
    name.ends_with("-result")
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

#[cfg(test)]
mod tests {
    use super::{json_artifact_text_from_outputs, parse_json_artifact};

    #[derive(Debug, serde::Deserialize)]
    struct WorkerResultArtifact {
        result: bool,
        reason: String,
    }

    #[test]
    fn extracts_trailing_json_from_text() {
        let artifact: WorkerResultArtifact =
            parse_json_artifact("analysis text\n{\"result\":true,\"reason\":\"ok\"}")
                .expect("json artifact should parse");

        assert!(artifact.result);
        assert_eq!(artifact.reason, "ok");
    }

    #[test]
    fn extracts_json_from_outputs_before_fallback() {
        let outputs = vec![
            "noise".to_string(),
            "{\"result\":false}".to_string(),
            "{\"result\":true}".to_string(),
        ];

        assert_eq!(
            json_artifact_text_from_outputs(&outputs, "{\"result\":false}"),
            Some("{\"result\":true}".to_string())
        );
    }
}
