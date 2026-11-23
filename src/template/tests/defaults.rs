use super::*;

#[test]
fn dry_run_payload_for_example() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let values: Value = serde_json::from_str(
        &fs::read_to_string(root.join("examples/values/example.json"))
            .expect("values should be readable"),
    )
    .expect("values should be valid JSON");
    let template = load_and_render(&root.join("templates"), "example", &values)
        .expect("example should render");
    let payload = template
        .into_rendered_message()
        .expect("example should convert")
        .payload(Some(123));

    assert_eq!(payload["method"], "sendRichMessage");
    assert_eq!(payload["chat_id"], 123);
    assert_eq!(
        payload["rich_message"]["html"],
        "<b>Disk alert</b>\n- sda 97%\n- sdb 12%"
    );
}

#[test]
fn dry_run_payload_for_rich_blocks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let values: Value = serde_json::from_str(
        &fs::read_to_string(root.join("examples/values/rich-blocks-example.json"))
            .expect("values should be readable"),
    )
    .expect("values should be valid JSON");
    let template = load_and_render(&root.join("templates"), "rich-blocks-example", &values)
        .expect("rich blocks should render");
    let payload = template
        .into_rendered_message()
        .expect("rich blocks should convert")
        .payload(Some(123));

    assert_eq!(payload["method"], "sendRichMessage");
    assert_eq!(payload["rich_message"]["blocks"][0]["type"], "heading");
    assert!(payload["rich_message"].get("html").is_none());
    assert!(payload["rich_message"].get("markdown").is_none());
}

#[test]
fn default_templates_render_messages_unchanged() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    let message = "quotes \"double\" and 'single', newline\nand backslash \\ end";

    for (name, mode) in [
        ("text", TemplateMode::Text),
        ("html", TemplateMode::Html),
        ("markdown", TemplateMode::Markdown),
    ] {
        let template = load_and_render(&dir, name, &json!({"message": message}))
            .unwrap_or_else(|error| panic!("{name} should render: {error}"));
        assert_eq!(template.mode, mode);
        assert_eq!(template.text.as_deref(), Some(message));
        assert_eq!(template.skip_entity_detection, None);
    }
}

#[test]
fn default_templates_require_message_value() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    for name in ["text", "html", "markdown"] {
        let error =
            load_and_render(&dir, name, &json!({})).expect_err("missing message should fail");
        assert!(matches!(error, TemplateError::Values(_)));
    }
}
