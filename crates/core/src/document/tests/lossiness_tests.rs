//! Round-trip tests for comments, `!must_fill`, and custom tags.
//!
//! Both own-line and trailing inline YAML comments round-trip at their
//! source position. Own-line comments in a block's payload (below the
//! `$quill` / `$kind` metadata header) also round-trip. Comments whose
//! host disappears at emit
//! time (empty-mapping omission, programmatic field removal) degrade to
//! own-line comments at the same indent so the comment text is preserved
//! even when its position shifts. `!must_fill` on scalars and sequences round-
//! trips. String quoting is normalised to saphyr's canonical form (plain
//! when safe, quoted when the value would otherwise be misread on
//! re-parse) — type fidelity is guaranteed; the exact quoting style is
//! not.

use crate::document::Document;

// ── Category: YAML comments ───────────────────────────────────────────────────

/// Top-level YAML comments survive a round-trip.
#[test]
fn top_level_comments_round_trip() {
    let src =
        "~~~card-yaml\n$quill: q\n$kind: main\n# recipient's full name\nrecipient: Jane\nauthor: Alice\n~~~\n\nBody.\n";

    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();

    assert!(
        emitted.contains("# recipient's full name"),
        "top-level YAML comment must survive round-trip\nGot:\n{}",
        emitted
    );

    // Value remains intact.
    let doc2 = Document::parse(&emitted).unwrap().document;
    assert_eq!(
        doc2.main()
            .payload()
            .get("recipient")
            .and_then(|v| v.as_str()),
        Some("Jane"),
    );

    // Comment idempotent across repeated round-trips.
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// Trailing inline comments on top-level fields round-trip inline.
#[test]
fn top_level_inline_comments_round_trip() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\ntitle: My Document # this is a comment\n~~~\n\nBody.\n";

    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();

    assert!(
        emitted.contains("title: My Document # this is a comment"),
        "trailing inline comment must round-trip on the same line\nGot:\n{}",
        emitted
    );
    assert!(
        !emitted.contains("My Document\n# this is a comment"),
        "trailing inline comment must NOT degrade to own-line\nGot:\n{}",
        emitted
    );

    // Value still intact.
    let doc2 = Document::parse(&emitted).unwrap().document;
    assert_eq!(
        doc2.main().payload().get("title").and_then(|v| v.as_str()),
        Some("My Document"),
    );

    // Idempotent across repeated round-trips.
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

// ── Category: Block scalars ───────────────────────────────────────────────────

/// A block-scalar value (markdown) whose content contains `#` heading lines
/// round-trips intact. Regression guard: the prescan comment-stripper must not
/// treat `#`-leading lines inside literal blocks as YAML comments, which would
/// silently delete markdown headings (and mis-read `- ` / `key:` lines).
#[test]
fn block_scalar_with_markdown_headings_round_trips() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nbio: |-\n  ## About me\n\n  - first point\n  Plain line.\ntitle: Resume\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    assert_eq!(
        doc.main().payload().get("bio").and_then(|v| v.as_str()),
        Some("## About me\n\n- first point\nPlain line."),
        "markdown heading / bullet / plain lines inside a block scalar must survive parse",
    );
    // The block ended; the field after it parses normally.
    assert_eq!(
        doc.main().payload().get("title").and_then(|v| v.as_str()),
        Some("Resume"),
    );

    // Full round-trip is value-stable and idempotent.
    let emitted = doc.to_markdown();
    let doc2 = Document::parse(&emitted).unwrap().document;
    assert_eq!(
        doc2.main().payload().get("bio").and_then(|v| v.as_str()),
        Some("## About me\n\n- first point\nPlain line."),
        "block-scalar content must survive a full round-trip\nGot:\n{}",
        emitted
    );
    assert_eq!(emitted, doc2.to_markdown(), "round-trip must be idempotent");
}

/// An array authored as `- |` block-scalar items (a `richtext[]` field)
/// preserves `#` heading / `- ` bullet content per element. Regression for the
/// sequence-item path of the block-scalar prescan fix.
#[test]
fn block_scalar_sequence_items_round_trip() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nsections:\n  - |-\n    ## First\n    body one\n  - |-\n    ## Second\n    body two\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    let arr = doc
        .main()
        .payload()
        .get("sections")
        .and_then(|v| v.as_array())
        .expect("sections array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].as_str(), Some("## First\nbody one"));
    assert_eq!(arr[1].as_str(), Some("## Second\nbody two"));
}

