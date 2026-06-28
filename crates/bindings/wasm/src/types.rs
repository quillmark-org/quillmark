//! Type definitions for the WASM API

use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

/// Output formats supported by backends.
///
/// Gated behind the engine surface (`render` or `pdfform`) so tsify omits
/// its `.d.ts` interface from the core bundle (`pkg/core/wasm.d.ts`), which
/// has no rendering surface.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Pdf,
    Svg,
    Txt,
    Png,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<OutputFormat> for quillmark_core::OutputFormat {
    fn from(format: OutputFormat) -> Self {
        match format {
            OutputFormat::Pdf => quillmark_core::OutputFormat::Pdf,
            OutputFormat::Svg => quillmark_core::OutputFormat::Svg,
            OutputFormat::Txt => quillmark_core::OutputFormat::Txt,
            OutputFormat::Png => quillmark_core::OutputFormat::Png,
        }
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<quillmark_core::OutputFormat> for OutputFormat {
    fn from(format: quillmark_core::OutputFormat) -> Self {
        match format {
            quillmark_core::OutputFormat::Pdf => OutputFormat::Pdf,
            quillmark_core::OutputFormat::Svg => OutputFormat::Svg,
            quillmark_core::OutputFormat::Txt => OutputFormat::Txt,
            quillmark_core::OutputFormat::Png => OutputFormat::Png,
        }
    }
}

/// Severity levels for diagnostics
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Note,
}

impl From<quillmark_core::Severity> for Severity {
    fn from(severity: quillmark_core::Severity) -> Self {
        match severity {
            quillmark_core::Severity::Error => Severity::Error,
            quillmark_core::Severity::Warning => Severity::Warning,
            quillmark_core::Severity::Note => Severity::Note,
        }
    }
}

impl From<Severity> for quillmark_core::Severity {
    fn from(severity: Severity) -> Self {
        match severity {
            Severity::Error => quillmark_core::Severity::Error,
            Severity::Warning => quillmark_core::Severity::Warning,
            Severity::Note => quillmark_core::Severity::Note,
        }
    }
}

/// Source location for errors and warnings
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

impl From<quillmark_core::Location> for Location {
    fn from(loc: quillmark_core::Location) -> Self {
        Location {
            file: loc.file,
            line: loc.line as usize,
            column: loc.column as usize,
        }
    }
}

impl From<Location> for quillmark_core::Location {
    fn from(loc: Location) -> Self {
        quillmark_core::Location {
            file: loc.file,
            line: loc.line as u32,
            column: loc.column as u32,
        }
    }
}

/// Diagnostic message (error, warning, or note)
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub code: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub location: Option<Location>,
    /// Document-model path anchor (e.g. `"cards.indorsement[0].signature_block"`).
    ///
    /// Set on schema validation diagnostics; `undefined` otherwise. See the
    /// Rust `quillmark_core::error` module docs for the path grammar.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub source_chain: Vec<String>,
}

impl From<quillmark_core::Diagnostic> for Diagnostic {
    fn from(diag: quillmark_core::Diagnostic) -> Self {
        Diagnostic {
            severity: diag.severity.into(),
            code: diag.code,
            message: diag.message,
            location: diag.location.map(Into::into),
            path: diag.path,
            hint: diag.hint,
            source_chain: diag.source_chain,
        }
    }
}

impl From<Diagnostic> for quillmark_core::Diagnostic {
    fn from(diag: Diagnostic) -> Self {
        quillmark_core::Diagnostic {
            severity: diag.severity.into(),
            code: diag.code,
            message: diag.message,
            location: diag.location.map(Into::into),
            path: diag.path,
            hint: diag.hint,
            source_chain: diag.source_chain,
        }
    }
}

