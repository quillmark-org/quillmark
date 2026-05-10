//! SPIKE — not for merge. Full end-to-end test of the "pdf-writer + hand-rolled
//! scanner" approach for injecting an unsigned SigField via a PDF incremental
//! update.
//!
//! Goal: produce a PDF that:
//!   1. Reparses cleanly with lopdf (as a stand-in for "any conformant reader")
//!   2. Has /AcroForm referenced from the catalog
//!   3. Has a SigField widget on page 1
//!   4. Preserves all original content of the typst_pdf output (incremental update,
//!      no rewriting of original bytes)
//!
//! Run with: `cargo test -p quillmark-typst --lib spike_d -- --nocapture`

#![cfg(test)]
#![allow(unused_imports)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use pdf_writer::types::{AnnotationFlags, AnnotationType, FieldType, SigFlags};
use pdf_writer::{Chunk, Finish, Name, Rect, Ref, TextStr};
use pdf_writer::writers::Form;
use quillmark_core::{FileTreeNode, QuillSource};
use typst::layout::PagedDocument;
use typst_pdf::PdfOptions;

use crate::world::QuillWorld;

// ─── fixture compile ──────────────────────────────────────────────────────────

fn load_fixture() -> QuillSource {
    fn walk(dir: &Path) -> std::io::Result<FileTreeNode> {
        let mut files = HashMap::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let p: PathBuf = entry.path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if p.is_file() {
                files.insert(name, FileTreeNode::File { contents: fs::read(&p)? });
            } else if p.is_dir() {
                files.insert(name, walk(&p)?);
            }
        }
        Ok(FileTreeNode::Directory { files })
    }
    let quill_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap()
        .join("fixtures").join("resources").join("quills")
        .join("usaf_memo").join("0.1.0");
    let tree = walk(&quill_path).expect("walk fixture");
    QuillSource::from_tree(tree).expect("load source")
}

fn produce_typst_pdf(main: &str) -> Vec<u8> {
    let world = QuillWorld::new(&load_fixture(), main).expect("world");
    let doc = typst::compile::<PagedDocument>(&world).output.expect("compile ok");
    typst_pdf::pdf(&doc, &PdfOptions::default()).expect("pdf ok")
}

const TINY_DOC: &str = r#"
#set page(width: 600pt, height: 400pt, margin: 50pt)
= Tiny test document
This PDF exists to be poked at by hand-rolled scanner + pdf-writer.
"#;

// ─── hand-rolled PDF scanner ──────────────────────────────────────────────────

/// Find the byte offset stored after the last `startxref` marker.
fn find_startxref(pdf: &[u8]) -> Option<usize> {
    let needle = b"startxref";
    // Search the last 1 KB; per spec, startxref lives near EOF.
    let from = pdf.len().saturating_sub(1024);
    let tail = &pdf[from..];
    let pos = tail.windows(needle.len()).rposition(|w| w == needle)?;
    let after = &tail[pos + needle.len()..];
    // Skip whitespace
    let after = skip_ws(after);
    // Parse decimal integer
    let mut end = 0;
    while end < after.len() && after[end].is_ascii_digit() {
        end += 1;
    }
    std::str::from_utf8(&after[..end]).ok()?.parse().ok()
}

fn skip_ws(s: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | b'\x0c') {
        i += 1;
    }
    &s[i..]
}

/// Locate an indirect object by its ID via linear scan — `N G obj` ... `endobj`.
/// Returns `(obj_start, endobj_end)`. The header `N G obj` is included; the
/// trailing `endobj` is included.
fn find_object_bytes(pdf: &[u8], id: u32) -> Option<(usize, usize)> {
    let header = format!("{} 0 obj", id);
    let h = header.as_bytes();
    // The header should be preceded by a newline. Search forward.
    let mut i = 0;
    while i + h.len() < pdf.len() {
        if pdf[i..].starts_with(h)
            && (i == 0 || matches!(pdf[i - 1], b'\n' | b'\r' | b' '))
        {
            // Found header. Find matching endobj.
            let needle = b"endobj";
            let from = i + h.len();
            let rest = &pdf[from..];
            let end_rel = rest.windows(needle.len()).position(|w| w == needle)?;
            return Some((i, from + end_rel + needle.len()));
        }
        i += 1;
    }
    None
}

