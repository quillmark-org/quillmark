//! End-to-end tests for the `$ext` system-metadata key.
//!
//! `$ext` is the opaque-mapping extension hook: parsers accept it, the
//! emitter preserves it, the storage DTO round-trips it, and the plate
//! JSON consumed by backends strips it.

use serde_json::json;

use crate::document::{Document, MetaKey, PayloadItem};

fn parse(src: &str) -> Document {
    Document::parse(src).expect("source should parse").document
}

// ── Parser ─────────────────────────────────────────────────────────────────

#[test]
fn ext_with_mapping_value_is_accepted() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$ext:
  presentation:
    title: \"My Display Name\"
  collapsed: false
title: Hi
~~~
",
    );
    let ext = doc.main().ext().expect("$ext present");
    assert_eq!(
        ext.get("presentation")
            .and_then(|v| v.get("title"))
            .and_then(|v| v.as_str()),
        Some("My Display Name"),
    );
    assert_eq!(ext.get("collapsed").and_then(|v| v.as_bool()), Some(false),);
}

#[test]
fn ext_with_empty_mapping_is_preserved() {
    // `$ext: {}` survives the round-trip as an explicit empty map — it is
    // distinct from "no `$ext` declared at all".
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$ext: {}
~~~
",
    );
    let ext = doc.main().ext().expect("$ext present");
    assert!(ext.is_empty());
}

#[test]
fn ext_with_scalar_value_is_rejected() {
    let err = Document::parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$ext: just-a-string
~~~
",
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("Invalid `$ext`") && err.contains("mapping"),
        "expected $ext-must-be-mapping rejection, got: {err}",
    );
}

#[test]
fn ext_with_sequence_value_is_rejected() {
    let err = Document::parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$ext:
  - foo
  - bar
~~~
",
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("Invalid `$ext`") && err.contains("mapping"),
        "expected $ext-must-be-mapping rejection, got: {err}",
    );
}

#[test]
fn fill_on_ext_is_rejected() {
    let err = Document::parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$ext: !must_fill
  foo: 1
~~~
",
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("`!must_fill`") && err.contains("$ext"),
        "expected !must_fill-on-$ext rejection, got: {err}",
    );
}

#[test]
fn ext_on_composable_card_is_accepted() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
~~~

~~~card-yaml
$kind: indorsement
$ext:
  rename: \"Cmdr's response\"
from: ORG1/SYMBOL
~~~
",
    );
    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.kind(), Some("indorsement"));
    let ext = card.ext().expect("composable card $ext present");
    assert_eq!(
        ext.get("rename").and_then(|v| v.as_str()),
        Some("Cmdr's response"),
    );
}

// ── Emit / round-trip ──────────────────────────────────────────────────────

#[test]
fn ext_round_trips_through_markdown() {
    let src = "\
~~~card-yaml
$quill: q@1.0
$kind: main
$ext:
  presentation:
    title: A
  flag: true
title: Body
~~~

Body content.
";
    let doc = parse(src);
    let emitted = doc.to_markdown();
    let reparsed = parse(&emitted);
    assert_eq!(doc, reparsed);
    // The emitted form is canonical, so the inner $ext map shows up as
    // indented block-style entries under `$ext:`.
    assert!(
        emitted.contains("$ext:\n  presentation:\n    title: A\n  flag: true\n"),
        "unexpected emit:\n{emitted}",
    );
}

#[test]
fn empty_ext_emits_as_inline_braces() {
    let src = "\
~~~card-yaml
$quill: q@1.0
$kind: main
$ext: {}
~~~
";
    let doc = parse(src);
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("$ext: {}\n"),
        "expected `$ext: {{}}` literal in emit, got:\n{emitted}",
    );
    // Survives a second round-trip too.
    let reparsed = parse(&emitted);
    assert_eq!(doc, reparsed);
}

#[test]
fn ext_emit_is_idempotent() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$ext:
  a: 1
~~~
",
    );
    let once = doc.to_markdown();
    let twice = parse(&once).to_markdown();
    assert_eq!(once, twice);
}

// ── Programmatic construction ──────────────────────────────────────────────

#[test]
fn set_ext_inserts_after_id_and_before_user_fields() {
    let mut doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$id: rev-1
title: Hi
~~~
",
    );
    let mut ext = serde_json::Map::new();
    ext.insert("rename".into(), json!("Greeting"));
    doc.main_mut().payload_mut().set_ext(ext);

    let items = doc.main().payload().items();
    // Canonical order: $quill, $kind, $id, $ext, then user fields in
    // source order.
    assert!(matches!(items[0], PayloadItem::Quill { .. }));
    assert!(matches!(items[1], PayloadItem::Kind { .. }));
    assert!(matches!(items[2], PayloadItem::Id { .. }));
    assert!(matches!(
        items[3],
        PayloadItem::Meta {
            key: MetaKey::Ext,
            ..
        }
    ));
    assert!(matches!(items[4], PayloadItem::Field { .. }));
}

#[test]
fn take_ext_removes_the_entry() {
    let mut doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$ext:
  a: 1
~~~
",
    );
    let taken = doc.main_mut().payload_mut().take_ext().unwrap();
    assert_eq!(taken.get("a").and_then(|v| v.as_i64()), Some(1));
    assert!(doc.main().ext().is_none());
}

// ── Plate JSON ─────────────────────────────────────────────────────────────

#[test]
fn ext_is_stripped_from_plate_json() {
    // Backends must never see `$ext` — it carries out-of-band UI / agent
    // state, not template data.
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$ext:
  presentation:
    title: \"Should not reach the backend\"
title: Hi
~~~
",
    );
    let plate = doc.to_plate_json();
    let obj = plate.as_object().expect("plate is an object");
    assert!(
        !obj.contains_key("$ext"),
        "plate must not contain `$ext`: {plate}",
    );
    assert!(
        !obj.contains_key("ext"),
        "plate must not contain `ext`: {plate}",
    );
    // Plate still carries user fields and the canonical $quill/$body/$cards keys.
    assert_eq!(obj.get("title").and_then(|v| v.as_str()), Some("Hi"));
    assert!(obj.contains_key("$quill"));
    assert!(obj.contains_key("$body"));
    assert!(obj.contains_key("$cards"));
}

// ── Storage DTO ────────────────────────────────────────────────────────────

#[test]
fn ext_round_trips_through_serde_json() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$ext:
  presentation:
    title: \"Greeting Card\"
  collapsed: false
title: Hi
~~~

Body.

~~~card-yaml
$kind: indorsement
$ext:
  rename: \"Cmdr's response\"
from: X
~~~
",
    );
    let json = serde_json::to_string(&doc).unwrap();
    let restored: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(doc, restored);
    assert_eq!(doc.to_markdown(), restored.to_markdown());

    // The new DTO variant carries `"type": "ext"` with the map inline.
    assert!(
        json.contains("\"type\":\"ext\""),
        "expected ext variant in DTO, got: {json}",
    );
}
