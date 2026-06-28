//! The incremental-update envelope shared by the stamp and flatten paths.
//!
//! Both [`stamp`](crate::stamp) (this crate) and the `pdfform` value-flatten
//! path open a base PDF the same way — validate it against the reader's input
//! contract, read the trailer, seed the object-id counter, optionally stamp
//! `/Info` `/Producer` — then build their own objects, then close the same way
//! with one incremental-update append. This module owns that open/close
//! envelope so the two paths can never drift on it; each path supplies only its
//! middle (the objects to write).

use crate::error::PdfError;
use crate::reader::{
    append_incremental_update, assert_overwrite_gen_zero, assert_traditional_xref,
    assert_unrotated_page, err, find_dict_value, find_startxref, find_trailer_dict,
    parse_indirect_ref, resolve_page_ids, UpdatedObject,
};
use crate::writer::apply_producer_stamp;
use crate::FieldSpec;

const CODE_PARSE: &str = "pdf::update_parse";

/// One incremental-update revision in progress: the validated entry points read
/// from the base's trailer, plus the accumulators a caller fills with its own
/// objects before [`finish`](PdfUpdate::finish).
pub struct PdfUpdate {
    xref_offset: usize,
    /// The base PDF's catalog (`/Root`) object id.
    pub catalog_id: u32,
    /// Next free object id, seeded at the trailer `/Size`. Hand out via
    /// [`alloc_id`](crate::writer::alloc_id).
    pub next_id: u32,
    /// Objects to write in this revision; callers push their own onto it.
    pub objects: Vec<UpdatedObject>,
    /// `Some` when a fresh `/Info` was allocated by the producer stamp (threaded
    /// into the new trailer by [`finish`](PdfUpdate::finish)).
    extra_info_ref: Option<u32>,
}

impl PdfUpdate {
    /// Open `pdf` for an incremental update: assert the input contract
    /// (traditional-xref, unencrypted), read `/Root`, `/Size`, and `/Info` from
    /// the trailer, seed the id counter at `/Size`, and apply the optional
    /// `/Info` `/Producer` stamp. The caller then pushes its objects onto
    /// [`objects`](Self::objects) and calls [`finish`](Self::finish).
    pub fn begin(pdf: &[u8], producer: Option<&str>) -> Result<Self, PdfError> {
        let xref_offset = find_startxref(pdf)?;
        assert_traditional_xref(pdf, xref_offset)?;

        let trailer = find_trailer_dict(pdf, xref_offset)?;
        if find_dict_value(trailer, "Encrypt").is_some() {
            return Err(err(
                "pdf::encrypted",
                "PDF is encrypted; the stamp spine does not handle encrypted PDFs",
            ));
        }
        let (catalog_id, _) = find_dict_value(trailer, "Root")
            .and_then(parse_indirect_ref)
            .ok_or_else(|| err(CODE_PARSE, "/Root missing or malformed in trailer"))?;
        // The new trailer re-references the catalog as `/Root <id> 0 R`, so a
        // non-zero-generation catalog would be silently corrupted even when only
        // the producer is stamped (catalog not itself overwritten).
        assert_overwrite_gen_zero(pdf, catalog_id, "catalog (/Root)")?;
        let size = find_dict_value(trailer, "Size")
            .and_then(|v| std::str::from_utf8(v.trim_ascii()).ok())
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or_else(|| err(CODE_PARSE, "/Size missing or malformed in trailer"))?;
        let info_ref = find_dict_value(trailer, "Info").and_then(parse_indirect_ref);

        // Object ids are handed out from a single counter (seeded at the trailer
        // `/Size`) so created objects never collide with the base's, nor with
        // each other. Allocation is checked (`alloc_id`): a malformed
        // near-`u32::MAX` `/Size` yields a clean error rather than an overflow
        // panic (debug) or a silently-wrapped, colliding id (release) —
        // matching the reader's hard-error contract.
        let mut next_id = size;
        let mut objects: Vec<UpdatedObject> = Vec::new();
        let mut extra_info_ref = None;
        if let Some(producer) = producer {
            extra_info_ref =
                apply_producer_stamp(pdf, info_ref, producer, &mut next_id, &mut objects)?;
        }

        Ok(Self {
            xref_offset,
            catalog_id,
            next_id,
            objects,
            extra_info_ref,
        })
    }

    /// Resolve the base's page object ids and bounds-check every field's `page`
    /// against the page count, so a spec targeting a non-existent page is a
    /// clean error rather than a later panic. Shared by both paths.
    pub fn resolve_pages(&self, pdf: &[u8], fields: &[FieldSpec]) -> Result<Vec<u32>, PdfError> {
        let page_ids = resolve_page_ids(pdf, self.catalog_id)?;
        let page_count = page_ids.len();
        for spec in fields {
            if spec.page >= page_count {
                return Err(err(
                    CODE_PARSE,
                    format!(
                        "field {:?} targets page {} but the PDF has {page_count} page(s)",
                        spec.name, spec.page
                    ),
                ));
            }
            // A targeted page node is overwritten (its `/Annots`) and referenced
            // by every widget on it as gen 0, so a non-zero-generation page would
            // be silently corrupted.
            assert_overwrite_gen_zero(pdf, page_ids[spec.page], "page")?;
            // Widget/content geometry is written in unrotated user space; a
            // rotated target page would mis-place every field. Reject cleanly.
            assert_unrotated_page(pdf, self.catalog_id, page_ids[spec.page])?;
        }
        Ok(page_ids)
    }

    /// Serialize the accumulated objects onto `pdf` via one incremental-update
    /// append, threading in a freshly-allocated `/Info` when the producer stamp
    /// created one. Consumes the update.
    pub fn finish(self, pdf: Vec<u8>) -> Result<Vec<u8>, PdfError> {
        append_incremental_update(
            pdf,
            self.xref_offset,
            self.catalog_id,
            self.next_id,
            self.extra_info_ref,
            &self.objects,
        )
    }
}
