use crate::errors::{CliError, Result};
use clap::Parser;
use quillmark::Quillmark;
use quillmark_core::quill::{validate_schema_literal, CardSchema, FieldSchema, QuillConfig};
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

    // Step 3: Validate field schemas including defaults
    validate_field_schemas(&config.main.fields, &mut result, "field");

    // Step 4: Validate card-kind schemas
    for card_schema in &config.card_kinds {
        validate_card_schema(&card_schema.name, card_schema, &mut result);
    }

    // Step 5: Try to load the full Quill (this validates schema generation)
    let engine = Quillmark::new();
    match engine.quill_from_path(&args.quill_path) {
        Ok(quill) => {
            if args.verbose {
                println!("  Schema generated successfully");
                println!(
                    "  Defaults extracted: {}",
                    quill.source().config().main.defaults().len()
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
    // Check plate_file reference
    if let Some(ref plate_file) = config.plate_file {
        let plate_path = quill_path.join(plate_file);
        if !plate_path.exists() {
            result.add_error(format!(
                "Referenced plate_file '{}' does not exist",
                plate_file
            ));
        }
    }
}

fn validate_field_schemas(
    fields: &BTreeMap<String, FieldSchema>,
    result: &mut ValidationResult,
    context: &str,
) {
    for (field_name, field_schema) in fields {
        let path = format!("{context} '{field_name}'");

        // Validate example and default via the shared conformance primitive.
        if let Some(ref example) = field_schema.example {
            for err in validate_schema_literal(field_schema, example, &path) {
                result.add_error(format!("{path} example: {err}"));
            }
        }
        if let Some(ref default) = field_schema.default {
            for err in validate_schema_literal(field_schema, default, &path) {
                result.add_error(format!("{path} default: {err}"));
            }
        }

        // Warn about empty enum constraint and missing description.
        if let Some(ref enum_values) = field_schema.enum_values {
            if enum_values.is_empty() {
                result.add_warning(format!("{context} '{field_name}': enum constraint is empty"));
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
    // Warn about missing description
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

    // Validate card fields
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
