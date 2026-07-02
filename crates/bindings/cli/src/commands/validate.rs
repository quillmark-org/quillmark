use crate::errors::{CliError, Result};
use clap::Parser;
use quillmark_core::quill::{CardSchema, FieldSchema, QuillConfig};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser)]
pub struct ValidateArgs {
    /// Path to quill directory
    #[arg(value_name = "QUILL_PATH")]
    quill_path: PathBuf,

    /// Show verbose output with all validation details
    #[arg(short, long)]
    verbose: bool,
}

/// Validation issue severity
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Severity {
    Error,
    Warning,
}

/// A single validation issue
#[derive(Debug)]
struct ValidationIssue {
    severity: Severity,
    message: String,
}

impl ValidationIssue {
    fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
        }
    }

    fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
        }
    }
}

/// Result of validating a Quill configuration
#[derive(Debug, Default)]
struct ValidationResult {
    issues: Vec<ValidationIssue>,
}

impl ValidationResult {
    fn new() -> Self {
        Self { issues: Vec::new() }
    }

    fn add_error(&mut self, message: impl Into<String>) {
        self.issues.push(ValidationIssue::error(message));
    }

    fn add_warning(&mut self, message: impl Into<String>) {
        self.issues.push(ValidationIssue::warning(message));
    }

    fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.severity == Severity::Error)
    }

    fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .count()
    }

    fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Warning)
            .count()
    }
}

pub fn execute(args: ValidateArgs) -> Result<()> {
    // Validate quill path exists
    if !args.quill_path.exists() {
        return Err(CliError::InvalidArgument(format!(
            "Quill directory not found: {}",
            args.quill_path.display()
        )));
    }

    let quill_yaml_path = args.quill_path.join("Quill.yaml");
    if !quill_yaml_path.exists() {
        return Err(CliError::InvalidArgument(format!(
            "Quill.yaml not found in: {}",
            args.quill_path.display()
        )));
    }

    if args.verbose {
        println!("Validating quill at: {}", args.quill_path.display());
    }

    let mut result = ValidationResult::new();

    // Step 1: Parse the YAML config first (before full Quill load)
    let yaml_content = fs::read_to_string(&quill_yaml_path).map_err(CliError::Io)?;

    let (config, config_warnings) = match QuillConfig::from_yaml_with_warnings(&yaml_content) {
        Ok(pair) => pair,
        Err(diags) => {
            for diag in &diags {
                eprintln!("{}", diag.fmt_pretty());
            }
            eprintln!(
                "\nValidation failed: {} error(s) in Quill.yaml",
                diags.len()
            );
            return Err(CliError::InvalidArgument(
                "Quill configuration is invalid".to_string(),
            ));
        }
    };

    for diag in &config_warnings {
        result.add_warning(diag.message.clone());
    }

    if args.verbose {
        println!("  Quill name: {}", config.name);
        println!("  Backend: {}", config.backend);
        println!("  Fields: {}", config.main.fields.len());
        println!("  Cards: {}", config.card_kinds.len());
    }

    // Step 2: Validate file references
    validate_file_references(&args.quill_path, &config, &mut result);

    // Step 3: Emit schema-quality warnings (example/default type errors were
    // already caught at parse time in Step 1).
    validate_field_schemas(&config.main.fields, &mut result, "field");

    // Step 4: Validate card-kind schemas
    for card_schema in &config.card_kinds {
        validate_card_schema(&card_schema.name, card_schema, &mut result);
    }

    // Step 5: Try to load the full Quill (this validates schema generation)
    match quillmark::quill_from_path(&args.quill_path) {
        Ok(quill) => {
            if args.verbose {
                println!("  Schema generated successfully");
                println!(
                    "  Defaults extracted: {}",
                    quill.config().main.defaults().len()
                );
            }
        }
        Err(e) => {
            result.add_error(format!("Failed to load Quill: {}", e));
        }
    }

    // Print results
    print_validation_result(&result, args.verbose);

    if result.has_errors() {
        Err(CliError::InvalidArgument(format!(
            "Validation failed with {} error(s)",
            result.error_count()
        )))
    } else {
        Ok(())
    }
}

fn validate_file_references(
    quill_path: &Path,
    config: &QuillConfig,
    result: &mut ValidationResult,
) {
    // Check a backend's `plate_file` reference (Typst declares it under its
    // `typst:` section). It comes from the (untrusted) Quill.yaml, so reject
    // anything that is not a simple relative filename before touching the
    // filesystem: `Path::join` with an absolute path replaces the base
    // entirely, and `..` escapes the quill root, either of which would turn
    // `plate_path.exists()` into a host path-probing oracle.
    if let Some(plate_file) = config
        .backend_config
        .get("plate_file")
        .and_then(|v| v.as_str())
    {
        let rel = Path::new(plate_file);
        if rel
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            result.add_error(format!(
                "plate_file '{}' must be a relative path within the quill (no '..' or absolute components)",
                plate_file
            ));
        } else {
            let plate_path = quill_path.join(rel);
            if !plate_path.exists() {
                result.add_error(format!(
                    "Referenced plate_file '{}' does not exist",
                    plate_file
                ));
            }
        }
    }
}

/// Emit schema-quality *warnings* for a card's fields.
///
/// Type/enum/format errors on `example:` and `default:` literals are caught
/// authoritatively at parse time (`QuillConfig::from_yaml_with_warnings`, Step 1)
/// via the shared `validate_schema_literal` core and reported there with full
/// diagnostics. This pass only adds the advisory checks the parser does not:
/// empty enum constraints and missing field descriptions.
fn validate_field_schemas(
    fields: &BTreeMap<String, FieldSchema>,
    result: &mut ValidationResult,
    context: &str,
) {
    for (field_name, field_schema) in fields {
        if let Some(ref enum_values) = field_schema.enum_values {
            if enum_values.is_empty() {
                result.add_warning(format!(
                    "{context} '{field_name}': enum constraint is empty"
                ));
            }
        }
        if field_schema
            .description
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            result.add_warning(format!(
                "{context} '{field_name}': missing or empty description"
            ));
        }
    }
}

fn validate_card_schema(card_name: &str, card_schema: &CardSchema, result: &mut ValidationResult) {
    if card_schema
        .description
        .as_deref()
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        result.add_warning(format!(
            "card '{}': missing or empty description",
            card_name
        ));
    }

    let context = format!("card '{}' field", card_name);
    validate_field_schemas(&card_schema.fields, result, &context);
}

fn print_validation_result(result: &ValidationResult, verbose: bool) {
    let error_count = result.error_count();
    let warning_count = result.warning_count();

    // Print issues
    for issue in &result.issues {
        match issue.severity {
            Severity::Error => eprintln!("[ERROR] {}", issue.message),
            Severity::Warning => {
                if verbose {
                    eprintln!("[WARNING] {}", issue.message)
                }
            }
        }
    }

    // Print summary
    if error_count == 0 && warning_count == 0 {
        println!("Validation passed: quill configuration is valid");
    } else if error_count == 0 {
        println!("Validation passed with {} warning(s)", warning_count);
    } else {
        eprintln!(
            "Validation failed: {} error(s), {} warning(s)",
            error_count, warning_count
        );
    }
}
