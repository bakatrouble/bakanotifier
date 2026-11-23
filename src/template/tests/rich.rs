use super::*;

#[test]
fn missing_mode_is_an_error() {
    let dir = temp_template_dir("missing-mode");
    fs::write(dir.join("broken.yaml"), "text: hello").expect("template should be written");

    let error =
        load_and_render(&dir, "broken", &json!({})).expect_err("template should be invalid");
    assert!(matches!(error, TemplateError::Invalid(_)));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn missing_text_is_an_error() {
    let dir = temp_template_dir("missing-text");
    fs::write(dir.join("broken.yaml"), "mode: html").expect("template should be written");

    let error =
        load_and_render(&dir, "broken", &json!({})).expect_err("template should be invalid");
    assert!(matches!(error, TemplateError::Invalid(_)));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn invalid_mode_is_an_error() {
    let dir = temp_template_dir("invalid-mode");
    fs::write(dir.join("broken.yaml"), "mode: blocks\ntext: hello")
        .expect("template should be written");

    let error =
        load_and_render(&dir, "broken", &json!({})).expect_err("template should be invalid");
    assert!(matches!(error, TemplateError::Invalid(_)));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn example_template_renders_expected_text() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    let values = json!({
        "title": "Disk alert",
        "items": ["sda 97%", "sdb 12%"],
        "urgent": true
    });
    let template = load_and_render(&dir, "example", &values).expect("example should render");
    assert_eq!(
        template.text.as_deref(),
        Some("<b>Disk alert</b>\n- sda 97%\n- sdb 12%")
    );
    assert_eq!(template.mode, TemplateMode::RichHtml);
    assert_eq!(template.skip_entity_detection, Some(true));
}

#[test]
fn example_values_file_uses_the_same_render_path() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let values_source = fs::read_to_string(root.join("examples/values/example.json"))
        .expect("example values should be readable");
    let values: Value =
        serde_json::from_str(&values_source).expect("example values should be valid JSON");

    let template = load_and_render(&root.join("templates"), "example", &values)
        .expect("example should render");
    assert_eq!(
        template.text.as_deref(),
        Some("<b>Disk alert</b>\n- sda 97%\n- sdb 12%")
    );
    assert_eq!(template.mode, TemplateMode::RichHtml);
    assert_eq!(template.skip_entity_detection, Some(true));
}

#[test]
fn example_template_without_urgent_keeps_entity_detection() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    let values = json!({"title": "Disk alert", "items": [], "urgent": false});
    let template = load_and_render(&dir, "example", &values).expect("example should render");
    assert_eq!(template.text.as_deref(), Some("<b>Disk alert</b>"));
    assert_eq!(template.skip_entity_detection, None);
}

#[test]
fn rich_html_kitchen_sink_renders_input_rich_message() {
    let template = render_kitchen_sink("rich-html-kitchen-sink", "rich-html-kitchen-sink.json");
    assert_eq!(template.mode, TemplateMode::RichHtml);
    assert_eq!(template.skip_entity_detection, Some(true));

    for needle in [
        "<h1>Rich HTML kitchen sink</h1>",
        "<b>bold</b>",
        "<tg-spoiler>spoiler</tg-spoiler>",
        "<a href=\"#chapter-1\">",
        "<tg-reference name=\"note-1\">",
        "<tg-emoji",
        "<tg-time",
        "<tg-math>x^2 + y^2</tg-math>",
        "<pre><code class=\"language-python\">",
        "<li>First item</li>",
        "<ul><li><input type=\"checkbox\" checked>",
        "<blockquote expandable>",
        "<aside>Optional note",
        "https://telegram.org/example/photo.jpg",
        "<figure><img",
        "<tg-map",
        "<tg-collage>",
        "<tg-slideshow>",
        "<table bordered striped compact>",
        "<details open>",
        "<tg-math-block>E = mc^2</tg-math-block>",
        "<tg-button-row align=\"left\">",
        "<tg-button type=\"callback_data\"",
        "<tg-button type=\"login_url\"",
    ] {
        assert!(
            template
                .text
                .as_deref()
                .unwrap_or_default()
                .contains(needle),
            "missing {needle} in HTML kitchen sink"
        );
    }

    let text = template.text.clone().expect("rich html text");
    let input = template
        .into_rich_message()
        .expect("rich blocks should convert");
    assert_eq!(input.html.as_deref(), Some(text.as_str()));
    assert!(input.markdown.is_none());
    assert_eq!(input.skip_entity_detection, Some(true));
}

#[test]
fn rich_markdown_kitchen_sink_renders_input_rich_message() {
    let template = render_kitchen_sink(
        "rich-markdown-kitchen-sink",
        "rich-markdown-kitchen-sink.json",
    );
    assert_eq!(template.mode, TemplateMode::RichMarkdown);
    assert_eq!(template.skip_entity_detection, Some(true));

    for needle in [
        "# Rich Markdown kitchen sink",
        "**bold**",
        "~~strikethrough~~",
        "==marked==",
        "||spoiler||",
        "[inline e-mail](mailto:user@example.com)",
        "![custom emoji](tg://emoji?id=5368324170671202286)",
        "$$E = mc^2$$",
        "```math",
        "```python",
        "|:---------|:--------:|",
        "[^id1]: Definition of the first footnote.",
        "<details open>",
        "<tg-collage>",
        "<tg-slideshow>",
        "<u>underlined text</u>",
        "<sup>superscript</sup>",
        "<a name=\"chapter-1\"></a>",
        "<tg-button type=\"url\"",
        "<tg-button-row align=\"right\">",
        "- First item",
        "> Optional note for the footer",
    ] {
        assert!(
            template
                .text
                .as_deref()
                .unwrap_or_default()
                .contains(needle),
            "missing {needle} in Markdown kitchen sink"
        );
    }

    let text = template.text.clone().expect("rich markdown text");
    let input = template
        .into_rich_message()
        .expect("rich blocks should convert");
    assert_eq!(input.markdown.as_deref(), Some(text.as_str()));
    assert!(input.html.is_none());
    assert_eq!(input.skip_entity_detection, Some(true));
}

#[test]
fn rich_html_rejects_blocks() {
    let dir = temp_template_dir("rich-html-with-blocks");
    fs::write(
        dir.join("broken.yaml"),
        "mode: rich_html\ntext: hello\nblocks:\n  - type: divider\n",
    )
    .expect("template should be written");

    let error = load_and_render(&dir, "broken", &json!({})).expect_err("blocks should be rejected");
    assert!(error.to_string().contains("must not set blocks"));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn skip_entity_detection_is_rejected_for_non_rich_modes() {
    let dir = temp_template_dir("skip-non-rich");
    fs::write(
        dir.join("plain.yaml"),
        "mode: html\ntext: hello\nskip_entity_detection: true",
    )
    .expect("template should be written");

    let error = load_and_render(&dir, "plain", &json!({}))
        .expect_err("skip_entity_detection should be rejected");
    assert!(
        error.to_string().contains("skip_entity_detection"),
        "unclear error: {error}"
    );

    fs::remove_dir_all(&dir).ok();
}
