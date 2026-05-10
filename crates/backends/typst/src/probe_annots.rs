//! PROBE — temporary, not for merge. Validates the /Annots indirect-reference
//! case identified as footgun #3 in the plan.
//!
//! Two questions:
//!   1. When the input PDF already has /Annots on a page (from `#link()`),
//!      how does typst_pdf encode it — inline array or indirect reference?
//!   2. Can we inject a SigField widget without breaking the existing link
//!      annotation, and does the result reparse cleanly?
//!
//! Run with: `cargo test -p quillmark-typst --lib probe_annots -- --nocapture`

#![cfg(test)]
#![allow(unused_imports)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use pdf_writer::types::{AnnotationFlags, AnnotationType, FieldType, SigFlags};
use pdf_writer::writers::Form;
use pdf_writer::{Chunk, Finish, Name, Rect, Ref, TextStr};
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
                files.insert(
                    name,
                    FileTreeNode::File {
                        contents: fs::read(&p)?,
                    },
                );
            } else if p.is_dir() {
                files.insert(name, walk(&p)?);
            }
        }
        Ok(FileTreeNode::Directory { files })
    }
    let quill_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
        .join("resources")
        .join("quills")
        .join("usaf_memo")
        .join("0.1.0");
    let tree = walk(&quill_path).expect("walk fixture");
    QuillSource::from_tree(tree).expect("load source")
}

fn produce_typst_pdf(main: &str) -> Vec<u8> {
    let world = QuillWorld::new(&load_fixture(), main).expect("world");
    let doc = typst::compile::<PagedDocument>(&world)
        .output
        .expect("compile ok");
    typst_pdf::pdf(&doc, &PdfOptions::default()).expect("pdf ok")
}

const LINK_DOC: &str = r#"
#set page(width: 600pt, height: 400pt, margin: 50pt)
= Doc with link
Click #link("https://example.com")[here] for more info.
"#;

// ─── minimal scanner helpers ports from spike_pdfwriter ───────────────────────

