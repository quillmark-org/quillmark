//! Regression coverage for the "string index N is not a character boundary"
//! panic class.
//!
//! `Document::parse` must not panic on multibyte-character inputs.
//! The vulnerable input shape is a `markdown`-typed field containing a mix of:
//!
//! - en-dashes (`–`, U+2013, 3 bytes in UTF-8) used as bullet markers
//! - em-dashes (`—`, U+2014) in adjacent prose
//! - mixed bullet marker styles (`-`, `*`, `–`) inside a block scalar
//!
//! Each test must not panic on the fixed code path; add new repros here as
//! they surface.

use quillmark::Document;

fn assert_parses(input: &str) {
    match Document::parse(input) {
        Ok(_) => {}
        Err(e) => panic!("expected parse to succeed on multibyte input, got: {e}\ninput:\n{input}"),
    }
}

#[test]
fn em_and_en_dashes_in_block_scalar_bullets_parse_without_panic() {
    // The trigger pattern: a `bullets: |` block scalar whose body mixes
    // ASCII `-`, `*`, and en-dash `–` as bullet markers and contains an
    // em-dash in the prose.
    let md = "~~~card-yaml\n\
              $quill: q@0.1\n\
              $kind: main\n\
              ~~~\n\
              \n\
              INDOPACOM Intelligence Briefing \u{2014} 15 April 2026\n\
              \n\
              ~~~card-yaml\n\
              $kind: slide\n\
              bullets: |\n  \
                - (U) ASCII bullet content.\n  \
                * (U) Asterisk bullet content.\n  \
                \u{2013} (U) En-dash bullet content.\n\
              ~~~\n";
    assert_parses(md);
}

#[test]
fn multibyte_after_dash_marker_does_not_panic() {
    // Slicing a `- <content>` marker must respect char boundaries: a naive
    // `trimmed[2..]` lands mid-character when `<content>` starts with a
    // multibyte codepoint. Pin every known variant.
    let variants = [
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\narr:\n  - \u{2013} en-dash leads\n~~~\n",
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\narr:\n  - \u{2014} em-dash leads\n~~~\n",
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\narr:\n  - \u{201C}smart-quoted\u{201D}\n~~~\n",
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\narr:\n  - \u{1F600} emoji leads\n~~~\n",
    ];
    for v in variants {
        assert_parses(v);
    }
}

#[test]
fn multibyte_in_quoted_scalar_parses() {
    // Quoted scalars with multibyte content land in a different scanner
    // path than plain scalars. Cover both styles.
    let single = "~~~card-yaml\n$quill: q@0.1\n$kind: main\nbluf: '\u{2014}leading em-dash'\n~~~\n";
    let double =
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\nbluf: \"\u{201C}smart-quoted\u{201D}\"\n~~~\n";
    assert_parses(single);
    assert_parses(double);
}

#[test]
fn multibyte_keys_do_not_panic_on_duplicate() {
    // YAML error formatting can include the offending key in its caret
    // message. If the key contains multibyte chars, the caret renderer is
    // the suspect path.
    let md = "~~~card-yaml\n$quill: q@0.1\n$kind: main\nf\u{2014}o: 1\nf\u{2014}o: 2\n~~~\n";
    // We expect a duplicate-key parse error here, not a panic. The crucial
    // assertion is "did not panic"; the error is fine.
    let _ = Document::parse(md);
}

#[test]
fn multibyte_in_value_with_yaml_error_does_not_panic() {
    // The model writes a value with multibyte chars and a YAML structural
    // bug elsewhere on the same line — caret-positioning has to scan past
    // the multibyte chars.
    let inputs = [
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\nx: hello \u{2014} world\nbluf: *bad-alias\n~~~\n",
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\nsystem_name: \u{201C}Service\u{201D}: Order API\n~~~\n",
    ];
    for input in inputs {
        let _ = Document::parse(input);
    }
}
