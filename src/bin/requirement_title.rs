use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::process;

const FALLBACK_TITLE: &str = "未命名需求";
const TARGET_LEN: usize = 10;
const MAX_LEN: usize = 18;
const MAX_SHINGLE: usize = 4;

const FOCUS_MARKERS: &[&str] = &[
    "需求",
    "需求描述",
    "核心需求",
    "目标",
    "目标描述",
    "标题",
    "一句话总结",
];
const LIGHT_FILLERS: &[&str] = &[
    "当前", "本次", "这次", "一个", "一种", "一套", "进行", "相关", "用于", "需要", "请", "将",
    "把",
];
const TRAILING_SUFFIXES: &[&str] = &[
    "详细设计",
    "设计方案",
    "实现方案",
    "实施方案",
    "重构计划",
    "优化计划",
    "改造计划",
    "功能说明",
    "需求说明",
    "方案",
    "计划",
    "说明",
];
const GENERIC_SECTION_WORDS: &[&str] = &[
    "实现", "设计", "方案", "改动", "改造", "接入", "规则", "布局", "约束", "验收", "背景", "目标",
    "结论", "状态", "模块", "步骤", "细节", "示例", "接口", "组件",
];

#[derive(Clone, Debug, PartialEq, Eq)]
enum LineRole {
    Heading { level: usize, numbered: bool },
    Paragraph,
    ListItem,
}

#[derive(Clone, Debug)]
struct Line {
    text: String,
    role: LineRole,
    line_index: usize,
}

#[derive(Clone, Debug)]
struct Candidate {
    text: String,
    base_score: i32,
    source_kind: CandidateSourceKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CandidateSourceKind {
    Heading,
    FocusFollow,
    ParagraphLead,
    Sentence,
    FirstLine,
}

#[derive(Clone, Debug, Default)]
struct Stats {
    ascii_freq: HashMap<String, usize>,
    cjk_shingle_freq: HashMap<String, usize>,
}

fn main() {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("用法: cargo run --bin requirement_title -- <文件路径>");
        process::exit(2);
    };

    if args.next().is_some() {
        eprintln!("只接受一个文件路径参数");
        process::exit(2);
    }

    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) => {
            eprintln!("读取文件失败: {error}");
            process::exit(1);
        }
    };

    println!("{}", generate_title(&content));
}

pub fn generate_title(text: &str) -> String {
    let lines = parse_lines(text);
    if lines.is_empty() {
        return FALLBACK_TITLE.to_string();
    }

    let paragraphs = build_paragraphs(&lines);
    let stats = build_stats(&lines, &paragraphs);

    if let Some(title) = strongest_structural_title(&lines, &stats) {
        return title;
    }
    if let Some(title) = narrative_lead_title(&paragraphs, &stats) {
        return title;
    }

    let candidates = build_candidates(&lines, &paragraphs);

    candidates
        .into_iter()
        .map(|candidate| finalize_candidate(candidate, &stats))
        .max_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| readability_bonus(&left.0).cmp(&readability_bonus(&right.0)))
        })
        .map(|(title, _)| title)
        .unwrap_or_else(|| FALLBACK_TITLE.to_string())
}

fn parse_lines(text: &str) -> Vec<Line> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = Vec::new();
    let mut in_code_block = false;

    for raw_line in normalized.lines() {
        let trimmed = raw_line.trim().trim_start_matches('\u{feff}');
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block || trimmed.is_empty() || is_table_delimiter(trimmed) {
            continue;
        }

        if let Some(level) = markdown_heading_level(trimmed) {
            let text = normalize_text(&strip_markdown_heading(trimmed, level));
            if !text.is_empty() {
                lines.push(Line {
                    text,
                    role: LineRole::Heading {
                        level,
                        numbered: has_leading_numbering(trimmed),
                    },
                    line_index: lines.len(),
                });
            }
            continue;
        }

        let stripped = normalize_text(&strip_list_marker(trimmed));
        if stripped.is_empty()
            || looks_repetitive(&stripped)
            || trailing_segment_after_delimiter(&stripped)
                .map(|segment| {
                    looks_repetitive(segment)
                        || sounds_like_discardable_tail(&leading_segment(segment))
                })
                .unwrap_or(false)
        {
            continue;
        }

        let role = if looks_like_list_item(trimmed) {
            LineRole::ListItem
        } else {
            LineRole::Paragraph
        };

        lines.push(Line {
            text: stripped,
            role,
            line_index: lines.len(),
        });
    }

    lines
}

