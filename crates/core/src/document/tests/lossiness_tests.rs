//! Round-trip tests for comments, `!fill`, and custom tags.
//!
//! Both own-line and trailing inline YAML comments round-trip at their
//! source position. An inline comment on the frontmatter `QUILL: r # …`
//! sentinel line also round-trips. Comments whose host disappears at emit
//! time (empty-mapping omission, programmatic field removal) degrade to
//! own-line comments at the same indent so the comment text is preserved
//! even when its position shifts. `!fill` on scalars and sequences round-
//! trips, and string quoting is normalised to double-quoted (the type-
//! fidelity guarantee).

use crate::document::Document;

// ── Category: YAML comments ───────────────────────────────────────────────────

/// Top-level YAML comments survive a round-trip.
#[test]
fn top_level_comments_round_trip() {
    let src =
        "---\nQUILL: q\n# recipient's full name\nrecipient: Jane\nauthor: Alice\n---\n\nBody.\n";

    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();

    assert!(
        emitted.contains("# recipient's full name"),
        "top-level YAML comment must survive round-trip\nGot:\n{}",
        emitted
    );

    // Value remains intact.
    let doc2 = Document::from_markdown(&emitted).unwrap();
    assert_eq!(
        doc2.main()
            .frontmatter()
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
    let src = "---\nQUILL: q\ntitle: My Document # this is a comment\n---\n\nBody.\n";

    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();

    assert!(
        emitted.contains("title: \"My Document\" # this is a comment"),
        "trailing inline comment must round-trip on the same line\nGot:\n{}",
        emitted
    );
    assert!(
        !emitted.contains("\"My Document\"\n# this is a comment"),
        "trailing inline comment must NOT degrade to own-line\nGot:\n{}",
        emitted
    );

    // Value still intact.
    let doc2 = Document::from_markdown(&emitted).unwrap();
    assert_eq!(
        doc2.main()
            .frontmatter()
            .get("title")
            .and_then(|v| v.as_str()),
        Some("My Document"),
    );

    // Idempotent across repeated round-trips.
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

// ── Category: Custom tags ─────────────────────────────────────────────────────

/// `!fill` tags round-trip; other custom tags are rejected with a warning
/// and the tag is dropped.
#[test]
fn custom_tags_lose_tag_but_keep_value() {
    // `!fill` case: round-trip with fill preserved.
    let src = "---\nQUILL: q\nmemo_from: !fill 2d lt example\n---\n";
    let doc = Document::from_markdown(src).unwrap();

    let fm = doc.main().frontmatter();
    assert_eq!(
        fm.get("memo_from").and_then(|v| v.as_str()),
        Some("2d lt example"),
        "string value must survive tag parsing"
    );
    assert!(fm.is_fill("memo_from"), "fill marker must be recorded");

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("memo_from: !fill"),
        "`!fill` tag must round-trip\nGot:\n{}",
        emitted
    );

    let doc2 = Document::from_markdown(&emitted).unwrap();
    assert!(
        doc2.main().frontmatter().is_fill("memo_from"),
        "fill marker must survive a full round-trip"
    );

    // Non-`!fill` tag case: warning + dropped tag.
    let src2 = "---\nQUILL: q\nmemo_from: !include value.txt\n---\n";
    let out = Document::from_markdown_with_warnings(src2).unwrap();
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

/// `!fill` on a bare key (no value) emits `key: !fill` and preserves null.
#[test]
fn fill_tag_bare_null_round_trip() {
    let src = "---\nQUILL: q\nrecipient: !fill\n---\n";

    let doc = Document::from_markdown(src).unwrap();
    let fm = doc.main().frontmatter();

    assert!(fm.get("recipient").map(|v| v.is_null()).unwrap_or(false));
    assert!(fm.is_fill("recipient"));

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("recipient: !fill\n"),
        "bare `!fill` must round-trip as `key: !fill`\nGot:\n{}",
        emitted
    );
}

/// `!fill` on a top-level block sequence round-trips, preserving items and
/// the fill marker.
#[test]
fn fill_tag_block_sequence_round_trip() {
    let src = "---\nQUILL: q\nrecipient: !fill\n  - Dr. Who\n  - 1 TARDIS Lane\n---\n";

    let doc = Document::from_markdown(src).unwrap();
    let fm = doc.main().frontmatter();

    assert!(fm.is_fill("recipient"));
    let arr = fm.get("recipient").and_then(|v| v.as_array()).unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].as_str(), Some("Dr. Who"));
    assert_eq!(arr[1].as_str(), Some("1 TARDIS Lane"));

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("recipient: !fill\n"),
        "`!fill` on sequence must emit `key: !fill` before the block\nGot:\n{}",
        emitted
    );

    let doc2 = Document::from_markdown(&emitted).unwrap();
    assert!(doc2.main().frontmatter().is_fill("recipient"));
    assert_eq!(doc2, doc, "full round-trip must be equal");
}

