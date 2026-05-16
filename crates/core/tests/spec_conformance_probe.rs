//! Independent spec conformance probes for prose/designs/MARKDOWN.md.
//!
//! These tests exercise concrete spec requirements that are most likely to
//! diverge between the parser and the written standard.

use quillmark_core::normalize::normalize_document;
use quillmark_core::Document;

// §4 F2 — Leading blank: a `---` line directly under non-blank text is not a fence.
#[test]
fn f2_fence_directly_under_paragraph_is_not_a_fence() {
    let md = "---\nQUILL: t\n---\n\nParagraph text.\n---\n\nAfter.";
    let doc = Document::from_markdown(md).unwrap();
    let body = doc.main().body();
    assert!(
        body.contains("Paragraph text.") && body.contains("---") && body.contains("After."),
        "stray `---` under paragraph must be left to CommonMark, body was: {:?}",
        body
    );
}

// §4 F1 — first fence must carry QUILL. A first fence with some other key must not
// be accepted silently.
#[test]
fn f1_first_fence_without_quill_is_rejected() {
    let md = "---\ntitle: X\n---\n\nBody.";
    let err = Document::from_markdown(md).unwrap_err().to_string();
    assert!(err.contains("Missing required QUILL field"), "got: {}", err);
}

// §4 F1 — YAML `#` comment lines at the top of a fence are skipped when
// locating the sentinel. A banner comment above `QUILL:` must not trip F1.
#[test]
fn f1_yaml_comment_banners_above_sentinel_are_accepted() {
    let md = "---\n# Essential\n#===========\nQUILL: t\ntitle: T\n---\n\nBody.";
    let doc = Document::from_markdown(md).unwrap();
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "T"
    );
}

// Inline `---/.../---` blocks are no longer card candidates — they are
// CommonMark thematic breaks (and the content between them is body prose).
// No near-miss warning is emitted for arbitrary YAML-looking content.
#[test]
fn inline_dash_blocks_are_body_not_cards() {
    let md = "---\nQUILL: t\n---\n\nB.\n\n---\n# banner\nCard: oops\nname: X\n---\n\nTrailing.";
    let out = Document::from_markdown_with_warnings(md).unwrap();
    assert!(
        out.document.cards().is_empty(),
        "inline ---/--- block must not register as a card"
    );
    assert!(out.document.main().body().contains("Card: oops"));
}

// Frontmatter F1: a typo'd lowercase `quill:` at the document head emits a
// near-miss warning and surfaces in the MissingQuillField error.
#[test]
fn frontmatter_quill_typo_emits_near_miss() {
    let md = "---\nquill: t\ntitle: T\n---\n\nBody.";
    let err = Document::from_markdown(md).unwrap_err().to_string();
    assert!(
        err.contains("expected `QUILL:`") || err.contains("Missing required QUILL"),
        "expected QUILL ordering hint, got: {}",
        err
    );
}

// §3 — Trailing whitespace on the fence marker must be accepted.
#[test]
fn fence_marker_with_trailing_whitespace_is_accepted() {
    let md = "---  \nQUILL: t\ntitle: T\n---\t\n\nBody.";
    let doc = Document::from_markdown(md).unwrap();
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "T"
    );
}

// §3 — `---` inside a fenced code block must be ignored.
#[test]
fn fences_inside_code_blocks_are_ignored() {
    let md = "---\nQUILL: t\n---\n\n```\n```card x\n```\n```\n\nBody.";
    let doc = Document::from_markdown(md).unwrap();
    assert!(
        doc.cards().is_empty(),
        "fences inside code blocks must not parse"
    );
}

// §3 — Reserved keys BODY/CARDS cannot be user-defined.
#[test]
fn reserved_keys_in_frontmatter_are_rejected() {
    for reserved in ["BODY", "CARDS"] {
        let md = format!("---\nQUILL: t\n{}: nope\n---\n\nBody.", reserved);
        let err = Document::from_markdown(&md).unwrap_err().to_string();
        assert!(
            err.contains(&format!("Reserved field name '{}'", reserved)),
            "reserved key {} must error, got: {}",
            reserved,
            err
        );
    }
}

