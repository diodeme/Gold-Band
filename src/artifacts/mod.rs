use anyhow::{Result, anyhow};
use serde::de::DeserializeOwned;

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

pub fn json_artifact_text(content: &str) -> Option<String> {
    json_object_text(content)
}

pub fn json_artifact_span(content: &str) -> Option<JsonArtifactSpan> {
    json_artifact_spans(content)
        .into_iter()
        .filter(|span| span.parse_status == "valid")
        .max_by_key(|span| span.start)
}

pub fn json_artifact_display_span(content: &str) -> Option<JsonArtifactSpan> {
    let spans = json_artifact_spans(content);
    spans
        .iter()
        .filter(|span| span.parse_status == "valid")
        .max_by_key(|span| span.start)
        .cloned()
        .or_else(|| spans.into_iter().max_by_key(|span| span.start))
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
    let top_level_starts = top_level_object_starts(content);
    spans.extend(
        raw_json_like_spans(content)
            .into_iter()
            // A `{` nested inside another object is part of that object; selecting
            // it would replace the model's root output with one of its fields.
            .filter(|span| top_level_starts.contains(&span.start))
            .filter(|span| !is_inside_fenced_span(span, &fenced_spans))
            .filter(|span| !has_same_display_span(span, &existing_spans)),
    );
    spans
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JsonSyntaxCategory {
    /// Input ended early, e.g. an unclosed object, array or string.
    Eof,
    /// Unexpected character, missing separator or trailing characters.
    Syntax,
}

/// Structured syntax failure of the selected JSON artifact candidate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("JSON artifact syntax error ({category:?}) at line {line} column {column}: {message}")]
pub struct JsonArtifactSyntaxError {
    pub category: JsonSyntaxCategory,
    pub line: usize,
    pub column: usize,
    pub message: String,
    /// Text of the failing line that ends at the error position.
    pub snippet: String,
}

const JSON_SYNTAX_SNIPPET_CHARS: usize = 40;

impl JsonArtifactSyntaxError {
    fn from_serde(json: &str, error: &serde_json::Error) -> Option<Self> {
        let category = match error.classify() {
            serde_json::error::Category::Eof => JsonSyntaxCategory::Eof,
            serde_json::error::Category::Syntax => JsonSyntaxCategory::Syntax,
            serde_json::error::Category::Data | serde_json::error::Category::Io => return None,
        };
        Some(Self {
            category,
            line: error.line(),
            column: error.column(),
            message: error.to_string(),
            snippet: json_error_snippet(json, error.line(), error.column()),
        })
    }
}

fn json_error_snippet(json: &str, line: usize, column: usize) -> String {
    let line_text = json
        .split('\n')
        .nth(line.saturating_sub(1))
        .unwrap_or_default();
    let mut end = column.min(line_text.len());
    while !line_text.is_char_boundary(end) {
        end += 1;
    }
    let before = &line_text[..end];
    let skip = before
        .chars()
        .count()
        .saturating_sub(JSON_SYNTAX_SNIPPET_CHARS);
    before.chars().skip(skip).collect()
}

/// Parses a JSON artifact, tolerating code fences and surrounding prose.
///
/// Only the selected top-level candidate is parsed. Malformed candidates fail
/// with [`JsonArtifactSyntaxError`]; nested objects are never salvaged.
pub fn parse_json_artifact<T: DeserializeOwned>(content: &str) -> Result<T> {
    if serde_json::from_str::<serde_json::Value>(content).is_ok() {
        return serde_json::from_str(content).map_err(Into::into);
    }
    let json = json_artifact_display_span(content)
        .map(|span| span.json_text)
        .unwrap_or_else(|| content.trim().to_string());
    serde_json::from_str(&json).map_err(|error| {
        match JsonArtifactSyntaxError::from_serde(&json, &error) {
            Some(syntax) => anyhow!(syntax),
            None => anyhow!(error),
        }
    })
}

/// Byte offsets of `{` that open an object at brace depth 0, skipping strings.
fn top_level_object_starts(content: &str) -> std::collections::HashSet<usize> {
    let mut starts = std::collections::HashSet::new();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
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
                    starts.insert(index);
                }
                depth += 1;
            }
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    starts
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
            let candidate = body.trim();
            if !candidate.is_empty() && looks_like_json_object(candidate) {
                let leading = body.len() - body.trim_start().len();
                let json_start = body_start + leading;
                let json_end = json_start + candidate.len();
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
        JsonArtifactSyntaxError, JsonSyntaxCategory, json_artifact_display_span,
        json_artifact_span, json_artifact_text, parse_json_artifact,
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
    fn extracts_json_from_single_runtime_message() {
        assert_eq!(
            json_artifact_text("explanation\n{\"result\":true}"),
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
    fn display_span_ignores_streaming_fence_with_only_whitespace() {
        assert!(json_artifact_display_span("before\n```json\n \n").is_none());
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
    fn display_span_prefers_valid_outer_json_over_invalid_inner_suffix() {
        let content = "{\"next\":{\"node\":{\"workspace\":{\"mode\":\"main\"}}}}";
        let span = json_artifact_display_span(content).expect("span should parse");
        assert_eq!(span.parse_status, "valid");
        assert_eq!(span.start, 0);
        assert_eq!(span.end, content.len());
        assert_eq!(span.json_text, content);
    }

    // Model output that forgot one closing brace of a three-level object.
    const UNCLOSED_NESTED_ROOT: &str = "{\"version\":\"0.1\",\"next\":{\"type\":\"single\",\"node\":{\"id\":\"plan\",\"task\":\"t\"}}";

    #[test]
    fn display_span_selects_unclosed_root_instead_of_nested_object() {
        let span = json_artifact_display_span(UNCLOSED_NESTED_ROOT).expect("span should exist");
        assert_eq!(span.parse_status, "invalid");
        assert_eq!(span.start, 0);
        assert_eq!(span.json_text, UNCLOSED_NESTED_ROOT);
    }

    #[test]
    fn display_span_ignores_braces_in_prose_before_unclosed_root() {
        let content = format!("示例 {{\"k\" 写法: 1}} 结束\n{UNCLOSED_NESTED_ROOT}");
        let span = json_artifact_display_span(&content).expect("span should exist");
        assert_eq!(span.parse_status, "invalid");
        assert_eq!(span.json_text, UNCLOSED_NESTED_ROOT);
    }

    #[test]
    fn parse_reports_structured_eof_for_unclosed_root_instead_of_nested_object() {
        let content = format!("说明\n```json\n{UNCLOSED_NESTED_ROOT}\n```");
        let error = parse_json_artifact::<serde_json::Value>(&content)
            .expect_err("unclosed root must not be salvaged");
        let syntax = error
            .downcast_ref::<JsonArtifactSyntaxError>()
            .expect("syntax error should be structured");
        assert_eq!(syntax.category, JsonSyntaxCategory::Eof);
        assert_eq!(syntax.line, 1);
        assert_eq!(syntax.column, UNCLOSED_NESTED_ROOT.len());
        assert!(UNCLOSED_NESTED_ROOT.ends_with(&syntax.snippet));
    }

    #[test]
    fn parse_reports_structured_syntax_error_position() {
        let error = parse_json_artifact::<serde_json::Value>("{\"a\":1,\n\"b\" 2}")
            .expect_err("missing colon must fail");
        let syntax = error.downcast_ref::<JsonArtifactSyntaxError>().unwrap();
        assert_eq!(syntax.category, JsonSyntaxCategory::Syntax);
        assert_eq!(syntax.line, 2);
        assert_eq!(syntax.snippet, "\"b\" 2");
    }

    #[test]
    fn parse_keeps_data_errors_unstructured() {
        let error =
            parse_json_artifact::<WorkerResultArtifact>("{\"result\":\"yes\",\"reason\":\"ok\"}")
                .expect_err("type mismatch must fail");
        assert!(error.downcast_ref::<JsonArtifactSyntaxError>().is_none());
    }

    #[test]
    fn runtime_json_text_extraction_rejects_invalid_final_message() {
        assert_eq!(json_artifact_text("hello\n{\"a\":\"unterminated}"), None);
    }
}