/// `!fill` on a flow sequence round-trips (normalised to block form).
#[test]
fn fill_tag_flow_sequence_round_trip() {
    let src = "---\nQUILL: q\ntags: !fill [a, b, c]\n---\n";
    let doc = Document::from_markdown(src).unwrap();
    let fm = doc.main().frontmatter();
    assert!(fm.is_fill("tags"));
    assert_eq!(
        fm.get("tags").and_then(|v| v.as_array()).map(|a| a.len()),
        Some(3)
    );

    let emitted = doc.to_markdown();
    let doc2 = Document::from_markdown(&emitted).unwrap();
    assert!(doc2.main().frontmatter().is_fill("tags"));
    assert_eq!(doc2, doc);
}

/// `!fill` on an empty sequence round-trips as `key: !fill []`.
#[test]
fn fill_tag_empty_sequence_round_trip() {
    let src = "---\nQUILL: q\nitems: !fill []\n---\n";
    let doc = Document::from_markdown(src).unwrap();
    let fm = doc.main().frontmatter();
    assert!(fm.is_fill("items"));
    assert_eq!(
        fm.get("items").and_then(|v| v.as_array()).map(|a| a.len()),
        Some(0)
    );

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("items: !fill []\n"),
        "empty fill-sequence must round-trip as `key: !fill []`\nGot:\n{}",
        emitted
    );

    let doc2 = Document::from_markdown(&emitted).unwrap();
    assert_eq!(doc2, doc);
}

/// `!fill` on a top-level mapping is rejected at parse.
#[test]
fn fill_tag_mapping_rejected() {
    let src = "---\nQUILL: q\nx: !fill {a: 1}\n---\n";
    let err = Document::from_markdown(src).unwrap_err();
    assert!(
        err.to_string().contains("!fill") && err.to_string().contains("mapping"),
        "expected mapping-rejection error; got: {}",
        err
    );
}

/// `!fill` on every supported scalar type round-trips with the correct type.
#[test]
fn fill_tag_all_scalar_types_round_trip() {
    let src = concat!(
        "---\nQUILL: q\n",
        "s: !fill hello\n",
        "i: !fill 42\n",
        "f: !fill 3.14\n",
        "b: !fill true\n",
        "n: !fill\n",
        "---\n",
    );

    let doc = Document::from_markdown(src).unwrap();
    let fm = doc.main().frontmatter();

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
    let doc2 = Document::from_markdown(&emitted).unwrap();
    for key in ["s", "i", "f", "b", "n"] {
        assert!(
            doc2.main().frontmatter().is_fill(key),
            "{} must remain fill-tagged after round-trip",
            key
        );
    }
}

// ── Category: Original quoting style ─────────────────────────────────────────