// ── Category: Custom tags ─────────────────────────────────────────────────────

/// `!must_fill` tags round-trip; other custom tags are rejected with a warning
/// and the tag is dropped.
#[test]
fn custom_tags_lose_tag_but_keep_value() {
    // `!must_fill` case: round-trip with fill preserved.
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nmemo_from: !must_fill 2d lt example\n~~~\n";
    let doc = Document::parse(src).unwrap().document;

    let fm = doc.main().payload();
    assert_eq!(
        fm.get("memo_from").and_then(|v| v.as_str()),
        Some("2d lt example"),
        "string value must survive tag parsing"
    );
    assert!(fm.is_fill("memo_from"), "fill marker must be recorded");

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("memo_from: !must_fill"),
        "`!must_fill` tag must round-trip\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    assert!(
        doc2.main().payload().is_fill("memo_from"),
        "fill marker must survive a full round-trip"
    );

    // Non-`!must_fill` tag case: warning + dropped tag.
    let src2 = "~~~card-yaml\n$quill: q\n$kind: main\nmemo_from: !include value.txt\n~~~\n";
    let out = Document::parse(src2).unwrap();
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code.as_deref() == Some("parse::unsupported_yaml_tag")),
        "expected unsupported_yaml_tag warning; got: {:?}",
        out.warnings
    );
    let emitted2 = out.document.to_markdown();
    assert!(
        !emitted2.contains("!include"),
        "unknown tag must not re-appear on emit\nGot:\n{}",
        emitted2
    );
}

/// `!fill` is not an alias for `!must_fill`: it is treated like any other
/// noncanonical tag — dropped with an unsupported-tag warning, the value kept,
/// and no fill marker recorded.
#[test]
fn fill_tag_is_rejected_as_noncanonical() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nmemo_from: !fill draft\n~~~\n";
    let out = Document::parse(src).unwrap();

    let fm = out.document.main().payload();
    assert_eq!(fm.get("memo_from").and_then(|v| v.as_str()), Some("draft"));
    assert!(
        !fm.is_fill("memo_from"),
        "`!fill` must not record a fill marker"
    );
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code.as_deref() == Some("parse::unsupported_yaml_tag")),
        "`!fill` must warn as an unsupported tag; got: {:?}",
        out.warnings
    );

    // The dropped tag means a plain value: no `!must_fill`, no `!fill` on emit.
    let emitted = out.document.to_markdown();
    assert!(
        !emitted.contains("!fill") && !emitted.contains("!must_fill"),
        "rejected tag must not reappear on emit\nGot:\n{}",
        emitted
    );
}

/// `!must_fill` in a position prescan cannot lift — inside a flow collection
/// or on a bare sequence element — would be silently dropped by the YAML
/// parser, so it is reported with a warning rather than lost quietly.
/// Block-style nested markers (the supported form) do not warn.
#[test]
fn unsupported_fill_position_warns_not_silently_dropped() {
    let code = "parse::fill_marker_unsupported_position";
    let warns = |src: &str| {
        Document::parse(src)
            .unwrap()
            .warnings
            .iter()
            .any(|w| w.code.as_deref() == Some(code))
    };

    // Flow collection containing a marker.
    assert!(
        warns("~~~card-yaml\n$quill: q\n$kind: main\naddr: {street: !must_fill, city: x}\n~~~\n"),
        "flow-collection marker must warn"
    );
    // Bare sequence element.
    assert!(
        warns("~~~card-yaml\n$quill: q\n$kind: main\ntags:\n  - !must_fill\n  - a\n~~~\n"),
        "bare sequence-element marker must warn"
    );
    // Supported block-style nested marker must NOT warn.
    assert!(
        !warns(
            "~~~card-yaml\n$quill: q\n$kind: main\naddr:\n  street: !must_fill\n  city: x\n~~~\n"
        ),
        "block-style nested marker must not warn"
    );
    // A quoted scalar that merely contains the literal text must NOT warn.
    assert!(
        !warns("~~~card-yaml\n$quill: q\n$kind: main\nnote: \"see !must_fill docs\"\n~~~\n"),
        "quoted literal must not warn"
    );
}

