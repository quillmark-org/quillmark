//! # quillmark-pdf — the AcroForm stamping spine
//!
//! Shared, Typst-free infra (not a backend) whose whole job is one pure
//! operation:
//!
//! ```text
//! (base_pdf_bytes, &[FieldSpec]) -> { stamped_pdf, regions }
//! ```
//!
//! via a single incremental-update append. Both Quillmark backends — Typst
//! (geometry from introspection) and `pdfform` (geometry from `form.json`) —
//! become producers of a base PDF plus a list of [`FieldSpec`]s; this crate
//! stamps. They unify exactly at the `&[FieldSpec]` seam.
//!
//! The crate owns its own [`PdfError`]; each backend maps it to
//! `quillmark_core::RenderError` at its boundary. It depends only on
//! `quillmark-core` (for the [`RenderedRegion`](quillmark_core::RenderedRegion)
//! sidecar types) and `pdf-writer` (for typed object construction); it never
//! depends on Typst.
//!
//! See [`stamp`] for the operation, [`FieldSpec`] for the currency, and
//! `crate::reader`'s docs for the input contract the base PDF must satisfy.

mod error;
pub mod reader;
mod stamp;
mod update;
pub mod writer;

pub use error::PdfError;
pub use stamp::{regions_of, stamp, StampOptions, CHECKBOX_ON_STATE};
pub use update::PdfUpdate;

/// The `/MediaBox` of every page of `base`, normalized to `[x0, y0, x1, y1]`
/// (lower-left, upper-right), in document order.
///
/// The geometry source for a backend that owns top-left page-relative rects
/// (e.g. `pdfform` reading `form.json`): read the page box here, flip to the
/// bottom-left origin the spine consumes (honouring a non-zero page origin),
/// then build the [`FieldSpec`].
pub fn page_media_boxes(base: &[u8]) -> Result<Vec<[f32; 4]>, PdfError> {
    reader::page_media_boxes(base)
}

/// The backend-agnostic currency of the stamp spine: one form field, fully
/// resolved.
///
/// `rect` is **final** geometry — PDF points, bottom-left origin,
/// `[x0, y0, x1, y1]`. The spine never reasons about page height or reflow;
/// whoever owns the geometry source converts before constructing the spec.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    /// Fully-qualified field name, written to `/T`. A spine-internal AcroForm
    /// identifier — never surfaced to a region consumer.
    pub name: String,
    /// The quill schema field address this widget maps to, if any. Opaque to
    /// the spine (it never interprets it), carried solely to key the region
    /// sidecar: a field with `Some(path)` emits a
    /// [`RenderedRegion`](quillmark_core::RenderedRegion), an unbound widget
    /// (`None`) emits none.
    pub schema_field: Option<String>,
    /// 0-based page index.
    pub page: usize,
    /// `[x0, y0, x1, y1]` in PDF points, bottom-left origin.
    pub rect: [f32; 4],
    /// The field's definition (no value).
    pub field_type: FieldType,
    /// The one uniform bound value (`None` = blank). For a checkbox, `Some`
    /// (carrying the on-state name) means checked; `None` means unchecked.
    pub value: Option<String>,
    /// Optional `/TU` tooltip / accessible name.
    pub tooltip: Option<String>,
}

/// A field's definition — never a runtime value (that rides in
/// [`FieldSpec::value`]). `form.json` reuses this directly with no parallel
/// enum.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    /// A text field; `multiline` is the single retained text trait.
    Text { multiline: bool },
    /// A checkbox with the engine's fixed on-state ([`CHECKBOX_ON_STATE`]).
    Checkbox,
    /// A dropdown choice over `options` (bare display strings).
    Choice { options: Vec<String> },
    /// An unsigned signature field.
    Signature,
}