/// Rendered artifact (PDF, SVG, etc.).
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub format: OutputFormat,
    /// Serialized via `serde_bytes` so `serde_wasm_bindgen` emits a real
    /// `Uint8Array` at the boundary instead of a `number[]`. Without this
    /// annotation, the declared `Uint8Array` type would silently lie.
    #[serde(with = "serde_bytes")]
    #[tsify(type = "Uint8Array")]
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl Artifact {
    fn mime_type_for_format(format: OutputFormat) -> String {
        quillmark_core::OutputFormat::from(format)
            .mime_type()
            .to_string()
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<quillmark_core::Artifact> for Artifact {
    fn from(artifact: quillmark_core::Artifact) -> Self {
        let format = artifact.output_format.into();
        Artifact {
            format,
            mime_type: Self::mime_type_for_format(format),
            bytes: artifact.bytes,
        }
    }
}

/// Result of a render operation.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    pub artifacts: Vec<Artifact>,
    pub warnings: Vec<Diagnostic>,
    pub output_format: OutputFormat,
    pub render_time_ms: f64,
    /// Form-field regions from stamped AcroForm backends (`pdfform`;
    /// Typst signature overlay). Ordered by page then field-spec order.
    /// Always an array — empty for backends / formats that produce no
    /// field geometry. Geometry for interactive overlays drawn on top of
    /// the complete raster; consumers never need them to composite a value
    /// (the canvas backends pre-flatten values into the page).
    #[serde(default)]
    pub regions: Vec<FieldRegion>,
}

/// A form-field region: geometry and bound value from a stamped AcroForm.
/// Emitted by backends that stamp form fields. Consumers use this geometry
/// to position interactive overlays on top of an already-complete raster.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct FieldRegion {
    /// Fully-qualified field name (matches the AcroForm widget `/T`).
    pub name: String,
    /// 0-based page index.
    pub page: usize,
    /// `[x0, y0, x1, y1]` in PDF points (1/72″), bottom-left origin.
    pub rect: [f32; 4],
    pub kind: FieldRegionKind,
}

