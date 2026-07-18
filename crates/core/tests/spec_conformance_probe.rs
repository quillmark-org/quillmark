//! Independent spec conformance probes for the card-yaml metadata syntax
//! described in `prose/references/markdown-spec.md`.
//!
//! These tests exercise concrete spec requirements that are most likely to
//! diverge between the parser and the written standard.

use quillmark_core::normalize::normalize_document;
use quillmark_core::Document;

// A bare `---` thematic break inside body prose is not a metadata block.
#[test]
fn thematic_break_in_body_is_not_a_block() {
    let md = "~~~card-yaml\n$quill: t\n$kind: main\n~~~\n\nParagraph text.\n\n---\n\nAfter.";
    let doc = Document::parse(md).unwrap().document;
    let body = doc.main().body_markdown();
    // `---` is delegated to CommonMark, not parsed as a metadata block: the
    // paragraphs survive and no card splits off. (The thematic break itself has
    // no corpus representation, so it is dropped by the projection.)
    assert!(
        body.contains("Paragraph text.") && body.contains("After."),
        "stray `---` in prose must be left to CommonMark, body was: {:?}",
        body
    );
    assert!(doc.cards().is_empty(), "no cards expected");
}

// The document's root card-yaml block must declare `$quill:`.
#[test]
fn first_block_without_quill_is_rejected() {
    let md = "~~~card-yaml\ntitle: X\n~~~\n\nBody.";
    let err = Document::parse(md).unwrap_err().to_string();
    assert!(err.contains("must declare `$quill"), "got: {}", err);
}

// YAML `#` comment lines inside a block are accepted as ordinary YAML.
#[test]
fn yaml_comment_banners_inside_block_are_accepted() {
    let md = "~~~card-yaml\n$quill: t\n$kind: main\n# Essential\ntitle: T\n~~~\n\nBody.";
    let doc = Document::parse(md).unwrap().document;
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "T"
    );
}

// A composable card-yaml block declares `$kind:`.
#[test]
fn composable_card_block_registers_a_card() {
    let md = "~~~card-yaml\n$quill: t\n$kind: main\n~~~\n\nB.\n\n~~~card-yaml\n$kind: endorsement\nname: X\n~~~\n\nTrailing.";
    let doc = Document::parse(md).unwrap().document;
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), Some("endorsement"));
}

// A non-root block missing `$kind:` is allowed — `$kind` is optional
// metadata.
#[test]
fn non_root_block_without_kind_is_allowed() {
    let md = "~~~card-yaml\n$quill: t\n$kind: main\n~~~\n\nB.\n\n~~~card-yaml\nname: X\n~~~\n";
    let doc = Document::parse(md).unwrap().document;
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), None);
}

// `~~~card-yaml` inside an ordinary fenced code block must be ignored.
#[test]
fn fences_inside_code_blocks_are_ignored() {
    let md = "~~~card-yaml\n$quill: t\n$kind: main\n~~~\n\n```\n~~~card-yaml\n$kind: x\n~~~\n```\n\nBody.";
    let doc = Document::parse(md).unwrap().document;
    assert!(
        doc.cards().is_empty(),
        "card-yaml blocks inside code blocks must not parse"
    );
}

// `$`-prefixed payload keys other than `$quill`/`$kind`/`$id` are
// rejected, so user content can't shadow the plate wire format's `$body`
// or `$cards` metadata.
#[test]
fn unknown_dollar_keys_in_payload_are_rejected() {
    for key in ["$body", "$cards", "$arbitrary"] {
        let md = format!(
            "~~~card-yaml\n$quill: t\n$kind: main\n{}: nope\n~~~\n\nBody.",
            key
        );
        let err = Document::parse(&md).unwrap_err().to_string();
        assert!(
            err.contains("system-metadata") || err.contains(key),
            "unknown $-key {} must error, got: {}",
            key,
            err
        );
    }
}

// CARDS is always accessible, even when empty.
#[test]
fn cards_is_always_present_even_when_empty() {
    let doc =
        Document::parse("~~~card-yaml\n$quill: t\n$kind: main\n~~~\n\nBody.").unwrap().document;
    assert!(doc.cards().is_empty());
}

// `$kind:` is name-validated at parse time against `[a-z_][a-z0-9_]*`.
#[test]
fn card_kind_is_name_validated() {
    let bad =
        "~~~card-yaml\n$quill: t\n$kind: main\n~~~\n\nB.\n\n~~~card-yaml\n$kind: ITEMS\n~~~\n\nX.";
    assert!(Document::parse(bad).is_err());

    let ok =
        "~~~card-yaml\n$quill: t\n$kind: main\n~~~\n\nB.\n\n~~~card-yaml\n$kind: items\n~~~\n\nX.";
    let doc = Document::parse(ok).unwrap().document;
    assert_eq!(doc.cards()[0].kind(), Some("items"));
}

