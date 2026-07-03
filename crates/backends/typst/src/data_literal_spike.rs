//! Spike (#795 follow-up, cascade 2): the generated helper carries the
//! document data as a **Typst literal** instead of a `json(bytes(..))` blob
//! plus runtime assembly.
//!
//! Premise: the backend regenerates `lib.typ` per render and already knows,
//! in Rust, everything the template's `#let data = { .. }` block re-derives
//! at runtime — which fields are content, which are dates, each card's kind
//! and ordinal. Emitting the assembled dict as a literal
//! (`#let data = ("subject": _qm_c0, ..)`) deletes the template's
//! data-processing program (`insert-content`, `parse-dates`, the card loop,
//! the `__meta__` strip) and, with it, the Rust↔Typst mirror coupling between
//! `content_entries` and `insert-content`.
//!
//! What must hold, each pinned below:
//!   1. Rust can serialize any `serde_json::Value` to a Typst literal that
//!      Typst itself judges `==` to the `json()` parse of the same document —
//!      including the int/float split, unicode, quotes/newlines, `$`-prefixed
//!      and empty keys, empty/single-element collections, and deep nesting.
//!   2. Content refs (`_qm_c0` block bindings) slot into the generated dict,
//!      and the resulting `data` composes with the **production span scan**
//!      unchanged — regions and `field_at` keep working when the dict is a
//!      literal rather than a runtime product.
//!
//! Known boundary (encoded in `lit`, asserted in tests): Typst has no
//! non-finite floats and `json()` itself rejects NaN/Infinity, so the
//! serializer needs no story for them; large u64s beyond `i64::MAX` fall back
//! to float, matching `json()`.
//!
//! Nothing here is wired into production; `#[cfg(test)]` only.
#![cfg(test)]

use std::collections::HashMap;

use quillmark_core::{FileTreeNode, Quill};

use crate::compile::compile_document;
use crate::convert::escape_string;
use crate::helper;
use crate::world::QuillWorld;

/// Serialize a JSON value as a Typst literal expression, mirroring the value
/// model `json()` produces: null → `none`, integral JSON numbers → int,
/// others → float, arrays → `()`/`(x,)`/`(a, b,)`, objects → `(:)` / quoted
/// string keys. The production home for this would live next to
/// `helper::generate_lib_typ`.
fn lit(v: &serde_json::Value) -> String {
    use serde_json::Value::*;
    match v {
        Null => "none".to_string(),
        Bool(b) => b.to_string(),
        Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else {
                // Non-i64 numbers (floats and u64 overflow) surface from
                // json() as floats. Typst float *literals* take no exponent
                // syntax, but `float(str)` parses Rust's shortest-repr
                // Display exactly — including 1e20 / 1.5e-10 forms — so
                // every finite f64 round-trips through the constructor.
                let f = n.as_f64().expect("json numbers are finite");
                format!("float(\"{f}\")")
            }
        }
        String(s) => format!("\"{}\"", escape_string(s)),
        Array(a) => {
            if a.is_empty() {
                "()".to_string()
            } else {
                // Trailing comma keeps the single-element case an array
                // rather than a parenthesized scalar.
                let items: Vec<std::string::String> = a.iter().map(lit).collect();
                format!("({},)", items.join(", "))
            }
        }
        Object(o) => {
            if o.is_empty() {
                "(:)".to_string()
            } else {
                let items: Vec<std::string::String> = o
                    .iter()
                    .map(|(k, v)| format!("\"{}\": {}", escape_string(k), lit(v)))
                    .collect();
                format!("({},)", items.join(", "))
            }
        }
    }
}

fn quill(plate: &str) -> Quill {
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: br#"
quill:
  name: data_literal_spike
  version: 0.1.0
  backend: typst
  description: generated data-literal spike
typst:
  plate_file: plate.typ
main:
  fields: {}
"#
            .to_vec(),
        },
    );
    files.insert(
        "plate.typ".to_string(),
        FileTreeNode::File {
            contents: plate.as_bytes().to_vec(),
        },
    );
    Quill::from_tree(FileTreeNode::Directory { files }).expect("load quill")
}

fn compile_with_helper(
    helper_src: &str,
    plate: &str,
) -> Result<(typst_layout::PagedDocument, QuillWorld), quillmark_core::RenderError> {
    let q = quill(plate);
    let mut world = QuillWorld::new(&q, plate).expect("world");
    world.set_source(QuillWorld::helper_fid("lib.typ"), helper_src);
    world.set_binary(
        QuillWorld::helper_fid("typst.toml"),
        helper::generate_typst_toml().into_bytes(),
    );
    compile_document(&world).map(|(doc, _)| (doc, world))
}