// §5 — CARDS is always accessible, even when empty.
#[test]
fn cards_is_always_present_even_when_empty() {
    let doc = Document::from_markdown("---\nQUILL: t\n---\n\nBody.").unwrap();
    assert!(doc.cards().is_empty());
}

// §3.2 / §4.2 — a `card`-prefixed fence commits to card parsing on that token
// alone; a missing, invalid, or extra info-string kind token is a hard parse
// error, not a silent fallthrough to body content — a hard error, not a
// silent classification miss.
#[test]
fn card_fence_with_bad_kind_token_is_rejected() {
    let cases = [
        // Missing kind token.
        "---\nQUILL: t\n---\n\n```card\nname: Widget\n```\n",
        // Empty card body, still missing the kind token.
        "---\nQUILL: t\n---\n\n```card\n```\n",
        // Invalid kind token (must match [a-z_][a-z0-9_]*).
        "---\nQUILL: t\n---\n\n```card Widget\nname: x\n```\n",
        // Extra info-string tokens.
        "---\nQUILL: t\n---\n\n```card a b\n```\n",
    ];
    for md in cases {
        let err = Document::from_markdown(md).unwrap_err().to_string();
        assert!(
            err.contains("Card fence at line"),
            "expected hard card-fence kind-token error for {md:?}, got: {err}"
        );
    }
}

// §3.2 — `KIND` is an output-only reserved key; supplying it as an input body
// key is a hard parse error.
#[test]
fn kind_as_card_body_key_is_rejected() {
    let md = "---\nQUILL: t\n---\n\n```card product\nKIND: product\n```\n";
    let err = Document::from_markdown(md).unwrap_err().to_string();
    assert!(
        err.contains("Reserved field name") && err.contains("KIND"),
        "got: {err}"
    );
}

// §3.2 — card kind-token pattern.
#[test]
fn card_name_pattern_enforced() {
    let md = "---\nQUILL: t\n---\n\nB.\n\n```card ITEMS\n```\n\nX.";
    let err = Document::from_markdown(md).unwrap_err().to_string();
    assert!(err.contains("invalid kind token"), "got: {}", err);
}

// §7 — Body bidi stripped during normalize_document.
#[test]
fn normalize_body_strips_bidi() {
    let md = "---\nQUILL: t\n---\n\nhi\u{202D}there";
    let doc = Document::from_markdown(md).unwrap();
    let doc = normalize_document(doc).unwrap();
    assert_eq!(doc.main().body(), "\nhithere");
}

// §7 — YAML scalar bidi NOT stripped.
#[test]
fn normalize_yaml_scalar_keeps_bidi() {
    let md = "---\nQUILL: t\ntitle: hi\u{202D}there\n---\n";
    let doc = Document::from_markdown(md).unwrap();
    let doc = normalize_document(doc).unwrap();
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "hi\u{202D}there"
    );
}

// §7 — Card body normalization reaches nested cards.
#[test]
fn normalize_reaches_card_body() {
    let md = "---\nQUILL: t\n---\n\n```card x\n```\n\n<!-- c -->trailing\u{202D}text";
    let doc = Document::from_markdown(md).unwrap();
    let doc = normalize_document(doc).unwrap();
    let body = doc.cards()[0].body();
    assert!(
        body.contains("<!-- c -->\ntrailingtext"),
        "card body missing repair/bidi-strip, got: {:?}",
        body
    );
}

// §4 F3 — A `---` line indented by four or more spaces is indented code.
#[test]
fn f3_indented_four_spaces_is_not_a_fence() {
    let md = "---\nQUILL: t\n---\n\n    ---\n    KIND: x\n    ---\n\nafter.";
    let doc = Document::from_markdown(md).unwrap();
    assert!(
        doc.cards().is_empty(),
        "indented `---` must not register as a fence"
    );
    let body = doc.main().body();
    assert!(
        body.contains("    ---") && body.contains("KIND: x"),
        "indented fence content must be delegated to CommonMark, body was: {:?}",
        body
    );
}

// §4 F3 — Up to three leading spaces is still a fence.
#[test]
fn f3_three_leading_spaces_is_still_a_fence() {
    let md = "   ---\nQUILL: t\n   ---\n\nBody.";
    let doc = Document::from_markdown(md).unwrap();
    assert!(doc.main().body().contains("Body."));
}

