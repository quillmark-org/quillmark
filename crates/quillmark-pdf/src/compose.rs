//! The page-merge primitive: concatenate the pages of several PDFs into one
//! document the stamp spine can consume.
//!
//! This is the load-bearing piece of the "compose content first, **stamp last**
//! over a combined page set" mechanism (proposal §7's continuation/overflow
//! item). The unbounded-continuation pipeline renders Typst-typeset continuation
//! pages, [`concat_pdf_pages`] them onto the gov background, then makes ONE
//! [`stamp`](crate::stamp) call with `FieldSpec.page` indices that span the
//! combined set. This module owns only the merge; the composition layer lives
//! above both backends in `quillmark` orchestration.
//!
//! Built on **lopdf** (a workspace dep already vendored for the qualification
//! layer), not the proposal's `hayro-write` plan — `hayro-write` is internal /
//! not-for-external-use, while lopdf load → mutate → re-serialize already
//! produces the traditional-xref output the spine's byte reader requires. The
//! merge forces [`XrefType::CrossReferenceTable`] exactly as
//! `quillmark-qualify` does, so the output satisfies the reader's input
//! contract (traditional-xref, unencrypted, resolvable `/Pages` tree).

use lopdf::xref::XrefType;
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::error::PdfError;

const CODE_COMPOSE: &str = "pdf::compose";

/// Build a `PdfError` for a compose failure.
fn err(msg: impl Into<String>) -> PdfError {
    PdfError::new(CODE_COMPOSE, msg)
}

/// Attributes a `/Page` may inherit from an ancestor `/Pages` node. When a page
/// is re-parented under a single flat `/Pages` root, any value it inherited from
/// its old ancestors would be lost, so we resolve each DOWN onto the page dict
/// before the re-parent. `/MediaBox` is the one the stamp spine's reader reads;
/// the rest are carried for fidelity so the merged page renders as it did.
const INHERITABLE_KEYS: &[&[u8]] = &[b"MediaBox", b"CropBox", b"Resources", b"Rotate"];

/// One page's inherited attributes resolved down: the `(key, value)` pairs to
/// write onto the page dict so geometry/resources survive the re-parent.
type Inherited = Vec<(Vec<u8>, Object)>;

/// Concatenate the pages of every input PDF (in order) into one document whose
/// page set is `pdfs[0]`'s pages, then `pdfs[1]`'s, … Output is a
/// traditional-xref, unencrypted PDF that satisfies the stamp spine's input
/// contract, so it can be passed straight to [`stamp`](crate::stamp) /
/// `flatten` with `FieldSpec.page` indices into the combined set. Page geometry
/// (`/MediaBox`, resolving inheritance) is preserved per page.
///
/// `/AcroForm` and page `/Annots` from the inputs are dropped: the merged base
/// is a clean background and callers stamp fresh widgets over it. An empty
/// `pdfs` slice is an error (there is no page set to build).
pub fn concat_pdf_pages(pdfs: &[&[u8]]) -> Result<Vec<u8>, PdfError> {
    let Some((first, rest)) = pdfs.split_first() else {
        return Err(err("concat_pdf_pages requires at least one input PDF"));
    };

    // The accumulator starts as a fresh document with a single flat /Pages root
    // and a catalog pointing at it. Every input's pages are flattened onto this
    // root, so the output tree is shallow (catalog → one /Pages → all pages) —
    // exactly the flat tree the reader walks.
    let mut acc = Document::new();
    acc.version = "1.7".to_string();
    // Reserve ids 1 (catalog) and 2 (root /Pages); input objects renumber above
    // these, so `max_id` starts at 2.
    let catalog_id: ObjectId = (1, 0);
    let pages_root_id: ObjectId = (2, 0);
    acc.max_id = 2;

    let mut kids: Vec<Object> = Vec::new();

    for bytes in std::iter::once(first).chain(rest.iter()) {
        let mut doc =
            Document::load_mem(bytes).map_err(|e| err(format!("input PDF load failed: {e}")))?;
        if doc.is_encrypted() {
            doc.decrypt("")
                .map_err(|e| err(format!("input PDF is encrypted: {e}")))?;
        }

        // Page object ids in document order, with each page's inherited
        // attributes resolved DOWN onto its own dict (read before we mutate ids,
        // so /Parent chains still resolve in the source doc).
        let page_ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
        if page_ids.is_empty() {
            return Err(err("input PDF has no pages"));
        }
        let resolved: Vec<(ObjectId, Inherited)> = page_ids
            .iter()
            .map(|&pid| (pid, resolve_inherited(&doc, pid)))
            .collect();

        // Renumber by a stable running offset, done manually (offset every id
        // and every Reference) rather than via lopdf's `renumber_objects_with`,
        // which reorders pages by a heuristic and traverses a single document —
        // neither fits a cross-document merge wanting a predictable offset.
        let offset = acc.max_id;

        // Splice this input's objects into the accumulator, minus its catalog
        // and /Pages-tree nodes (we build our own flat tree). Each object's id
        // and internal references are offset so they sit above the accumulator's
        // current max and can't collide.
        for (&(num, gen), obj) in doc.objects.iter() {
            if is_structural_node(obj) {
                continue;
            }
            let mut cloned = obj.clone();
            offset_references(&mut cloned, offset);
            acc.objects.insert((num + offset, gen), cloned);
        }

        // Re-parent each merged page under the flat root, write its resolved
        // inheritable attrs onto it, drop /Annots, and append it to /Kids. The
        // page dicts now live in `acc.objects` (a BTreeMap), so mutate there.
        for (pid, inherited) in resolved {
            let new_pid: ObjectId = (pid.0 + offset, pid.1);
            kids.push(Object::Reference(new_pid));
            if let Some(Object::Dictionary(page)) = acc.objects.get_mut(&new_pid) {
                page.set("Parent", Object::Reference(pages_root_id));
                page.remove(b"Annots");
                for (key, mut value) in inherited {
                    // Only fill an attr the page does not already declare; a
                    // page's own value always wins over the inherited one.
                    if !page.has(&key) {
                        // The inherited value was read from the SOURCE doc, so any
                        // indirect references in it (e.g. an inherited
                        // `/Resources N 0 R`) carry source numbering — offset them
                        // by the same amount as this input's spliced objects, or
                        // they would dangle.
                        offset_references(&mut value, offset);
                        page.set(key, value);
                    }
                }
            }
        }

        acc.max_id = acc.max_id.max(offset + doc.max_id);
    }

    // The flat /Pages root and the catalog.
    let mut pages_root = Dictionary::new();
    pages_root.set("Type", Object::Name(b"Pages".to_vec()));
    pages_root.set("Count", Object::Integer(kids.len() as i64));
    pages_root.set("Kids", Object::Array(kids));
    acc.objects
        .insert(pages_root_id, Object::Dictionary(pages_root));

    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_root_id));
    acc.objects.insert(catalog_id, Object::Dictionary(catalog));

    // Trailer: point /Root at the catalog and force a clean traditional-xref
    // trailer with no carried-over xref-stream keys (no input trailer is reused,
    // so there is nothing stale to strip — but mirror qualify's intent by being
    // explicit about what the trailer carries).
    acc.trailer.set("Root", Object::Reference(catalog_id));
    acc.trailer.remove(b"Encrypt");
    acc.trailer.remove(b"Prev");

    // FORCE traditional xref output (lopdf defaults a fresh Document to an xref
    // *stream*, which violates the reader's `assert_traditional_xref`). This is
    // the exact same forcing `quillmark-qualify` applies.
    acc.reference_table.cross_reference_type = XrefType::CrossReferenceTable;

    let mut buf = Vec::new();
    acc.save_to(&mut buf)
        .map_err(|e| err(format!("merged PDF save failed: {e}")))?;
    Ok(buf)
}