fn build_paragraphs(lines: &[Line]) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current = String::new();

    for line in lines {
        match line.role {
            LineRole::Paragraph => {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(&line.text);
            }
            _ => {
                if !current.is_empty() {
                    paragraphs.push(current.trim().to_string());
                    current.clear();
                }
            }
        }
    }

    if !current.is_empty() {
        paragraphs.push(current.trim().to_string());
    }

    paragraphs
}

fn build_stats(lines: &[Line], paragraphs: &[String]) -> Stats {
    let mut stats = Stats::default();

    for line in lines {
        add_text_to_stats(&line.text, &mut stats);
    }
    for paragraph in paragraphs {
        add_text_to_stats(paragraph, &mut stats);
    }

    stats
}

fn add_text_to_stats(text: &str, stats: &mut Stats) {
    for token in ascii_tokens(text) {
        let token = normalize_ascii_token(&token);
        if token.is_empty() {
            continue;
        }
        *stats.ascii_freq.entry(token).or_insert(0) += 1;
    }

    for shingle in cjk_shingles(text, 2, MAX_SHINGLE) {
        if shingle
            .chars()
            .all(|ch| ch == shingle.chars().next().unwrap_or(ch))
        {
            continue;
        }
        *stats.cjk_shingle_freq.entry(shingle).or_insert(0) += 1;
    }
}

fn strongest_structural_title(lines: &[Line], stats: &Stats) -> Option<String> {
    let h1 = lines
        .iter()
        .find(|line| matches!(line.role, LineRole::Heading { level: 1, .. }))?;
    let normalized = normalize_title_text(&h1.text);
    if normalized.is_empty() || is_focus_marker(&normalized) || is_overly_generic(&normalized) {
        return None;
    }
    let title = compress_title(&normalized, stats);
    if !title.is_empty() && !looks_repetitive(&title) {
        return Some(title);
    }
    None
}

fn narrative_lead_title(paragraphs: &[String], stats: &Stats) -> Option<String> {
    let paragraph = paragraphs.first()?;
    let lead = compress_title(&leading_segment(first_sentence(paragraph)), stats);
    if lead.is_empty() || looks_repetitive(&lead) {
        return None;
    }
    if specificity_score(&lead) >= 10 {
        return Some(lead);
    }
    None
}

fn build_candidates(lines: &[Line], paragraphs: &[String]) -> Vec<Candidate> {
    let mut candidates = Vec::new();

    for line in lines {
        if let LineRole::Heading { level, numbered } = line.role {
            let base = match level {
                1 => 170,
                2 => 95,
                3 => 70,
                _ => 52,
            } - (line.line_index as i32 * 3)
                - if numbered { 22 } else { 0 };

            candidates.push(Candidate {
                text: line.text.clone(),
                base_score: base,
                source_kind: CandidateSourceKind::Heading,
            });
        }
    }

    for (index, line) in lines.iter().enumerate() {
        if is_focus_marker(&line.text) {
            if let Some(next_line) = lines.get(index + 1) {
                candidates.push(Candidate {
                    text: next_line.text.clone(),
                    base_score: 130 - (next_line.line_index as i32 * 2),
                    source_kind: CandidateSourceKind::FocusFollow,
                });
            }
            if let Some(next_paragraph) = paragraphs.first() {
                candidates.push(Candidate {
                    text: first_sentence(next_paragraph).to_string(),
                    base_score: 118,
                    source_kind: CandidateSourceKind::FocusFollow,
                });
            }
        }
    }

    for (index, paragraph) in paragraphs.iter().take(3).enumerate() {
        let paragraph_score = 100 - (index as i32 * 10);
        candidates.push(Candidate {
            text: leading_segment(first_sentence(paragraph)),
            base_score: paragraph_score + 12,
            source_kind: CandidateSourceKind::ParagraphLead,
        });

        for (sentence_index, sentence) in split_sentences(paragraph).into_iter().take(3).enumerate()
        {
            let lead = leading_segment(&sentence);
            candidates.push(Candidate {
                text: lead,
                base_score: paragraph_score - (sentence_index as i32 * 8),
                source_kind: CandidateSourceKind::Sentence,
            });
        }
    }

    if let Some(first_line) = lines.first() {
        candidates.push(Candidate {
            text: first_line.text.clone(),
            base_score: 110,
            source_kind: CandidateSourceKind::FirstLine,
        });
    }

    candidates
}

