use quillmark_wasm::Quillmark;
use wasm_bindgen_test::*;

mod common;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn test_quill_from_tree_versioned() {
    let engine = Quillmark::new();

    let q1 = engine.quill(common::tree(&[
        (
            "Quill.yaml",
            b"quill:\n  name: usaf_memo\n  version: \"0.1.0\"\n  backend: typst\n  main_file: main.typ\n  description: Version 0.1.0\n",
        ),
        ("main.typ", b"hello 1"),
    ])).unwrap();

    let q2 = engine.quill(common::tree(&[
        (
            "Quill.yaml",
            b"quill:\n  name: usaf_memo\n  version: \"0.2.0\"\n  backend: typst\n  main_file: main.typ\n  description: Version 0.2.0\n",
        ),
        ("main.typ", b"hello 2"),
    ])).unwrap();

    let _ = q1;
    let _ = q2;
}
