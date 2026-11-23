#[allow(dead_code)]
#[path = "src/config.rs"]
mod config;

use std::env;
use std::fs;
use std::path::PathBuf;

use schemars::schema_for;

fn main() -> anyhow::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/config.rs");

    let schemas_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("schemas");
    fs::create_dir_all(&schemas_dir)?;

    let schema = schema_for!(config::Config);
    let json = serde_json::to_string_pretty(&schema)?;
    let content = format!("{json}\n");
    write_if_changed(&schemas_dir.join("config.schema.json"), &content)?;
    Ok(())
}

fn write_if_changed(path: &std::path::Path, content: &str) -> anyhow::Result<()> {
    if fs::read_to_string(path).ok().as_deref() == Some(content) {
        return Ok(());
    }
    fs::write(path, content)?;
    Ok(())
}
