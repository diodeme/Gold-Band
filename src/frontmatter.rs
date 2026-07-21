use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontmatterDocument {
    pub fields: BTreeMap<String, String>,
    pub field_sources: BTreeMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone, Copy)]
pub struct FrontmatterUpdate<'a> {
    pub key: &'a str,
    pub value: &'a str,
    pub source: Option<&'a str>,
}

pub fn parse_frontmatter_document(content: &str) -> Result<FrontmatterDocument> {
    let content = strip_utf8_bom(content);
    let Some(parts) = split_frontmatter(content) else {
        bail!("document is missing front matter");
    };
    let fields = parse_frontmatter_fields(parts.frontmatter)?;
    let field_sources = parse_frontmatter_field_sources(parts.frontmatter, &fields);
    Ok(FrontmatterDocument {
        fields,
        field_sources,
        body: parts.body.to_string(),
    })
}

pub fn parse_optional_frontmatter_document(content: &str) -> Result<FrontmatterDocument> {
    let content = strip_utf8_bom(content);
    if !has_frontmatter_start(content) {
        return Ok(FrontmatterDocument {
            fields: BTreeMap::new(),
            field_sources: BTreeMap::new(),
            body: content.to_string(),
        });
    }
    if split_frontmatter(content).is_none() {
        return Ok(FrontmatterDocument {
            fields: BTreeMap::new(),
            field_sources: BTreeMap::new(),
            body: content.to_string(),
        });
    }
    parse_frontmatter_document(content)
}

pub fn render_frontmatter_document(updates: &[FrontmatterUpdate<'_>], body: &str) -> String {
    let line_ending = "\n";
    let mut output = String::from("---\n");
    for update in updates {
        output.push_str(&render_field(
            update.key,
            update.source.unwrap_or(update.value),
            None,
            line_ending,
        ));
    }
    output.push_str("---\n");
    output.push_str(body);
    output
}

pub fn update_frontmatter_document(
    content: &str,
    updates: &[FrontmatterUpdate<'_>],
    body: &str,
) -> Result<String> {
    let content = strip_utf8_bom(content);
    let Some(parts) = split_frontmatter(content) else {
        return Ok(render_frontmatter_document(updates, body));
    };
    let parsed_fields = parse_frontmatter_fields(parts.frontmatter)?;
    let original_sources = parse_frontmatter_field_sources(parts.frontmatter, &parsed_fields);
    let update_keys = updates
        .iter()
        .map(|update| update.key)
        .collect::<BTreeSet<_>>();
    let updates_by_key = updates
        .iter()
        .map(|update| (update.key, *update))
        .collect::<BTreeMap<_, _>>();
    let line_ending = detect_line_ending(content);
    let lines = frontmatter_lines(parts.frontmatter);
    let mut output = String::new();
    output.push_str("---");
    output.push_str(line_ending);

    let mut seen = BTreeSet::new();
    let mut index = 0;
    while index < lines.len() {
        if let Some(key) = top_level_key(&lines[index]) {
            if let Some(update) = updates_by_key.get(key) {
                let next = next_top_level_field_index(&lines, index + 1);
                let original_source = original_sources.get(key).map(String::as_str);
                let update_source = update.source.unwrap_or(update.value);
                if original_source == Some(update_source) {
                    for line in &lines[index..next] {
                        output.push_str(line);
                        output.push_str(line_ending);
                    }
                } else {
                    let original_style = scalar_style(&lines[index]);
                    output.push_str(&render_field(
                        key,
                        update_source,
                        original_style,
                        line_ending,
                    ));
                }
                seen.insert(key);
                index = next;
                continue;
            }
        }
        output.push_str(&lines[index]);
        output.push_str(line_ending);
        index += 1;
    }

    for update in updates {
        if !seen.contains(update.key) && update_keys.contains(update.key) {
            output.push_str(&render_field(
                update.key,
                update.source.unwrap_or(update.value),
                None,
                line_ending,
            ));
        }
    }

    output.push_str("---");
    output.push_str(line_ending);
    output.push_str(body);
    Ok(output)
}

struct FrontmatterParts<'a> {
    frontmatter: &'a str,
    body: &'a str,
}

fn strip_utf8_bom(content: &str) -> &str {
    content.strip_prefix('\u{FEFF}').unwrap_or(content)
}

fn has_frontmatter_start(content: &str) -> bool {
    content.starts_with("---\n") || content.starts_with("---\r\n")
}

fn split_frontmatter(content: &str) -> Option<FrontmatterParts<'_>> {
    let rest = content
        .strip_prefix("---\r\n")
        .or_else(|| content.strip_prefix("---\n"))?;
    let mut search_start = 0;

    loop {
        let relative = rest[search_start..].find("---")?;
        let delimiter_start = search_start + relative;
        let delimiter_end = delimiter_start + 3;
        let starts_line = delimiter_start == 0 || rest[..delimiter_start].ends_with('\n');
        let after_delimiter = &rest[delimiter_end..];
        let ends_line = after_delimiter.is_empty()
            || after_delimiter.starts_with('\n')
            || after_delimiter.starts_with("\r\n");

        if starts_line && ends_line {
            let frontmatter = rest[..delimiter_start]
                .strip_suffix("\r\n")
                .or_else(|| rest[..delimiter_start].strip_suffix('\n'))
                .unwrap_or(&rest[..delimiter_start]);
            let body = after_delimiter
                .strip_prefix("\r\n")
                .or_else(|| after_delimiter.strip_prefix('\n'))
                .unwrap_or(after_delimiter);
            return Some(FrontmatterParts { frontmatter, body });
        }

        search_start = delimiter_end;
    }
}

