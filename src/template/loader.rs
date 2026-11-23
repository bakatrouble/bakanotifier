use std::fs;
use std::path::Path;

use serde_json::Value;

use super::error::TemplateError;
use super::model::RichTemplate;

#[must_use]
pub fn is_valid_template_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

pub fn load_raw(templates_dir: &Path, name: &str) -> Result<String, TemplateError> {
    if !is_valid_template_name(name) {
        return Err(TemplateError::Invalid(format!(
            "invalid template name {name:?}"
        )));
    }
    let yaml = templates_dir.join(format!("{name}.yaml"));
    let yml_path = templates_dir.join(format!("{name}.yml"));
    let path = if yaml.is_file() {
        yaml
    } else if yml_path.is_file() {
        yml_path
    } else {
        return Err(TemplateError::NotFound);
    };
    fs::read_to_string(&path)
        .map_err(|error| TemplateError::Io(format!("{}: {error}", path.display())))
}

pub fn load_and_render(
    templates_dir: &Path,
    name: &str,
    values: &Value,
) -> Result<RichTemplate, TemplateError> {
    let source = load_raw(templates_dir, name)?;
    let mut template = parse_template(&source)?;
    validate_values(&mut template, values)?;
    let rendered = json_e::render(&template, values)
        .map_err(|error| TemplateError::Render(error.to_string()))?;
    let template: RichTemplate = serde_json::from_value(rendered)
        .map_err(|error| TemplateError::Invalid(error.to_string()))?;
    template.validate()?;
    Ok(template)
}

fn validate_values(template: &mut Value, values: &Value) -> Result<(), TemplateError> {
    let Some(object) = template.as_object_mut() else {
        return Ok(());
    };
    let Some(schema) = object.remove("values") else {
        return Ok(());
    };
    if !schema.is_object() {
        return Err(TemplateError::Invalid(
            "template values must be a JSON Schema object".to_string(),
        ));
    }
    let validator = jsonschema::validator_for(&schema).map_err(|error| {
        TemplateError::Invalid(format!("invalid template values schema: {error}"))
    })?;
    let violations = validator
        .iter_errors(values)
        .map(|error| {
            let path = error.instance_path().to_string();
            let path = if path.is_empty() { "/" } else { path.as_str() };
            format!("{path}: {error}")
        })
        .collect::<Vec<_>>();
    if !violations.is_empty() {
        return Err(TemplateError::Values(violations.join("; ")));
    }
    Ok(())
}

/// Parse the YAML template source into a JSON value. Anchors and aliases are
/// resolved by serde-saphyr; mapping keys are coerced to strings by serde_json and tags are rejected.
pub fn parse_template(source: &str) -> Result<Value, TemplateError> {
    crate::parsing::parse_yaml_value(source).map_err(TemplateError::Yaml)
}
