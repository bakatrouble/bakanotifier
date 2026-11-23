//! Shared JSON/YAML value parsing used by templates, request bodies and values.

use std::path::Path;

use serde_json::Value;

pub fn parse_json_value(source: &str) -> Result<Value, String> {
    serde_json::from_str(source).map_err(|error| error.to_string())
}

/// Parse YAML into a JSON value using serde-saphyr.
///
/// Duplicate keys and non-finite floats are errors. Leading-zero and
/// underscored numeric spellings stay strings, booleans are strict YAML 1.2
/// (`yes`/`on` stay strings), and null is not converted to a string. Tags and
/// mapping keys are coerced to strings by serde_json.
pub fn parse_yaml_value(source: &str) -> Result<Value, String> {
    serde_saphyr::from_str_with_options(source, yaml_options()).map_err(format_yaml_error)
}

fn yaml_options() -> serde_saphyr::Options {
    serde_saphyr::options! {
        duplicate_keys: serde_saphyr::DuplicateKeyPolicy::Error,
        non_finite_float_policy: serde_saphyr::NonFiniteFloatPolicy::Reject,
        leading_zero_numbers_as_strings: true,
        underscored_numbers_as_strings: true,
        null_as_string: false,
        strict_booleans: true,
        reject_unsupported_tags: true,
    }
}

fn format_yaml_error(error: serde_saphyr::Error) -> String {
    let message = error.to_string();
    if message.contains("unsupported tag") {
        format!("unsupported YAML tag: {message}")
    } else {
        message
    }
}

/// Parse a `--values` argument from an existing file.
pub fn parse_values_argument(argument: &str) -> Result<Value, String> {
    let path = Path::new(argument);
    let source = std::fs::read_to_string(path)
        .map_err(|error| format!("reading values file {}: {error}", path.display()))?;

    let value = match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => parse_json_value(&source)
            .map_err(|error| format!("parsing values file as JSON: {error}"))?,
        Some("yaml" | "yml") => parse_yaml_value(&source)
            .map_err(|error| format!("parsing values file as YAML: {error}"))?,
        _ => parse_json_then_yaml(&source)?,
    };

    if !value.is_object() {
        return Err("values must be a JSON/YAML object".to_string());
    }
    Ok(value)
}

fn parse_json_then_yaml(source: &str) -> Result<Value, String> {
    match parse_json_value(source) {
        Ok(value) => Ok(value),
        Err(json_error) => match parse_yaml_value(source) {
            Ok(value) => Ok(value),
            Err(yaml_error) => Err(format!(
                "values could not be parsed as JSON ({json_error}) or YAML ({yaml_error})"
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bakanotifier-values-test-{}-{name}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn yaml_anchors_and_aliases_resolve() {
        let value = parse_yaml_value("base: &base hello\ncopy: *base\n").unwrap();
        assert_eq!(value["copy"], "hello");
    }

    #[test]
    fn yaml_strict_booleans_keep_yes_as_string() {
        let value = parse_yaml_value("a: yes\nb: on\nc: true\nd: false\n").unwrap();
        assert_eq!(value["a"], "yes");
        assert_eq!(value["b"], "on");
        assert_eq!(value["c"], true);
        assert_eq!(value["d"], false);
    }

    #[test]
    fn yaml_number_like_scalars_stay_strings() {
        let value = parse_yaml_value("octal: 0755\nunderscored: 1_000\n").unwrap();
        assert_eq!(value["octal"], "0755");
        assert_eq!(value["underscored"], "1_000");
    }

    #[test]
    fn yaml_null_is_null_not_string() {
        let value = parse_yaml_value("value: null\nother: ~\n").unwrap();
        assert!(value["value"].is_null());
        assert!(value["other"].is_null());
    }

    #[test]
    fn yaml_numeric_and_boolean_keys_are_coerced_to_strings() {
        let value = parse_yaml_value("1: numeric\ntrue: boolean\n").unwrap();
        assert_eq!(value["1"], "numeric");
        assert_eq!(value["true"], "boolean");
    }

    #[test]
    fn yaml_null_keys_are_rejected() {
        let error = parse_yaml_value("null: null-key\n").expect_err("null key should fail");
        assert!(error.contains("null"), "unexpected error: {error}");
    }

    #[test]
    fn yaml_numeric_and_quoted_keys_collide_after_coercion() {
        let value = parse_yaml_value("1: numeric\n\"1\": quoted\n").unwrap();
        assert_eq!(value["1"], "quoted");
    }

    #[test]
    fn yaml_tags_are_rejected() {
        let error = parse_yaml_value("value: !custom text").expect_err("tag should fail");
        assert!(
            error.contains("tag") || error.contains("Tag"),
            "unclear error: {error}"
        );
    }

    #[test]
    fn yaml_duplicate_keys_are_rejected() {
        assert!(parse_yaml_value("a: 1\na: 2\n").is_err());
    }

    #[test]
    fn yaml_non_finite_floats_are_rejected() {
        assert!(parse_yaml_value("value: .nan\n").is_err());
    }

    #[test]
    fn parses_json_and_yaml_files() {
        let dir = temp_dir("files");
        let json = dir.join("values.json");
        let yaml = dir.join("values.yaml");
        fs::write(&json, r#"{"message": "hello"}"#).unwrap();
        fs::write(&yaml, "message: hello\n").unwrap();

        assert_eq!(
            parse_values_argument(json.to_str().unwrap()).unwrap()["message"],
            "hello"
        );
        assert_eq!(
            parse_values_argument(yaml.to_str().unwrap()).unwrap()["message"],
            "hello"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_non_object_values() {
        let dir = temp_dir("non-object");
        let json = dir.join("array.json");
        let yaml = dir.join("scalar.yaml");
        fs::write(&json, "[1, 2]").unwrap();
        fs::write(&yaml, "hello\n").unwrap();

        assert!(parse_values_argument(json.to_str().unwrap()).is_err());
        assert!(parse_values_argument(yaml.to_str().unwrap()).is_err());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_yaml_tags_in_values() {
        let dir = temp_dir("tagged");
        let path = dir.join("tagged.yaml");
        fs::write(&path, "!custom {message: hello}\n").unwrap();

        let error = parse_values_argument(path.to_str().unwrap()).unwrap_err();
        assert!(error.contains("unsupported YAML tag"), "{error}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_values_file_names_the_path() {
        let error = parse_values_argument("/nonexistent/values.json").unwrap_err();
        assert!(error.contains("/nonexistent/values.json"), "{error}");
    }
}