fn finalize_candidate(candidate: Candidate, stats: &Stats) -> (String, i32) {
    let raw = normalize_title_text(&candidate.text);
    let mut best_title = compress_title(&raw, stats);
    let mut best_score = score_title(
        &best_title,
        candidate.base_score,
        stats,
        &raw,
        &candidate.source_kind,
    );

    for (index, clause) in split_clauses(&raw).into_iter().enumerate() {
        let compressed = compress_title(&clause, stats);
        let clause_base = candidate.base_score - 6 - (index as i32 * 14);
        let score = score_title(
            &compressed,
            clause_base,
            stats,
            &raw,
            &candidate.source_kind,
        );
        if score > best_score {
            best_title = compressed;
            best_score = score;
        }
    }

    (best_title, best_score)
}

fn compress_title(text: &str, stats: &Stats) -> String {
    let mut title = normalize_title_text(text);
    if title.is_empty() {
        return FALLBACK_TITLE.to_string();
    }
    if char_len(&title) <= MAX_LEN {
        return title;
    }

    let clauses = split_clauses(&title);
    if let Some(best_clause) = best_clause(&clauses, stats) {
        title = normalize_title_text(best_clause);
    }
    if char_len(&title) <= MAX_LEN {
        return title;
    }

    title = keyword_skeleton(&title, stats);
    if title.is_empty() {
        return FALLBACK_TITLE.to_string();
    }
    if char_len(&title) <= MAX_LEN {
        return title;
    }

    smart_truncate(&title, MAX_LEN)
}

fn score_title(
    title: &str,
    base_score: i32,
    stats: &Stats,
    raw_source: &str,
    source_kind: &CandidateSourceKind,
) -> i32 {
    let mut score = base_score;
    let length = char_len(title);
    let distance = length.abs_diff(TARGET_LEN) as i32;

    score += relevance_score(title, stats);
    score -= distance * 2;

    score += match source_kind {
        CandidateSourceKind::Heading => 40,
        CandidateSourceKind::FocusFollow => 16,
        CandidateSourceKind::ParagraphLead => 8,
        CandidateSourceKind::Sentence => 0,
        CandidateSourceKind::FirstLine => 10,
    };

    if length < 4 {
        score -= 25;
    }
    if length > MAX_LEN {
        score -= (length - MAX_LEN) as i32 * 3;
    }
    if starts_with_numbering(title) {
        score -= 28;
    }
    if is_overly_generic(title) {
        score -= 18;
    }
    if title == raw_source && length > MAX_LEN {
        score -= 10;
    }
    if has_acronym(title) {
        score += 10;
    }
    if has_path_token(title) {
        score += 8;
    }
    if has_class_like_target(title) {
        score += 12;
    }
    if looks_repetitive(title) {
        score -= 45;
    }
    if sounds_like_discardable_tail(title) {
        score -= 30;
    }

    score
}

