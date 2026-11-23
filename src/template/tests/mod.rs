use crate::template::*;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn render(source: &str, values: &Value) -> Result<Value, TemplateError> {
    let template = parse_template(source)?;
    json_e::render(&template, values).map_err(|error| TemplateError::Render(error.to_string()))
}

pub(super) fn temp_template_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "bakanotifier-template-test-{}-{name}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("temp dir should be created");
    dir
}

pub(super) fn render_kitchen_sink(template_name: &str, values_name: &str) -> RichTemplate {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let values_source = fs::read_to_string(root.join("examples/values").join(values_name))
        .expect("kitchen-sink values should be readable");
    let values: Value =
        serde_json::from_str(&values_source).expect("kitchen-sink values should be valid JSON");
    load_and_render(&root.join("templates"), template_name, &values)
        .expect("kitchen-sink template should render")
}

mod defaults;
mod render;
mod rich;
mod rich_blocks;
mod values;