/// `!must_fill` on a nested mapping leaf is captured on that leaf's tree
/// node and round-trips at the correct depth.
#[test]
fn nested_must_fill_round_trips() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\naddr:\n  street: !must_fill\n  city: Springfield\n~~~\n";
    let doc = Document::parse(src).unwrap().document;

    let fm = doc.main().payload();
    let addr = fm.get("addr").unwrap();
    assert!(
        addr.get("street").unwrap().fill(),
        "nested `street` must carry the fill marker"
    );
    assert!(!addr.get("city").unwrap().fill(), "`city` must not");
    assert_eq!(addr.get("city").unwrap().as_str(), Some("Springfield"));
    // The marker is on the leaf node, not the parent object.
    assert!(!addr.fill());

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("street: !must_fill") && emitted.contains("city: Springfield"),
        "nested fill must round-trip at depth\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    assert!(doc2
        .main()
        .payload()
        .get("addr")
        .unwrap()
        .get("street")
        .unwrap()
        .fill());
}

/// `!must_fill` on a nested mapping (object-valued node) is rejected, the
/// same as at the top level.
#[test]
fn nested_must_fill_on_mapping_is_rejected() {
    let src =
        "~~~card-yaml\n$quill: q\n$kind: main\nouter:\n  inner: !must_fill\n    leaf: 1\n~~~\n";
    let err = Document::parse(src);
    assert!(
        err.is_err(),
        "`!must_fill` on an object-valued nested key must be rejected"
    );
}

/// `!must_fill` on a bare key (no value) emits `key: !must_fill` and preserves null.
#[test]
fn fill_tag_bare_null_round_trip() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nrecipient: !must_fill\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    let fm = doc.main().payload();

    assert!(fm.get("recipient").map(|v| v.is_null()).unwrap_or(false));
    assert!(fm.is_fill("recipient"));

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("recipient: !must_fill\n"),
        "bare `!must_fill` must round-trip as `key: !must_fill`\nGot:\n{}",
        emitted
    );
}

/// `!must_fill` on a top-level block sequence round-trips, preserving items and
/// the fill marker.
#[test]
fn fill_tag_block_sequence_round_trip() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nrecipient: !must_fill\n  - Dr. Who\n  - 1 TARDIS Lane\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    let fm = doc.main().payload();

    assert!(fm.is_fill("recipient"));
    let arr = fm.get("recipient").and_then(|v| v.as_array()).unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].as_str(), Some("Dr. Who"));
    assert_eq!(arr[1].as_str(), Some("1 TARDIS Lane"));

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("recipient: !must_fill\n"),
        "`!must_fill` on sequence must emit `key: !must_fill` before the block\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    assert!(doc2.main().payload().is_fill("recipient"));
    assert_eq!(doc2, doc, "full round-trip must be equal");
}

/// `!must_fill` on a flow sequence round-trips (normalised to block form).
#[test]
fn fill_tag_flow_sequence_round_trip() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\ntags: !must_fill [a, b, c]\n~~~\n";
    let doc = Document::parse(src).unwrap().document;
    let fm = doc.main().payload();
    assert!(fm.is_fill("tags"));
    assert_eq!(
        fm.get("tags").and_then(|v| v.as_array()).map(|a| a.len()),
        Some(3)
    );

    let emitted = doc.to_markdown();
    let doc2 = Document::parse(&emitted).unwrap().document;
    assert!(doc2.main().payload().is_fill("tags"));
    assert_eq!(doc2, doc);
}

/// `!must_fill` on an empty sequence round-trips as `key: !must_fill []`.
#[test]
fn fill_tag_empty_sequence_round_trip() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nitems: !must_fill []\n~~~\n";
    let doc = Document::parse(src).unwrap().document;
    let fm = doc.main().payload();
    assert!(fm.is_fill("items"));
    assert_eq!(
        fm.get("items").and_then(|v| v.as_array()).map(|a| a.len()),
        Some(0)
    );

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("items: !must_fill []\n"),
        "empty fill-sequence must round-trip as `key: !must_fill []`\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    assert_eq!(doc2, doc);
}

