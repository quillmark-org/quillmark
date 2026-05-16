use crate::errors::{CliError, Result};
use clap::Parser;
use quillmark::Quillmark;
use std::path::PathBuf;

/// Standard metadata keys that are surfaced as top-level fields in the output.
/// These are excluded from the "additional metadata" section.
const STANDARD_METADATA_KEYS: &[&str] = &["backend", "version", "author", "description"];

#[derive(Parser)]
pub struct InfoArgs {
    /// Path to quill directory
    #[arg(value_name = "QUILL_PATH")]
    quill_path: PathBuf,

    /// Output as JSON instead of human-readable format
    #[arg(long)]
    json: bool,
}

pub fn execute(args: InfoArgs) -> Result<()> {
    // Validate quill path exists
    if !args.quill_path.exists() {
        return Err(CliError::InvalidArgument(format!(
            "Quill directory not found: {}",
            args.quill_path.display()
        )));
    }

    // Load Quill
    let engine = Quillmark::new();
    let quill = engine.quill_from_path(&args.quill_path)?;

    if args.json {
        print_json(&quill)?;
    } else {
        print_human_readable(&quill);
    }

    Ok(())
}

fn print_json(quill: &quillmark::Quill) -> Result<()> {
    let source = quill.source();
    // Build a JSON object with the metadata
    let mut info = serde_json::Map::new();
    info.insert(
        "name".to_string(),
        serde_json::Value::String(source.name().to_string()),
    );
    info.insert(
        "backend".to_string(),
        serde_json::Value::String(quill.backend_id().to_string()),
    );

    // Extract metadata fields: version, author, description
    let metadata = source.metadata();
    if let Some(version) = metadata.get("version") {
        info.insert("version".to_string(), version.as_json().clone());
    }
    if let Some(author) = metadata.get("author") {
        info.insert("author".to_string(), author.as_json().clone());
    }
    if let Some(description) = metadata.get("description") {
        info.insert("description".to_string(), description.as_json().clone());
    }

    // Add counts
    info.insert(
        "field_count".to_string(),
        serde_json::Value::Number(source.config().main.fields.len().into()),
    );
    let card_count = source.config().cards.len();
    if card_count > 0 {
        info.insert(
            "card_count".to_string(),
            serde_json::Value::Number(card_count.into()),
        );
    }
    info.insert(
        "has_plate".to_string(),
        serde_json::Value::Bool(source.plate().is_some()),
    );

    // Add any additional metadata (excluding the standard fields already included)
    let mut extra_metadata = serde_json::Map::new();
    for (key, value) in metadata {
        if !STANDARD_METADATA_KEYS.contains(&key.as_str()) {
            extra_metadata.insert(key.clone(), value.as_json().clone());
        }
    }
    if !extra_metadata.is_empty() {
        info.insert(
            "metadata".to_string(),
            serde_json::Value::Object(extra_metadata),
        );
    }

    let json_str = serde_json::to_string_pretty(&serde_json::Value::Object(info))
        .map_err(|e| CliError::InvalidArgument(format!("Failed to serialize info: {}", e)))?;
    println!("{}", json_str);

    Ok(())
}

fn print_human_readable(quill: &quillmark::Quill) {
    let source = quill.source();
    let metadata = source.metadata();
    let config = source.config();
    println!("Quill: {}", source.name());

    if let Some(description) = metadata.get("description") {
        if let Some(desc_str) = description.as_str() {
            if !desc_str.is_empty() {
                println!("  Description: {}", desc_str);
            }
        }
    }

    if let Some(version) = metadata.get("version") {
        if let Some(ver_str) = version.as_str() {
            println!("  Version:     {}", ver_str);
        }
    }

    if let Some(author) = metadata.get("author") {
        if let Some(auth_str) = author.as_str() {
            println!("  Author:      {}", auth_str);
        }
    }

    println!("  Backend:     {}", quill.backend_id());

    // Field count from schema properties
    let field_count = config.main.fields.len();
    println!("  Fields:      {}", field_count);

    // Card count from schema $defs
    let card_count = config.cards.len();
    if card_count > 0 {
        println!("  Cards:       {}", card_count);
    }

    // Defaults and examples
    let defaults_count = config.main.defaults().len();
    if defaults_count > 0 {
        println!("  Defaults:    {}", defaults_count);
    }

    // Plate
    println!(
        "  Has plate:   {}",
        if source.plate().is_some() {
            "yes"
        } else {
            "no"
        }
    );

    // Additional metadata
    let extra_keys: Vec<&String> = metadata
        .keys()
        .filter(|k| !STANDARD_METADATA_KEYS.contains(&k.as_str()))
        .collect();
    if !extra_keys.is_empty() {
        println!("  Metadata:");
        for key in extra_keys {
            if let Some(value) = metadata.get(key) {
                println!("    {}: {}", key, format_metadata_value(value));
            }
        }
    }
}

fn format_metadata_value(value: &quillmark_core::QuillValue) -> String {
    let json = value.as_json();
    match json {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            let items: Vec<String> = arr
                .iter()
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect();
            items.join(", ")
        }
        other => other.to_string(),
    }
}
