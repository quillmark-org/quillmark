//! The form-field adapter: a thin introspection→[`FieldSpec`] bridge onto the
//! shared `quillmark-pdf` stamping spine.
//!
//! Public entry points: [`extract`] (called from `lib.rs`, once per compile)
//! walks the Typst document for `form-field` placements; [`build_field_specs`]
//! (called from `lib.rs` and `compile.rs`) converts those (Typst top-left
//! origin) into spine [`FieldSpec`]s (PDF bottom-left origin) — coordinate
//! ownership lives here, in the backend, so the spine never imports
//! `typst_layout`. [`default_producer`] (called from `compile.rs`) supplies
//! the default `/Info` `/Producer` string the product layer threads down.

use quillmark_core::{Diagnostic, RenderError, Severity};
use quillmark_pdf::{FieldSpec, FieldType, CHECKBOX_ON_STATE};
use typst_layout::PagedDocument;

mod extract;
mod span_scan;

pub(crate) use span_scan::FieldWindow;

/// Regions for content fields and direct scalar references, read from the
/// laid-out frames' glyph spans and keyed on the schema path — each tracked
/// window's first placement, one region per page it touches. `helper` is the
/// helper `lib.typ` [`Source`](typst::syntax::Source) snapshot the served
/// document was compiled from — never the world's live copy, which a failed
/// apply may have already replaced. See [`span_scan`].
pub(crate) fn scan_content_regions(
    doc: &PagedDocument,
    world: &crate::world::QuillWorld,
    helper: &typst::syntax::Source,
    windows: &[FieldWindow],
) -> Vec<quillmark_core::RenderedRegion> {
    span_scan::scan(doc, world, helper, windows)
}

/// The schema field under a point (PDF points, bottom-left origin) — the
/// forward click→field direction. Every placement answers, not just the
/// first. See [`span_scan::field_at`].
pub(crate) fn field_at(
    doc: &PagedDocument,
    world: &crate::world::QuillWorld,
    helper: &typst::syntax::Source,
    windows: &[FieldWindow],
    page: usize,
    x: f32,
    y: f32,
) -> Option<String> {
    span_scan::field_at(doc, world, helper, windows, page, x, y)
}

/// Byte windows for the plate's direct `data.<field>` scalar references. See
/// [`span_scan::scalar_windows`].
pub(crate) fn scalar_windows(
    source: &typst::syntax::Source,
    fields: &[String],
) -> Vec<(String, std::ops::Range<usize>)> {
    span_scan::scalar_windows(source, fields)
}

/// The kind of form field a placement declares, plus its per-kind payload.
/// Mirrors the spine's [`FieldType`] but carries the *resolved* Typst value so
/// the adapter can map it to the spine's value representation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FieldKind {
    /// A text box. `multiline` toggles the spine's MULTILINE flag; `value` is
    /// the bound display string (numbers already stringified) or `None` (blank).
    Text {
        multiline: bool,
        value: Option<String>,
    },
    /// A checkbox. `checked` is the resolved boolean binding.
    Checkbox { checked: bool },
    /// A dropdown over `options`. `value` is the bound choice string, mapped to
    /// `/V` only if it matches an option (see [`build_field_specs`]).
    Choice {
        options: Vec<String>,
        value: Option<String>,
    },
    /// An unsigned signature field (value-free).
    Signature,
}

/// One form field's name + page + rect in Typst (top-left origin) points, plus
/// its kind/value payload. [`build_field_specs`] converts to the spine's
/// bottom-left geometry and value representation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FieldPlacement {
    pub name: String,
    /// Schema-field path the region keys on (the `field:` argument). `None`
    /// when the plate omits it — the widget then exposes no region (its `/T`
    /// `name` is not a schema address).
    pub schema_field: Option<String>,
    pub page: usize,
    pub rect_typst_pt: [f32; 4],
    pub kind: FieldKind,
}

/// Build a single-`Diagnostic` `RenderError` with `code`. Shared by the
/// adapter and the introspection walk: every fail site just needs a code plus a
/// message.
pub(crate) fn err(code: &'static str, msg: impl Into<String>) -> RenderError {
    RenderError::from_diag(Diagnostic::new(Severity::Error, msg.into()).with_code(code.into()))
}

/// Default `/Producer` value: `Quillmark <crate-version>`. Owned by the backend
/// (the product layer), never defaulted from the leaf spine's version.
pub(crate) fn default_producer() -> String {
    format!("Quillmark {}", env!("CARGO_PKG_VERSION"))
}

pub(crate) fn extract(doc: &PagedDocument) -> Result<Vec<FieldPlacement>, RenderError> {
    extract::extract(doc)
}

/// Convert form-field placements into spine [`FieldSpec`]s, flipping each rect
/// from Typst's top-left origin to the PDF bottom-left origin the spine
/// consumes, and mapping each kind's resolved value to the spine's value
/// representation. Page heights come from the Typst document (the geometry
/// source); the spine only ever sees final bottom-left geometry.
///
/// The value coercion here (checkbox truthiness already resolved in the plate;
/// choice option-matching) mirrors `quillmark-pdfform`'s resolver, but is
/// duplicated rather than shared because this crate must NOT depend on
/// `quillmark-pdfform` — the two backends meet only at the `&[FieldSpec]` seam.
pub(crate) fn build_field_specs(
    doc: &PagedDocument,
    placements: &[FieldPlacement],
) -> Result<Vec<FieldSpec>, RenderError> {
    let page_heights: Vec<f32> = doc
        .pages()
        .iter()
        .map(|p| p.frame.size().y.to_pt() as f32)
        .collect();

    placements
        .iter()
        .map(|p| {
            let page_h = *page_heights.get(p.page).ok_or_else(|| {
                err(
                    "typst::form_field_page_out_of_range",
                    format!(
                        "form-field {:?} targets page {} but the document has {} page(s)",
                        p.name,
                        p.page,
                        page_heights.len()
                    ),
                )
            })?;
            let [x0, y0, x1, y1] = p.rect_typst_pt;
            let (field_type, value) = match &p.kind {
                FieldKind::Text { multiline, value } => (
                    FieldType::Text {
                        multiline: *multiline,
                    },
                    value.clone(),
                ),
                FieldKind::Checkbox { checked } => (
                    FieldType::Checkbox,
                    checked.then(|| CHECKBOX_ON_STATE.to_string()),
                ),
                FieldKind::Choice { options, value } => {
                    // Mirror pdfform's `coerce_choice`: a choice value binds
                    // only if it matches one of the declared options exactly.
                    let bound = value
                        .as_ref()
                        .filter(|v| options.iter().any(|o| o == *v))
                        .cloned();
                    (
                        FieldType::Choice {
                            options: options.clone(),
                        },
                        bound,
                    )
                }
                FieldKind::Signature => (FieldType::Signature, None),
            };
            Ok(FieldSpec {
                name: p.name.clone(),
                // The region keys on the explicit `field:` schema path. A widget
                // that binds none is not a schema-addressable field — its `/T`
                // name is a backend identifier, never a schema address — so it
                // carries no `schema_field` and `regions_of` exposes no region.
                schema_field: p.schema_field.clone(),
                page: p.page,
                // Typst top-left → PDF bottom-left.
                rect: [x0, page_h - y1, x1, page_h - y0],
                field_type,
                value,
                tooltip: None,
            })
        })
        .collect()
}
