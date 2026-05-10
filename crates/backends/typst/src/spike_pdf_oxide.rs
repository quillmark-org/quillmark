//! SPIKE — not for merge. Mirror of spike_lopdf.rs using pdf_oxide instead
//! of lopdf, to compare API ergonomics, output size, and behavioural fit
//! for unsigned SigField injection.
//!
//! Run with: `cargo test -p quillmark-typst --lib spike_c -- --nocapture`

#![cfg(test)]
#![allow(unused_imports)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use pdf_oxide::editor::DocumentEditor;
use pdf_oxide::geometry::Rect;
use pdf_oxide::writer::form_fields::SignatureWidget;
use quillmark_core::{FileTreeNode, QuillSource};
use typst::layout::PagedDocument;
use typst_pdf::PdfOptions;

use crate::world::QuillWorld;

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

const TINY_DOC: &str = r#"
#set page(width: 600pt, height: 400pt, margin: 50pt)
= Tiny test document
This PDF exists to be poked at by pdf_oxide.
"#;

/// Probe 1: Can pdf_oxide open a typst_pdf output?
#[test]
fn spike_c1_parse() {
    let pdf = produce_typst_pdf(TINY_DOC);
    let original_size = pdf.len();

    println!("\n--- spike_c1_parse ---");
    println!("typst_pdf produced {} bytes", original_size);

    let editor = DocumentEditor::from_bytes(pdf).expect("pdf_oxide open");
    let page_count = editor.current_page_count();
    let version = editor.version();

    println!("PDF version: {}.{}", version.0, version.1);
    println!("page count: {}", page_count);
    println!("source path: {}", editor.source_path());
}

/// Probe 2: Round-trip open → save and report size delta.
#[test]
fn spike_c2_round_trip() {
    let pdf = produce_typst_pdf(TINY_DOC);
    let original_size = pdf.len();

    let mut editor = DocumentEditor::from_bytes(pdf).expect("open");
    let out = editor.save_to_bytes().expect("save");

    let out_path = "/tmp/qm_spike_c2_roundtrip.pdf";
    fs::write(out_path, &out).expect("write");

    println!("\n--- spike_c2_round_trip ---");
    println!("input  : {} bytes", original_size);
    println!(
        "output : {} bytes ({:+} bytes, {:+.1}%)",
        out.len(),
        out.len() as i64 - original_size as i64,
        (out.len() as f64 - original_size as f64) / original_size as f64 * 100.0,
    );
    println!("written to {}", out_path);

    // Reparse to confirm
    let _editor2 = DocumentEditor::from_bytes(out).expect("reparse");
    println!("reparse: OK");
}

/// Probe 3: Inject an unsigned SigField via the typed API.
#[test]
fn spike_c3_inject_sigfield() {
    let pdf = produce_typst_pdf(TINY_DOC);
    let original_size = pdf.len();

    let mut editor = DocumentEditor::from_bytes(pdf).expect("open");

    let widget = SignatureWidget::new(
        "approver",
        Rect::new(200.0, 200.0, 100.0, 50.0),
    );

    let assigned = editor
        .add_form_field(0, widget)
        .expect("add_form_field on page 0");

    println!("\n--- spike_c3_inject_sigfield ---");
    println!("typst_pdf input : {} bytes", original_size);
    println!("assigned name   : {}", assigned);

    let out = editor.save_to_bytes().expect("save");
    let out_path = "/tmp/qm_spike_c3_signed.pdf";
    fs::write(out_path, &out).expect("write");

    println!("output          : {} bytes", out.len());
    println!("written to {}", out_path);

    // Reparse and (via lopdf, to use the same inspection lens as spike b3)
    // confirm /AcroForm landed.
    let doc = lopdf::Document::load_mem(&out).expect("reparse via lopdf");
    let cat = doc.catalog().unwrap();
    println!("output catalog has /AcroForm: {}", cat.has(b"AcroForm"));
    if let Ok(lopdf::Object::Reference(af_ref)) = cat.get(b"AcroForm") {
        if let Ok(af) = doc.get_object(*af_ref).and_then(|o| o.as_dict()) {
            println!("AcroForm /Fields = {:?}", af.get(b"Fields").ok());
            println!("AcroForm /SigFlags = {:?}", af.get(b"SigFlags").ok());
            println!("AcroForm /NeedAppearances = {:?}", af.get(b"NeedAppearances").ok());
        }
    } else if let Ok(lopdf::Object::Dictionary(af)) = cat.get(b"AcroForm") {
        println!("AcroForm /Fields = {:?}", af.get(b"Fields").ok());
        println!("AcroForm /SigFlags = {:?}", af.get(b"SigFlags").ok());
        println!("AcroForm /NeedAppearances = {:?}", af.get(b"NeedAppearances").ok());
    }
}

/// Probe 4: Same-named field twice — pdf_oxide's docs claim auto-uniquing.
#[test]
fn spike_c4_dedup_names() {
    let pdf = produce_typst_pdf(TINY_DOC);
    let mut editor = DocumentEditor::from_bytes(pdf).expect("open");

    let a = editor
        .add_form_field(0, SignatureWidget::new("approver", Rect::new(100.0, 200.0, 100.0, 50.0)))
        .expect("first");
    let b = editor
        .add_form_field(0, SignatureWidget::new("approver", Rect::new(100.0, 100.0, 100.0, 50.0)))
        .expect("second");

    println!("\n--- spike_c4_dedup_names ---");
    println!("first  -> {a}");
    println!("second -> {b}");

    let _ = editor.save_to_bytes().expect("save");
    assert_ne!(a, b, "second add should produce a distinct name");
}