/// The original quoting style (`'single-quoted'`, unquoted, block scalars)
/// is not preserved.  All strings are re-emitted double-quoted with JSON-style
/// escaping, regardless of how they were written in the source.
///
/// This is intentional: normalizing to double-quoted style is
/// what guarantees type fidelity for ambiguous strings like `on` and `01234`.
#[test]
fn original_quoting_style_is_not_preserved() {
    // Mix of single-quoted, unquoted, and double-quoted strings.
    let src = "---\nQUILL: q\nsingle_q: 'hello'\nunquoted: world\ndouble_q: \"already\"\n---\n";

    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();

    // Single-quoted must become double-quoted.
    assert!(
        !emitted.contains("'hello'"),
        "single-quoted string must not be re-emitted single-quoted\nGot:\n{}",
        emitted
    );
    assert!(
        emitted.contains("\"hello\""),
        "single-quoted string must be re-emitted double-quoted\nGot:\n{}",
        emitted
    );

    // Unquoted must become double-quoted.
    assert!(
        emitted.contains("\"world\""),
        "unquoted string must be re-emitted double-quoted\nGot:\n{}",
        emitted
    );

    // Already double-quoted is fine — stays double-quoted.
    assert!(
        emitted.contains("\"already\""),
        "double-quoted string must survive as double-quoted\nGot:\n{}",
        emitted
    );

    // Values must survive round-trip.
    let doc2 = Document::from_markdown(&emitted).unwrap();
    assert_eq!(
        doc2.main()
            .frontmatter()
            .get("single_q")
            .and_then(|v| v.as_str()),
        Some("hello")
    );
    assert_eq!(
        doc2.main()
            .frontmatter()
            .get("unquoted")
            .and_then(|v| v.as_str()),
        Some("world")
    );
    assert_eq!(
        doc2.main()
            .frontmatter()
            .get("double_q")
            .and_then(|v| v.as_str()),
        Some("already")
    );
}

// ── Category: Nested comments round-trip ─────────────────────────────────────

/// Comments inside nested sequences round-trip at the matching position.
#[test]
fn nested_sequence_comments_round_trip() {
    let src =
        "---\nQUILL: q\nitems:\n  # before-first\n  - a\n  # between\n  - b\n  # after-last\n---\n";

    let out = Document::from_markdown_with_warnings(src).unwrap();
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
    let doc2 = Document::from_markdown(&emitted).unwrap();
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// Comments inside nested mappings round-trip at the matching position.
#[test]
fn nested_mapping_comments_round_trip() {
    let src = "---\nQUILL: q\nouter:\n  # leading\n  inner: 1\n  # trailing\n---\n";

    let doc = Document::from_markdown(src).unwrap();
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
    let doc2 = Document::from_markdown(&emitted).unwrap();
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2);
}

