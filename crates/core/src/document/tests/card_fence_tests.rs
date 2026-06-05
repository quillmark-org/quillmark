//! Tests for the `~~~` card-yaml composable-card syntax.
//!
//! Coverage:
//! - Composable cards declared with `$kind:` parse to `Card`s; `$kind` is optional.
//! - Every card round-trips to the canonical bare `~~~` form.
//! - The `~~~card-yaml` opener is accepted on input as a non-canonical alias.
//! - Ordinary fenced code blocks and the blank-line rule for `~~~` openers.
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
        "~~~\n$quill: q\n$kind: main\n~~~\n\n~~~\n$kind: product\nname: Widget\n~~~\n"
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
        emitted.contains("~~~\n$kind: product\n# a banner\nname: Widget\n~~~\n"),
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

// ── Bare `~~~` is the canonical card-yaml fence ───────────────────────────────

#[test]
fn bare_tilde_fence_opens_a_card_yaml_block() {
    // The canonical opener is a bare `~~~` (no info string). It parses the
    // same as the `~~~card-yaml` alias.
    let src =
        "~~~\n$quill: q\n$kind: main\ntitle: Hi\n~~~\n\nBody.\n\n~~~\n$kind: note\nname: N\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.quill_reference().name, "q");
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str(),
        Some("Hi")
    );
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), Some("note"));
}

#[test]
fn bare_tilde_fence_round_trips_byte_equal() {
    let src = "~~~\n$quill: q\n$kind: main\ntitle: Hi\n~~~\n\nBody.\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.to_markdown(), src);
}

#[test]
fn legacy_card_yaml_info_string_normalizes_to_bare_tilde() {
    // `~~~card-yaml` is accepted on input but converges to a bare `~~~` opener
    // on first emit.
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n";
    let emitted = Document::from_markdown(src).unwrap().to_markdown();
    assert_eq!(emitted, "~~~\n$quill: q\n$kind: main\n~~~\n");
}

// ── Escape hatches: not every tilde fence is a card-yaml block ─────────────────

#[test]
fn longer_tilde_run_still_opens_a_card() {
    // A four-tilde fence is NOT an escape hatch — it is a (non-canonical) card
    // opener whose closer must be at least as long, and which re-emits as the
    // canonical three-tilde form.
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\n~~~~\n$kind: note\nname: Widget\n~~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), Some("note"));
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("~~~\n$kind: note\nname: Widget\n~~~\n"),
        "{emitted}"
    );
    assert!(
        !emitted.contains("~~~~"),
        "longer runs normalise to `~~~`: {emitted}"
    );
}

#[test]
fn shorter_tilde_run_does_not_close_a_longer_fence() {
    // A `~~~` line inside a `~~~~`-fenced block is payload, not a closer
    // (CommonMark fence matching: the closer must be >= the opener length).
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\n~~~~\nbody: \"a ~~~ b\"\n~~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(
        doc.cards()[0].payload().get("body").unwrap().as_str(),
        Some("a ~~~ b")
    );
}

#[test]
fn backtick_fence_is_the_code_block_escape_hatch() {
    // The way to write a literal fenced code block in body prose is a backtick
    // fence — it is never a card-yaml block, even when it contains `~~~` lines.
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\n```\n~~~\nnot a card\n~~~\n```\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 0);
    assert!(doc.main().body().contains("not a card"));
}

#[test]
fn tilde_fence_with_language_info_is_an_ordinary_code_block() {
    // A `~~~` fence carrying a non-`card-yaml` info string (e.g. a language)
    // stays an ordinary CommonMark code block.
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\n~~~rust\nlet x = 1;\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 0);
    assert!(doc.main().body().contains("let x = 1;"));
}

#[test]
fn tilde_code_block_without_blank_line_above_stays_in_body() {
    // A `~~~` fence with no blank line above it fails the blank-line rule, so
    // the scanner does NOT claim it as a card-yaml opener — it is left in the
    // body verbatim for the CommonMark renderer to treat as a code block.
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\nText line\n~~~\ncode\n~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 0);
    assert!(doc.main().body().contains("~~~\ncode\n~~~"));
}

#[test]
fn indented_tilde_opener_is_not_a_card() {
    // A `~~~` opener must be at column zero (spec §3.2). An indented `~~~`
    // (1–3 spaces) is a CommonMark code fence, not a card opener, so it stays
    // in the body rather than splitting off a card.
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\nBody.\n\n   ~~~\n$kind: note\nx: 1\n   ~~~\n";
    let doc = Document::from_markdown(src).unwrap();
    assert_eq!(doc.cards().len(), 0);
    assert!(doc.main().body().contains("   ~~~"));
}

#[test]
fn unclosed_bare_tilde_in_body_falls_through_to_commonmark() {
    // A bare `~~~` opener in the body with no matching closer is not a hard
    // error: per CommonMark an unclosed fence is an ordinary code block running
    // to end of document. The root parses, the stray `~~~` stays in the body,
    // and a non-fatal unclosed-fence warning is emitted.
    let src = "~~~\n$quill: q\n$kind: main\n~~~\n\nIntro.\n\n~~~\nstray\n";
    let out = Document::from_markdown_with_warnings(src).unwrap();
    assert_eq!(out.document.cards().len(), 0);
    assert!(out.document.main().body().contains("~~~\nstray"));
    assert!(out
        .warnings
        .iter()
        .any(|w| w.code.as_deref() == Some("parse::unclosed_code_block")));
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
