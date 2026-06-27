//! `quillmark qualify <INPUT_PDF> <OUTPUT_DIR> [--password <PW>] [--name <NAME>]`
//!
//! The inverse of the stamping spine: turn a real AcroForm PDF into the two
//! assets a `pdfform` quill ships — a stripped `form.pdf` background and a
//! `form.json` field spec — plus a starter `Quill.yaml` scaffolded from the
//! produced spec (composing with the `scaffold` machinery).

use crate::errors::{CliError, Result};
use clap::Parser;
use quillmark_pdfform::{scaffold_quill_yaml, FormSpec};
use quillmark_qualify::qualify;
use std::path::{Path, PathBuf};

#[derive(Parser)]
pub struct QualifyArgs {
    /// Path to the source AcroForm PDF (e.g. a government form)
    #[arg(value_name = "INPUT_PDF")]
    input_pdf: PathBuf,

    /// Directory to write `form.pdf`, `form.json`, and a starter `Quill.yaml`
    /// (created if absent)
    #[arg(value_name = "OUTPUT_DIR")]
    output_dir: PathBuf,

    /// Password for an encrypted input PDF (defaults to the empty password)
    #[arg(long, value_name = "PW")]
    password: Option<String>,

    /// Quill name to embed in the generated Quill.yaml (default: sanitized
    /// OUTPUT_DIR name, falling back to the INPUT_PDF stem, then "quill")
    #[arg(long, value_name = "NAME")]
    name: Option<String>,
}

pub fn execute(args: QualifyArgs) -> Result<()> {
    let pdf_bytes = std::fs::read(&args.input_pdf)?;

    // Qualify: decrypt → extract → strip → re-serialize. `?` maps QualifyError
    // → CliError::Qualify via the `From` impl.
    let qualified = qualify(&pdf_bytes, args.password.as_deref())?;

    // Create the output directory if needed.
    std::fs::create_dir_all(&args.output_dir)?;

    let form_pdf_path = args.output_dir.join("form.pdf");
    let form_json_path = args.output_dir.join("form.json");
    std::fs::write(&form_pdf_path, &qualified.form_pdf)?;
    std::fs::write(&form_json_path, &qualified.form_json)?;

    // Scaffold a starter Quill.yaml from the produced form.json (compose with
    // #756). Parse the spec we just emitted; it is guaranteed well-formed.
    let spec = FormSpec::parse(&qualified.form_json).map_err(|e| {
        CliError::InvalidArgument(format!("produced form.json did not re-parse: {e}"))
    })?;
    let name = args
        .name
        .unwrap_or_else(|| derive_quill_name(&args.output_dir, &args.input_pdf));
    let yaml = scaffold_quill_yaml(&spec, &name);
    let quill_yaml_path = args.output_dir.join("Quill.yaml");
    std::fs::write(&quill_yaml_path, yaml.as_bytes())?;

    eprintln!("Qualified {} into:", args.input_pdf.display());
    eprintln!("  {}", form_pdf_path.display());
    eprintln!("  {}", form_json_path.display());
    eprintln!("  {}", quill_yaml_path.display());
    eprintln!("  ({} field(s); quill name `{}`)", spec.fields.len(), name);

    Ok(())
}

/// Derive a quill name: prefer the OUTPUT_DIR's own name, then the INPUT_PDF
/// stem, then `"quill"` — each sanitized to a valid quill identifier.
fn derive_quill_name(output_dir: &Path, input_pdf: &Path) -> String {
    output_dir
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(sanitize_quill_name)
        .or_else(|| {
            input_pdf
                .file_stem()
                .and_then(|n| n.to_str())
                .and_then(sanitize_quill_name)
        })
        .unwrap_or_else(|| "quill".to_string())
}

/// Lowercase `raw`, map non-alphanumerics to `_`, trim underscores. Returns
/// `Some(name)` only when the result is a valid quill identifier (leading
/// `[a-z]`), else `None`.
fn sanitize_quill_name(raw: &str) -> Option<String> {
    let sanitized: String = raw
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let sanitized = sanitized.trim_matches('_').to_string();
    match sanitized.chars().next() {
        Some(c) if c.is_ascii_lowercase() => Some(sanitized),
        _ => None,
    }
}
