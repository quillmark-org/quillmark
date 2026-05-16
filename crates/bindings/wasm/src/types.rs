//! Type definitions for the WASM API

use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

/// Output formats supported by backends
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Pdf,
    Svg,
    Txt,
    Png,
}

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

/// Diagnostic message (error, warning, or note)
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// Document-model path anchor (e.g. `"cards.indorsement[0].signature_block"`).
    ///
    /// Set on schema validation diagnostics; `undefined` otherwise. See the
    /// Rust `quillmark_core::error` module docs for the path grammar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
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

/// Rendered artifact (PDF, SVG, etc.)
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

impl Artifact {
    fn mime_type_for_format(format: OutputFormat) -> String {
        match format {
            OutputFormat::Pdf => "application/pdf".to_string(),
            OutputFormat::Svg => "image/svg+xml".to_string(),
            OutputFormat::Txt => "text/plain".to_string(),
            OutputFormat::Png => "image/png".to_string(),
        }
    }
}

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

/// Result of a render operation
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    pub artifacts: Vec<Artifact>,
    pub warnings: Vec<Diagnostic>,
    pub output_format: OutputFormat,
    pub render_time_ms: f64,
}

/// A single frontmatter item — either a field or a comment line.
///
/// Exposed via `Card.frontmatterItems`.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FrontmatterItem {
    Field {
        key: String,
        #[tsify(type = "unknown")]
        value: serde_json::Value,
        /// `true` when the field was written as `key: !fill <value>` in
        /// source or was marked via `Card.setFill()`.
        #[serde(default)]
        fill: bool,
    },
    Comment {
        text: String,
        /// `true` when the comment was a trailing inline comment in source
        /// (`field: value # text`). Inline comments attach to the previous
        /// field on emit; `Comment{inline:true}` at index 0 attaches to the
        /// sentinel line. Inline comments without a host degrade to
        /// own-line on emit.
        #[serde(default)]
        inline: bool,
    },
}

/// A single card block parsed from a Quillmark Markdown document.
///
/// Exposed as a plain JS object via `Document.main`, `Document.cards`, etc.
/// Carries a sentinel that distinguishes the document entry (main) card from
/// composable cards, a `tag` string (the QUILL reference or KIND tag), typed
/// frontmatter (map view under `frontmatter`, ordered item list under
/// `frontmatterItems`), and the body.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    /// `"main"` for the document entry (QUILL) card; `"card"` for composable cards.
    pub sentinel: String,
    /// The KIND sentinel value (e.g. `"indorsement"`) or the QUILL reference
    /// string for the main card.
    pub tag: String,
    /// Map-keyed view of frontmatter values (no `KIND`/`QUILL` key, comments invisible).
    #[tsify(type = "Record<string, unknown>")]
    pub frontmatter: serde_json::Value,
    /// Ordered frontmatter item list — fields and comments, in source order.
    #[tsify(type = "FrontmatterItem[]")]
    pub frontmatter_items: Vec<FrontmatterItem>,
    /// Markdown body after the card's closing `---`. Empty string when absent.
    pub body: String,
}

impl From<&quillmark_core::Card> for Card {
    fn from(card: &quillmark_core::Card) -> Self {
        let mut fields_map = serde_json::Map::new();
        for (k, v) in card.frontmatter().iter() {
            fields_map.insert(k.clone(), v.as_json().clone());
        }

        let items: Vec<FrontmatterItem> = card
            .frontmatter()
            .items()
            .iter()
            .map(|item| match item {
                quillmark_core::FrontmatterItem::Field { key, value, fill } => {
                    FrontmatterItem::Field {
                        key: key.clone(),
                        value: value.as_json().clone(),
                        fill: *fill,
                    }
                }
                quillmark_core::FrontmatterItem::Comment { text, inline } => {
                    FrontmatterItem::Comment {
                        text: text.clone(),
                        inline: *inline,
                    }
                }
            })
            .collect();

        let sentinel = if card.is_main() { "main" } else { "card" };

        Card {
            sentinel: sentinel.to_string(),
            tag: card.tag(),
            frontmatter: serde_json::Value::Object(fields_map),
            frontmatter_items: items,
            body: card.body().to_string(),
        }
    }
}

/// Options for rendering
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
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            format: Some(OutputFormat::Pdf),
            ppi: None,
            pages: None,
        }
    }
}