/// `!must_fill` on a top-level mapping is rejected at parse.
#[test]
fn fill_tag_mapping_rejected() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nx: !must_fill {a: 1}\n~~~\n";
    let err = Document::parse(src).unwrap_err();
    assert!(
        err.to_string().contains("!must_fill") && err.to_string().contains("mapping"),
        "expected mapping-rejection error; got: {}",
        err
    );
}

/// `!must_fill` on every supported scalar type round-trips with the correct type.
#[test]
fn fill_tag_all_scalar_types_round_trip() {
    let src = concat!(
        "~~~card-yaml\n$quill: q\n$kind: main\n",
        "s: !must_fill hello\n",
        "i: !must_fill 42\n",
        "f: !must_fill 3.14\n",
        "b: !must_fill true\n",
        "n: !must_fill\n",
        "~~~\n",
    );

    let doc = Document::parse(src).unwrap().document;
    let fm = doc.main().payload();

    assert_eq!(fm.get("s").and_then(|v| v.as_str()), Some("hello"));
    assert_eq!(fm.get("i").and_then(|v| v.as_i64()), Some(42));
    #[allow(clippy::approx_constant)]
    let expected_f = 3.14;
    assert_eq!(fm.get("f").and_then(|v| v.as_f64()), Some(expected_f));
    assert_eq!(fm.get("b").and_then(|v| v.as_bool()), Some(true));
    assert!(fm.get("n").map(|v| v.is_null()).unwrap_or(false));

    for key in ["s", "i", "f", "b", "n"] {
        assert!(fm.is_fill(key), "{} must be fill-tagged", key);
    }

    let emitted = doc.to_markdown();
    let doc2 = Document::parse(&emitted).unwrap().document;
    for key in ["s", "i", "f", "b", "n"] {
        assert!(
            doc2.main().payload().is_fill(key),
            "{} must remain fill-tagged after round-trip",
            key
        );
    }
}

// ── Category: Canonical quoting style ────────────────────────────────────────

/// Quoting style is normalized on emit — saphyr picks the canonical form
/// (plain when safe, quoted when the unquoted form would be re-parsed as
/// the wrong type). The original quoting in the source is not preserved,
/// but values survive round-trip with type fidelity, which is what
/// callers actually depend on.
#[test]
fn quoting_normalises_to_canonical_form_with_type_fidelity() {
    // Mix of single-quoted, unquoted, and double-quoted strings — all of
    // them safe-to-emit-plain after parse.
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nsingle_q: 'hello'\nunquoted: world\ndouble_q: \"already\"\nambiguous: \"on\"\nnumeric_str: \"01234\"\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();

    // Source-side single-quoting is dropped; safe strings emit plain.
    assert!(
        !emitted.contains("'hello'"),
        "original single-quote style must not survive\nGot:\n{}",
        emitted
    );

    // Ambiguous values stay quoted so they re-parse as strings.
    assert!(
        emitted.contains("\"on\"") || emitted.contains("'on'"),
        "ambiguous string `on` must stay quoted\nGot:\n{}",
        emitted
    );
    assert!(
        emitted.contains("\"01234\"") || emitted.contains("'01234'"),
        "numeric-looking string `01234` must stay quoted\nGot:\n{}",
        emitted
    );

    // Values survive the full round-trip with the right types.
    let doc2 = Document::parse(&emitted).unwrap().document;
    for (key, expected) in [
        ("single_q", "hello"),
        ("unquoted", "world"),
        ("double_q", "already"),
        ("ambiguous", "on"),
        ("numeric_str", "01234"),
    ] {
        assert_eq!(
            doc2.main().payload().get(key).and_then(|v| v.as_str()),
            Some(expected),
            "field {key} must round-trip as string {expected:?}",
        );
    }

    // And emission is idempotent: a second round-trip is byte-equal.
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

// ── Category: Nested comments round-trip ─────────────────────────────────────

/// Comments inside nested sequences round-trip at the matching position.
#[test]
fn nested_sequence_comments_round_trip() {
    let src =
        "~~~card-yaml\n$quill: q\n$kind: main\nitems:\n  # before-first\n  - a\n  # between\n  - b\n  # after-last\n~~~\n";

    let out = Document::parse(src).unwrap();
    assert!(
        !out.warnings
            .iter()
            .any(|w| w.code.as_deref() == Some("parse::comments_in_nested_yaml_dropped")),
        "no dropped-comment warning expected; nested comments are now preserved"
    );

    let emitted = out.document.to_markdown();
    assert!(
        emitted.contains("# before-first"),
        "leading nested comment must round-trip\nGot:\n{}",
        emitted
    );
    assert!(
        emitted.contains("# between"),
        "between-items nested comment must round-trip\nGot:\n{}",
        emitted
    );
    assert!(
        emitted.contains("# after-last"),
        "trailing nested comment must round-trip\nGot:\n{}",
        emitted
    );

    // Round-trip is idempotent across repeated parse/emit cycles.
    let doc2 = Document::parse(&emitted).unwrap().document;
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// Comments inside nested mappings round-trip at the matching position.
#[test]
fn nested_mapping_comments_round_trip() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nouter:\n  # leading\n  inner: 1\n  # trailing\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("# leading"),
        "leading nested mapping comment must round-trip\nGot:\n{}",
        emitted
    );
    assert!(
        emitted.contains("# trailing"),
        "trailing nested mapping comment must round-trip\nGot:\n{}",
        emitted
    );

    // Re-parse and idempotency.
    let doc2 = Document::parse(&emitted).unwrap().document;
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2);
}