/// The kind and payload of a [`FieldRegion`]. An open enum — future region
/// types extend here without breaking existing consumers.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FieldRegionKind {
    /// An interactive form field stamped onto the page.
    Field {
        /// Lowercase type id: `"text"`, `"checkbox"`, `"choice"`, or `"signature"`.
        #[serde(rename = "fieldType")]
        field_type: String,
        /// The bound value, or `undefined` for a blank / unbound field.
        /// `default` pairs with `skip_serializing_if` so the declared
        /// `from_wasm_abi` round-trip is total: a blank field omits the key on
        /// the way out and deserializes back to `None` rather than erroring with
        /// `missing field 'value'`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<String>,
    },
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<quillmark_core::RenderedRegion> for FieldRegion {
    fn from(r: quillmark_core::RenderedRegion) -> Self {
        FieldRegion {
            name: r.name,
            page: r.page,
            rect: r.rect,
            kind: r.kind.into(),
        }
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<quillmark_core::RegionKind> for FieldRegionKind {
    fn from(k: quillmark_core::RegionKind) -> Self {
        match k {
            quillmark_core::RegionKind::Field { field_type, value } => {
                FieldRegionKind::Field { field_type, value }
            }
        }
    }
}

/// Options for rendering.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct RenderOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<OutputFormat>,
    /// Pixels per inch for raster output formats (PNG).
    /// Ignored for vector/document formats (PDF, SVG, TXT).
    /// Defaults to 144.0 (2x at 72pt/inch) when omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ppi: Option<f32>,
    /// Optional 0-based page indices to render (e.g., `[0, 2]` for the
    /// first and third pages). `undefined` renders all pages. Any index
    /// `>= pageCount` causes the render to throw — read
    /// `RenderSession.pageCount` first if validation is needed.
    /// **Not supported for PDF output** — passing `pages` with
    /// `format: "pdf"` yields a `FormatNotSupported` error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<Vec<usize>>,
    /// Override for the PDF `/Info` `/Producer` metadata string. Omit to use
    /// the default (`Quillmark <version>`). Applies to PDF output only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            format: Some(OutputFormat::Pdf),
            ppi: None,
            pages: None,
            producer: None,
        }
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<RenderOptions> for quillmark_core::RenderOptions {
    fn from(opts: RenderOptions) -> Self {
        Self {
            output_format: opts.format.map(|f| f.into()),
            ppi: opts.ppi,
            pages: opts.pages,
            producer: opts.producer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── OutputFormat ──────────────────────────────────────────────────────────

    #[test]
    #[cfg(any(feature = "typst", feature = "pdfform"))]
    fn test_output_format_serialization() {
        let pdf = OutputFormat::Pdf;
        let json_pdf = serde_json::to_string(&pdf).unwrap();
        assert_eq!(json_pdf, "\"pdf\"");

        let svg = OutputFormat::Svg;
        let json_svg = serde_json::to_string(&svg).unwrap();
        assert_eq!(json_svg, "\"svg\"");

        let txt = OutputFormat::Txt;
        let json_txt = serde_json::to_string(&txt).unwrap();
        assert_eq!(json_txt, "\"txt\"");
    }

    #[test]
    #[cfg(any(feature = "typst", feature = "pdfform"))]
    fn test_output_format_deserialization() {
        let pdf: OutputFormat = serde_json::from_str("\"pdf\"").unwrap();
        assert_eq!(pdf, OutputFormat::Pdf);

        let svg: OutputFormat = serde_json::from_str("\"svg\"").unwrap();
        assert_eq!(svg, OutputFormat::Svg);

        let txt: OutputFormat = serde_json::from_str("\"txt\"").unwrap();
        assert_eq!(txt, OutputFormat::Txt);
    }

    #[test]
    fn test_severity_serialization() {
        let error = Severity::Error;
        let json_error = serde_json::to_string(&error).unwrap();
        assert_eq!(json_error, "\"error\"");

        let warning = Severity::Warning;
        let json_warning = serde_json::to_string(&warning).unwrap();
        assert_eq!(json_warning, "\"warning\"");

        let note = Severity::Note;
        let json_note = serde_json::to_string(&note).unwrap();
        assert_eq!(json_note, "\"note\"");
    }

    #[test]
    fn test_severity_deserialization() {
        let error: Severity = serde_json::from_str("\"error\"").unwrap();
        assert_eq!(error, Severity::Error);

        let warning: Severity = serde_json::from_str("\"warning\"").unwrap();
        assert_eq!(warning, Severity::Warning);

        let note: Severity = serde_json::from_str("\"note\"").unwrap();
        assert_eq!(note, Severity::Note);
    }

    #[test]
    fn test_diagnostic_serialization() {
        let diag = quillmark_core::Diagnostic::new(
            quillmark_core::Severity::Error,
            "Test error message".to_string(),
        )
        .with_code("E001".to_string())
        .with_location(quillmark_core::Location {
            file: "test.typ".to_string(),
            line: 10,
            column: 5,
        })
        .with_hint("This is a hint".to_string());

        let wasm_diag: Diagnostic = diag.into();
        let json = serde_json::to_string(&wasm_diag).unwrap();

        assert!(json.contains("\"severity\":\"error\""));
        assert!(json.contains("\"code\":\"E001\""));
        assert!(json.contains("\"message\":\"Test error message\""));
        assert!(json.contains("\"hint\":\"This is a hint\""));
        assert!(json.contains("\"file\":\"test.typ\""));
        assert!(json.contains("\"line\":10"));
        assert!(json.contains("\"column\":5"));
    }

    #[test]
    fn test_diagnostic_with_source_chain() {
        let root_error = std::io::Error::new(std::io::ErrorKind::NotFound, "File not found");
        let diag = quillmark_core::Diagnostic::new(
            quillmark_core::Severity::Error,
            "Failed to load template".to_string(),
        )
        .with_code("E002".to_string())
        .with_source(&root_error);

        let wasm_diag: Diagnostic = diag.into();
        let json = serde_json::to_string(&wasm_diag).unwrap();

        assert!(json.contains("\"severity\":\"error\""));
        assert!(json.contains("\"code\":\"E002\""));
        assert!(json.contains("\"message\":\"Failed to load template\""));
        assert!(json.contains("\"sourceChain\""));
        assert!(json.contains("File not found"));
    }

    #[test]
    #[cfg(any(feature = "typst", feature = "pdfform"))]
    fn test_render_options_with_format() {
        let options = RenderOptions {
            format: Some(OutputFormat::Pdf),
            ppi: None,
            pages: None,
            producer: None,
        };
        let json = serde_json::to_string(&options).unwrap();
        assert!(json.contains("\"format\":\"pdf\""));

        let options_from_json: RenderOptions = serde_json::from_str(r#"{"format":"svg"}"#).unwrap();
        assert_eq!(options_from_json.format, Some(OutputFormat::Svg));
    }

    #[test]
    fn test_wasm_error_single_diagnostic() {
        use crate::error::WasmError;
        use quillmark_core::{Diagnostic, Location, Severity};

        let diag = Diagnostic::new(Severity::Error, "Test error message".to_string())
            .with_code("E001".to_string())
            .with_location(Location {
                file: "test.typ".to_string(),
                line: 10,
                column: 5,
            })
            .with_hint("This is a hint".to_string());

        let render_err = quillmark_core::RenderError::InvalidPayload { diags: vec![diag] };
        let wasm_err: WasmError = render_err.into();

        assert_eq!(wasm_err.message(), "Test error message");
        assert_eq!(wasm_err.diagnostics.len(), 1);
        let d = &wasm_err.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("E001"));
        assert_eq!(d.message, "Test error message");
        assert_eq!(d.hint.as_deref(), Some("This is a hint"));
        let loc = d.location.as_ref().unwrap();
        assert_eq!(loc.file, "test.typ");
        assert_eq!(loc.line, 10);
        assert_eq!(loc.column, 5);
    }

    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_wasm_error_to_js_value() {
        use crate::error::WasmError;

        let wasm_err: WasmError = "Test error".into();
        let js_value = wasm_err.to_js_value();

        assert!(!js_value.is_undefined());
        assert!(!js_value.is_null());
    }

    // ── FieldRegion / FieldRegionKind ─────────────────────────────────────────

    #[test]
    #[cfg(any(feature = "typst", feature = "pdfform"))]
    fn field_region_serializes_to_expected_shape() {
        let region = FieldRegion {
            name: "FullName".to_string(),
            page: 0,
            rect: [180.0, 672.0, 520.0, 692.0],
            kind: FieldRegionKind::Field {
                field_type: "text".to_string(),
                value: Some("Ada Lovelace".to_string()),
            },
        };
        let json = serde_json::to_string(&region).unwrap();
        assert!(json.contains("\"name\":\"FullName\""));
        assert!(json.contains("\"page\":0"));
        assert!(json.contains("\"rect\":[180.0,672.0,520.0,692.0]"));
        assert!(json.contains("\"type\":\"field\""));
        assert!(json.contains("\"fieldType\":\"text\""));
        assert!(json.contains("\"value\":\"Ada Lovelace\""));
    }

    #[test]
    #[cfg(any(feature = "typst", feature = "pdfform"))]
    fn field_region_blank_value_omitted() {
        let region = FieldRegion {
            name: "Signature".to_string(),
            page: 0,
            rect: [180.0, 422.0, 520.0, 462.0],
            kind: FieldRegionKind::Field {
                field_type: "signature".to_string(),
                value: None,
            },
        };
        let json = serde_json::to_string(&region).unwrap();
        // value:None → absent from JSON (skip_serializing_if)
        assert!(!json.contains("\"value\""));
        assert!(json.contains("\"fieldType\":\"signature\""));
    }

    #[test]
    #[cfg(any(feature = "typst", feature = "pdfform"))]
    fn field_region_blank_value_round_trips() {
        // The `from_wasm_abi` (JS→Rust) path uses the same serde derive. A blank
        // value omits the key on the way out, so deserializing that JSON back
        // must default to `None` rather than error with `missing field 'value'`.
        // (Regression guard for the `#[serde(default)]` on `value`.)
        let region = FieldRegion {
            name: "Signature".to_string(),
            page: 0,
            rect: [180.0, 422.0, 520.0, 462.0],
            kind: FieldRegionKind::Field {
                field_type: "signature".to_string(),
                value: None,
            },
        };
        let json = serde_json::to_string(&region).unwrap();
        let back: FieldRegion = serde_json::from_str(&json).expect("blank value round-trips");
        #[allow(irrefutable_let_patterns)]
        let FieldRegionKind::Field { value, .. } = back.kind else {
            panic!("expected a Field region kind");
        };
        assert_eq!(value, None);

        // Also accept JSON that omits `value` outright (the shape a JS caller
        // would hand back for an unbound field).
        let bare = r#"{"name":"Sig","page":0,"rect":[0.0,0.0,1.0,1.0],"kind":{"type":"field","fieldType":"signature"}}"#;
        let parsed: FieldRegion = serde_json::from_str(bare).expect("missing value defaults");
        #[allow(irrefutable_let_patterns)]
        let FieldRegionKind::Field { value, .. } = parsed.kind else {
            panic!("expected a Field region kind");
        };
        assert_eq!(value, None);
    }

    #[test]
    #[cfg(any(feature = "typst", feature = "pdfform"))]
    fn field_region_from_core_conversion() {
        use quillmark_core::{RegionKind, RenderedRegion};

        let core_region = RenderedRegion {
            name: "Agree".to_string(),
            page: 0,
            rect: [180.0, 538.0, 194.0, 552.0],
            kind: RegionKind::Field {
                field_type: "checkbox".to_string(),
                value: Some("Yes".to_string()),
            },
        };
        let wasm_region: FieldRegion = core_region.into();
        assert_eq!(wasm_region.name, "Agree");
        assert_eq!(wasm_region.page, 0);
        assert_eq!(wasm_region.rect, [180.0, 538.0, 194.0, 552.0]);
        // Refutable form so adding a `FieldRegionKind` variant is non-breaking
        // here; the `allow` covers the single-variant-today warning and lapses
        // once a second variant exists.
        #[allow(irrefutable_let_patterns)]
        let FieldRegionKind::Field { field_type, value } = wasm_region.kind else {
            panic!("expected a Field region kind");
        };
        assert_eq!(field_type, "checkbox");
        assert_eq!(value.as_deref(), Some("Yes"));
    }
}
