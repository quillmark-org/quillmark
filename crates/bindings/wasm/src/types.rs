//! Type definitions for the WASM API

use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

/// Output formats supported by backends.
///
/// Gated behind the engine surface (`typst` or `pdfform`) so tsify omits
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
}

impl From<quillmark_core::Severity> for Severity {
    fn from(severity: quillmark_core::Severity) -> Self {
        match severity {
            quillmark_core::Severity::Error => Severity::Error,
            quillmark_core::Severity::Warning => Severity::Warning,
        }
    }
}

impl From<Severity> for quillmark_core::Severity {
    fn from(severity: Severity) -> Self {
        match severity {
            Severity::Error => quillmark_core::Severity::Error,
            Severity::Warning => quillmark_core::Severity::Warning,
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
    /// Schema-field geometry sidecar — populated only when
    /// `RenderOptions.regions` requested it; empty otherwise. The same entries
    /// `LiveSession.regions()` serves, for consumers without a live session.
    /// Page indices are document-space even under a `pages` subset render.
    pub regions: Vec<FieldRegion>,
}

/// What a committed `LiveSession.apply` changed. `dirtyPages` lists the pages
/// whose rendered content differs from the previous compile, including pages
/// the edit added; removed pages are implied by `pageCount`. A preview
/// repaints `dirty ∩ visible` and nothing else.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSet {
    pub page_count: usize,
    pub dirty_pages: Vec<usize>,
}

/// A rendered field region: the quill schema field address plus its geometry on
/// the page. Emitted for schema-bound fields — span-tracked content (richtext
/// bodies, `richtext[]` elements, card content fields, direct scalar
/// references) and form-field widgets (pdfform AcroForm, Typst `form-field`).
/// Consumers use it to scroll to / highlight the focused field; for the
/// reverse click direction use `LiveSession.fieldAt`, which answers over any
/// placement. Geometry only: the raster is already complete, so a region is
/// never a compositing input.
///
/// `field` is **not** unique: content fields surface one region **per segment**
/// (paragraph, heading, whole code fence) and per page each touches, a scalar
/// referenced at several plate sites surfaces each site, and tracked content
/// plus a `field:`-bound widget yields both. Group by `field` — every entry
/// routes to that field. The whole-field highlight is the **union of a page's
/// `span`-bearing segment rects**, so inter-paragraph whitespace stays
/// uncovered (#829). Later placements of one content value are not enumerated;
/// `fieldAt` / `positionAt` still resolve clicks on them.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct FieldRegion {
    /// Quill schema field path (e.g. `"signature_block"`,
    /// `"$cards.indorsement.1.from"`), not any backend widget name. The address
    /// the editor uses for this field.
    pub field: String,
    /// 0-based page index.
    pub page: usize,
    /// `[x0, y0, x1, y1]` in PDF points (1/72″), bottom-left origin.
    pub rect: [f32; 4],
    /// The corpus slice this box covers — USV `[start, end)` into the field's
    /// `RichText` for content ink (one segment), `undefined` for a scalar
    /// reference site or widget. Consumers key segment highlights on it and
    /// union same-page segments for the whole-field box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<[usize; 2]>,
    /// The `LiveSession.revision` this geometry was read at — `regions()` and
    /// `locate()` stamp the current revision so a consumer can pair a highlight
    /// box or caret with the edit state it reflects and map a position forward
    /// through later edits (`mapFieldPos`). `undefined` on a one-shot
    /// `RenderResult.regions` sidecar (no live session). Additive-optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u32>,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<quillmark_core::RenderedRegion> for FieldRegion {
    fn from(r: quillmark_core::RenderedRegion) -> Self {
        FieldRegion {
            field: r.field,
            page: r.page,
            rect: r.rect,
            span: r.span,
            // A session revision is a small monotonic counter; narrowing to the
            // JS-number-friendly u32 at the boundary avoids a BigInt.
            revision: r.revision.map(|v| v as u32),
        }
    }
}

/// A resolved point → corpus position: the field a click landed in and the USV
/// offset into its `RichText`. The `LiveSession.positionAt` result, paired with
/// `locate` (corpus position → caret rect). `pos` is cluster-exact and degrades
/// to the containing segment's start on origin-less ink.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct CorpusHit {
    /// Quill schema field path (same address space as `FieldRegion.field`).
    pub field: String,
    /// USV offset into the field's `RichText`.
    pub pos: usize,
    /// The `LiveSession.revision` this hit was resolved at — `positionAt`
    /// stamps it so a caller can record the captured `pos` against this base
    /// revision and map it forward (`mapFieldPos`). Additive-optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u32>,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<quillmark_core::CorpusHit> for CorpusHit {
    fn from(h: quillmark_core::CorpusHit) -> Self {
        CorpusHit {
            field: h.field,
            pos: h.pos,
            revision: h.revision.map(|v| v as u32),
        }
    }
}

/// A text-splice delta over a field's USV corpus — CodeMirror `ChangeSet`
/// semantics: `retain` / `insert` / `delete` ops applied left-to-right,
/// consuming base positions. The native form-editor edit unit `applyFieldDelta`
/// consumes; carries no formatting (marks/lines are separate channels).
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Delta {
    pub ops: Vec<DeltaOp>,
}

