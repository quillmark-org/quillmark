//! Tests for the `~~~card-yaml` composable-card syntax.
//!
//! Coverage:
//! - Composable cards declared with `#@kind:` parse to `Card`s.
//! - Every card round-trips to the canonical `~~~card-yaml` form.
//! - Card-kind validation and error reporting.
//! - The blank-line rule for `~~~card-yaml` openers.

use crate::document::Document;

// ── Card-yaml blocks parse ────────────────────────────────────────────────────

#[test]
fn card_fence_parses_kind_fields_and_body() {
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\n~~~card-yaml\n#@kind: product\nname: Widget\nprice: 19\n~~~\n\nWidget description.\n";
    let doc = Document::from_markdown(src).unwrap();

    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.kind(), Some("product"));
    assert_eq!(card.tag(), "product");
    assert_eq!(
        card.payload().get("name").unwrap().as_str(),
        Some("Widget")
    );
    assert_eq!(card.body(), "\nWidget description.\n");
}

#[test]
fn card_fence_empty_body_has_no_fields() {
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\n~~~card-yaml\n#@kind: marker\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert!(doc.cards()[0].payload().is_empty());
    assert_eq!(doc.cards()[0].body(), "");
}

#[test]
fn card_fence_multiple_cards() {
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\n~~~card-yaml\n#@kind: product\nname: A\n~~~\n\n~~~card-yaml\n#@kind: product\nname: B\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 2);
    assert_eq!(
        doc.cards()[0].payload().get("name").unwrap().as_str(),
        Some("A")
    );
    assert_eq!(
        doc.cards()[1].payload().get("name").unwrap().as_str(),
        Some("B")
    );
}

// ── Canonical emission ────────────────────────────────────────────────────────

#[test]
fn emit_uses_canonical_card_fence() {
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\n~~~card-yaml\n#@kind: product\nname: Widget\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();
    assert_eq!(
        emitted,
        "~~~card-yaml\n#@quill: q\n~~~\n\n~~~card-yaml\n#@kind: product\nname: \"Widget\"\n~~~\n"
    );
}

#[test]
fn emit_is_idempotent_for_card_fences() {
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\n~~~card-yaml\n#@kind: product\nname: Widget\n~~~\n\nTrailing body.\n";
    let doc = Document::from_markdown(src).unwrap();
    let once = doc.to_markdown();
    let twice = Document::from_markdown(&once).unwrap().to_markdown();
    assert_eq!(once, twice);
}

#[test]
fn card_fence_body_round_trips() {
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\nMain body.\n\n~~~card-yaml\n#@kind: product\nname: Widget\n~~~\n\nCard body.\n";
    let a = Document::from_markdown(src).unwrap();
    let b = Document::from_markdown(&a.to_markdown()).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.main().body(), "\nMain body.\n");
    assert_eq!(a.cards()[0].body(), "\nCard body.\n");
}

#[test]
fn card_fence_preserves_yaml_comments() {
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\n~~~card-yaml\n#@kind: product\n# a banner\nname: Widget\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("~~~card-yaml\n#@kind: product\n# a banner\nname: \"Widget\"\n~~~\n"),
        "emit:\n{emitted}"
    );
    let reparsed = Document::from_markdown(&emitted).unwrap();
    assert_eq!(doc, reparsed);
}

// ── Validation and errors ─────────────────────────────────────────────────────

#[test]
fn card_fence_without_kind_is_allowed() {
    // A composable block with no `#@kind` — `#@kind` is optional metadata.
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\n~~~card-yaml\nname: Widget\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), None);
    assert_eq!(doc.cards()[0].tag(), "");
}

#[test]
fn card_fence_unknown_meta_key_is_allowed() {
    // A composable block carrying an arbitrary `#@` key — `#@` entries are
    // generic system metadata, no validation beyond `#@quill` on the root.
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\n~~~card-yaml\n#@foo: bar\nname: Widget\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].meta().get("foo"), Some("bar"));
}

#[test]
fn card_fence_unusual_kind_is_accepted_verbatim() {
    // `#@kind` carries no parse-time name-pattern validation.
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\n~~~card-yaml\n#@kind: BadKind\nname: Widget\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].tag(), "BadKind");
}

#[test]
fn card_fence_unclosed_is_error() {
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\n~~~card-yaml\n#@kind: product\nname: Widget\n";
    let err = Document::from_markdown(src).unwrap_err().to_string();
    assert!(
        err.contains("never closed with `~~~`"),
        "got: {err}"
    );
}

#[test]
fn card_fence_reserved_key_is_error() {
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\n~~~card-yaml\n#@kind: product\nBODY: nope\n~~~\n";
    let err = Document::from_markdown(src).unwrap_err().to_string();
    assert!(err.contains("Reserved field name"), "got: {err}");
}

#[test]
fn non_root_block_declaring_quill_is_ignored_metadata() {
    // A composable block may carry `#@quill` — it is just ignored system
    // metadata on the card, not an error.
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\n~~~card-yaml\n#@quill: other\nname: Widget\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].meta().quill(), Some("other"));
}

// ── Non-card fenced code blocks are untouched ─────────────────────────────────

#[test]
fn ordinary_code_fence_is_not_a_card() {
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\n```rust\nlet x = 1;\n```\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 0);
    assert!(doc.main().body().contains("```rust"));
}

#[test]
fn card_yaml_info_inside_outer_code_fence_is_not_a_card() {
    // A `~~~card-yaml` line shielded by an outer code fence is plain text.
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\n````text\n~~~card-yaml\n#@kind: product\nname: Widget\n~~~\n````\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 0);
}

#[test]
fn card_fence_without_blank_line_above_is_not_a_card() {
    // The blank-line rule fails — the `~~~card-yaml` fence is delegated to
    // CommonMark as a code block, with a non-fatal lint warning.
    let src = "~~~card-yaml\n#@quill: q\n~~~\n\nSome prose.\n~~~card-yaml\n#@kind: product\nname: Widget\n~~~\n";
    let out = Document::from_markdown_with_warnings(src).unwrap();
    assert_eq!(out.document.cards().len(), 0);
    assert!(out
        .warnings
        .iter()
        .any(|w| w.code.as_deref() == Some("parse::card_fence_missing_blank")));
}