/// Trailing inline comments on nested sequence items round-trip inline.
#[test]
fn nested_sequence_inline_comments_round_trip() {
    let src = "---\nQUILL: q\nitems:\n  - a # inline\n  - b\n---\n";

    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("- \"a\" # inline"),
        "trailing inline comment on a sequence item must round-trip on the same line\nGot:\n{}",
        emitted
    );

    // Idempotent across repeated round-trips.
    let doc2 = Document::from_markdown(&emitted).unwrap();
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// Trailing inline comments on nested mapping fields round-trip inline.
#[test]
fn nested_mapping_inline_comments_round_trip() {
    let src = "---\nQUILL: q\nouter:\n  inner: 1 # tail\n---\n";

    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("inner: 1 # tail"),
        "trailing inline comment on a nested mapping field must round-trip\nGot:\n{}",
        emitted
    );

    let doc2 = Document::from_markdown(&emitted).unwrap();
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// Inline comment on a container key (`outer: # tail`) lands on the key
/// line, before the indented children.
#[test]
fn inline_on_container_key_round_trips() {
    let src = "---\nQUILL: q\nouter: # describes outer\n  inner: 1\n---\n";

    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("outer: # describes outer\n  inner: 1"),
        "inline comment on a container key must land on the key line\nGot:\n{}",
        emitted
    );

    let doc2 = Document::from_markdown(&emitted).unwrap();
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// Inline comment on the QUILL sentinel line round-trips on that line.
#[test]
fn sentinel_inline_comment_round_trips() {
    let src = "---\nQUILL: q # main entry\ntitle: Hi\n---\n";

    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();
    assert!(
        emitted.starts_with("---\nQUILL: q # main entry\n"),
        "inline comment on the QUILL sentinel must round-trip on the sentinel line\nGot:\n{}",
        emitted
    );

    let doc2 = Document::from_markdown(&emitted).unwrap();
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// Inline comment on a leaf body field round-trips on that field's line.
/// (A leaf's kind lives in the info string, which cannot carry a comment.)
#[test]
fn leaf_field_inline_comment_round_trips() {
    let src = "---\nQUILL: q\n---\n\n```leaf foo\nx: 1 # the x field\n```\n";

    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("x: 1 # the x field\n"),
        "inline comment on a leaf field must round-trip on that line\nGot:\n{}",
        emitted
    );

    let doc2 = Document::from_markdown(&emitted).unwrap();
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// Inline comment with `!fill` round-trips with the tag intact.
#[test]
fn fill_with_inline_comment_round_trips() {
    let src = "---\nQUILL: q\ndept: !fill Sales # placeholder\n---\n";

    let doc = Document::from_markdown(src).unwrap();
    assert!(
        doc.main().frontmatter().is_fill("dept"),
        "fill marker must be set"
    );

    let emitted = doc.to_markdown();
    assert!(
        emitted.contains("dept: !fill \"Sales\" # placeholder"),
        "`!fill` and inline comment must round-trip together\nGot:\n{}",
        emitted
    );

    let doc2 = Document::from_markdown(&emitted).unwrap();
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// Multiple inline comments — top-level scalar, nested scalar, sequence
/// item — all preserved in one document.
#[test]
fn mixed_inline_comments_round_trip() {
    let src = "---\nQUILL: q\ntitle: Hello # greeting\nitems:\n  - a # first\n  - b\nouter:\n  inner: 1 # nested tail\n---\n";

    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();

    assert!(emitted.contains("title: \"Hello\" # greeting"));
    assert!(emitted.contains("- \"a\" # first"));
    assert!(emitted.contains("inner: 1 # nested tail"));

    let doc2 = Document::from_markdown(&emitted).unwrap();
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}

/// Orphan inline comment whose host is removed via `Frontmatter::remove`
/// degrades to an own-line comment instead of being silently dropped.
#[test]
fn orphan_inline_after_remove_degrades_to_own_line() {
    let src = "---\nQUILL: q\nfield: value # tail\nother: 2\n---\n";

    let mut doc = Document::from_markdown(src).unwrap();
    // Remove the host field. The inline comment is now orphaned in items.
    doc.main_mut().frontmatter_mut().remove("field");

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
    let doc2 = Document::from_markdown(&emitted).unwrap();
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
    let src = "---\nQUILL: q\n---\n";
    let mut doc = Document::from_markdown(src).unwrap();
    doc.main_mut()
        .frontmatter_mut()
        .insert("empty", QuillValue::from_json(serde_json::json!({})));
    // Append an inline comment item right after the empty-mapping field.
    {
        let fm = doc.main_mut().frontmatter_mut();
        let items = fm.items().to_vec();
        let mut new_items = items;
        new_items.push(crate::FrontmatterItem::comment_inline("notes about empty"));
        *fm = crate::document::Frontmatter::from_items(new_items);
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
    let src = "---\nQUILL: q\n# header\ntitle: Hi # tail\n# footer\n---\n";

    let doc = Document::from_markdown(src).unwrap();
    let emitted = doc.to_markdown();

    assert!(emitted.contains("# header\n"));
    assert!(emitted.contains("title: \"Hi\" # tail\n"));
    assert!(emitted.contains("# footer\n"));

    let doc2 = Document::from_markdown(&emitted).unwrap();
    let emitted2 = doc2.to_markdown();
    assert_eq!(emitted, emitted2, "round-trip must be idempotent");
}