/// Recursively bump every `Object::Reference` by `offset`, descending into
/// arrays, dictionaries, and stream dicts.
fn offset_references(obj: &mut Object, offset: u32) {
    match obj {
        Object::Reference((num, _)) => *num += offset,
        Object::Array(arr) => {
            for o in arr {
                offset_references(o, offset);
            }
        }
        Object::Dictionary(dict) => offset_dict_references(dict, offset),
        Object::Stream(stream) => offset_dict_references(&mut stream.dict, offset),
        _ => {}
    }
}

fn offset_dict_references(dict: &mut Dictionary, offset: u32) {
    for (_, value) in dict.iter_mut() {
        offset_references(value, offset);
    }
}

/// Resolve the inheritable attributes a page would have read from its ancestors
/// in the SOURCE document, as `(key, value)` pairs. Indirect values are returned
/// as-is (their references are offset later with the rest of the input), so a
/// `/Resources N 0 R` inherited from the parent still resolves after the merge.
fn resolve_inherited(doc: &Document, page_id: ObjectId) -> Inherited {
    let mut out = Vec::new();
    for &key in INHERITABLE_KEYS {
        // Skip keys the page already declares — only inherited ones need filling.
        let page_has = doc
            .get_object(page_id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .map(|d| d.has(key))
            .unwrap_or(false);
        if page_has {
            continue;
        }
        if let Some(value) = inherited_value(doc, page_id, key) {
            out.push((key.to_vec(), value));
        }
    }
    out
}

/// Walk `/Parent` from `page_id` looking for `key`; return the first owning
/// node's value (cloned, references intact). Capped to avoid runaway on a
/// malformed parent cycle.
fn inherited_value(doc: &Document, page_id: ObjectId, key: &[u8]) -> Option<Object> {
    let mut cur = doc
        .get_object(page_id)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"Parent").ok().and_then(|o| o.as_reference().ok()));
    let mut guard = 0;
    while let Some(id) = cur {
        guard += 1;
        if guard > 64 {
            break;
        }
        let dict = doc.get_object(id).ok().and_then(|o| o.as_dict().ok())?;
        if let Ok(value) = dict.get(key) {
            return Some(value.clone());
        }
        cur = dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }
    None
}

/// A node is structural (the catalog or a `/Pages` tree node) and is dropped
/// from the merge — we build a fresh flat catalog + `/Pages` root instead. A
/// `/Page` leaf is kept; everything else (content streams, fonts, resources) is
/// kept.
fn is_structural_node(obj: &Object) -> bool {
    match obj {
        Object::Dictionary(d) => {
            matches!(d.get(b"Type").ok().and_then(|o| o.as_name().ok()), Some(t) if t == b"Catalog" || t == b"Pages")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_references_descends_all_containers() {
        let mut obj = Object::Dictionary({
            let mut d = Dictionary::new();
            d.set("A", Object::Reference((5, 0)));
            d.set(
                "B",
                Object::Array(vec![Object::Reference((7, 0)), Object::Integer(3)]),
            );
            d
        });
        offset_references(&mut obj, 100);
        if let Object::Dictionary(d) = &obj {
            assert_eq!(d.get(b"A").unwrap().as_reference().unwrap(), (105, 0));
            if let Ok(Object::Array(arr)) = d.get(b"B") {
                assert_eq!(arr[0].as_reference().unwrap(), (107, 0));
            } else {
                panic!("B not an array");
            }
        } else {
            panic!("not a dict");
        }
    }
}