/// Trailing inline comments on nested sequence items round-trip inline.
#[test]
fn nested_sequence_inline_comments_round_trip() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nitems:\n  - a # inline\n  - b\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("- a # inline"),
        "trailing inline comment on a sequence item must round-trip on the same line\nGot:\n{}",
        emitted
    );

    // Idempotent across repeated round-trips.
    let doc2 = Document::parse(&emitted).unwrap().document;
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// Trailing inline comments on nested mapping fields round-trip inline.
#[test]
fn nested_mapping_inline_comments_round_trip() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nouter:\n  inner: 1 # tail\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("inner: 1 # tail"),
        "trailing inline comment on a nested mapping field must round-trip\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// Inline comment on a container key (`outer: # tail`) lands on the key
/// line, before the indented children.
#[test]
fn inline_on_container_key_round_trips() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nouter: # describes outer\n  inner: 1\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("outer: # describes outer\n  inner: 1"),
        "inline comment on a container key must land on the key line\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// An own-line comment in the root payload — directly below the `$quill`
/// metadata header — round-trips at its source position.
#[test]
fn root_payload_comment_round_trips() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n# main entry\ntitle: Hi\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();
    assert!(
        emitted.starts_with("~~~\n$quill: q\n$kind: main\n# main entry\n"),
        "own-line comment below the `$quill` header must round-trip\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// An own-line comment in a card payload — directly below the `$kind`
/// metadata header — round-trips at its source position.
#[test]
fn card_payload_comment_round_trips() {
    let src =
        "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: foo\n# the foo card\nx: 1\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("~~~\n$kind: foo\n# the foo card\n"),
        "own-line comment below the `$kind` header must round-trip\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// Inline comment with `!must_fill` round-trips with the tag intact.
#[test]
fn fill_with_inline_comment_round_trips() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\ndept: !must_fill Sales # placeholder\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    assert!(
        doc.main().payload().is_fill("dept"),
        "fill marker must be set"
    );

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("dept: !must_fill Sales # placeholder"),
        "`!must_fill` and inline comment must round-trip together\nGot:\n{}",
        emitted
    );

    let doc2 = Document::parse(&emitted).unwrap().document;
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// Multiple inline comments — top-level scalar, nested scalar, sequence
/// item — all preserved in one document.
#[test]
fn mixed_inline_comments_round_trip() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\ntitle: Hello # greeting\nitems:\n  - a # first\n  - b\nouter:\n  inner: 1 # nested tail\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();

    assert!(emitted.contains("title: Hello # greeting"));
    assert!(emitted.contains("- a # first"));
    assert!(emitted.contains("inner: 1 # nested tail"));

    let doc2 = Document::parse(&emitted).unwrap().document;
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// Orphan inline comment whose host is removed via `Payload::remove`
/// degrades to an own-line comment instead of being silently dropped.
#[test]
fn orphan_inline_after_remove_degrades_to_own_line() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nfield: value # tail\nother: 2\n~~~\n";

    let mut doc = Document::parse(src).unwrap().document;
    // Remove the host field. The inline comment is now orphaned in items.
    doc.main_mut().payload_mut().remove("field");

    let emitted = doc.to_markdown();
    // Comment text preserved as own-line.
    assert!(
        emitted.contains("# tail"),
        "orphan comment text must be preserved\nGot:\n{}",
        emitted
    );
    // It must NOT have ended up inline on any value line.
    assert!(
        !emitted.contains("\" # tail"),
        "orphan comment must not appear inline on another line\nGot:\n{}",
        emitted
    );

    // Re-parsing the emitted form yields a stable round-trip.
    let doc2 = Document::parse(&emitted).unwrap().document;
    let emitted2 = doc2.to_markdown();
    assert_eq!(
        emitted, emitted2,
        "post-orphan round-trip must be idempotent"
    );
}