/// Within a dict-bounded byte slice, find `/Key (something)` and return the
/// raw bytes between the key name and the next key or end. Caller does
/// further parsing. Very shallow: does not understand nested braces in keys.
fn find_dict_value<'a>(dict_bytes: &'a [u8], key: &str) -> Option<&'a [u8]> {
    let key_marker = format!("/{}", key);
    let km = key_marker.as_bytes();
    let mut i = 0;
    while i + km.len() < dict_bytes.len() {
        if dict_bytes[i..].starts_with(km) {
            // Check the char after the key is whitespace or delimiter
            let next = dict_bytes.get(i + km.len()).copied();
            if matches!(
                next,
                Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'/') | Some(b'[') | Some(b'<')
            ) {
                let start = i + km.len();
                // Find the next `/` (next key) or `>>` (end of dict).
                let mut j = start;
                while j < dict_bytes.len() {
                    if dict_bytes[j..].starts_with(b">>") {
                        return Some(&dict_bytes[start..j]);
                    }
                    // Only treat `/` as next key when at depth 0 (no nested
                    // dict/array). For shallow needs we don't track depth;
                    // typst_pdf's catalog/page dicts have keys at top level.
                    if dict_bytes[j] == b'/' && j > start {
                        return Some(&dict_bytes[start..j]);
                    }
                    j += 1;
                }
                return Some(&dict_bytes[start..j]);
            }
        }
        i += 1;
    }
    None
}

/// Parse an indirect reference `N G R` from a value slice. Returns (id, gen).
fn parse_indirect_ref(s: &[u8]) -> Option<(u32, u16)> {
    let s = skip_ws(s);
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() { i += 1; }
    let id: u32 = std::str::from_utf8(&s[..i]).ok()?.parse().ok()?;
    let s = skip_ws(&s[i..]);
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() { i += 1; }
    let gen: u16 = std::str::from_utf8(&s[..i]).ok()?.parse().ok()?;
    let s = skip_ws(&s[i..]);
    if !s.starts_with(b"R") { return None; }
    Some((id, gen))
}

/// Parse a leading integer from a slice.
fn parse_int(s: &[u8]) -> Option<i64> {
    let s = skip_ws(s);
    let (negate, s) = if s.starts_with(b"-") { (true, &s[1..]) } else { (false, s) };
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() { i += 1; }
    let n: i64 = std::str::from_utf8(&s[..i]).ok()?.parse().ok()?;
    Some(if negate { -n } else { n })
}

