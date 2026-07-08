use anyhow::{Result, anyhow};
use serde::de::DeserializeOwned;

const JSON_ARTIFACT_OUTPUT_SEARCH_LIMIT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonArtifactSpan {
    pub json_text: String,
    pub start: usize,
    pub end: usize,
    pub json_start: usize,
    pub json_end: usize,
    pub fenced: bool,
    pub parse_status: &'static str,
}

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

pub fn json_artifact_span(content: &str) -> Option<JsonArtifactSpan> {
    json_artifact_spans(content)
        .into_iter()
        .filter(|span| span.parse_status == "valid")
        .max_by_key(|span| span.start)
}

pub fn json_artifact_display_span(content: &str) -> Option<JsonArtifactSpan> {
    json_artifact_spans(content)
        .into_iter()
        .max_by_key(|span| span.start)
}

fn json_artifact_spans(content: &str) -> Vec<JsonArtifactSpan> {
    let fenced_spans = fenced_json_spans(content);
    let mut spans = fenced_spans.clone();
    spans.extend(
        raw_json_object_spans(content)
            .into_iter()
            .filter(|span| !is_inside_fenced_span(span, &fenced_spans)),
    );
    let existing_spans = spans.clone();
    spans.extend(
        raw_json_like_spans(content)
            .into_iter()
            .filter(|span| !is_inside_fenced_span(span, &fenced_spans))
            .filter(|span| !has_same_display_span(span, &existing_spans)),
    );
    spans
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
    json_artifact_span(content).map(|span| span.json_text)
}

fn raw_json_object_spans(content: &str) -> Vec<JsonArtifactSpan> {
    if serde_json::from_str::<serde_json::Value>(content).is_ok() {
        return vec![JsonArtifactSpan {
            json_text: content.to_string(),
            start: 0,
            end: content.len(),
            json_start: 0,
            json_end: content.len(),
            fenced: false,
            parse_status: "valid",
        }];
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

    spans
        .into_iter()
        .filter_map(|(start, end)| {
            let candidate = &content[start..end];
            serde_json::from_str::<serde_json::Value>(candidate)
                .ok()
                .map(|_| JsonArtifactSpan {
                    json_text: candidate.to_string(),
                    start,
                    end,
                    json_start: start,
                    json_end: end,
                    fenced: false,
                    parse_status: "valid",
                })
        })
        .collect()
}

fn fenced_json_spans(content: &str) -> Vec<JsonArtifactSpan> {
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    while let Some(open_rel) = content[cursor..].find("```") {
        let open = cursor + open_rel;
        let info_start = open + 3;
        let Some(line_end_rel) = content[info_start..].find('\n') else {
            break;
        };
        let line_end = info_start + line_end_rel;
        let info = content[info_start..line_end].trim().to_ascii_lowercase();
        let body_start = line_end + 1;
        let (close, display_end) = match content[body_start..].find("```") {
            Some(close_rel) => {
                let close = body_start + close_rel;
                (close, close + 3)
            }
            None => (content.len(), content.len()),
        };
        let body = &content[body_start..close];

        if info.is_empty() || info == "json" || info.starts_with("json ") {
            let leading = body.len() - body.trim_start().len();
            let trailing = body.trim_end().len();
            let json_start = body_start + leading;
            let json_end = body_start + trailing;
            let candidate = &content[json_start..json_end];
            if looks_like_json_object(candidate) {
                let parse_status = if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
                    "valid"
                } else {
                    "invalid"
                };
                spans.push(JsonArtifactSpan {
                    json_text: candidate.to_string(),
                    start: open,
                    end: display_end,
                    json_start,
                    json_end,
                    fenced: true,
                    parse_status,
                });
            }
        }

        if display_end >= content.len() {
            break;
        }
        cursor = display_end;
    }
    spans
}

fn raw_json_like_spans(content: &str) -> Vec<JsonArtifactSpan> {
    content
        .char_indices()
        .filter(|(_, ch)| *ch == '{')
        .filter_map(|(start, _)| {
            let end = raw_json_like_end(content, start);
            let candidate = content[start..end].trim_end();
            let end = start + candidate.len();
            if !looks_like_json_object(candidate)
                || serde_json::from_str::<serde_json::Value>(candidate).is_ok()
            {
                return None;
            }
            Some(JsonArtifactSpan {
                json_text: candidate.to_string(),
                start,
                end,
                json_start: start,
                json_end: end,
                fenced: false,
                parse_status: "invalid",
            })
        })
        .collect()
}

