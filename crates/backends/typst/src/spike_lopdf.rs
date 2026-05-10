//! SPIKE — not for merge. Verifies that lopdf can:
//!   1. Parse a typst_pdf-produced PDF without error
//!   2. Re-save it without losing semantics
//!   3. Inject an `/AcroForm` + a single SigField widget on page 1
//!
//! Output PDFs are written to /tmp/qm_spike_*.pdf for manual inspection.
//! Run with: `cargo test -p quillmark-typst --lib spike_b -- --nocapture`

#![cfg(test)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use lopdf::{dictionary, Document as LoDoc, Object};
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
This PDF exists to be poked at by lopdf.
"#;

/// Probe 1: Can lopdf parse a typst_pdf output at all?
#[test]
fn spike_b1_parse() {
    let pdf = produce_typst_pdf(TINY_DOC);
    let original_size = pdf.len();

    println!("\n--- spike_b1_parse ---");
    println!("typst_pdf produced {} bytes", original_size);

    let doc = LoDoc::load_mem(&pdf).expect("lopdf parse");
    println!("PDF version: {}", doc.version);
    println!("object count: {}", doc.objects.len());
    println!("trailer keys: {:?}", doc.trailer.iter().map(|(k, _)| String::from_utf8_lossy(k).to_string()).collect::<Vec<_>>());

    let pages = doc.get_pages();
    println!("page count: {}", pages.len());

    let cat = doc.catalog().unwrap();
    println!("catalog has /AcroForm: {}", cat.has(b"AcroForm"));
}

/// Probe 2: Round-trip parse → save and report size delta.
#[test]
fn spike_b2_round_trip() {
    let pdf = produce_typst_pdf(TINY_DOC);
    let original_size = pdf.len();

    let mut doc = LoDoc::load_mem(&pdf).expect("lopdf parse");
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("lopdf save");

    let out_path = "/tmp/qm_spike_b2_roundtrip.pdf";
    fs::write(out_path, &out).expect("write pdf");

    println!("\n--- spike_b2_round_trip ---");
    println!("input  : {} bytes", original_size);
    println!("output : {} bytes ({:+} bytes, {:+.1}%)",
        out.len(),
        out.len() as i64 - original_size as i64,
        (out.len() as f64 - original_size as f64) / original_size as f64 * 100.0,
    );
    println!("written to {}", out_path);

    // Verify the output reparses
    let _doc2 = LoDoc::load_mem(&out).expect("output should reparse");
    println!("reparse: OK");
}

/// Probe 3: Inject an /AcroForm + one SigField widget on page 1.
#[test]
fn spike_b3_inject_sigfield() {
    let pdf = produce_typst_pdf(TINY_DOC);
    let mut doc = LoDoc::load_mem(&pdf).expect("lopdf parse");

    // Find page 1 by enumerating
    let pages = doc.get_pages();
    let (&_page_num, &page_id) = pages
        .iter()
        .next()
        .expect("at least one page");

    println!("\n--- spike_b3_inject_sigfield ---");
    println!("target page object id: {:?}", page_id);

    // Build the SigField widget annotation (merged field+widget dict)
    // Rect: 100 x 50 pt at (200, 200) in PDF space (lower-left origin)
    let sig_dict = dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "FT" => "Sig",
        "T" => Object::string_literal("approver"),
        "Rect" => vec![Object::Real(200.0), Object::Real(200.0), Object::Real(300.0), Object::Real(250.0)],
        "P" => page_id,
        "F" => 4i64,  // /Print flag
    };
    let sig_id = doc.add_object(sig_dict);
    println!("created sigfield object id: {:?}", sig_id);

    // Append to page's /Annots array
    {
        let page = doc.get_object_mut(page_id).unwrap().as_dict_mut().unwrap();
        let annots = page.get_mut(b"Annots").ok().cloned();
        match annots {
            Some(Object::Array(mut arr)) => {
                arr.push(Object::Reference(sig_id));
                page.set("Annots", Object::Array(arr));
            }
            Some(Object::Reference(arr_id)) => {
                let arr_obj = doc.get_object_mut(arr_id).unwrap();
                if let Object::Array(arr) = arr_obj {
                    arr.push(Object::Reference(sig_id));
                }
            }
            None => {
                page.set("Annots", Object::Array(vec![Object::Reference(sig_id)]));
            }
            _ => panic!("unexpected /Annots type"),
        }
    }

    // Build /AcroForm dict and attach to catalog
    let acroform_dict = dictionary! {
        "Fields" => vec![Object::Reference(sig_id)],
        "SigFlags" => 3i64,  // SignaturesExist + AppendOnly
    };
    let acroform_id = doc.add_object(acroform_dict);

    let catalog_id = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let catalog = doc.get_object_mut(catalog_id).unwrap().as_dict_mut().unwrap();
    catalog.set("AcroForm", Object::Reference(acroform_id));

    // Save
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save with acroform");
    let out_path = "/tmp/qm_spike_b3_signed.pdf";
    fs::write(out_path, &out).expect("write");

    println!("output size: {} bytes", out.len());
    println!("written to {}", out_path);

    // Reparse and verify the structure landed
    let doc2 = LoDoc::load_mem(&out).expect("reparse");
    let cat = doc2.catalog().unwrap();
    println!("output catalog has /AcroForm: {}", cat.has(b"AcroForm"));

    if let Ok(Object::Reference(af_ref)) = cat.get(b"AcroForm") {
        let af = doc2.get_object(*af_ref).unwrap().as_dict().unwrap();
        println!("AcroForm /Fields = {:?}", af.get(b"Fields").ok());
        println!("AcroForm /SigFlags = {:?}", af.get(b"SigFlags").ok());
    }

}
