//! Tests for the ```` ```card <kind> ```` fenced composable-card syntax.
//!
//! Coverage:
//! - The new fenced syntax parses to the same `Document` as the legacy
//!   `---\nCARD:` syntax.
//! - Every card round-trips to the canonical ```` ```card ```` form, whether
//!   authored with the new or the legacy syntax.
//! - Card-kind / info-string validation and error reporting.

use crate::document::Document;
use crate::Sentinel;

// ── New syntax parses ─────────────────────────────────────────────────────────

#[test]
fn card_fence_parses_kind_fields_and_body() {
    let src = "---\nQUILL: q\n---\n\n```card product\nname: Widget\nprice: 19\n```\n\nWidget description.\n";
    let doc = Document::from_markdown(src).unwrap();

    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.sentinel(), &Sentinel::Card("product".to_string()));
    assert_eq!(card.tag(), "product");
    assert_eq!(
        card.frontmatter().get("name").unwrap().as_str(),
        Some("Widget")
    );
    assert_eq!(card.body(), "\nWidget description.\n");
}

#[test]
fn card_fence_empty_body_has_no_fields() {
    let src = "---\nQUILL: q\n---\n\n```card marker\n```\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert!(doc.cards()[0].frontmatter().is_empty());
    assert_eq!(doc.cards()[0].body(), "");
}

#[test]
fn card_fence_tilde_fence_accepted() {
    let src = "---\nQUILL: q\n---\n\n~~~card product\nname: Widget\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].tag(), "product");
}

#[test]
fn card_fence_multiple_cards() {
    let src = "---\nQUILL: q\n---\n\n```card product\nname: A\n```\n\n```card product\nname: B\n```\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 2);
    assert_eq!(doc.cards()[0].frontmatter().get("name").unwrap().as_str(), Some("A"));
    assert_eq!(doc.cards()[1].frontmatter().get("name").unwrap().as_str(), Some("B"));
}

// ── Equivalence with the legacy syntax ────────────────────────────────────────

#[test]
fn new_and_legacy_syntax_parse_to_equal_documents() {
    let new = "---\nQUILL: q\n---\n\n```card product\nname: Widget\nprice: 19\n```\n\nBody.\n";
    let legacy =
        "---\nQUILL: q\n---\n\n---\nCARD: product\nname: Widget\nprice: 19\n---\n\nBody.\n";

    let from_new = Document::from_markdown(new).unwrap();
    let from_legacy = Document::from_markdown(legacy).unwrap();
    assert_eq!(from_new, from_legacy);
}

#[test]
fn mixed_new_and_legacy_cards_in_one_document() {
    let src = "---\nQUILL: q\n---\n\n```card product\nname: A\n```\n\n---\nCARD: product\nname: B\n---\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 2);
    assert_eq!(doc.cards()[0].tag(), "product");
    assert_eq!(doc.cards()[1].tag(), "product");
}

// ── Canonical emission ────────────────────────────────────────────────────────

#[test]
fn emit_uses_canonical_card_fence_for_new_syntax() {
    let src = "---\nQUILL: q\n---\n\n```card product\nname: Widget\n```\n";
    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();
    assert_eq!(
        emitted,
        "---\nQUILL: q\n---\n\n```card product\nname: \"Widget\"\n```\n"
    );
}

#[test]
fn legacy_syntax_round_trips_to_canonical_card_fence() {
    let src = "---\nQUILL: q\n---\n\n---\nCARD: product\nname: Widget\n---\n";
    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();
    // The legacy `---\nCARD:` fence is never emitted — only the canonical form.
    assert!(!emitted.contains("CARD:"), "legacy fence must not survive emit:\n{emitted}");
    assert!(emitted.contains("```card product\n"), "emit:\n{emitted}");
    assert_eq!(
        emitted,
        "---\nQUILL: q\n---\n\n```card product\nname: \"Widget\"\n```\n"
    );
}

