use super::*;

#[test]
fn rich_blocks_example_renders() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let values: Value = serde_json::from_str(
        &fs::read_to_string(root.join("examples/values/rich-blocks-example.json"))
            .expect("values should be readable"),
    )
    .expect("values should be valid JSON");

    let template = load_and_render(&root.join("templates"), "rich-blocks-example", &values)
        .expect("rich blocks example should render");
    assert_eq!(template.mode, TemplateMode::RichBlocks);
    assert!(template.text.is_none());
    let blocks = template.blocks.as_ref().expect("blocks should be present");
    assert_eq!(blocks.len(), 5);
    assert_eq!(blocks[0]["type"], "heading");
    assert_eq!(blocks[0]["text"], "Rich blocks example");
    assert_eq!(blocks[2]["type"], "list");
    assert_eq!(blocks[3]["type"], "footer");
    assert_eq!(blocks[4]["type"], "divider");

    let input = template
        .into_rich_message()
        .expect("rich blocks should convert");
    assert!(input.html.is_none());
    assert!(input.markdown.is_none());
    assert_eq!(input.blocks.expect("blocks should exist").len(), 5);
}

#[test]
fn rich_blocks_reject_invalid_block_shape() {
    let dir = temp_template_dir("bad-rich-block");
    fs::write(
        dir.join("broken.yaml"),
        "mode: rich_blocks\nblocks:\n  - type: paragraph\n",
    )
    .expect("template should be written");

    let error =
        load_and_render(&dir, "broken", &json!({})).expect_err("missing block text should fail");
    assert!(
        error.to_string().contains("invalid rich_blocks blocks"),
        "unclear error: {error}"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn rich_blocks_reject_text_and_skip_entity_detection() {
    let dir = temp_template_dir("mixed-rich-block");
    fs::write(
        dir.join("with-text.yaml"),
        "mode: rich_blocks\ntext: hello\nblocks:\n  - type: divider\n",
    )
    .expect("template should be written");
    let error =
        load_and_render(&dir, "with-text", &json!({})).expect_err("text should be rejected");
    assert!(error.to_string().contains("must not set text"));

    fs::write(
        dir.join("with-skip.yaml"),
        "mode: rich_blocks\nskip_entity_detection: true\nblocks:\n  - type: divider\n",
    )
    .expect("template should be written");
    let error = load_and_render(&dir, "with-skip", &json!({}))
        .expect_err("skip_entity_detection should be rejected");
    assert!(
        error
            .to_string()
            .contains("must not set skip_entity_detection")
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn rich_blocks_accepts_is_rtl() {
    let dir = temp_template_dir("rich-blocks-rtl");
    fs::write(
        dir.join("rtl.yaml"),
        "mode: rich_blocks\nis_rtl: true\nblocks:\n  - type: divider\n",
    )
    .expect("template should be written");

    let template = load_and_render(&dir, "rtl", &json!({})).expect("rtl should be accepted");
    assert_eq!(template.is_rtl, Some(true));

    fs::remove_dir_all(&dir).ok();
}