/// One [`Delta`] op. On the wire: `{ retain: n }`, `{ insert: "text" }`, or
/// `{ delete: n }` — exactly one key.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub enum DeltaOp {
    /// Keep `n` USV of the base unchanged.
    Retain(usize),
    /// Insert this text at the cursor.
    Insert(String),
    /// Drop `n` USV of the base.
    Delete(usize),
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<Delta> for quillmark_core::Delta {
    fn from(d: Delta) -> Self {
        quillmark_core::Delta {
            ops: d
                .ops
                .into_iter()
                .map(|op| match op {
                    DeltaOp::Retain(n) => quillmark_core::Op::Retain(n),
                    DeltaOp::Insert(s) => quillmark_core::Op::Insert(s),
                    DeltaOp::Delete(n) => quillmark_core::Op::Delete(n),
                })
                .collect(),
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
    /// `>= pageCount` throws with the `typst::page_index_out_of_bounds`
    /// code — read `LiveSession.pageCount` first if validation is needed.
    /// **Not supported for PDF output** — passing `pages` with
    /// `format: "pdf"` throws with the
    /// `typst::pdf_page_selection_not_supported` code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<Vec<usize>>,
    /// Override for the PDF `/Info` `/Producer` metadata string. Omit to use
    /// the default (`Quillmark <version>`). Applies to PDF output only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
    /// Populate `RenderResult.regions` with the schema-field geometry sidecar
    /// (the same entries `LiveSession.regions()` serves), for consumers
    /// without a live session — e.g. overlays over a one-shot SVG export.
    /// Defaults to `false`: exports pay no introspection cost. The sidecar
    /// always describes the whole document — page indices are document-space
    /// even when `pages` selects a subset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regions: Option<bool>,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            format: Some(OutputFormat::Pdf),
            ppi: None,
            pages: None,
            producer: None,
            regions: None,
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
            regions: opts.regions.unwrap_or(false),
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
    }

    #[test]
    fn test_severity_deserialization() {
        let error: Severity = serde_json::from_str("\"error\"").unwrap();
        assert_eq!(error, Severity::Error);

        let warning: Severity = serde_json::from_str("\"warning\"").unwrap();
        assert_eq!(warning, Severity::Warning);

        // "note" is not a severity; two values only.
        assert!(serde_json::from_str::<Severity>("\"note\"").is_err());
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
            regions: None,
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

        let render_err = quillmark_core::RenderError::from_diag(diag);
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

    // ── FieldRegion ───────────────────────────────────────────────────────────

    #[test]
    #[cfg(any(feature = "typst", feature = "pdfform"))]
    fn field_region_serializes_to_expected_shape() {
        let region = FieldRegion {
            field: "full_name".to_string(),
            page: 0,
            rect: [180.0, 672.0, 520.0, 692.0],
            span: None,
            revision: None,
        };
        let json = serde_json::to_string(&region).unwrap();
        assert!(json.contains("\"field\":\"full_name\""));
        assert!(json.contains("\"page\":0"));
        assert!(json.contains("\"rect\":[180.0,672.0,520.0,692.0]"));
        // A scalar/widget region omits `span`; an unstamped region omits
        // `revision`; no backend widget name or kind/value leaks either.
        assert!(!json.contains("\"span\""));
        assert!(!json.contains("\"revision\""));
        assert!(!json.contains("\"name\""));
        assert!(!json.contains("\"kind\""));
    }

    #[test]
    #[cfg(any(feature = "typst", feature = "pdfform"))]
    fn field_region_round_trips() {
        // The `from_wasm_abi` (JS→Rust) path uses the same serde derive.
        let region = FieldRegion {
            field: "signature_block".to_string(),
            page: 0,
            rect: [180.0, 422.0, 520.0, 462.0],
            span: Some([0, 25]),
            revision: Some(4),
        };
        let json = serde_json::to_string(&region).unwrap();
        assert!(json.contains("\"revision\":4"), "{json}");
        let back: FieldRegion = serde_json::from_str(&json).expect("round-trips");
        assert_eq!(back.field, "signature_block");
        assert_eq!(back.rect, [180.0, 422.0, 520.0, 462.0]);
        assert_eq!(back.span, Some([0, 25]));
        assert_eq!(back.revision, Some(4));
    }

    #[test]
    #[cfg(any(feature = "typst", feature = "pdfform"))]
    fn field_region_from_core_conversion() {
        use quillmark_core::RenderedRegion;

        let core_region = RenderedRegion {
            field: "agree".to_string(),
            page: 0,
            rect: [180.0, 538.0, 194.0, 552.0],
            span: None,
            revision: Some(9),
        };
        let wasm_region: FieldRegion = core_region.into();
        assert_eq!(wasm_region.field, "agree");
        assert_eq!(wasm_region.page, 0);
        assert_eq!(wasm_region.rect, [180.0, 538.0, 194.0, 552.0]);
        assert_eq!(wasm_region.span, None);
        // The core u64 revision narrows to u32 at the JS boundary.
        assert_eq!(wasm_region.revision, Some(9));
    }

    #[test]
    #[cfg(any(feature = "typst", feature = "pdfform"))]
    fn delta_wire_converts_to_core() {
        // `{ retain }`, `{ insert }`, `{ delete }` — one key each.
        let json = r#"{"ops":[{"retain":3},{"insert":"XY"},{"delete":2}]}"#;
        let wire: Delta = serde_json::from_str(json).expect("delta wire parses");
        let core: quillmark_core::Delta = wire.into();
        assert_eq!(
            core.ops,
            vec![
                quillmark_core::Op::Retain(3),
                quillmark_core::Op::Insert("XY".to_string()),
                quillmark_core::Op::Delete(2),
            ]
        );
        // The delta applies as CodeMirror text-splice semantics.
        assert_eq!(core.apply("abcde"), "abcXY");
    }
}
