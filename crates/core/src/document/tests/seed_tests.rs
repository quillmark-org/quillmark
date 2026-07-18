//! End-to-end tests for the `$seed` system-metadata key.
//!
//! `$seed` is the per-card-kind seed-overlay map: parsers accept it, the
//! emitter preserves it, the storage DTO round-trips it, and the plate JSON
//! consumed by backends strips it. Unlike `$ext` the seeding layer interprets
//! it — see `crate::Quill::seed_card` and the `quill::seed` tests for layering.

use serde_json::json;

use crate::document::{Document, MetaKey, PayloadItem};

fn parse(src: &str) -> Document {
    Document::from_markdown(src).expect("source should parse")
}

// ── Parser ─────────────────────────────────────────────────────────────────

#[test]
fn seed_with_mapping_value_is_accepted() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed:
  indorsement:
    from: 49 FW/CC
    signature_block:
      - \"JANE A. DOE, Col, USAF\"
      - Commander
title: Hi
~~~
",
    );
    let seed = doc.main().payload().seed().expect("$seed present");
    let ind = seed.get("indorsement").and_then(|v| v.as_object()).unwrap();
    assert_eq!(ind.get("from").and_then(|v| v.as_str()), Some("49 FW/CC"));
}

#[test]
fn seed_with_empty_mapping_is_preserved() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed: {}
~~~
",
    );
    let seed = doc.main().payload().seed().expect("$seed present");
    assert!(seed.is_empty());
}

#[test]
fn seed_with_scalar_value_is_rejected() {
    let err = Document::from_markdown(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed: just-a-string
~~~
",
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("Invalid `$seed`") && err.contains("mapping"),
        "expected $seed-must-be-mapping rejection, got: {err}",
    );
}

#[test]
fn seed_with_sequence_value_is_rejected() {
    let err = Document::from_markdown(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed:
  - foo
  - bar
~~~
",
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("Invalid `$seed`") && err.contains("mapping"),
        "expected $seed-must-be-mapping rejection, got: {err}",
    );
}

#[test]
fn unknown_dollar_key_message_lists_seed() {
    // The closed-set rejection lists `$seed` among the accepted keys.
    let err = Document::from_markdown(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$bogus: x
~~~
",
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("$seed"),
        "closed-set message should list $seed: {err}"
    );
}

#[test]
fn seed_on_composable_card_is_rejected() {
    // `$seed` is root-only (like `$quill`): a composable block carrying it is a
    // parse error, not silently-inert data.
    let err = Document::from_markdown(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
~~~

~~~card-yaml
$kind: indorsement
$seed:
  note:
    from: X
~~~
",
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("must not carry `$seed`"),
        "expected composable-$seed rejection, got: {err}",
    );
}

// ── Emit / round-trip ──────────────────────────────────────────────────────

#[test]
fn seed_round_trips_through_markdown() {
    let src = "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed:
  indorsement:
    from: 49 FW/CC
title: Body
~~~

Body content.
";
    let doc = parse(src);
    let emitted = doc.to_markdown();
    let reparsed = parse(&emitted);
    assert_eq!(doc, reparsed);
    assert!(
        emitted.contains("$seed:\n  indorsement:\n    from: 49 FW/CC\n"),
        "unexpected emit:\n{emitted}",
    );
}

#[test]
fn empty_seed_emits_as_inline_braces() {
    let src = "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed: {}
~~~
";
    let doc = parse(src);
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("$seed: {}\n"),
        "expected `$seed: {{}}` literal in emit, got:\n{emitted}",
    );
    let reparsed = parse(&emitted);
    assert_eq!(doc, reparsed);
}

#[test]
fn comments_inside_seed_round_trip() {
    // Exercises the `$seed` branches of the nested-comment machinery
    // (parse → per-item nested_comments → emit, and the storage-DTO flatten).
    let src = "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed:
  indorsement:
    # pin the squadron office symbol
    from: 49 FW/CC
~~~
";
    let doc = parse(src);
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("# pin the squadron office symbol"),
        "nested $seed comment must survive emit:\n{emitted}",
    );
    assert_eq!(doc, parse(&emitted));

    // And it survives the storage DTO round-trip too.
    let json = serde_json::to_string(&doc).unwrap();
    let restored: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(doc, restored);
}

#[test]
fn seed_emit_is_idempotent() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed:
  note:
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
fn set_seed_inserts_after_ext_and_before_user_fields() {
    let mut doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$id: rev-1
$ext:
  a: 1
title: Hi
~~~
",
    );
    let mut seed = serde_json::Map::new();
    seed.insert("indorsement".into(), json!({ "from": "X" }));
    doc.main_mut().payload_mut().set_seed(seed);

    let items = doc.main().payload().items();
    // Canonical order: $quill, $kind, $id, $ext, $seed, then user fields.
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
    assert!(matches!(
        items[4],
        PayloadItem::Meta {
            key: MetaKey::Seed,
            ..
        }
    ));
    assert!(matches!(items[5], PayloadItem::Field { .. }));
}