fn relevance_score(title: &str, stats: &Stats) -> i32 {
    let mut score = 0;
    let mut seen_ascii = HashSet::new();
    let mut seen_cjk = HashSet::new();

    for token in ascii_tokens(title) {
        let token = normalize_ascii_token(&token);
        if token.is_empty() || !seen_ascii.insert(token.clone()) {
            continue;
        }
        score += stats.ascii_freq.get(&token).copied().unwrap_or(0) as i32 * 6;
    }

    for shingle in cjk_shingles(title, 2, MAX_SHINGLE) {
        if !seen_cjk.insert(shingle.clone()) {
            continue;
        }
        let freq = stats.cjk_shingle_freq.get(&shingle).copied().unwrap_or(0) as i32;
        score += freq * shingle.chars().count() as i32;
    }

    score
}

fn keyword_skeleton(text: &str, stats: &Stats) -> String {
    let mut pieces = ranked_pieces(text, stats);
    pieces.sort_by(|left, right| left.0.cmp(&right.0));

    let mut result = String::new();
    for (_, piece, _) in pieces {
        if result.contains(&piece) {
            continue;
        }
        let next_len = char_len(&result) + char_len(&piece);
        if next_len > MAX_LEN {
            continue;
        }
        result.push_str(&piece);
    }

    if result.is_empty() {
        smart_truncate(text, MAX_LEN)
    } else {
        result
    }
}

fn ranked_pieces(text: &str, stats: &Stats) -> Vec<(usize, String, i32)> {
    let mut pieces = Vec::new();

    for token in ascii_token_spans(text) {
        let normalized = normalize_ascii_token(&token.1);
        if normalized.is_empty() {
            continue;
        }
        let freq = stats.ascii_freq.get(&normalized).copied().unwrap_or(1) as i32;
        pieces.push((token.0, normalized, freq * 10));
    }

    for (start, shingle) in cjk_shingle_spans(text, 2, MAX_SHINGLE) {
        let freq = stats.cjk_shingle_freq.get(&shingle).copied().unwrap_or(0) as i32;
        if freq == 0 {
            continue;
        }
        let generic_penalty = if GENERIC_SECTION_WORDS.contains(&shingle.as_str()) {
            8
        } else {
            0
        };
        let score = freq * shingle.chars().count() as i32 - generic_penalty;
        if score > 0 {
            pieces.push((start, shingle, score));
        }
    }

    pieces.sort_by(|left, right| right.2.cmp(&left.2).then_with(|| left.0.cmp(&right.0)));

    let mut selected = Vec::new();
    let mut occupied = Vec::<(usize, usize)>::new();

    for (start, piece, score) in pieces {
        let end = start + piece.chars().count();
        if occupied
            .iter()
            .any(|(used_start, used_end)| ranges_overlap(start, end, *used_start, *used_end))
        {
            continue;
        }
        occupied.push((start, end));
        selected.push((start, piece, score));
    }

    selected
}

fn best_clause<'a>(clauses: &'a [String], stats: &Stats) -> Option<&'a str> {
    clauses
        .iter()
        .map(|clause| {
            let normalized = normalize_title_text(clause);
            let score = relevance_score(&normalized, stats)
                - (char_len(&normalized).abs_diff(TARGET_LEN) as i32);
            (clause.as_str(), score)
        })
        .max_by_key(|(_, score)| *score)
        .map(|(clause, _)| clause)
}

fn normalize_title_text(text: &str) -> String {
    let mut value = normalize_text(text);
    value = strip_leading_function_words(&value);
    value = trim_trailing_punctuation(&value);

    for suffix in TRAILING_SUFFIXES {
        if value.ends_with(suffix) && char_len(&value) > TARGET_LEN {
            value = value.strip_suffix(suffix).unwrap_or(&value).to_string();
            value = trim_trailing_punctuation(&value);
            break;
        }
    }

    value
}