/// Premise 1, judged by Typst: for each document in the corpus, generate a
/// helper defining both the `json()` parse and the literal, and let the plate
/// `assert(a == b)`. A compile success is a Typst-level equality proof —
/// covering the int/float split, dict key handling, and collection shapes,
/// with none of our own comparison logic in the loop.
#[test]
fn typst_judges_literal_equal_to_json_parse() {
    let corpus: Vec<serde_json::Value> = vec![
        serde_json::json!({}),
        serde_json::json!({ "title": "Hello", "count": 42, "ratio": 1.5 }),
        serde_json::json!({
            "$kind": "indorsement",
            "$body": "",
            "nested": { "deep": { "deeper": [1, 2, 3] } },
            "empty_dict": {},
            "empty_arr": [],
            "single": ["one"],
            "nulls": [null, null],
            "flags": [true, false],
        }),
        serde_json::json!({
            "unicode": "naïve café — “quotes” 你好 🎉",
            "quoted": "she said \"hi\"\nsecond line\ttabbed",
            "backslash": "C:\\path\\to",
            "": "empty key",
            "key with space": 0,
            "negatives": [-1, -0.25],
            "int_edge": 9007199254740991i64,
            "float_integral": 3.0,
        }),
        serde_json::json!({
            "huge": 1e20,
            "tiny": 1.5e-10,
            "extreme": -1e300,
            "fraction": 0.1,
            "i64_min": i64::MIN,
            "i64_max": i64::MAX,
        }),
        // The shape a real render produces: cards with kinds, ordinals
        // implied by order, mixed scalar types.
        serde_json::json!({
            "subject": "Request for Quarters",
            "date": "2026-07-03",
            "refs": ["a", "b"],
            "$cards": [
                { "$kind": "alpha", "note": "one", "n": 1 },
                { "$kind": "beta",  "note": "two", "n": 2.5 },
            ],
        }),
    ];

    for (i, doc) in corpus.iter().enumerate() {
        let json_str = serde_json::to_string(doc).unwrap();
        let helper_src = format!(
            "#let a = json(bytes(\"{}\"))\n#let b = {}\n",
            escape_string(&json_str),
            lit(doc)
        );
        let plate = r#"
#import "@local/quillmark-helper:0.1.0": a, b
#assert(a == b, message: "literal != json parse: " + repr(a) + " vs " + repr(b))
equal
"#;
        compile_with_helper(&helper_src, plate).unwrap_or_else(|e| {
            panic!("corpus[{i}] literal must equal its json() parse: {e:?}\nlit: {}", lit(doc))
        });
    }
}

/// Premise 2: a fully generated `data` dict — content refs to block bindings
/// (cascade 1's shape), a date as a `datetime()` constructor, cards carrying
/// `$path` — composes with the **production span scan and `field_at`**
/// untouched: the scan classifies by window, and neither knows nor cares that
/// the dict is a literal instead of a runtime product.
#[test]
fn generated_dict_with_content_refs_feeds_the_production_scan() {
    let helper_src = r#"#let _qm_c0 = [A generated *note* placed by the plate.]
#let _qm_c1 = [The card body, generated per instance.]
#let data = (
  "title": "Hello",
  "issued": datetime(year: 2026, month: 7, day: 3),
  "note": _qm_c0,
  "$cards": (
    ("$kind": "alpha", "$path": "$cards.alpha.0.", "$body": _qm_c1,),
  ),
)
"#;
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 500pt, height: 500pt, margin: 40pt)

#data.title

#data.note

#data.issued.display("[year]-[month]-[day]")

#for card in data.at("$cards") {
  block(card.at("$body"))
}
"#;
    let (doc, world) = compile_with_helper(helper_src, plate).expect("compile");

    // Windows exactly as production would record them: each block binding's
    // bracketed range in the generated helper, plus the plate's scalar
    // reference sites from the AST pass.
    let helper_id = QuillWorld::helper_fid("lib.typ");
    let block = |needle: &str| {
        let start = helper_src.find(needle).expect(needle);
        crate::overlay::FieldWindow {
            path: if needle.contains("note") {
                "note".to_string()
            } else {
                "$cards.alpha.0.$body".to_string()
            },
            file: helper_id,
            range: start..start + needle.len(),
        }
    };
    let mut windows = vec![
        block("[A generated *note* placed by the plate.]"),
        block("[The card body, generated per instance.]"),
    ];
    {
        use typst::World as _;
        let main_id = world.main();
        let src = world.source(main_id).expect("plate source");
        windows.extend(
            crate::overlay::scalar_windows(&src, &["title".to_string()])
                .into_iter()
                .map(|(path, range)| crate::overlay::FieldWindow {
                    path,
                    file: main_id,
                    range,
                }),
        );
    }

    let regions = crate::overlay::scan_content_regions(&doc, &world, &windows);
    for expected in ["title", "note", "$cards.alpha.0.$body"] {
        assert!(
            regions.iter().any(|r| r.field == expected),
            "expected a region keyed {expected:?} from the literal dict: {regions:?}"
        );
    }

    // field_at over the same windows: a point inside the note's region
    // resolves to `note`.
    let note = regions.iter().find(|r| r.field == "note").unwrap();
    let cx = (note.rect[0] + note.rect[2]) / 2.0;
    let cy = (note.rect[1] + note.rect[3]) / 2.0;
    assert_eq!(
        crate::overlay::field_at(&doc, &world, &windows, note.page, cx, cy).as_deref(),
        Some("note"),
        "clicks resolve through the literal-dict content"
    );
}
