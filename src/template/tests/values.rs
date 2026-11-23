use super::*;

#[test]
fn values_schema_rejects_missing_wrong_and_extra_fields() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");

    let missing = load_and_render(&dir, "text", &json!({})).expect_err("missing field");
    assert!(matches!(missing, TemplateError::Values(_)));
    assert!(missing.to_string().contains("message"));

    let wrong = load_and_render(&dir, "text", &json!({"message": 42})).expect_err("wrong type");
    assert!(matches!(wrong, TemplateError::Values(_)));

    let extra = load_and_render(&dir, "text", &json!({"message": "hello", "extra": true}))
        .expect_err("extra property");
    assert!(matches!(extra, TemplateError::Values(_)));
}

#[test]
fn template_without_values_schema_accepts_any_object() {
    let dir = temp_template_dir("unconstrained-values");
    fs::write(
        dir.join("plain.yaml"),
        "mode: text\ntext: hello ${anything}\n",
    )
    .expect("template should be written");

    let template = load_and_render(&dir, "plain", &json!({"anything": "value"}))
        .expect("unconstrained values should be accepted");
    assert_eq!(template.text.as_deref(), Some("hello value"));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn values_metadata_is_not_rendered() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let values: Value = serde_json::from_str(
        &fs::read_to_string(root.join("examples/values/example.json"))
            .expect("values should be readable"),
    )
    .expect("values should be valid JSON");
    let template = load_and_render(&root.join("templates"), "example", &values)
        .expect("example should render");
    let rendered = serde_json::to_value(template).expect("template should serialize");
    assert!(rendered.get("values").is_none());
}

#[test]
fn example_values_files_validate_against_template_values_schemas() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cases = [
        ("example", "example.json"),
        ("example", "example.yaml"),
        ("rich-html-kitchen-sink", "rich-html-kitchen-sink.json"),
        ("rich-html-kitchen-sink", "rich-html-kitchen-sink.yaml"),
        (
            "rich-markdown-kitchen-sink",
            "rich-markdown-kitchen-sink.json",
        ),
        (
            "rich-markdown-kitchen-sink",
            "rich-markdown-kitchen-sink.yaml",
        ),
        ("rich-blocks-example", "rich-blocks-example.json"),
    ];

    for (template_name, values_name) in cases {
        let path = root.join("examples/values").join(values_name);
        let values = crate::parsing::parse_values_argument(path.to_str().unwrap())
            .unwrap_or_else(|error| panic!("{values_name} should parse: {error}"));
        load_and_render(&root.join("templates"), template_name, &values).unwrap_or_else(|error| {
            panic!("{values_name} should validate against {template_name}: {error}")
        });
    }
}