/// Inline comment on an empty-mapping field — the field is omitted on emit
/// per the canonical-emission rule, but the inline trailer survives as an
/// own-line comment at the same indent so its text is not lost.
#[test]
fn inline_on_empty_mapping_degrades_to_own_line() {
    use crate::QuillValue;

    // Construct programmatically since `key: {}` doesn't appear in source.
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n";
    let mut doc = Document::parse(src).unwrap().document;
    doc.main_mut()
        .payload_mut()
        .insert("empty", QuillValue::from_json(serde_json::json!({})));
    // Append an inline comment item right after the empty-mapping field.
    {
        let fm = doc.main_mut().payload_mut();
        let items = fm.items().to_vec();
        let mut new_items = items;
        new_items.push(crate::PayloadItem::comment_inline("notes about empty"));
        *fm = crate::document::Payload::from_items(new_items);
    }

    let emitted = doc.to_markdown();
    // Empty-mapping host is omitted. Trailer surfaces as own-line.
    assert!(
        !emitted.contains("empty:"),
        "empty mapping must be omitted\nGot:\n{}",
        emitted
    );
    assert!(
        emitted.contains("# notes about empty"),
        "inline trailer for an omitted host must degrade to own-line\nGot:\n{}",
        emitted
    );
}

/// Mixed: own-line and inline comments referencing the same field.
#[test]
fn own_line_then_inline_round_trip() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\n# header\ntitle: Hi # tail\n# footer\n~~~\n";

    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();

    assert!(emitted.contains("# header\n"));
    assert!(emitted.contains("title: Hi # tail\n"));
    assert!(emitted.contains("# footer\n"));

    let doc2 = Document::parse(&emitted).unwrap().document;
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// A `!must_fill` marker on the *first* key of a sequence-item mapping
/// (`- key: !must_fill`) sits on the dash line, which Case 4 never sees.
/// Prescan inspects it inline so the marker survives parse and round-trips.
#[test]
fn seq_item_inline_first_key_fill_round_trips() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nrecipients:\n  - name: !must_fill\n    role: lead\n~~~\n\nBody.\n";
    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("- name: !must_fill"),
        "first-key fill marker must survive round-trip\nGot:\n{}",
        emitted
    );
    // Non-fill sibling keys are unaffected.
    assert!(emitted.contains("role: lead"), "Got:\n{}", emitted);
    let emitted2 = Document::parse(&emitted).unwrap().document.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// A `!must_fill` marker on an own-line (non-first) key inside an array element
/// survives both the markdown round-trip and the storage (serde) round-trip,
/// and a second emit cycle is idempotent.
#[test]
fn array_element_nested_fill_survives_markdown_and_storage() {
    let src = "~~~card-yaml\n$quill: q@0.1\n$kind: main\nrecipients:\n  - name: Alice\n    org: !must_fill\n~~~\n\nBody.\n";
    let doc = Document::parse(src).unwrap().document;
    let emitted = doc.to_markdown();
    assert!(emitted.contains("org: !must_fill"), "Got:\n{}", emitted);
    assert!(emitted.contains("name: Alice"), "Got:\n{}", emitted);

    // Storage (serde) round-trip preserves the nested marker on recipients[0].org.
    let restored: Document = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert_eq!(
        doc, restored,
        "nested array-element fill must survive storage"
    );
    assert_eq!(
        emitted,
        restored.to_markdown(),
        "markdown must be identical after a storage round-trip"
    );
}
