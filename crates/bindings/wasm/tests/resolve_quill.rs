use quillmark_wasm::Quill;
use wasm_bindgen_test::*;

mod common;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn test_quill_from_tree_versioned() {
    let q1 = Quill::from_tree(common::tree(&[
        (
            "Quill.yaml",
            b"quill:\n  name: usaf_memo\n  version: \"0.1.0\"\n  backend: typst\n  description: Version 0.1.0\n",
        ),
        ("plate.typ", b"hello 1"),
    ])).unwrap();

    let q2 = Quill::from_tree(common::tree(&[
        (
            "Quill.yaml",
            b"quill:\n  name: usaf_memo\n  version: \"0.2.0\"\n  backend: typst\n  description: Version 0.2.0\n",
        ),
        ("plate.typ", b"hello 2"),
    ])).unwrap();

    let _ = q1;
    let _ = q2;
}
