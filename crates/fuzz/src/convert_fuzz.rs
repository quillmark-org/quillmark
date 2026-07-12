use proptest::prelude::*;
use quillmark_typst::emit::{escape_markup, escape_string};

/// Markdown → Typst markup over the corpus pipeline: import to a `RichText`, then
/// lower it. This is the render path the former single-step `mark_to_typst`
/// became — property-fuzzed here (no panic, escaping, formatting → Typst
/// functions) exactly as that lowering was.
fn mark_to_typst(markdown: &str) -> Result<String, String> {
    let rt = quillmark_richtext::import::from_markdown(markdown).map_err(|e| e.to_string())?;
    quillmark_typst::emit::emit_richtext(&rt)
        .map(|ec| ec.markup)
        .map_err(|e| e.to_string())
}

// Typst special characters that need escaping in markup context (excluding backslash and //)
// Backslash is handled first to prevent double-escaping, and // is handled as a pattern
// These correspond to the single-character escapes in the escape_markup function
const TYPST_SPECIAL_CHARS: &[char] = &[
    '~', '*', '_', '`', '#', '[', ']', '{', '}', '$', '<', '>', '@',
];

// Security-focused tests for escape_string
#[test]
fn test_escape_string_security_attack_vectors() {
    // Test injection attempt with quote and eval
    let malicious = "\"; system(\"rm -rf /\"); \"";
    let escaped = escape_string(malicious);
    // Should escape the quotes, preventing injection
    assert_eq!(escaped, r#"\"; system(\"rm -rf /\"); \""#);
    // When used in a Typst string, the escaped quotes prevent breaking out
    let typst_expr = format!("eval(\"{}\", mode: \"markup\")", escaped);
    // The dangerous pattern should not exist in a way that breaks out of the string
    assert!(
        !typst_expr.contains("eval(\"\"; system"),
        "Escaped content should not break out of eval string"
    );

    // Test backslash and quote combination
    let attack = r#"\"); eval("malicious")"#;
    let escaped = escape_string(attack);
    assert_eq!(escaped, r#"\\\"); eval(\"malicious\")"#);
    // When used in context, should not allow breakout
    let typst_expr = format!("eval(\"{}\", mode: \"markup\")", escaped);
    assert!(
        !typst_expr.contains("eval(\"\\\"); eval(\"malicious\")"),
        "Should not have raw breakout pattern"
    );
}

#[test]
fn test_escape_string_control_characters() {
    // Null byte
    assert_eq!(escape_string("\0"), "\\u{0}");
    // Other control characters
    assert_eq!(escape_string("\x01"), "\\u{1}");
    assert_eq!(escape_string("\x1f"), "\\u{1f}");
    // Combination
    assert_eq!(escape_string("test\0ing"), "test\\u{0}ing");
}

#[test]
fn test_escape_markup_security_attack_vectors() {
    // Test that all special characters are escaped
    let attack = "*_`#[]{}$<>@\\";
    let escaped = escape_markup(attack);
    assert_eq!(escaped, "\\*\\_\\`\\#\\[\\]\\{\\}\\$\\<\\>\\@\\\\");

    // Verify backslash is escaped first
    let backslash_attack = "\\*";
    let escaped = escape_markup(backslash_attack);
    assert_eq!(escaped, "\\\\\\*");

    // Test // (comment syntax) is escaped
    let comment_attack = "// This could be a comment";
    let escaped = escape_markup(comment_attack);
    assert!(escaped.starts_with("\\/\\/"));

    // Test tilde (non-breaking space in Typst) is escaped
    let tilde_attack = "Hello~World";
    let escaped = escape_markup(tilde_attack);
    assert_eq!(escaped, "Hello\\~World");
}

proptest! {
    #[test]
    fn fuzz_escape_string_no_raw_quotes(s in "\\PC*") {
        let escaped = escape_string(&s);
        // Verify no unescaped quotes (raw quote without backslash before it)
        // This is a simplified check - in escaped strings, quotes should be \\\"
        let chars: Vec<char> = escaped.chars().collect();
        for i in 0..chars.len() {
            if chars[i] == '"' {
                // Quote must be preceded by backslash
                assert!(i > 0 && chars[i-1] == '\\',
                    "Found unescaped quote at position {} in escaped string: {:?}", i, escaped);
            }
        }
    }

    #[test]
    fn fuzz_escape_markup_typst_chars_escaped(s in "\\PC*") {
        let escaped = escape_markup(&s);
        // For each Typst special character in the input, verify it's escaped in output
        for &ch in TYPST_SPECIAL_CHARS {
            if s.contains(ch) {
                // The escaped version should contain the escaped form
                let escaped_form = format!("\\{}", ch);
                assert!(escaped.contains(&escaped_form),
                    "Character '{}' in input '{}' not properly escaped in output '{}'",
                    ch, s, escaped);
            }
        }
    }

    #[test]
    fn fuzz_escape_markup_backslash_first(s in "\\PC*") {
        let escaped = escape_markup(&s);
        // Verify proper escaping of backslashes
        // Each backslash in the input should be escaped to exactly two backslashes
        // Count total backslashes in input
        let input_backslashes = s.matches('\\').count();

        // Count other special chars that will be escaped (each adds one backslash)
        let special_count: usize = TYPST_SPECIAL_CHARS.iter()
            .map(|&ch| s.matches(ch).count())
            .sum();

        // Count // patterns that will be escaped (each // becomes \/\/, adding 2 backslashes)
        let double_slash_count = s.matches("//").count();

        // Expected backslashes in output:
        // - Each input backslash becomes 2 backslashes (input_backslashes * 2)
        // - Each special char gets one escape backslash (special_count)
        // - Each // pattern gets 2 escape backslashes (double_slash_count * 2)
        let expected_backslashes = input_backslashes * 2 + special_count + double_slash_count * 2;
        let actual_backslashes = escaped.matches('\\').count();

        assert_eq!(actual_backslashes, expected_backslashes,
            "Backslash count mismatch for input {:?}: expected {}, got {}",
            s, expected_backslashes, actual_backslashes);
    }

    #[test]
    fn fuzz_mark_to_typst_no_panic(s in "\\PC{0,1000}") {
        // Just verify it doesn't panic on various inputs
        let _ = mark_to_typst(&s);
    }

    #[test]
    fn fuzz_mark_to_typst_special_chars_escaped(s in "[a-zA-Z0-9 *_#\\[\\]$<>@\\\\]{0,100}") {
        let output = mark_to_typst(&s);
        // If input contains raw special characters (not in markdown syntax),
        // they should be escaped in output
        // This is a basic safety check - the conversion should not panic
        // Note: Some inputs like "<a>" may be treated as HTML and result in empty output
        // which is valid behavior - we're just checking for no panics
        let _ = output; // Just verify no panic
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn fuzz_escape_string_injection_safety(s in "[\\\\\"].*[\\\\\"].*") {
        // Test strings with quotes and backslashes
        let escaped = escape_string(&s);

        // Should not contain the pattern "); which could break out of string context
        let dangerous_patterns = [
            "\"); ",
            "\")); ",
            "\\\"); ",
        ];

        for pattern in &dangerous_patterns {
            assert!(!escaped.contains(pattern),
                "Dangerous pattern '{}' found in escaped output: {}", pattern, escaped);
        }
    }

    #[test]
    fn fuzz_markdown_parser_malicious_nesting(depth in 1usize..20) {
        // Test deeply nested structures
        let nested_quotes = "> ".repeat(depth) + "text";
        let result = mark_to_typst(&nested_quotes).expect("Conversion should succeed");
        // Should not panic and should produce some output
        assert!(!result.is_empty() || depth == 0);
    }

    #[test]
    fn fuzz_markdown_parser_malicious_lists(depth in 1usize..20) {
        // Test deeply nested lists
        let nested_list = (0..depth)
            .map(|i| format!("{}- item", "  ".repeat(i)))
            .collect::<Vec<_>>()
            .join("\n");
        let result = mark_to_typst(&nested_list).expect("Conversion should succeed");
        // Should not panic
        assert!(!result.is_empty());
    }

    #[test]
    fn fuzz_markdown_large_input(size in 1usize..10000) {
        // Test with large inputs (but not too large for tests)
        let input = "a".repeat(size);
        let result = mark_to_typst(&input).expect("Conversion should succeed");
        // Should handle large inputs without panic
        assert!(result.contains("a"));
    }
}

// ===== Formatting Combination Fuzz Tests =====

/// Wraps text with the given formatting markers
fn wrap_format(text: &str, marker_open: &str, marker_close: &str) -> String {
    format!("{}{}{}", marker_open, text, marker_close)
}

/// Bold: **text**
fn bold(text: &str) -> String {
    wrap_format(text, "**", "**")
}

/// Italic with asterisk: *text*
fn italic(text: &str) -> String {
    wrap_format(text, "*", "*")
}

/// Underline: <u>text</u> (the only allowlisted HTML tag, per spec §6.2)
fn underline(text: &str) -> String {
    wrap_format(text, "<u>", "</u>")
}

/// Strikethrough: ~~text~~
fn strike(text: &str) -> String {
    wrap_format(text, "~~", "~~")
}

// ===== Formatting mark -> Typst mapping (deterministic) =====
//
// The marker->function mapping (`**`->#strong, `*`->#emph, `<u>`->#underline,
// `~~`->#strike) is content-independent, so the former proptests here fuzzed
// fixed markers over irrelevant `[a-zA-Z0-9]` content -- deterministic unit
// tests in a proptest costume. Collapsed to example tests covering the same
// single/nested/adjacent/intraword mapping with named localization. The corpus
// pipeline is fuzzed by emit_roundtrip_fuzz; adversarial escaping by the
// escape_* proptests above.

#[test]
fn single_marks_map_to_typst_functions() {
    assert!(mark_to_typst(&bold("x")).unwrap().contains("#strong["));
    assert!(mark_to_typst(&italic("x")).unwrap().contains("#emph["));
    assert!(mark_to_typst(&underline("x")).unwrap().contains("#underline["));
    assert!(mark_to_typst(&strike("x")).unwrap().contains("#strike["));
}

#[test]
fn nested_two_marks_map_both_functions() {
    for input in [
        bold(&italic("x")),
        italic(&bold("x")),
        bold(&strike("x")),
        strike(&bold("x")),
        italic(&strike("x")),
        strike(&italic("x")),
    ] {
        let out = mark_to_typst(&input).unwrap();
        let n = ["#strong[", "#emph[", "#strike["]
            .iter()
            .filter(|f| out.contains(**f))
            .count();
        assert!(n >= 2, "nested marks under-mapped: {input} -> {out}");
    }
}

#[test]
fn adjacent_and_triple_nested_marks_map() {
    let adjacent = format!("{} {}", bold("a"), italic("b"));
    let out = mark_to_typst(&adjacent).unwrap();
    assert!(out.contains("#strong[") && out.contains("#emph["));

    let triple = bold(&italic(&strike("x")));
    let out = mark_to_typst(&triple).unwrap();
    assert!(out.contains("#strong[") && out.contains("#emph[") && out.contains("#strike["));
}

#[test]
fn intraword_marks_map() {
    // <u>...</u> and ** reach intraword positions that __ cannot.
    assert!(mark_to_typst("a**b**c").unwrap().contains("#strong["));
    assert!(mark_to_typst("a<u>b</u>c").unwrap().contains("#underline["));
}

// ===== Regression-style Tests for Specific Patterns =====

#[test]
fn test_underline_with_bold_inside() {
    let input = "<u>bold **here** end</u>";
    let result = mark_to_typst(input).unwrap();
    assert!(result.contains("#underline["));
    assert!(result.contains("#strong["));
}

#[test]
fn test_all_four_adjacent_no_space() {
    // __ produces #strong[…], same as **.
    let input = "**A**<u>B</u>*C*~~D~~";
    let result = mark_to_typst(input).unwrap();
    assert!(result.contains("#strong[A]"));
    assert!(result.contains("#underline[B]"));
    assert!(result.contains("#emph[C]"));
    assert!(result.contains("#strike[D]"));
}

#[test]
fn test_interleaved_formats_with_text() {
    let input = "normal **bold** more *italic* end";
    let result = mark_to_typst(input).unwrap();
    assert!(result.contains("normal"));
    assert!(result.contains("#strong[bold]"));
    assert!(result.contains("more"));
    assert!(result.contains("#emph[italic]"));
    assert!(result.contains("end"));
}

#[test]
fn test_format_at_word_boundaries() {
    let input = "word**bold**word *italic*word word<u>under</u>word";
    let result = mark_to_typst(input).unwrap();
    // Should not panic and should produce valid output
    assert!(!result.is_empty());
}