#[test]
fn seed_overlay_parses_with_body() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed:
  indorsement:
    from: 49 FW/CC
    $body: \"Standard endorsement text.\"
~~~
",
    );
    // The overlay is read off the main card's `$seed` map and parsed via
    // `SeedOverlay::from_json` (there is no `Document::seed` convenience).
    let seed = doc.main().seed();
    let overlay = seed
        .and_then(|m| m.get("indorsement"))
        .and_then(crate::SeedOverlay::from_json)
        .expect("overlay present");
    assert_eq!(
        overlay.fields.get("from").and_then(|v| v.as_str()),
        Some("49 FW/CC"),
    );
    assert_eq!(overlay.body.as_deref(), Some("Standard endorsement text."));
    // `$body` is the body override, not a field.
    assert!(!overlay.fields.contains_key("$body"));
    // An undeclared kind yields no overlay.
    assert!(seed.and_then(|m| m.get("missing")).is_none());
}

#[test]
fn seed_namespace_mutators_preserve_siblings() {
    let mut doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
~~~
",
    );
    let card = doc.main_mut();
    card.store_seed_namespace("indorsement", json!({ "from": "A" }))
        .unwrap();
    card.store_seed_namespace("attachment", json!({ "label": "B" }))
        .unwrap();
    assert_eq!(card.seed().map(|m| m.len()), Some(2));

    // Removing one kind leaves the sibling intact.
    let removed = card.remove_seed_namespace("indorsement").unwrap();
    assert_eq!(removed.get("from").and_then(|v| v.as_str()), Some("A"));
    assert_eq!(card.seed().map(|m| m.len()), Some(1));
    assert!(card.seed().unwrap().contains_key("attachment"));

    // Removing the last kind drops `$seed` entirely (not `$seed: {}`).
    card.remove_seed_namespace("attachment");
    assert!(card.seed().is_none());
}

#[test]
fn set_seed_namespace_rejects_invalid_and_reserved_kinds() {
    // `$seed` is keyed by composable card-kind, so the writer must reject
    // names that could never name a composable card (unlike free-form `$ext`).
    let mut doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
~~~
",
    );
    let card = doc.main_mut();

    assert!(matches!(
        card.store_seed_namespace("main", json!({ "from": "A" })),
        Err(crate::document::EditError::ReservedKind)
    ));
    assert!(matches!(
        card.store_seed_namespace("Bad-Kind", json!({ "from": "A" })),
        Err(crate::document::EditError::InvalidKindName(_))
    ));

    // A rejected write leaves the card untouched — no `$seed` map appears.
    assert!(card.seed().is_none());
}

#[test]
fn seed_overlay_drops_reserved_keys_other_than_body() {
    // An overlay only ever carries user fields plus the reserved `$body`;
    // any other `$`-key must be dropped, never smuggled in as a user field.
    let overlay = crate::SeedOverlay::from_json(&json!({
        "from": "49 FW/CC",
        "$body": "Body override.",
        "$kind": "smuggled",
        "$quill": "x@1.0",
    }))
    .expect("overlay is an object");

    assert_eq!(overlay.body.as_deref(), Some("Body override."));
    assert!(overlay.fields.contains_key("from"));
    assert!(!overlay.fields.contains_key("$kind"));
    assert!(!overlay.fields.contains_key("$quill"));
    assert_eq!(
        overlay.fields.len(),
        1,
        "only the user field should survive"
    );
}

// ── Plate JSON ─────────────────────────────────────────────────────────────

#[test]
fn seed_is_stripped_from_plate_json() {
    // Backends must never see `$seed` — it is curation data, not template data.
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed:
  indorsement:
    from: \"Should not reach the backend\"
title: Hi
~~~
",
    );
    let plate = doc.to_plate_json();
    let obj = plate.as_object().expect("plate is an object");
    assert!(
        !obj.contains_key("$seed"),
        "plate must not contain `$seed`: {plate}"
    );
    assert!(
        !obj.contains_key("seed"),
        "plate must not contain `seed`: {plate}"
    );
    assert_eq!(obj.get("title").and_then(|v| v.as_str()), Some("Hi"));
    assert!(obj.contains_key("$quill"));
    assert!(obj.contains_key("$cards"));
}

// ── Storage DTO ────────────────────────────────────────────────────────────

#[test]
fn seed_round_trips_through_serde_json() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$seed:
  indorsement:
    from: 49 FW/CC
    $body: \"Body override.\"
title: Hi
~~~

Body.
",
    );
    let json = serde_json::to_string(&doc).unwrap();
    let restored: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(doc, restored);
    assert_eq!(doc.to_markdown(), restored.to_markdown());

    // The DTO carries `"type": "seed"` under the current 0.93.0 schema tag.
    assert!(
        json.contains("\"type\":\"seed\""),
        "expected seed variant in DTO: {json}"
    );
    assert!(
        json.contains("quillmark/document@0.93.0"),
        "expected 0.93.0 schema tag: {json}",
    );
}