impl From<RenderOptions> for quillmark_core::RenderOptions {
    fn from(opts: RenderOptions) -> Self {
        Self {
            output_format: opts.format.map(|f| f.into()),
            ppi: opts.ppi,
            pages: opts.pages,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── FrontmatterItem ───────────────────────────────────────────────────────

    #[test]
    fn frontmatter_item_field_serializes_with_kind_tag() {
        let item = FrontmatterItem::Field {
            key: "title".into(),
            value: serde_json::json!("Hello"),
            fill: false,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"kind\":\"field\""));
        assert!(json.contains("\"key\":\"title\""));
        assert!(json.contains("\"value\":\"Hello\""));
        assert!(json.contains("\"fill\":false"));
    }

    #[test]
    fn frontmatter_item_field_fill_flag() {
        let item = FrontmatterItem::Field {
            key: "dept".into(),
            value: serde_json::json!("Sales"),
            fill: true,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"fill\":true"));
    }

    #[test]
    fn frontmatter_item_own_line_comment_serializes() {
        let item = FrontmatterItem::Comment {
            text: "required".into(),
            inline: false,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"kind\":\"comment\""));
        assert!(json.contains("\"text\":\"required\""));
        // inline:false is the default — presence in JSON is an implementation
        // detail, but the round-trip must deserialize back to false.
        let back: FrontmatterItem = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            back,
            FrontmatterItem::Comment { inline: false, .. }
        ));
    }

    #[test]
    fn frontmatter_item_inline_comment_round_trips() {
        let item = FrontmatterItem::Comment {
            text: "sentinel".into(),
            inline: true,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"kind\":\"comment\""));
        assert!(json.contains("\"inline\":true"));
        let back: FrontmatterItem = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            back,
            FrontmatterItem::Comment { inline: true, .. }
        ));
    }

    #[test]
    fn frontmatter_item_inline_default_is_false() {
        // `inline` is serde(default) — omitting it from JSON must deserialize as false.
        let json = r#"{"kind":"comment","text":"note"}"#;
        let item: FrontmatterItem = serde_json::from_str(json).unwrap();
        assert!(matches!(
            item,
            FrontmatterItem::Comment { inline: false, .. }
        ));
    }

    #[test]
    fn card_from_core_carries_frontmatter_items() {
        use quillmark_core::Document;

        let md = "---\nQUILL: q\ntitle: Hi # inline note\nauthor: Alice\n---\n";
        let doc = Document::from_markdown(md).unwrap();
        let card = Card::from(doc.main());

        // Map view: title and author present, QUILL absent.
        assert_eq!(card.frontmatter["title"], "Hi");
        assert_eq!(card.frontmatter["author"], "Alice");
        assert!(card.frontmatter.get("QUILL").is_none());

        // Item list: field + inline comment + field.
        let fields: Vec<&str> = card
            .frontmatter_items
            .iter()
            .filter_map(|it| match it {
                FrontmatterItem::Field { key, .. } => Some(key.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(fields, vec!["title", "author"]);

        let inline_comment = card
            .frontmatter_items
            .iter()
            .find(|it| matches!(it, FrontmatterItem::Comment { inline: true, .. }));
        assert!(
            inline_comment.is_some(),
            "inline comment must appear in frontmatter_items"
        );
    }

    // ── OutputFormat ──────────────────────────────────────────────────────────

    #[test]
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
    fn test_render_options_with_format() {
        let options = RenderOptions {
            format: Some(OutputFormat::Pdf),
            ppi: None,
            pages: None,
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

        let render_err = quillmark_core::RenderError::InvalidFrontmatter {
            diag: Box::new(diag),
        };
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
    fn test_wasm_error_multiple_diagnostics() {
        use crate::error::WasmError;
        use quillmark_core::{Diagnostic, Severity};

        let diag1 = Diagnostic::new(Severity::Error, "Error 1".to_string());
        let diag2 = Diagnostic::new(Severity::Error, "Error 2".to_string());

        let render_err = quillmark_core::RenderError::CompilationFailed {
            diags: vec![diag1, diag2],
        };
        let wasm_err: WasmError = render_err.into();

        assert_eq!(wasm_err.diagnostics.len(), 2);
        assert_eq!(wasm_err.diagnostics[0].message, "Error 1");
        assert_eq!(wasm_err.diagnostics[1].message, "Error 2");
        assert!(wasm_err.message().contains("2"));
    }

    #[test]
    fn test_wasm_error_from_string() {
        use crate::error::WasmError;

        let wasm_err: WasmError = "Simple error message".into();
        assert_eq!(wasm_err.message(), "Simple error message");
        assert_eq!(wasm_err.diagnostics.len(), 1);
        assert_eq!(wasm_err.diagnostics[0].message, "Simple error message");
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
}