// §4 F3 — Tab indentation disqualifies a line from being a fence marker.
#[test]
fn f3_tab_indented_is_not_a_fence() {
    let md = "---\nQUILL: t\n---\n\n\t---\n\tKIND: x\n\t---\n\nafter.";
    let doc = Document::from_markdown(md).unwrap();
    assert!(
        doc.cards().is_empty(),
        "tab-indented `---` must not register as a fence"
    );
}

// §7 — CRLF line endings in the body are canonicalized to LF.
#[test]
fn body_crlf_line_endings_are_normalized() {
    let md = "---\nQUILL: t\n---\n\nLine one.\r\nLine two.\r\n";
    let doc = Document::from_markdown(md).unwrap();
    let doc = normalize_document(doc).unwrap();
    let body = doc.main().body();
    assert!(
        !body.contains('\r'),
        "body must not contain bare \\r after normalization, got: {:?}",
        body
    );
    assert!(body.contains("Line one.\nLine two."));
}

// §7 — CRLF normalization reaches card bodies.
#[test]
fn card_body_crlf_line_endings_are_normalized() {
    let md = "---\nQUILL: t\n---\n\n```card x\n```\n\nCard line one.\r\nCard line two.\r\n";
    let doc = Document::from_markdown(md).unwrap();
    let doc = normalize_document(doc).unwrap();
    let body = doc.cards()[0].body();
    assert!(
        !body.contains('\r'),
        "card body must not contain bare \\r after normalization, got: {:?}",
        body
    );
}

// §3 — UTF-8 BOM at the start of the document must not defeat F2.
#[test]
fn utf8_bom_at_start_is_stripped() {
    let md = "\u{FEFF}---\nQUILL: t\ntitle: T\n---\n\nBody.";
    let doc = Document::from_markdown(md).unwrap();
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "T"
    );
}

// §4 / §9 — First-fence near-miss (case-only) produces a specific error.
#[test]
fn first_fence_case_near_miss_error_is_specific() {
    let err = Document::from_markdown("---\nQuill: t\n---\n\nBody.")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("Quill") && err.contains("QUILL") && err.contains("uppercase"),
        "expected case-hint error, got: {}",
        err
    );
}

// §4 / §9 — First-fence ordering error (QUILL not first) names the actual first key.
#[test]
fn first_fence_out_of_order_error_is_specific() {
    let err = Document::from_markdown("---\ntitle: X\nQUILL: t\n---\n\nBody.")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("title") && err.contains("first key"),
        "expected ordering-hint error, got: {}",
        err
    );
}

// §3 — Unclosed fenced code block at end-of-document emits a warning.
#[test]
fn unclosed_code_block_emits_warning() {
    // 4-backtick opener with no matching 4+-backtick closer → unclosed.
    // The 3-backtick `card` fence inside is shielded (3 < 4 cannot close).
    let md = "---\nQUILL: t\n---\n\n````\ncode line\n\n```card x\n```\n\ntrailing body";
    let out = Document::from_markdown_with_warnings(md).unwrap();
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code.as_deref() == Some("parse::unclosed_code_block")),
        "expected unclosed-code-block warning, got: {:?}",
        out.warnings
            .iter()
            .map(|w| (w.code.clone(), w.message.clone()))
            .collect::<Vec<_>>()
    );
    // And the shielded card fence must NOT have registered.
    assert!(
        out.document.cards().is_empty(),
        "shielded card fence must not have been parsed"
    );
}

// §8 — Per-fence field-count cap.
#[test]
fn per_fence_field_count_cap() {
    let mut s = String::from("---\nQUILL: t\n");
    for i in 0..1001 {
        s.push_str(&format!("f{}: v\n", i));
    }
    s.push_str("---\n\nBody.");
    let err = Document::from_markdown(&s).unwrap_err().to_string();
    assert!(err.contains("Input too large"), "got: {}", err);
}

// §8 — Card count cap counts cards only.
#[test]
fn card_count_cap_is_per_card() {
    let mut s = String::from("---\nQUILL: t\n---\n");
    for _ in 0..1001 {
        s.push_str("\n```card x\n```\n\nB.\n");
    }
    let err = Document::from_markdown(&s).unwrap_err().to_string();
    assert!(err.contains("Input too large"), "got: {}", err);
}
