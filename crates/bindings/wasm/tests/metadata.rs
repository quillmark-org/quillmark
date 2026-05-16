use quillmark_wasm::Quillmark;
use wasm_bindgen_test::*;

mod common;

#[wasm_bindgen_test]
fn test_quill_from_tree_with_ui_metadata() {
    let tree = common::tree(&[
        (
            "Quill.yaml",
            b"quill:\n  name: ui_test_quill\n  version: \"0.1\"\n  backend: typst\n  main_file: main.typ\n  description: Test quill for UI metadata\n\nmain:\n  fields:\n    my_field:\n      type: string\n      ui:\n        group: Personal Info\n",
        ),
        ("main.typ", b"= Title"),
    ]);
    let engine = Quillmark::new();
    let quill = engine.quill(tree).expect("quill failed");
    let _ = quill;
}