fn normalize_text(text: &str) -> String {
    let mut value = text.trim().replace('`', "");
    value = collapse_whitespace(&value);
    value = strip_markdown_emphasis(&value);
    value = remove_leading_numbering(&value).trim().to_string();
    value = normalize_case_words(&value);
    value = cleanup_path_phrases(&value);

    for filler in LIGHT_FILLERS {
        if !value.starts_with('.') {
            value = value.replace(filler, "");
        }
    }

    value = collapse_whitespace(&value).replace(' ', "");
    trim_trailing_punctuation(&value)
}

fn cleanup_path_phrases(text: &str) -> String {
    text.replace("目录下", "")
        .replace("目录", "")
        .replace("文件中", "")
}

fn strip_leading_function_words(text: &str) -> String {
    let mut value = text.to_string();
    for prefix in ["请在", "请将", "请把", "在", "把", "将", "需", "要"] {
        if value.starts_with(prefix) && char_len(&value) > 6 {
            value = value[prefix.len()..].to_string();
            break;
        }
    }
    value
}

fn trim_trailing_punctuation(text: &str) -> String {
    text.trim_matches(|ch: char| {
        matches!(ch, '，' | '。' | '；' | '：' | ':' | '-' | '_' | '、' | ' ')
    })
    .to_string()
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '。' | '！' | '？' | '!' | '?' | '\n') {
            let normalized = trim_trailing_punctuation(&current);
            if !normalized.is_empty() {
                sentences.push(normalized);
            }
            current.clear();
        }
    }

    if !current.trim().is_empty() {
        let normalized = trim_trailing_punctuation(&current);
        if !normalized.is_empty() {
            sentences.push(normalized);
        }
    }

    if sentences.is_empty() {
        vec![text.to_string()]
    } else {
        sentences
    }
}

fn first_sentence(text: &str) -> &str {
    split_once_by_chars(text, &['。', '！', '？', '!', '?', '\n']).unwrap_or(text)
}

fn leading_segment(text: &str) -> String {
    split_once_by_chars(text, &['，', '。', '；', '：', ':'])
        .unwrap_or(text)
        .to_string()
}

fn trailing_segment_after_delimiter(text: &str) -> Option<&str> {
    for delimiter in ['：', ':', '，', '。', '；'] {
        if let Some((_, tail)) = text.split_once(delimiter) {
            let tail = tail.trim();
            if !tail.is_empty() {
                return Some(tail);
            }
        }
    }
    None
}

fn split_clauses(text: &str) -> Vec<String> {
    text.split(['，', '。', '；', '：', ':', '|'])
        .map(normalize_title_text)
        .filter(|clause| !clause.is_empty())
        .collect()
}

fn markdown_heading_level(text: &str) -> Option<usize> {
    let hashes = text.chars().take_while(|ch| *ch == '#').count();
    if hashes == 0 {
        return None;
    }
    text.chars().nth(hashes).filter(|ch| ch.is_whitespace())?;
    Some(hashes)
}

fn strip_markdown_heading(text: &str, level: usize) -> String {
    text.chars()
        .skip(level)
        .collect::<String>()
        .trim()
        .to_string()
}

