use gold_band::artifacts::json_artifact_display_span;

#[test]
fn streaming_json_fence_with_only_whitespace_has_no_artifact_span() {
    assert!(json_artifact_display_span("before\n```json\n \n").is_none());
}

#[test]
fn fenced_json_span_keeps_trimmed_content_and_original_display_bounds() {
    let content = "before\n```json\n \n{\"状态\":true}\n```\nafter";
    let span = json_artifact_display_span(content).expect("JSON artifact span");

    assert_eq!(span.json_text, "{\"状态\":true}");
    assert_eq!(
        &content[span.start..span.end],
        "```json\n \n{\"状态\":true}\n```"
    );
    assert_eq!(&content[span.json_start..span.json_end], span.json_text);
}