#[test]
fn emit_is_idempotent_for_card_fences() {
    let src = "---\nQUILL: q\n---\n\n```card product\nname: Widget\n```\n\nTrailing body.\n";
    let doc = Document::from_markdown(src).unwrap();
    let once = doc.to_markdown();
    let twice = Document::from_markdown(&once).unwrap().to_markdown();
    assert_eq!(once, twice);
}

#[test]
fn card_fence_body_round_trips() {
    let src = "---\nQUILL: q\n---\n\nMain body.\n\n```card product\nname: Widget\n```\n\nCard body.\n";
    let a = Document::from_markdown(src).unwrap();
    let b = Document::from_markdown(&a.to_markdown()).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.main().body(), "\nMain body.\n");
    assert_eq!(a.cards()[0].body(), "\nCard body.\n");
}

#[test]
fn card_fence_preserves_yaml_comments() {
    let src = "---\nQUILL: q\n---\n\n```card product\n# a banner\nname: Widget\n```\n";
    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();
    assert!(emitted.contains("```card product\n# a banner\nname: \"Widget\"\n```\n"), "emit:\n{emitted}");
    let reparsed = Document::from_markdown(&emitted).unwrap();
    assert_eq!(doc, reparsed);
}

// ── Validation and errors ─────────────────────────────────────────────────────

#[test]
fn card_fence_missing_kind_is_error() {
    let src = "---\nQUILL: q\n---\n\n```card\nname: Widget\n```\n";
    let err = Document::from_markdown(src).unwrap_err().to_string();
    assert!(err.contains("missing a card kind"), "got: {err}");
}

#[test]
fn card_fence_invalid_kind_is_error() {
    let src = "---\nQUILL: q\n---\n\n```card BadKind\nname: Widget\n```\n";
    let err = Document::from_markdown(src).unwrap_err().to_string();
    assert!(err.contains("Invalid card kind"), "got: {err}");
}

#[test]
fn card_fence_extra_info_tokens_is_error() {
    let src = "---\nQUILL: q\n---\n\n```card product extra\nname: Widget\n```\n";
    let err = Document::from_markdown(src).unwrap_err().to_string();
    assert!(err.contains("exactly ```card <kind>```"), "got: {err}");
}

#[test]
fn card_fence_unclosed_is_error() {
    let src = "---\nQUILL: q\n---\n\n```card product\nname: Widget\n";
    let err = Document::from_markdown(src).unwrap_err().to_string();
    assert!(err.contains("not closed"), "got: {err}");
}

#[test]
fn card_fence_reserved_key_is_error() {
    let src = "---\nQUILL: q\n---\n\n```card product\nBODY: nope\n```\n";
    let err = Document::from_markdown(src).unwrap_err().to_string();
    assert!(err.contains("Reserved field name"), "got: {err}");
}

// ── Non-card fenced code blocks are untouched ─────────────────────────────────

#[test]
fn ordinary_code_fence_is_not_a_card() {
    let src = "---\nQUILL: q\n---\n\n```rust\nlet x = 1;\n```\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 0);
    assert!(doc.main().body().contains("```rust"));
}

#[test]
fn card_info_inside_outer_code_fence_is_not_a_card() {
    // A ```` ```card ```` line shielded by an outer code fence is plain text.
    let src = "---\nQUILL: q\n---\n\n````text\n```card product\nname: Widget\n```\n````\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 0);
}

#[test]
fn card_fence_without_blank_line_above_is_not_a_card() {
    // F2 fails — the `card` fence is delegated to CommonMark as a code block,
    // with a non-fatal lint warning.
    let src = "---\nQUILL: q\n---\n\nSome prose.\n```card product\nname: Widget\n```\n";
    let out = Document::from_markdown_with_warnings(src).unwrap();
    assert_eq!(out.document.cards().len(), 0);
    assert!(out
        .warnings
        .iter()
        .any(|w| w.code.as_deref() == Some("parse::card_fence_missing_blank")));
}