fn strip_list_marker(text: &str) -> String {
    let trimmed = text.trim_start();
    if let Some(rest) = trimmed.strip_prefix("- ") {
        return rest.to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("* ") {
        return rest.to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("+ ") {
        return rest.to_string();
    }

    let without_numbering = remove_leading_numbering(trimmed);
    without_numbering.trim().to_string()
}

fn looks_like_list_item(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("+ ")
        || has_leading_numbering(trimmed)
}

fn has_leading_numbering(text: &str) -> bool {
    remove_leading_numbering(text) != text.trim_start()
}

fn remove_leading_numbering(text: &str) -> String {
    let trimmed = text.trim_start();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.is_empty() {
        return String::new();
    }

    let mut index = 0usize;

    if chars.first() == Some(&'(') || chars.first() == Some(&'（') {
        index += 1;
        while index < chars.len() && is_numbering_char(chars[index]) {
            index += 1;
        }
        if index < chars.len() && (chars[index] == ')' || chars[index] == '）') {
            index += 1;
            while index < chars.len() && chars[index].is_whitespace() {
                index += 1;
            }
            return chars[index..].iter().collect();
        }
        return trimmed.to_string();
    }

    let start = index;
    while index < chars.len() && is_numbering_char(chars[index]) {
        index += 1;
    }

    if index == start {
        return trimmed.to_string();
    }

    if index < chars.len() && matches!(chars[index], '.' | '、' | ')' | '）') {
        index += 1;
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        return chars[index..].iter().collect();
    }

    trimmed.to_string()
}

fn is_numbering_char(ch: char) -> bool {
    ch.is_ascii_digit()
        || matches!(ch, '.' | '-' | '_')
        || matches!(
            ch,
            '一' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十'
        )
        || matches!(ch, 'a'..='z' | 'A'..='Z')
}

fn starts_with_numbering(text: &str) -> bool {
    remove_leading_numbering(text) != text
}

fn strip_markdown_emphasis(text: &str) -> String {
    text.replace("**", "").replace("__", "")
}

fn collapse_whitespace(text: &str) -> String {
    let mut result = String::new();
    let mut last_was_space = false;

    for ch in text.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                result.push(' ');
            }
            last_was_space = true;
        } else {
            result.push(ch);
            last_was_space = false;
        }
    }

    result.trim().to_string()
}

fn normalize_case_words(text: &str) -> String {
    text.replace("python", "Python")
        .replace("rust", "Rust")
        .replace("hello world", "hello-world")
}

fn is_focus_marker(text: &str) -> bool {
    FOCUS_MARKERS.contains(&text)
}

fn ascii_tokens(text: &str) -> Vec<String> {
    ascii_token_spans(text)
        .into_iter()
        .map(|(_, token)| token)
        .collect()
}

fn ascii_token_spans(text: &str) -> Vec<(usize, String)> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut start_index = 0usize;
    let mut char_index = 0usize;

    for ch in text.chars() {
        if is_ascii_token_char(ch) {
            if current.is_empty() {
                start_index = char_index;
            }
            current.push(ch);
        } else if !current.is_empty() {
            tokens.push((start_index, current.clone()));
            current.clear();
        }
        char_index += 1;
    }

    if !current.is_empty() {
        tokens.push((start_index, current));
    }

    tokens
}

fn normalize_ascii_token(token: &str) -> String {
    if token.eq_ignore_ascii_case("python") {
        return "Python".to_string();
    }
    if token.eq_ignore_ascii_case("rust") {
        return "Rust".to_string();
    }
    if token.starts_with('.') || token.contains('/') {
        return token.to_string();
    }
    if token
        .chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '-')
    {
        return token.replace('-', "");
    }
    if token.contains('-')
        && token
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
    {
        return token.to_string();
    }
    String::new()
}

fn has_acronym(text: &str) -> bool {
    ascii_tokens(text)
        .into_iter()
        .map(|token| normalize_ascii_token(&token))
        .any(|token| {
            !token.is_empty()
                && token
                    .chars()
                    .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
        })
}

fn has_path_token(text: &str) -> bool {
    ascii_tokens(text)
        .into_iter()
        .any(|token| token.starts_with('.') || token.contains('/'))
}

fn specificity_score(text: &str) -> i32 {
    let mut score = 0;
    if has_path_token(text) {
        score += 8;
    }
    if has_acronym(text) {
        score += 6;
    }
    if has_class_like_target(text) {
        score += 6;
    }
    score += ascii_tokens(text)
        .into_iter()
        .map(|token| normalize_ascii_token(&token))
        .filter(|token| !token.is_empty())
        .count() as i32
        * 2;
    score
}

fn is_ascii_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/')
}

fn cjk_shingles(text: &str, min_len: usize, max_len: usize) -> Vec<String> {
    cjk_shingle_spans(text, min_len, max_len)
        .into_iter()
        .map(|(_, shingle)| shingle)
        .collect()
}

