use super::*;

#[test]
fn interpolates_values() {
    let rendered = render(r#"{"text": "<b>${title}</b>"}"#, &json!({"title": "hi"}))
        .expect("render should succeed");
    assert_eq!(rendered["text"], "<b>hi</b>");
}

#[test]
fn if_operator_on_and_off() {
    let source = r#"{"text": "x", "skip_entity_detection": {"$if": "urgent", "then": true}}"#;
    let on = render(source, &json!({"urgent": true})).expect("render should succeed");
    let off = render(source, &json!({"urgent": false})).expect("render should succeed");
    assert_eq!(on["skip_entity_detection"], true);
    assert!(off.get("skip_entity_detection").is_none());
}

#[test]
fn map_operator_builds_a_list() {
    let source = r#"{"lines": {"$map": {"$eval": "items"}, "each(item)": "- ${item}"}}"#;
    let rendered = render(source, &json!({"items": ["a", "b"]})).expect("render should succeed");
    assert_eq!(rendered["lines"], json!(["- a", "- b"]));
}

#[test]
fn quotes_newlines_and_backslashes_stay_plain() {
    let value = "he said \"hi\"\nback\\slash";
    let source = r#"{"text": "prefix ${value} suffix"}"#;
    let rendered = render(source, &json!({"value": value})).expect("render should succeed");
    assert_eq!(rendered["text"], format!("prefix {value} suffix"));
}

#[test]
fn render_error_is_reported() {
    let error = render(r#"{"text": {"$eval": "missing_var + 1"}}"#, &json!({}))
        .expect_err("undefined variable should fail");
    assert!(matches!(error, TemplateError::Render(_)));
}

#[test]
fn invalid_template_yaml_is_an_error() {
    let error =
        parse_template("mode: html\ntext: [unterminated").expect_err("invalid YAML should fail");
    assert!(matches!(error, TemplateError::Yaml(_)));
}

#[test]
fn validates_template_names() {
    for name in ["alert", "a_b-1", "A1"] {
        assert!(is_valid_template_name(name), "{name} should be valid");
    }
    for name in ["", "../alert", "a/b", "a.json", "a b", "alert!", "é"] {
        assert!(!is_valid_template_name(name), "{name} should be invalid");
    }
}

#[test]
fn yaml_anchors_and_aliases_are_resolved() {
    let value = parse_template("mode: html\ntext: &message hello\ncopy: *message")
        .expect("aliases should resolve");
    assert_eq!(value["copy"], "hello");
}

#[test]
fn yml_extension_is_accepted() {
    let dir = temp_template_dir("yml-extension");
    fs::write(dir.join("alert.yml"), "mode: html\ntext: hello")
        .expect("template should be written");

    let template = load_and_render(&dir, "alert", &json!({})).expect("yml template should render");
    assert_eq!(template.text.as_deref(), Some("hello"));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn yaml_tags_are_rejected() {
    let error = parse_template("mode: !custom html").expect_err("tags should be rejected");
    assert!(
        error.to_string().contains("unsupported YAML tag"),
        "unclear error: {error}"
    );
}

#[test]
fn numeric_yaml_keys_are_coerced_to_strings() {
    let value = parse_template("1: value").expect("numeric keys are coerced");
    assert_eq!(value["1"], "value");
}

#[test]
fn missing_template_is_not_found() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    let error = load_raw(&dir, "does-not-exist").expect_err("template should be missing");
    assert!(matches!(error, TemplateError::NotFound));
}