pub fn parse_frontmatter_fields(frontmatter: &str) -> Result<BTreeMap<String, String>> {
    if frontmatter.trim().is_empty() {
        return Ok(BTreeMap::new());
    }

    let value: serde_yaml::Value =
        serde_yaml::from_str(frontmatter).context("failed to parse front matter YAML")?;
    let Some(mapping) = value.as_mapping() else {
        bail!("front matter must be a YAML mapping");
    };

    let mut fields = BTreeMap::new();
    for (key, value) in mapping {
        let Some(key) = key.as_str() else {
            continue;
        };
        if let Some(value) = yaml_value_to_string(value) {
            fields.insert(key.to_string(), value);
        }
    }
    Ok(fields)
}

fn parse_frontmatter_field_sources(
    frontmatter: &str,
    fields: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let lines = frontmatter_lines(frontmatter);
    let mut sources = BTreeMap::new();
    let mut index = 0;
    while index < lines.len() {
        let Some(key) = top_level_key(&lines[index]) else {
            index += 1;
            continue;
        };
        let next = next_top_level_field_index(&lines, index + 1);
        if let Some(value) = source_value_for_field(&lines[index..next], fields.get(key)) {
            sources.insert(key.to_string(), value);
        }
        index = next;
    }
    sources
}

fn source_value_for_field(lines: &[String], parsed_value: Option<&String>) -> Option<String> {
    let first = lines.first()?;
    let (_, value) = first.split_once(':')?;
    let trimmed = value.trim_start();
    if matches!(trimmed.chars().next(), Some('>' | '|')) {
        let block_lines = lines
            .iter()
            .skip(1)
            .map(|line| strip_block_indent(line))
            .collect::<Vec<_>>();
        return Some(block_lines.join("\n").trim_end_matches('\n').to_string());
    }
    Some(
        parsed_value
            .cloned()
            .unwrap_or_else(|| unquote(trimmed).to_string()),
    )
}

fn yaml_value_to_string(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::Null => None,
        serde_yaml::Value::String(value) => Some(value.clone()),
        serde_yaml::Value::Bool(value) => Some(value.to_string()),
        serde_yaml::Value::Number(value) => Some(value.to_string()),
        _ => serde_yaml::to_string(value)
            .ok()
            .map(|value| value.trim().to_string()),
    }
}

