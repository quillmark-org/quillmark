//! Tests for the `~~~card-yaml` composable-card syntax.
//!
//! Coverage:
//! - Composable cards declared with `$kind:` parse to `Card`s; `$kind` is optional.
//! - Every card round-trips to the canonical `~~~card-yaml` form.
//! - Ordinary fenced code blocks and the blank-line rule for `~~~card-yaml` openers.
//!
//! Parse-error and metadata-validation cases live in `assemble_tests.rs`.

use crate::document::Document;

// ── Card-yaml blocks parse ────────────────────────────────────────────────────

#[test]
fn card_fence_parses_kind_fields_and_body() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: product\nname: Widget\nprice: 19\n~~~\n\nWidget description.\n";
    let doc = Document::from_markdown(src).unwrap();

    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.kind(), Some("product"));
    assert_eq!(card.payload().get("name").unwrap().as_str(), Some("Widget"));
    assert_eq!(card.body(), "\nWidget description.\n");
}

#[test]
fn card_fence_empty_body_has_no_fields() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: marker\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert!(doc.cards()[0].payload().is_empty());
    assert_eq!(doc.cards()[0].body(), "");
}

#[test]
fn card_fence_multiple_cards() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: product\nname: A\n~~~\n\n~~~card-yaml\n$kind: product\nname: B\n~~~\n";
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
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: product\nname: Widget\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();
    assert_eq!(
        emitted,
        "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: product\nname: Widget\n~~~\n"
    );
}

#[test]
fn emit_is_idempotent_for_card_fences() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: product\nname: Widget\n~~~\n\nTrailing body.\n";
    let doc = Document::from_markdown(src).unwrap();
    let once = doc.to_markdown();
    let twice = Document::from_markdown(&once).unwrap().to_markdown();
    assert_eq!(once, twice);
}

#[test]
fn card_fence_body_round_trips() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\nMain body.\n\n~~~card-yaml\n$kind: product\nname: Widget\n~~~\n\nCard body.\n";
    let a = Document::from_markdown(src).unwrap();
    let b = Document::from_markdown(&a.to_markdown()).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.main().body(), "\nMain body.\n");
    assert_eq!(a.cards()[0].body(), "\nCard body.\n");
}

#[test]
fn card_fence_preserves_yaml_comments() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: product\n# a banner\nname: Widget\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("~~~card-yaml\n$kind: product\n# a banner\nname: Widget\n~~~\n"),
        "emit:\n{emitted}"
    );
    let reparsed = Document::from_markdown(&emitted).unwrap();
    assert_eq!(doc, reparsed);
}

// ── Validation and errors ─────────────────────────────────────────────────────

#[test]
fn card_fence_without_kind_is_allowed() {
    // A composable block with no `$kind` — `$kind` is optional metadata.
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\nname: Widget\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), None);
}

// ── Non-card fenced code blocks are untouched ─────────────────────────────────

#[test]
fn ordinary_code_fence_is_not_a_card() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n```rust\nlet x = 1;\n```\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 0);
    assert!(doc.main().body().contains("```rust"));
}

#[test]
fn card_yaml_info_inside_outer_code_fence_is_not_a_card() {
    // A `~~~card-yaml` line shielded by an outer code fence is plain text.
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n````text\n~~~card-yaml\n$kind: product\nname: Widget\n~~~\n````\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 0);
}

#[test]
fn card_fence_without_blank_line_above_is_not_a_card() {
    // The blank-line rule fails — the `~~~card-yaml` fence is delegated to
    // CommonMark as a code block, with a non-fatal lint warning.
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\nSome prose.\n~~~card-yaml\n$kind: product\nname: Widget\n~~~\n";
    let out = Document::from_markdown_with_warnings(src).unwrap();
    assert_eq!(out.document.cards().len(), 0);
    assert!(out
        .warnings
        .iter()
        .any(|w| w.code.as_deref() == Some("parse::card_fence_missing_blank")));
}