fn find_startxref(pdf: &[u8]) -> Option<usize> {
    let needle = b"startxref";
    let from = pdf.len().saturating_sub(1024);
    let tail = &pdf[from..];
    let pos = tail.windows(needle.len()).rposition(|w| w == needle)?;
    let after = &tail[pos + needle.len()..];
    let after = skip_ws(after);
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

fn find_object_bytes(pdf: &[u8], id: u32) -> Option<(usize, usize)> {
    let header = format!("{} 0 obj", id);
    let h = header.as_bytes();
    let mut i = 0;
    while i + h.len() < pdf.len() {
        if pdf[i..].starts_with(h) && (i == 0 || matches!(pdf[i - 1], b'\n' | b'\r' | b' ')) {
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

fn find_dict_value<'a>(dict_bytes: &'a [u8], key: &str) -> Option<&'a [u8]> {
    let key_marker = format!("/{}", key);
    let km = key_marker.as_bytes();
    let mut i = 0;
    while i + km.len() < dict_bytes.len() {
        if dict_bytes[i..].starts_with(km) {
            let next = dict_bytes.get(i + km.len()).copied();
            if matches!(
                next,
                Some(b' ')
                    | Some(b'\t')
                    | Some(b'\n')
                    | Some(b'\r')
                    | Some(b'/')
                    | Some(b'[')
                    | Some(b'<')
            ) {
                let start = i + km.len();
                let mut j = start;
                while j < dict_bytes.len() {
                    if dict_bytes[j..].starts_with(b">>") {
                        return Some(&dict_bytes[start..j]);
                    }
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

fn parse_indirect_ref(s: &[u8]) -> Option<(u32, u16)> {
    let s = skip_ws(s);
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let id: u32 = std::str::from_utf8(&s[..i]).ok()?.parse().ok()?;
    let s = skip_ws(&s[i..]);
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let gen: u16 = std::str::from_utf8(&s[..i]).ok()?.parse().ok()?;
    let s = skip_ws(&s[i..]);
    if !s.starts_with(b"R") {
        return None;
    }
    Some((id, gen))
}

fn parse_int(s: &[u8]) -> Option<i64> {
    let s = skip_ws(s);
    let (negate, s) = if s.starts_with(b"-") {
        (true, &s[1..])
    } else {
        (false, s)
    };
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let n: i64 = std::str::from_utf8(&s[..i]).ok()?.parse().ok()?;
    Some(if negate { -n } else { n })
}

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

fn parse_traditional_trailer(pdf: &[u8], xref_offset: usize) -> Option<(u32, u32)> {
    let needle = b"trailer";
    let from = xref_offset;
    let pos = pdf[from..]
        .windows(needle.len())
        .position(|w| w == needle)?
        + from;
    let dict = extract_outer_dict(&pdf[pos + needle.len()..])?;
    let (root_id, _) = parse_indirect_ref(find_dict_value(dict, "Root")?)?;
    let size = parse_int(find_dict_value(dict, "Size")?)? as u32;
    Some((root_id, size))
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[test]
fn probe_p1_observe_annots_shape() {
    let pdf = produce_typst_pdf(LINK_DOC);
    println!("\n--- probe_p1_observe_annots_shape ---");
    println!("pdf size: {} bytes", pdf.len());

    // Cross-check with lopdf first — it gives us a known-good view.
    let doc = lopdf::Document::load_mem(&pdf).expect("lopdf reparse");
    let pages = doc.get_pages();
    println!("page count: {}", pages.len());
    let (&_, &page_id) = pages.iter().next().expect("at least one page");
    let page = doc.get_object(page_id).unwrap().as_dict().unwrap();
    println!("page dict keys: {:?}", page.iter().map(|(k, _)| String::from_utf8_lossy(k).to_string()).collect::<Vec<_>>());

    let annots = page.get(b"Annots").expect("/Annots should be present on link page");
    println!("/Annots variant: {:?}", annots);
    println!("/Annots is_array: {}", annots.as_array().is_ok());
    println!("/Annots is_reference: {}", annots.as_reference().is_ok());

    match annots {
        lopdf::Object::Array(arr) => {
            println!("INLINE array, {} entries:", arr.len());
            for (i, e) in arr.iter().enumerate() {
                println!("  [{i}] {:?}", e);
            }
        }
        lopdf::Object::Reference(r) => {
            println!("INDIRECT reference -> {} {} R", r.0, r.1);
            let array_obj = doc.get_object(*r).unwrap();
            println!("  target object: {:?}", array_obj);
            if let Ok(arr) = array_obj.as_array() {
                println!("  target is array of {} entries", arr.len());
                for (i, e) in arr.iter().enumerate() {
                    println!("    [{i}] {:?}", e);
                }
            }
        }
        other => panic!("unexpected /Annots variant: {:?}", other),
    }

    // Now cross-check our scanner sees the same thing in the raw bytes.
    let xref_offset = find_startxref(&pdf).unwrap();
    assert!(pdf[xref_offset..].starts_with(b"xref"));
    let (catalog_id, _) = parse_traditional_trailer(&pdf, xref_offset).unwrap();
    let (cs, ce) = find_object_bytes(&pdf, catalog_id).unwrap();
    let cat_dict = extract_outer_dict(&pdf[cs..ce]).unwrap();
    let pages_ref = parse_indirect_ref(find_dict_value(cat_dict, "Pages").unwrap()).unwrap();
    let (ps, pe) = find_object_bytes(&pdf, pages_ref.0).unwrap();
    let pages_dict = extract_outer_dict(&pdf[ps..pe]).unwrap();
    let kids_val = find_dict_value(pages_dict, "Kids").unwrap();
    let l = kids_val.iter().position(|&b| b == b'[').unwrap();
    let r = kids_val.iter().position(|&b| b == b']').unwrap();
    let (page_obj_id, _) = parse_indirect_ref(&kids_val[l + 1..r]).unwrap();
    let (pgs, pge) = find_object_bytes(&pdf, page_obj_id).unwrap();
    let pg_dict = extract_outer_dict(&pdf[pgs..pge]).unwrap();
    let raw_annots = find_dict_value(pg_dict, "Annots").expect("/Annots in raw page dict");
    let trimmed = String::from_utf8_lossy(raw_annots).trim().to_string();
    println!("raw /Annots value bytes: {:?}", trimmed);

    let starts_with_bracket = trimmed.starts_with('[');
    let is_indirect_ref = parse_indirect_ref(raw_annots).is_some();
    println!("scanner: inline_array={}, indirect_ref={}", starts_with_bracket, is_indirect_ref);
}

#[test]
fn probe_p2_inject_alongside_existing_annots() {
    let pdf = produce_typst_pdf(LINK_DOC);
    let orig_len = pdf.len();
    println!("\n--- probe_p2_inject_alongside_existing_annots ---");
    println!("orig pdf: {} bytes", orig_len);

    let xref_offset = find_startxref(&pdf).unwrap();
    let (catalog_id, size) = parse_traditional_trailer(&pdf, xref_offset).unwrap();
    let next_id = size;

    let (cs, ce) = find_object_bytes(&pdf, catalog_id).unwrap();
    let cat_dict = extract_outer_dict(&pdf[cs..ce]).unwrap();
    let pages_ref = parse_indirect_ref(find_dict_value(cat_dict, "Pages").unwrap()).unwrap();
    let (ps, pe) = find_object_bytes(&pdf, pages_ref.0).unwrap();
    let pages_dict = extract_outer_dict(&pdf[ps..pe]).unwrap();
    let kids_val = find_dict_value(pages_dict, "Kids").unwrap();
    let l = kids_val.iter().position(|&b| b == b'[').unwrap();
    let r = kids_val.iter().position(|&b| b == b']').unwrap();
    let (page_obj_id, _) = parse_indirect_ref(&kids_val[l + 1..r]).unwrap();
    let (pgs, pge) = find_object_bytes(&pdf, page_obj_id).unwrap();
    let pg_dict = extract_outer_dict(&pdf[pgs..pge]).unwrap();

    // Strategy: replace /Annots <existing> with /Annots [<existing-entries> widget_ref].
    // To keep this probe minimal we don't strip the existing /Annots; we
    // rewrite the page dict by removing the original /Annots ... section and
    // appending the new one.
    let existing_annots = find_dict_value(pg_dict, "Annots").expect("/Annots present");
    let existing_trimmed = std::str::from_utf8(existing_annots).unwrap().trim();
    println!("existing /Annots: {:?}", existing_trimmed);

    let widget_id = Ref::new(next_id as i32);
    let acroform_id = Ref::new((next_id + 1) as i32);

    // Build merged annots list as text — handle both inline-array and indirect-ref cases.
    let merged_annots = if existing_trimmed.starts_with('[') {
        // inline array — splice widget ref into the array
        let inner = existing_trimmed.trim_start_matches('[').trim_end_matches(']');
        format!("[{} {} 0 R]", inner.trim(), widget_id.get())
    } else if parse_indirect_ref(existing_annots).is_some() {
        // indirect reference — keep it and add a new inline array containing the original ref + ours
        format!("[{} {} 0 R]", existing_trimmed, widget_id.get())
    } else {
        panic!("unrecognized /Annots shape: {:?}", existing_trimmed);
    };
    println!("merged /Annots: {}", merged_annots);

    // Build chunks for widget + acroform via pdf-writer.
    let mut chunk = Chunk::new();
    {
        let mut field = chunk.form_field(widget_id);
        field
            .field_type(FieldType::Signature)
            .partial_name(TextStr("approver"));
        let mut ann = field.into_annotation();
        ann.subtype(AnnotationType::Widget)
            .rect(Rect::new(200.0, 200.0, 300.0, 250.0))
            .page(Ref::new(page_obj_id as i32))
            .flags(AnnotationFlags::PRINT);
        ann.finish();
    }
    {
        let mut form: Form<'_> = chunk.indirect(acroform_id).start::<Form>();
        form.fields([widget_id])
            .sig_flags(SigFlags::SIGNATURES_EXIST | SigFlags::APPEND_ONLY);
        form.pair(Name(b"NeedAppearances"), true);
        form.finish();
    }

    // Build updated catalog by appending /AcroForm.
    let updated_catalog = {
        let mut v = Vec::new();
        v.extend_from_slice(format!("{} 0 obj\n<< ", catalog_id).as_bytes());
        v.extend_from_slice(cat_dict);
        v.extend_from_slice(format!(" /AcroForm {} 0 R >>\nendobj\n", acroform_id.get()).as_bytes());
        v
    };

    // Build updated page by stripping its existing /Annots key+value and replacing.
    // Cheap approach: locate "/Annots" in the dict bytes, find the byte range to strip,
    // then concatenate prefix + suffix + new /Annots.
    let updated_page = {
        let key = b"/Annots";
        let pg_str = pg_dict;
        let key_at = pg_str
            .windows(key.len())
            .position(|w| w == key)
            .expect("locate /Annots");
        // Find end of the value: next "/" at depth 0, or end of dict.
        let val_start = key_at + key.len();
        let mut depth_b = 0i32; // [
        let mut depth_d = 0i32; // <<
        let mut end = val_start;
        let mut i = val_start;
        while i < pg_str.len() {
            if pg_str[i..].starts_with(b"<<") {
                depth_d += 1;
                i += 2;
                continue;
            }
            if pg_str[i..].starts_with(b">>") {
                depth_d -= 1;
                if depth_d < 0 {
                    end = i;
                    break;
                }
                i += 2;
                continue;
            }
            match pg_str[i] {
                b'[' => {
                    depth_b += 1;
                    i += 1;
                }
                b']' => {
                    depth_b -= 1;
                    i += 1;
                    if depth_b == 0 && depth_d == 0 {
                        // After array close, the next non-space char terminates the value
                        // if it's `/` (next key) or `>>`.
                        let mut k = i;
                        while k < pg_str.len()
                            && matches!(pg_str[k], b' ' | b'\t' | b'\n' | b'\r')
                        {
                            k += 1;
                        }
                        end = k;
                        break;
                    }
                }
                b'/' => {
                    if depth_b == 0 && depth_d == 0 && i > val_start {
                        end = i;
                        break;
                    }
                    i += 1;
                }
                _ => {
                    i += 1;
                }
            }
        }
        let prefix = &pg_str[..key_at];
        let suffix = &pg_str[end..];
        let mut v = Vec::new();
        v.extend_from_slice(format!("{} 0 obj\n<< ", page_obj_id).as_bytes());
        v.extend_from_slice(prefix);
        v.extend_from_slice(format!(" /Annots {} ", merged_annots).as_bytes());
        v.extend_from_slice(suffix);
        v.extend_from_slice(b" >>\nendobj\n");
        v
    };

    // Assemble incremental update.
    let mut out = pdf.clone();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }

    let widget_obj_off = out.len();
    out.extend_from_slice(chunk.as_bytes());

    let chunk_bytes = chunk.as_bytes();
    let widget_marker = format!("{} 0 obj", widget_id.get());
    let acroform_marker = format!("{} 0 obj", acroform_id.get());
    let widget_off_in_chunk = chunk_bytes
        .windows(widget_marker.len())
        .position(|w| w == widget_marker.as_bytes())
        .unwrap();
    let acroform_off_in_chunk = chunk_bytes
        .windows(acroform_marker.len())
        .position(|w| w == acroform_marker.as_bytes())
        .unwrap();
    let widget_off = widget_obj_off + widget_off_in_chunk;
    let acroform_off = widget_obj_off + acroform_off_in_chunk;

    let new_catalog_off = out.len();
    out.extend_from_slice(&updated_catalog);
    let new_page_off = out.len();
    out.extend_from_slice(&updated_page);

    let new_xref_off = out.len();
    let mut entries: Vec<(u32, usize)> = vec![
        (widget_id.get() as u32, widget_off),
        (acroform_id.get() as u32, acroform_off),
        (catalog_id, new_catalog_off),
        (page_obj_id, new_page_off),
    ];
    entries.sort_by_key(|(id, _)| *id);
    out.extend_from_slice(b"xref\n");
    let mut i = 0;
    while i < entries.len() {
        let mut j = i;
        while j + 1 < entries.len() && entries[j + 1].0 == entries[j].0 + 1 {
            j += 1;
        }
        out.extend_from_slice(format!("{} {}\n", entries[i].0, j - i + 1).as_bytes());
        for &(_, off) in &entries[i..=j] {
            out.extend_from_slice(format!("{:010} {:05} n \n", off, 0).as_bytes());
        }
        i = j + 1;
    }
    out.extend_from_slice(b"trailer\n<< ");
    out.extend_from_slice(format!("/Size {} ", next_id + 2).as_bytes());
    out.extend_from_slice(format!("/Root {} 0 R ", catalog_id).as_bytes());
    out.extend_from_slice(format!("/Prev {} ", xref_offset).as_bytes());
    out.extend_from_slice(b">>\n");
    out.extend_from_slice(b"startxref\n");
    out.extend_from_slice(format!("{}\n", new_xref_off).as_bytes());
    out.extend_from_slice(b"%%EOF\n");

    println!(
        "output: {} bytes ({:+} vs orig)",
        out.len(),
        out.len() as i64 - orig_len as i64
    );
    fs::write("/tmp/qm_probe_p2_annots.pdf", &out).expect("write");
    println!("written to /tmp/qm_probe_p2_annots.pdf");

    // Verify with lopdf.
    let doc = lopdf::Document::load_mem(&out).expect("lopdf reparse");
    let cat = doc.catalog().expect("catalog");
    assert!(cat.has(b"AcroForm"), "/AcroForm missing");
    let af_ref = cat.get(b"AcroForm").unwrap().as_reference().unwrap();
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();
    let fields = af.get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 1, "expected exactly one field");
    let widget_ref_check = fields[0].as_reference().unwrap();
    let widget = doc.get_object(widget_ref_check).unwrap().as_dict().unwrap();
    assert_eq!(widget.get(b"FT").unwrap().as_name().unwrap(), b"Sig");

    let pages = doc.get_pages();
    let (&_, &page_obj_id_after) = pages.iter().next().unwrap();
    let pg = doc.get_object(page_obj_id_after).unwrap().as_dict().unwrap();
    let annots = pg.get(b"Annots").unwrap().as_array().unwrap();
    println!("post-inject /Annots entries: {}", annots.len());
    for (i, e) in annots.iter().enumerate() {
        println!("  [{i}] {:?}", e);
    }
    assert!(
        annots.len() >= 2,
        "expected at least 2 entries (existing link + widget), got {}",
        annots.len()
    );

    // Confirm the widget ref is in there.
    let has_widget = annots
        .iter()
        .any(|e| e.as_reference().ok() == Some(widget_ref_check));
    assert!(has_widget, "widget ref not found in merged /Annots");
    println!("✓ widget present in merged /Annots");

    // Confirm the existing link annotation survived: every original /Annots
    // entry resolves to a dict with /Subtype /Link.
    let mut link_count = 0;
    for e in annots {
        if let Ok(r) = e.as_reference() {
            if r == widget_ref_check {
                continue;
            }
            if let Ok(obj) = doc.get_object(r) {
                if let Ok(d) = obj.as_dict() {
                    if d.get(b"Subtype").ok().and_then(|v| v.as_name().ok()) == Some(&b"Link"[..]) {
                        link_count += 1;
                    }
                }
            }
        }
    }
    println!("preserved /Link entries: {}", link_count);
    assert!(link_count >= 1, "no surviving /Link annotation");
    println!("\nAll structural checks passed.");
}

#[test]
fn probe_p3_link_semantic_integrity() {
    // Re-run the inject from p2, then verify the link annotation still has
    // its URL action and rect — not just that the dict survives.
    let pdf = produce_typst_pdf(LINK_DOC);

    // Snapshot the original link annot for comparison.
    let orig = lopdf::Document::load_mem(&pdf).unwrap();
    let pages = orig.get_pages();
    let (&_, &pid) = pages.iter().next().unwrap();
    let pg = orig.get_object(pid).unwrap().as_dict().unwrap();
    let orig_link_ref = pg
        .get(b"Annots")
        .unwrap()
        .as_array()
        .unwrap()[0]
        .as_reference()
        .unwrap();
    let orig_link = orig.get_object(orig_link_ref).unwrap().as_dict().unwrap();
    let orig_rect = format!("{:?}", orig_link.get(b"Rect").unwrap());
    let orig_action = format!("{:?}", orig_link.get(b"A").unwrap());
    println!("\n--- probe_p3 (original link) ---");
    println!("  Rect: {}", orig_rect);
    println!("  A:    {}", orig_action);

    // Read the file written by p2 (must have been run first).
    let out = std::fs::read("/tmp/qm_probe_p2_annots.pdf")
        .expect("run probe_p2 first to produce /tmp/qm_probe_p2_annots.pdf");
    let doc = lopdf::Document::load_mem(&out).unwrap();
    let pages = doc.get_pages();
    let (&_, &pid_after) = pages.iter().next().unwrap();
    let pg_after = doc.get_object(pid_after).unwrap().as_dict().unwrap();
    let annots_after = pg_after.get(b"Annots").unwrap().as_array().unwrap();

    let mut found = false;
    for e in annots_after {
        let r = e.as_reference().unwrap();
        if r == orig_link_ref {
            let d = doc.get_object(r).unwrap().as_dict().unwrap();
            let new_rect = format!("{:?}", d.get(b"Rect").unwrap());
            let new_action = format!("{:?}", d.get(b"A").unwrap());
            println!("--- post-inject link ---");
            println!("  Rect: {}", new_rect);
            println!("  A:    {}", new_action);
            assert_eq!(orig_rect, new_rect, "link /Rect changed");
            assert_eq!(orig_action, new_action, "link /A changed");
            found = true;
        }
    }
    assert!(found, "original link ref missing from post-inject /Annots");
    println!("\n✓ link annotation byte-identical after inject");
}