fn cjk_shingle_spans(text: &str, min_len: usize, max_len: usize) -> Vec<(usize, String)> {
    let mut spans = Vec::new();
    let chars: Vec<(usize, char)> = text.chars().enumerate().collect();

    for start in 0..chars.len() {
        if !is_cjk(chars[start].1) {
            continue;
        }
        let mut run = Vec::new();
        let mut offset = start;
        while offset < chars.len() && is_cjk(chars[offset].1) {
            run.push(chars[offset]);
            offset += 1;
        }
        if run.len() >= min_len {
            for len in min_len..=max_len.min(run.len()) {
                for index in 0..=run.len() - len {
                    let piece: String = run[index..index + len].iter().map(|(_, ch)| *ch).collect();
                    spans.push((run[index].0, piece));
                }
            }
        }
    }

    spans
}

fn split_once_by_chars<'a>(text: &'a str, separators: &[char]) -> Option<&'a str> {
    let mut end = None;
    for (index, ch) in text.char_indices() {
        if separators.contains(&ch) {
            end = Some(index);
            break;
        }
    }
    end.map(|index| &text[..index])
}

fn is_table_delimiter(text: &str) -> bool {
    text.starts_with('|') && text.ends_with('|')
}

fn is_overly_generic(text: &str) -> bool {
    GENERIC_SECTION_WORDS.contains(&text)
}

fn has_class_like_target(text: &str) -> bool {
    text.contains("Python类") || text.contains("Rust类") || text.ends_with('类')
}

fn looks_repetitive(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 6 {
        return false;
    }

    for size in 2..=4 {
        if chars.len() < size * 2 {
            continue;
        }
        let prefix: String = chars[..size].iter().collect();
        if text == prefix.repeat(chars.len() / size) || text.starts_with(&prefix.repeat(2)) {
            return true;
        }
    }

    false
}

fn sounds_like_discardable_tail(text: &str) -> bool {
    text.contains("忽略") || text.contains("不用关注") || text.contains("补充内容")
}

fn ranges_overlap(start_a: usize, end_a: usize, start_b: usize, end_b: usize) -> bool {
    start_a < end_b && start_b < end_a
}

fn readability_bonus(title: &str) -> i32 {
    -(char_len(title).abs_diff(TARGET_LEN) as i32)
}

fn smart_truncate(text: &str, max_len: usize) -> String {
    text.chars().take(max_len).collect()
}

fn char_len(text: &str) -> usize {
    text.chars().count()
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x4E00..=0x9FFF |
        0x3400..=0x4DBF |
        0x20000..=0x2A6DF |
        0x2A700..=0x2B73F |
        0x2B740..=0x2B81F |
        0x2B820..=0x2CEAF |
        0xF900..=0xFAFF
    )
}

#[cfg(test)]
mod tests {
    use super::generate_title;

    #[test]
    fn generates_title_from_requirement_with_noise() {
        let input = r#"
# 需求
在.claude目录下输出一个python类，输出hello-world
下面文字你不用关注：测试用测试用测试用测试用测试用测试用测试用测试用
"#;

        assert_eq!(generate_title(input), ".claude输出Python类");
    }

    #[test]
    fn prefers_document_title_over_section_heading() {
        let input = r#"
# ACP 消息头像与时间展示

## 3.2 各消息类型接入
- Agent 文本消息接入头像与时间。
- Tool call 卡片接入头像与时间。
"#;

        assert_eq!(generate_title(input), "ACP消息头像与时间展示");
    }

    #[test]
    fn compresses_existing_heading() {
        let input = r#"
# ACP-first Agent 会话可视化重构计划

## 1. 核心决策
Gold Band 后续不再自研 progress.events.jsonl。
"#;

        assert_eq!(generate_title(input), "ACP会话可视化重构");
    }

    #[test]
    fn falls_back_to_plain_natural_language() {
        let input = "请在 .claude 目录下输出一个 python 类，用于打印 hello-world，其他补充内容都可以先忽略。";
        assert_eq!(generate_title(input), ".claude输出Python类");
    }
}
