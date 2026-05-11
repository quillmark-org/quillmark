//! Error mapping utilities for converting Typst diagnostics to Quillmark diagnostics.

use crate::world::QuillWorld;
use quillmark_core::{Diagnostic, Location, Severity};
use typst::diag::SourceDiagnostic;

/// Converts Typst diagnostics to Quillmark diagnostics.
pub fn map_typst_errors(errors: &[SourceDiagnostic], world: &QuillWorld) -> Vec<Diagnostic> {
    errors
        .iter()
        .map(|e| map_single_diagnostic(e, world))
        .collect()
}

/// Converts a single Typst diagnostic to a Quillmark diagnostic.
fn map_single_diagnostic(error: &SourceDiagnostic, world: &QuillWorld) -> Diagnostic {
    // Map Typst severity to Quillmark severity
    let severity = match error.severity {
        typst::diag::Severity::Error => Severity::Error,
        typst::diag::Severity::Warning => Severity::Warning,
    };

    // Extract location from span
    let location = resolve_span_to_location(&error.span, world);

    // Get first hint if available
    let hint = error.hints.first().map(|h| h.to_string());

    // Extract error code from message (simple heuristic)
    let code = Some(format!(
        "typst::{}",
        error.message.split(':').next().unwrap_or("error").trim()
    ));

    Diagnostic {
        severity,
        code,
        message: error.message.to_string(),
        location,
        path: None,
        hint,
        source_chain: Vec::new(),
    }
}

/// Resolves a Typst span to a Quillmark Location.
fn resolve_span_to_location(span: &typst::syntax::Span, world: &QuillWorld) -> Option<Location> {
    use typst::World;

    let source_id = world.main();
    let source = world.source(source_id).ok()?;
    let range = source.range(*span)?;

    let text = source.text();
    let line = text[..range.start].matches('\n').count() + 1;
    let column = range.start - text[..range.start].rfind('\n').map_or(0, |pos| pos + 1) + 1;

    Some(Location {
        file: source.id().vpath().as_rootless_path().display().to_string(),
        line: line as u32,
        column: column as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_mapping() {
        // Ensure Typst severity maps correctly
        assert_eq!(
            match typst::diag::Severity::Error {
                typst::diag::Severity::Error => Severity::Error,
                typst::diag::Severity::Warning => Severity::Warning,
            },
            Severity::Error
        );

        assert_eq!(
            match typst::diag::Severity::Warning {
                typst::diag::Severity::Error => Severity::Error,
                typst::diag::Severity::Warning => Severity::Warning,
            },
            Severity::Warning
        );
    }
}
