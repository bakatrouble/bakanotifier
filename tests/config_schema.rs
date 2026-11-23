use std::fs;
use std::path::Path;

use jsonschema::validator_for;
use serde_json::Value;

use bakanotifier::config::CONFIG_SCHEMA;

#[test]
fn config_schema_is_up_to_date() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/config.schema.json");
    let checked_in = fs::read_to_string(path).expect("schemas/config.schema.json should exist");
    assert_eq!(
        checked_in, CONFIG_SCHEMA,
        "schemas/config.schema.json is stale; run cargo build"
    );
}

#[test]
fn config_example_matches_schema() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.toml");
    let raw = fs::read_to_string(path).expect("config example should be readable");
    let instance: Value = toml::from_str(&raw).expect("config example should be valid TOML");
    let schema: Value = serde_json::from_str(CONFIG_SCHEMA).expect("schema should be valid JSON");
    let validator = validator_for(&schema).expect("schema should be valid JSON Schema");

    if let Err(error) = validator.validate(&instance) {
        panic!("config.example.toml does not match schema: {error}");
    }
}