fn raw_json_like_end(content: &str, start: usize) -> usize {
    content[start..]
        .rfind('}')
        .map(|relative| start + relative + 1)
        .unwrap_or(content.len())
}

fn looks_like_json_object(candidate: &str) -> bool {
    let candidate = candidate.trim();
    candidate.starts_with('{') && candidate.contains('"') && candidate.contains(':')
}

fn is_inside_fenced_span(span: &JsonArtifactSpan, fenced_spans: &[JsonArtifactSpan]) -> bool {
    fenced_spans
        .iter()
        .any(|fenced| fenced.fenced && span.start >= fenced.start && span.end <= fenced.end)
}

fn has_same_display_span(span: &JsonArtifactSpan, spans: &[JsonArtifactSpan]) -> bool {
    spans
        .iter()
        .any(|existing| existing.start == span.start && existing.end == span.end)
}

#[cfg(test)]
mod tests {
    use super::{
        json_artifact_display_span, json_artifact_span, json_artifact_text_from_outputs,
        parse_json_artifact,
    };

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

    #[test]
    fn extracts_fenced_json_span() {
        let content = "hello\n```json\n{\"a\":\"b\"}\n```";
        let span = json_artifact_span(content).expect("span should parse");
        assert!(span.fenced);
        assert_eq!(span.json_text, "{\"a\":\"b\"}");
        assert_eq!(
            &content[span.start..span.end],
            "```json\n{\"a\":\"b\"}\n```"
        );
    }

    #[test]
    fn extracts_bare_json_span_with_prefix_text() {
        let content = "hello\n{\"a\":\"b\"}";
        let span = json_artifact_span(content).expect("span should parse");
        assert!(!span.fenced);
        assert_eq!(span.json_text, "{\"a\":\"b\"}");
        assert_eq!(&content[span.start..span.end], "{\"a\":\"b\"}");
    }

    #[test]
    fn extracts_only_json_span() {
        let content = "{\"a\":\"b\"}";
        let span = json_artifact_span(content).expect("span should parse");
        assert_eq!(span.start, 0);
        assert_eq!(span.end, content.len());
        assert_eq!(span.json_text, content);
    }

    #[test]
    fn does_not_extract_when_json_is_missing() {
        assert!(json_artifact_span("hello world").is_none());
    }

    #[test]
    fn handles_escaped_strings_and_nested_objects() {
        let content = "prefix {\"a\":\"brace } in string\",\"b\":{\"c\":true}} suffix";
        let span = json_artifact_span(content).expect("span should parse");
        assert_eq!(
            span.json_text,
            "{\"a\":\"brace } in string\",\"b\":{\"c\":true}}"
        );
    }

    #[test]
    fn display_span_extracts_invalid_fenced_json() {
        let content = "hello\n```json\n{\"a\":\"unterminated}\n```";
        let span = json_artifact_display_span(content).expect("span should parse");
        assert!(span.fenced);
        assert_eq!(span.parse_status, "invalid");
        assert_eq!(span.json_text, "{\"a\":\"unterminated}");
        assert_eq!(
            &content[span.start..span.end],
            "```json\n{\"a\":\"unterminated}\n```"
        );
        assert!(json_artifact_span(content).is_none());
    }

    #[test]
    fn display_span_extracts_unclosed_invalid_fenced_json() {
        let content = "hello\n```json\n{\"a\":\"unterminated}";
        let span = json_artifact_display_span(content).expect("span should parse");
        assert!(span.fenced);
        assert_eq!(span.parse_status, "invalid");
        assert_eq!(span.end, content.len());
        assert_eq!(
            &content[span.start..span.end],
            "```json\n{\"a\":\"unterminated}"
        );
    }

    #[test]
    fn display_span_extracts_invalid_bare_json() {
        let content = "hello\n{\"a\":\"unterminated}";
        let span = json_artifact_display_span(content).expect("span should parse");
        assert!(!span.fenced);
        assert_eq!(span.parse_status, "invalid");
        assert_eq!(span.json_text, "{\"a\":\"unterminated}");
        assert!(json_artifact_span(content).is_none());
    }

    #[test]
    fn runtime_json_text_extraction_ignores_invalid_json() {
        let outputs = vec!["hello\n{\"a\":\"unterminated}".to_string()];

        assert_eq!(
            json_artifact_text_from_outputs(&outputs, "{\"result\":false}"),
            Some("{\"result\":false}".to_string())
        );
    }
}