/// Find the `<< ... >>` substring at the top level of the given object body.
/// The object body looks like: `N G obj << ... >> stream ... endstream endobj`
/// or `N G obj << ... >> endobj`.  Returns the slice between the outer `<<` and `>>`.
fn extract_outer_dict(obj_bytes: &[u8]) -> Option<&[u8]> {
    let open = obj_bytes.windows(2).position(|w| w == b"<<")?;
    let mut depth = 0i32;
    let mut i = open;
    while i + 1 < obj_bytes.len() {
        if obj_bytes[i..].starts_with(b"<<") {
            depth += 1;
            i += 2;
        } else if obj_bytes[i..].starts_with(b">>") {
            depth -= 1;
            if depth == 0 {
                return Some(&obj_bytes[open + 2..i]);
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}

/// Parse the trailer that follows a traditional `xref` table starting at
/// `xref_offset`. Returns (catalog_id, /Size value).
fn parse_traditional_trailer(pdf: &[u8], xref_offset: usize) -> Option<(u32, u32)> {
    // Find the "trailer" keyword after xref_offset.
    let needle = b"trailer";
    let from = xref_offset;
    let pos = pdf[from..].windows(needle.len())
        .position(|w| w == needle)? + from;
    let dict = extract_outer_dict(&pdf[pos + needle.len()..])?;
    let (root_id, _) = parse_indirect_ref(find_dict_value(dict, "Root")?)?;
    let size = parse_int(find_dict_value(dict, "Size")?)? as u32;
    Some((root_id, size))
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[test]
fn spike_d1_scanner_finds_xref_and_trailer() {
    let pdf = produce_typst_pdf(TINY_DOC);
    println!("\n--- spike_d1_scanner_finds_xref_and_trailer ---");
    println!("pdf size: {} bytes", pdf.len());

    let xref_offset = find_startxref(&pdf).expect("startxref");
    println!("startxref offset: {}", xref_offset);

    // typst_pdf emits a traditional `xref` table (not an xref stream).
    let head = &pdf[xref_offset..xref_offset + 40.min(pdf.len() - xref_offset)];
    println!("at xref_offset: {:?}", String::from_utf8_lossy(head));
    assert!(pdf[xref_offset..].starts_with(b"xref"),
        "expected traditional xref table");

    let (catalog_id, size) =
        parse_traditional_trailer(&pdf, xref_offset).expect("trailer");
    println!("/Root = {} 0 R", catalog_id);
    println!("/Size = {}", size);

    assert!(catalog_id > 0);
    assert!(size > 0);
}

#[test]
fn spike_d2_locate_catalog_and_first_page() {
    let pdf = produce_typst_pdf(TINY_DOC);
    println!("\n--- spike_d2_locate_catalog_and_first_page ---");

    let xref_offset = find_startxref(&pdf).unwrap();
    let (catalog_id, _) = parse_traditional_trailer(&pdf, xref_offset).unwrap();
    println!("catalog id: {}", catalog_id);

    // Find catalog bytes
    let (cat_start, cat_end) = find_object_bytes(&pdf, catalog_id).expect("catalog obj bytes");
    let cat_dict = extract_outer_dict(&pdf[cat_start..cat_end]).unwrap();
    println!("catalog dict: {:?}", String::from_utf8_lossy(cat_dict));

    // Descend to first page
    let pages_ref = parse_indirect_ref(find_dict_value(cat_dict, "Pages").expect("/Pages")).unwrap();
    println!("/Pages -> {} {} R", pages_ref.0, pages_ref.1);

    let (pages_start, pages_end) = find_object_bytes(&pdf, pages_ref.0).unwrap();
    let pages_dict = extract_outer_dict(&pdf[pages_start..pages_end]).unwrap();
    let kids_val = find_dict_value(pages_dict, "Kids").expect("/Kids");
    println!("/Kids = {:?}", String::from_utf8_lossy(kids_val));

    // First page: parse first ref out of [...]
    let kids_inner = {
        let l = kids_val.iter().position(|&b| b == b'[').expect("[");
        let r = kids_val.iter().position(|&b| b == b']').expect("]");
        &kids_val[l + 1..r]
    };
    let (first_page_id, _) = parse_indirect_ref(kids_inner).unwrap();
    println!("first page id: {}", first_page_id);

    let (pg_start, pg_end) = find_object_bytes(&pdf, first_page_id).unwrap();
    let pg_dict = extract_outer_dict(&pdf[pg_start..pg_end]).unwrap();
    let pg_str = String::from_utf8_lossy(pg_dict);
    println!("page dict (first 300 B): {:?}", &pg_str[..pg_str.len().min(300)]);
    assert!(pg_str.contains("/Type") || pg_str.contains("Type"));
}

#[test]
fn spike_d3_full_incremental_update() {
    let pdf = produce_typst_pdf(TINY_DOC);
    let orig_len = pdf.len();
    println!("\n--- spike_d3_full_incremental_update ---");
    println!("orig pdf: {} bytes", orig_len);

    // ── scan ──
    let xref_offset = find_startxref(&pdf).unwrap();
    let (catalog_id, size) = parse_traditional_trailer(&pdf, xref_offset).unwrap();
    let next_id = size; // next free indirect ID

    // Catalog dict (bytes, for splicing)
    let (cat_start, cat_end) = find_object_bytes(&pdf, catalog_id).unwrap();
    let cat_dict = extract_outer_dict(&pdf[cat_start..cat_end]).unwrap();

    // First-page locate
    let pages_ref = parse_indirect_ref(find_dict_value(cat_dict, "Pages").unwrap()).unwrap();
    let (pages_start, pages_end) = find_object_bytes(&pdf, pages_ref.0).unwrap();
    let pages_dict = extract_outer_dict(&pdf[pages_start..pages_end]).unwrap();
    let kids_val = find_dict_value(pages_dict, "Kids").unwrap();
    let kids_inner = {
        let l = kids_val.iter().position(|&b| b == b'[').unwrap();
        let r = kids_val.iter().position(|&b| b == b']').unwrap();
        &kids_val[l + 1..r]
    };
    let (page_id, _) = parse_indirect_ref(kids_inner).unwrap();
    let (pg_start, pg_end) = find_object_bytes(&pdf, page_id).unwrap();
    let pg_dict = extract_outer_dict(&pdf[pg_start..pg_end]).unwrap();

    println!("catalog={}, pages={}, page={}", catalog_id, pages_ref.0, page_id);
    println!("next free id: {}", next_id);

    // ── build new objects with pdf-writer Chunk ──
    let widget_id = Ref::new(next_id as i32);
    let acroform_id = Ref::new((next_id + 1) as i32);
    let page_ref = Ref::new(page_id as i32);

    let mut chunk = Chunk::new();

    // Widget annotation (merged field + widget dict).
    // pdf-writer: form_field(id) returns Field, Field::into_annotation()
    // returns an Annotation writer on the SAME object id.
    {
        let mut field = chunk.form_field(widget_id);
        field
            .field_type(FieldType::Signature)
            .partial_name(TextStr("approver"));
        let mut ann = field.into_annotation();
        ann.subtype(AnnotationType::Widget)
            .rect(Rect::new(200.0, 200.0, 300.0, 250.0))
            .page(page_ref)
            .flags(AnnotationFlags::PRINT);
        ann.finish();
    }

    // AcroForm dict as a stand-alone indirect object.
    {
        let mut form: Form<'_> = chunk.indirect(acroform_id).start::<Form>();
        form.fields([widget_id])
            .sig_flags(SigFlags::SIGNATURES_EXIST | SigFlags::APPEND_ONLY);
        // /NeedAppearances true (Form derefs to Dict; use pair()).
        form.pair(Name(b"NeedAppearances"), true);
        form.finish();
    }

    // Updated catalog: existing keys + /AcroForm (indirect ref).
    // Build via byte splicing: take cat_dict's inner bytes, append the new pair.
    {
        let mut new_obj: Vec<u8> = Vec::new();
        new_obj.extend_from_slice(format!("{} 0 obj\n<< ", catalog_id).as_bytes());
        new_obj.extend_from_slice(cat_dict);
        new_obj.extend_from_slice(
            format!(" /AcroForm {} 0 R >>\nendobj\n", acroform_id.get()).as_bytes(),
        );
        // Note: we're appending the raw object bytes to the Chunk would force
        // pdf-writer to renumber.  Instead we'll write this object directly
        // into the outgoing byte stream below.  For now stash it.
        SCRATCH_RAW.with(|s| s.borrow_mut().0 = Some(new_obj));
    }

    // Updated page: existing keys + /Annots [<widget>].
    {
        let mut new_obj: Vec<u8> = Vec::new();
        new_obj.extend_from_slice(format!("{} 0 obj\n<< ", page_id).as_bytes());
        new_obj.extend_from_slice(pg_dict);
        new_obj.extend_from_slice(
            format!(" /Annots [{} 0 R] >>\nendobj\n", widget_id.get()).as_bytes(),
        );
        SCRATCH_RAW.with(|s| s.borrow_mut().1 = Some(new_obj));
    }

    // ── assemble incremental update ──
    let mut out = pdf.clone();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }

    // Record offsets for xref table.
    let widget_obj_off = out.len();
    out.extend_from_slice(chunk.as_bytes());

    // The Chunk wrote BOTH widget and AcroForm objects; we don't have per-
    // object offsets from the Chunk API, so we have to derive them by
    // scanning the chunk bytes for the two `N 0 obj` markers.  Simpler:
    // we wrote them in known order, find each.
    let chunk_bytes = chunk.as_bytes();
    let widget_marker = format!("{} 0 obj", widget_id.get());
    let acroform_marker = format!("{} 0 obj", acroform_id.get());
    let widget_off_in_chunk = chunk_bytes.windows(widget_marker.len())
        .position(|w| w == widget_marker.as_bytes()).unwrap();
    let acroform_off_in_chunk = chunk_bytes.windows(acroform_marker.len())
        .position(|w| w == acroform_marker.as_bytes()).unwrap();
    let widget_off = widget_obj_off + widget_off_in_chunk;
    let acroform_off = widget_obj_off + acroform_off_in_chunk;

    // Append updated catalog
    let new_catalog_off = out.len();
    let cat_raw = SCRATCH_RAW.with(|s| s.borrow_mut().0.take()).unwrap();
    out.extend_from_slice(&cat_raw);

    // Append updated page
    let new_page_off = out.len();
    let pg_raw = SCRATCH_RAW.with(|s| s.borrow_mut().1.take()).unwrap();
    out.extend_from_slice(&pg_raw);

    // ── xref subsection ──
    // We're writing in *traditional* xref form (a "hybrid-reference file" — spec
    // explicitly permits mixing an xref stream with an incremental traditional
    // table).  Subsections: object 0 unused, then four updates.
    let new_xref_off = out.len();
    // We need subsection headers per contiguous run; objects: widget_id,
    // acroform_id, catalog_id, page_id.  Sort them and group runs.
    let mut entries: Vec<(u32, usize)> = vec![
        (widget_id.get() as u32, widget_off),
        (acroform_id.get() as u32, acroform_off),
        (catalog_id, new_catalog_off),
        (page_id, new_page_off),
    ];
    entries.sort_by_key(|(id, _)| *id);

    out.extend_from_slice(b"xref\n");
    // Group runs of consecutive IDs.
    let mut i = 0;
    while i < entries.len() {
        let mut j = i;
        while j + 1 < entries.len() && entries[j + 1].0 == entries[j].0 + 1 {
            j += 1;
        }
        out.extend_from_slice(format!("{} {}\n", entries[i].0, j - i + 1).as_bytes());
        for &(_, off) in &entries[i..=j] {
            // 10-digit offset, 5-digit gen, ' n \n' = 20 bytes per entry.
            out.extend_from_slice(format!("{:010} {:05} n \n", off, 0).as_bytes());
        }
        i = j + 1;
    }

    // ── trailer ──
    out.extend_from_slice(b"trailer\n<< ");
    out.extend_from_slice(format!("/Size {} ", next_id + 2).as_bytes());
    out.extend_from_slice(format!("/Root {} 0 R ", catalog_id).as_bytes());
    out.extend_from_slice(format!("/Prev {} ", xref_offset).as_bytes());
    out.extend_from_slice(b">>\n");

    // startxref + EOF
    out.extend_from_slice(b"startxref\n");
    out.extend_from_slice(format!("{}\n", new_xref_off).as_bytes());
    out.extend_from_slice(b"%%EOF\n");

    println!("output: {} bytes ({:+} vs orig)", out.len(), out.len() as i64 - orig_len as i64);

    fs::write("/tmp/qm_spike_d3_pdfwriter.pdf", &out).expect("write output");
    println!("written to /tmp/qm_spike_d3_pdfwriter.pdf");

    // ── verify with lopdf ──
    let doc = lopdf::Document::load_mem(&out).expect("lopdf reparse");
    let cat = doc.catalog().expect("catalog");
    assert!(cat.has(b"AcroForm"), "/AcroForm missing from new catalog");
    println!("✓ /AcroForm present");

    // Verify the AcroForm dict has /Fields with our widget
    let af_ref = cat.get(b"AcroForm").unwrap().as_reference().expect("AcroForm indirect");
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();
    println!("AcroForm /Fields = {:?}", af.get(b"Fields").unwrap());
    println!("AcroForm /SigFlags = {:?}", af.get(b"SigFlags").unwrap());
    println!("AcroForm /NeedAppearances = {:?}", af.get(b"NeedAppearances").unwrap());

    let fields = af.get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 1);
    let widget_ref = fields[0].as_reference().unwrap();
    let widget = doc.get_object(widget_ref).unwrap().as_dict().unwrap();
    assert_eq!(widget.get(b"FT").unwrap().as_name().unwrap(), b"Sig");
    assert_eq!(widget.get(b"Subtype").unwrap().as_name().unwrap(), b"Widget");
    println!("✓ widget /FT /Sig and /Subtype /Widget");

    // Verify the page's /Annots references the widget
    let pages = doc.get_pages();
    let (&_, &page_obj_id) = pages.iter().next().unwrap();
    let pg = doc.get_object(page_obj_id).unwrap().as_dict().unwrap();
    let annots = pg.get(b"Annots").unwrap().as_array().unwrap();
    assert_eq!(annots.len(), 1);
    assert_eq!(annots[0].as_reference().unwrap(), widget_ref);
    println!("✓ page /Annots references widget");

    println!("\nAll structural checks passed.");
}

// Scratch storage for raw object bytes that don't fit pdf-writer's Chunk API
// (we splice into existing catalog/page dict bytes; Chunk can't accept raw bytes
// with caller-controlled object IDs without renumbering).
thread_local! {
    static SCRATCH_RAW: std::cell::RefCell<(Option<Vec<u8>>, Option<Vec<u8>>)>
        = std::cell::RefCell::new((None, None));
}