fn frontmatter_lines(frontmatter: &str) -> Vec<String> {
    frontmatter
        .replace("\r\n", "\n")
        .split('\n')
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn top_level_key(line: &str) -> Option<&str> {
    if line.starts_with(char::is_whitespace) {
        return None;
    }
    let (key, _) = line.split_once(':')?;
    let key = key.trim();
    if key.is_empty() { None } else { Some(key) }
}

fn next_top_level_field_index(lines: &[String], start: usize) -> usize {
    lines[start..]
        .iter()
        .position(|line| top_level_key(line).is_some())
        .map(|offset| start + offset)
        .unwrap_or(lines.len())
}

fn scalar_style(line: &str) -> Option<char> {
    let (_, value) = line.split_once(':')?;
    match value.trim_start().chars().next() {
        Some('>' | '|') => value.trim_start().chars().next(),
        _ => None,
    }
}

fn render_field(
    key: &str,
    value: &str,
    preferred_style: Option<char>,
    line_ending: &str,
) -> String {
    let style = preferred_style.or_else(|| value.contains('\n').then_some('|'));
    if let Some(style) = style {
        let mut output = format!("{key}: {style}{line_ending}");
        let lines = if value.is_empty() {
            vec![""]
        } else {
            value.lines().collect::<Vec<_>>()
        };
        for line in lines {
            output.push_str("  ");
            output.push_str(line);
            output.push_str(line_ending);
        }
        return output;
    }
    format!("{key}: {}{line_ending}", yaml_scalar(value.trim()))
}

fn strip_block_indent(line: &str) -> &str {
    line.strip_prefix("  ")
        .or_else(|| line.strip_prefix('\t'))
        .unwrap_or(line)
}

fn yaml_scalar(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
    {
        return value.to_string();
    }
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn detect_line_ending(content: &str) -> &str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_folded_block_scalar() {
        let document = parse_frontmatter_document(
            "---\nsummary: >\n  one line\n  two line\nnext: value\n---\nbody",
        )
        .unwrap();

        assert_eq!(
            document.fields.get("summary").map(String::as_str),
            Some("one line two line\n")
        );
        assert_eq!(
            document.field_sources.get("summary").map(String::as_str),
            Some("one line\ntwo line")
        );
        assert_eq!(
            document.fields.get("next").map(String::as_str),
            Some("value")
        );
        assert_eq!(document.body, "body");
    }

    #[test]
    fn parses_crlf_frontmatter_delimiters() {
        let document = parse_frontmatter_document(
            "---\r\nsummary: >\r\n  one line\r\n  two line\r\nnext: value\r\n---\r\nbody",
        )
        .unwrap();

        assert_eq!(
            document.fields.get("summary").map(String::as_str),
            Some("one line two line\n")
        );
        assert_eq!(
            document.fields.get("next").map(String::as_str),
            Some("value")
        );
        assert_eq!(document.body, "body");
    }

    #[test]
    fn updates_known_fields_and_preserves_unknown_fields() {
        let updated = update_frontmatter_document(
            "---\nname: demo\ndescription: >\n  old line\ncompatibility: claude-code-only\n---\nbody",
            &[
                FrontmatterUpdate {
                    key: "name",
                    value: "demo",
                    source: None,
                },
                FrontmatterUpdate {
                    key: "description",
                    value: "new line\nnext line",
                    source: Some("new line\nnext line"),
                },
            ],
            "new body",
        )
        .unwrap();

        assert!(updated.contains("compatibility: claude-code-only"));
        assert!(updated.contains("description: >\n  new line\n  next line\n"));
        assert!(updated.ends_with("---\nnew body"));
    }

    #[test]
    fn parses_document_prefixed_with_utf8_bom() {
        let document =
            parse_frontmatter_document("\u{FEFF}---\nname: demo\nnext: value\n---\nbody").unwrap();

        assert_eq!(document.fields.get("name").map(String::as_str), Some("demo"));
        assert_eq!(document.fields.get("next").map(String::as_str), Some("value"));
        assert_eq!(document.body, "body");
    }
}
