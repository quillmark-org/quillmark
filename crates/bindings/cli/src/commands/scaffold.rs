use crate::errors::{CliError, Result};
use clap::Parser;
use quillmark_pdfform::{scaffold_quill_yaml, FormSpec};
use std::path::{Path, PathBuf};

#[derive(Parser)]
pub struct ScaffoldArgs {
    /// Path to a pdfform `form.json` field-spec file
    #[arg(value_name = "FORM_JSON")]
    form_json: PathBuf,

    /// Quill name to embed in the generated Quill.yaml (default: parent
    /// directory name, sanitized to snake_case, falling back to "quill")
    #[arg(long, value_name = "NAME")]
    name: Option<String>,
}

pub fn execute(args: ScaffoldArgs) -> Result<()> {
    let bytes = std::fs::read(&args.form_json)?;

    let spec = FormSpec::parse(&bytes).map_err(|e| {
        CliError::InvalidArgument(format!(
            "Failed to parse {}: {}",
            args.form_json.display(),
            e
        ))
    })?;

    let name = args
        .name
        .unwrap_or_else(|| derive_quill_name(&args.form_json));

    let yaml = scaffold_quill_yaml(&spec, &name);
    print!("{}", yaml);
    Ok(())
}

/// Derive a quill name from `form_json`'s location. Walks the path's ancestor
/// directories (closest first) and returns the first whose name sanitizes to a
/// valid quill identifier — so a standard `<name>/<version>/form.json` layout
/// yields `<name>` (skipping the version dir, which is not a valid quill name
/// because it starts with a digit). Falls back to `"quill"`.
fn derive_quill_name(form_json: &Path) -> String {
    form_json
        .ancestors()
        .skip(1) // the file itself
        .filter_map(|dir| dir.file_name().and_then(|n| n.to_str()))
        .filter_map(sanitize_quill_name)
        .next()
        .unwrap_or_else(|| "quill".to_string())
}

/// Lowercase `raw`, map non-alphanumerics to `_`, and trim underscores. Returns
/// `Some(name)` only when the result is a valid quill identifier (matches the
/// `snake_case` rule the quill loader enforces: leading `[a-z]`, then
/// `[a-z0-9_]`), else `None`.
fn sanitize_quill_name(raw: &str) -> Option<String> {
    let sanitized: String = raw
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let sanitized = sanitized.trim_matches('_').to_string();
    let mut chars = sanitized.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => Some(sanitized),
        _ => None,
    }
}