// Card body normalization reaches nested cards.
#[test]
fn normalize_reaches_card_body() {
    let md = "~~~card-yaml\n$quill: t\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: x\n~~~\n\n<!-- c -->trailing\u{202D}text";
    let doc = Document::parse(md).unwrap().document;
    let doc = normalize_document(doc).unwrap();
    let body = doc.cards()[0].body_markdown();
    // Bidi-strip normalization reaches the nested card body at import
    // (`trailing\u{202D}text` → `trailingtext`). The HTML comment is not
    // representable in the corpus and is dropped by the projection.
    assert!(
        body.contains("trailingtext"),
        "card body missing bidi-strip, got: {:?}",
        body
    );
}

// CRLF line endings in the body are canonicalized to LF.
#[test]
fn body_crlf_line_endings_are_normalized() {
    let md = "~~~card-yaml\n$quill: t\n$kind: main\n~~~\n\nLine one.\r\nLine two.\r\n";
    let doc = Document::parse(md).unwrap().document;
    let doc = normalize_document(doc).unwrap();
    let body = doc.main().body_markdown();
    assert!(
        !body.contains('\r'),
        "body must not contain bare \\r after normalization, got: {:?}",
        body
    );
    // Both lines survive (the soft break's exact projection is import's concern).
    assert!(body.contains("Line one.") && body.contains("Line two."));
}

// CRLF normalization reaches card bodies.
#[test]
fn card_body_crlf_line_endings_are_normalized() {
    let md = "~~~card-yaml\n$quill: t\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: x\n~~~\n\nCard line one.\r\nCard line two.\r\n";
    let doc = Document::parse(md).unwrap().document;
    let doc = normalize_document(doc).unwrap();
    let body = doc.cards()[0].body_markdown();
    assert!(
        !body.contains('\r'),
        "card body must not contain bare \\r after normalization, got: {:?}",
        body
    );
}

// Empty input is rejected with a specific error.
#[test]
fn empty_input_is_rejected() {
    let err = Document::parse("").unwrap_err().to_string();
    assert!(err.contains("Empty markdown input"), "got: {}", err);
}

// A document with no card-yaml block is rejected.
#[test]
fn missing_root_block_is_rejected() {
    let err = Document::parse("Just prose, no metadata.")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("Missing required root card-yaml block"),
        "got: {}",
        err
    );
}

// An unclosed root fence is delegated to CommonMark (a code block to EOF), so
// no root block is recognised and the document fails with MissingQuill.
#[test]
fn unclosed_card_yaml_block_falls_through_to_commonmark() {
    let md = "~~~card-yaml\n$quill: t\n$kind: main\ntitle: T\n";
    let err = Document::parse(md).unwrap_err().to_string();
    assert!(err.contains("Missing required root"), "got: {}", err);
}

// A card-yaml block with no blank line above is treated as an ordinary code
// block and emits a `parse::card_fence_missing_blank` warning.
#[test]
fn card_fence_missing_blank_emits_warning() {
    let md = "~~~card-yaml\n$quill: t\n$kind: main\n~~~\nBody line.\n~~~card-yaml\n$kind: x\n~~~\n";
    let out = Document::parse(md).unwrap();
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code.as_deref() == Some("parse::card_fence_missing_blank")),
        "expected card_fence_missing_blank warning, got: {:?}",
        out.warnings
            .iter()
            .map(|w| (w.code.clone(), w.message.clone()))
            .collect::<Vec<_>>()
    );
    // The block without a blank line above must not register as a card.
    assert!(out.document.cards().is_empty());
}

// Unclosed fenced code block at end-of-document emits a warning.
#[test]
fn unclosed_code_block_emits_warning() {
    let md = "~~~card-yaml\n$quill: t\n$kind: main\n~~~\n\n```\ncode line\n\n~~~card-yaml\n$kind: x\n~~~\n\ntrailing body";
    let out = Document::parse(md).unwrap();
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
    // And the shielded card block must NOT have registered.
    assert!(
        out.document.cards().is_empty(),
        "shielded card block must not have been parsed"
    );
}

// Per-block field-count cap.
#[test]
fn per_block_field_count_cap() {
    let mut s = String::from("~~~card-yaml\n$quill: t\n$kind: main\n");
    for i in 0..1001 {
        s.push_str(&format!("f{}: v\n", i));
    }
    s.push_str("~~~\n\nBody.");
    let err = Document::parse(&s).unwrap_err().to_string();
    assert!(err.contains("Input too large"), "got: {}", err);
}

// Card count cap counts cards only.
#[test]
fn card_count_cap_is_per_card() {
    let mut s = String::from("~~~card-yaml\n$quill: t\n$kind: main\n~~~\n");
    for _ in 0..1001 {
        s.push_str("\n~~~card-yaml\n$kind: x\n~~~\n\nB.\n");
    }
    let err = Document::parse(&s).unwrap_err().to_string();
    assert!(err.contains("Input too large"), "got: {}", err);
}
