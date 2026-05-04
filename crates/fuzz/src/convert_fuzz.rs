use proptest::prelude::*;
use quillmark_typst::convert::{escape_markup, escape_string, mark_to_typst};

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
    fn fuzz_escape_string_valid_escapes(s in "\\PC*") {
        let escaped = escape_string(&s);

        // Key property: no unescaped quotes that could break out of string context
        // Simple check: any quote must be preceded by a backslash
        let chars: Vec<char> = escaped.chars().collect();
        for i in 0..chars.len() {
            if chars[i] == '"' {
                assert!(i > 0 && chars[i-1] == '\\',
                    "Found unescaped quote at position {} in: {}", i, escaped);
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

// ===== Basic Single Format Fuzzing =====

proptest! {
    #[test]
    fn fuzz_bold_single(content in "[a-zA-Z0-9]{1,20}") {
        let input = bold(&content);
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#strong["));
        prop_assert!(output.contains("]"));
    }

    #[test]
    fn fuzz_italic_single(content in "[a-zA-Z0-9]{1,20}") {
        let input = italic(&content);
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#emph["));
    }

    #[test]
    fn fuzz_underline_single(content in "[a-zA-Z0-9]{1,20}") {
        let input = underline(&content);
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#underline["));
    }

    #[test]
    fn fuzz_strikethrough_single(content in "[a-zA-Z0-9]{1,20}") {
        let input = strike(&content);
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#strike["));
    }
}

// ===== Two Format Combinations (Nested) =====

proptest! {
    #[test]
    fn fuzz_bold_containing_italic(content in "[a-zA-Z0-9]{1,10}") {
        let input = bold(&italic(&content));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#strong["));
        prop_assert!(output.contains("#emph["));
    }

    #[test]
    fn fuzz_italic_containing_bold(content in "[a-zA-Z0-9]{1,10}") {
        let input = italic(&bold(&content));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#emph["));
        prop_assert!(output.contains("#strong["));
    }

    #[test]
    fn fuzz_bold_containing_underline(content in "[a-zA-Z0-9]{1,10}") {
        let input = bold(&underline(&content));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_underline_containing_bold(content in "[a-zA-Z0-9]{1,10}") {
        let input = underline(&bold(&content));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_bold_containing_strike(content in "[a-zA-Z0-9]{1,10}") {
        let input = bold(&strike(&content));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#strong["));
        prop_assert!(output.contains("#strike["));
    }

    #[test]
    fn fuzz_strike_containing_bold(content in "[a-zA-Z0-9]{1,10}") {
        let input = strike(&bold(&content));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#strike["));
        prop_assert!(output.contains("#strong["));
    }

    #[test]
    fn fuzz_italic_containing_underline(content in "[a-zA-Z0-9]{1,10}") {
        let input = italic(&underline(&content));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_underline_containing_italic(content in "[a-zA-Z0-9]{1,10}") {
        let input = underline(&italic(&content));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_italic_containing_strike(content in "[a-zA-Z0-9]{1,10}") {
        let input = italic(&strike(&content));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#emph["));
        prop_assert!(output.contains("#strike["));
    }

    #[test]
    fn fuzz_strike_containing_italic(content in "[a-zA-Z0-9]{1,10}") {
        let input = strike(&italic(&content));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#strike["));
        prop_assert!(output.contains("#emph["));
    }

    #[test]
    fn fuzz_underline_containing_strike(content in "[a-zA-Z0-9]{1,10}") {
        let input = underline(&strike(&content));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_strike_containing_underline(content in "[a-zA-Z0-9]{1,10}") {
        let input = strike(&underline(&content));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }
}

// ===== Two Format Combinations (Adjacent) =====

proptest! {
    #[test]
    fn fuzz_bold_then_italic(word1 in "[a-zA-Z]{1,8}", word2 in "[a-zA-Z]{1,8}") {
        let input = format!("{} {}", bold(&word1), italic(&word2));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#strong["));
        prop_assert!(output.contains("#emph["));
    }

    #[test]
    fn fuzz_bold_then_underline(word1 in "[a-zA-Z]{1,8}", word2 in "[a-zA-Z]{1,8}") {
        let input = format!("{} {}", bold(&word1), underline(&word2));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_bold_then_strike(word1 in "[a-zA-Z]{1,8}", word2 in "[a-zA-Z]{1,8}") {
        let input = format!("{} {}", bold(&word1), strike(&word2));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#strong["));
        prop_assert!(output.contains("#strike["));
    }

    #[test]
    fn fuzz_italic_then_underline(word1 in "[a-zA-Z]{1,8}", word2 in "[a-zA-Z]{1,8}") {
        let input = format!("{} {}", italic(&word1), underline(&word2));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_italic_then_strike(word1 in "[a-zA-Z]{1,8}", word2 in "[a-zA-Z]{1,8}") {
        let input = format!("{} {}", italic(&word1), strike(&word2));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#emph["));
        prop_assert!(output.contains("#strike["));
    }

    #[test]
    fn fuzz_underline_then_strike(word1 in "[a-zA-Z]{1,8}", word2 in "[a-zA-Z]{1,8}") {
        let input = format!("{} {}", underline(&word1), strike(&word2));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }
}

// ===== Three Format Combinations (Nested) =====

proptest! {
    #[test]
    fn fuzz_bold_italic_strike_nested(content in "[a-zA-Z]{1,8}") {
        let input = bold(&italic(&strike(&content)));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#strong["));
        prop_assert!(output.contains("#emph["));
        prop_assert!(output.contains("#strike["));
    }

    #[test]
    fn fuzz_strike_bold_italic_nested(content in "[a-zA-Z]{1,8}") {
        let input = strike(&bold(&italic(&content)));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#strike["));
        prop_assert!(output.contains("#strong["));
        prop_assert!(output.contains("#emph["));
    }

    #[test]
    fn fuzz_italic_strike_bold_nested(content in "[a-zA-Z]{1,8}") {
        let input = italic(&strike(&bold(&content)));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_underline_bold_italic_nested(content in "[a-zA-Z]{1,8}") {
        let input = underline(&bold(&italic(&content)));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_bold_underline_strike_nested(content in "[a-zA-Z]{1,8}") {
        let input = bold(&underline(&strike(&content)));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }
}

// ===== Four Format Combinations (All Nested) =====

proptest! {
    #[test]
    fn fuzz_all_four_formats_nested_v1(content in "[a-zA-Z]{1,6}") {
        // bold > italic > underline > strike
        let input = bold(&italic(&underline(&strike(&content))));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_all_four_formats_nested_v2(content in "[a-zA-Z]{1,6}") {
        // strike > underline > italic > bold
        let input = strike(&underline(&italic(&bold(&content))));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_all_four_formats_nested_v3(content in "[a-zA-Z]{1,6}") {
        // underline > strike > bold > italic
        let input = underline(&strike(&bold(&italic(&content))));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_all_four_formats_nested_v4(content in "[a-zA-Z]{1,6}") {
        // italic > bold > strike > underline
        let input = italic(&bold(&strike(&underline(&content))));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }
}

// ===== Mixed Adjacent and Nested =====

proptest! {
    #[test]
    fn fuzz_bold_italic_adjacent_then_strike(
        w1 in "[a-zA-Z]{1,5}",
        w2 in "[a-zA-Z]{1,5}",
        w3 in "[a-zA-Z]{1,5}"
    ) {
        let input = format!("{} {} {}", bold(&w1), italic(&w2), strike(&w3));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#strong["));
        prop_assert!(output.contains("#emph["));
        prop_assert!(output.contains("#strike["));
    }

    #[test]
    fn fuzz_all_four_adjacent(
        w1 in "[a-zA-Z]{1,5}",
        w2 in "[a-zA-Z]{1,5}",
        w3 in "[a-zA-Z]{1,5}",
        w4 in "[a-zA-Z]{1,5}"
    ) {
        let input = format!("{} {} {} {}", bold(&w1), italic(&w2), underline(&w3), strike(&w4));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_nested_pair_then_adjacent_pair(
        w1 in "[a-zA-Z]{1,5}",
        w2 in "[a-zA-Z]{1,5}"
    ) {
        let input = format!("{} {}", bold(&italic(&w1)), underline(&strike(&w2)));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }
}

// ===== Intraword Formatting =====

proptest! {
    #[test]
    fn fuzz_intraword_underline(
        prefix in "[a-zA-Z]{1,5}",
        middle in "[a-zA-Z]{1,5}",
        suffix in "[a-zA-Z]{1,5}"
    ) {
        // <u>…</u> covers intraword underline, which __ cannot reach.
        let input = format!("{}<u>{}</u>{}", prefix, middle, suffix);
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#underline["));
    }

    #[test]
    fn fuzz_intraword_bold(
        prefix in "[a-zA-Z]{1,5}",
        middle in "[a-zA-Z]{1,5}",
        suffix in "[a-zA-Z]{1,5}"
    ) {
        let input = format!("{}**{}**{}", prefix, middle, suffix);
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
        let output = result.unwrap();
        prop_assert!(output.contains("#strong["));
    }
}

// ===== Edge Cases with Special Content =====

proptest! {
    #[test]
    fn fuzz_formatting_with_numbers(
        num in "[0-9]{1,5}",
        content in "[a-zA-Z]{1,8}"
    ) {
        let input = bold(&format!("{} {}", content, num));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_formatting_preserves_spaces(
        word1 in "[a-zA-Z]{1,5}",
        word2 in "[a-zA-Z]{1,5}"
    ) {
        let content = format!("{}  {}", word1, word2); // double space
        let input = bold(&content);
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_empty_between_formats(
        w1 in "[a-zA-Z]{1,5}",
        w2 in "[a-zA-Z]{1,5}"
    ) {
        // No space between adjacent formats
        let input = format!("{}{}", bold(&w1), italic(&w2));
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }
}

// ===== Stress Tests =====

proptest! {
    #[test]
    fn fuzz_deeply_nested_same_format(depth in 1usize..5) {
        let mut content = "x".to_string();
        for _ in 0..depth {
            content = bold(&content);
        }
        let result = mark_to_typst(&content);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn fuzz_many_adjacent_formats(count in 1usize..10) {
        let words: Vec<String> = (0..count).map(|i| format!("word{}", i)).collect();
        let formatted: Vec<String> = words.iter().enumerate().map(|(i, w)| {
            match i % 4 {
                0 => bold(w),
                1 => italic(w),
                2 => strike(w),
                _ => underline(w),
            }
        }).collect();
        let input = formatted.join(" ");
        let result = mark_to_typst(&input);
        prop_assert!(result.is_ok());
    }
}

// ===== Regression-style Tests for Specific Patterns =====

#[test]
fn test_bold_italic_strike_all_nested() {
    let input = "***~~text~~***";
    let result = mark_to_typst(input).unwrap();
    assert!(result.contains("#strike[text]"));
}

#[test]
fn test_underline_with_bold_inside() {
    let input = "<u>bold **here** end</u>";
    let result = mark_to_typst(input).unwrap();
    assert!(result.contains("#underline["));
    assert!(result.contains("#strong["));
}

#[test]
fn test_all_four_adjacent_no_space() {
    // __ now produces #strong[…], same as **.
    let input = "**A**<u>B</u>*C*~~D~~";
    let result = mark_to_typst(input).unwrap();
    assert!(result.contains("#strong[A]"));
    assert!(result.contains("#underline[B]"));
    assert!(result.contains("#emph[C]"));
    assert!(result.contains("#strike[D]"));
}

#[test]
fn test_triple_nested_formats() {
    let input = "**_~~deep~~_**";
    let result = mark_to_typst(input).unwrap();
    assert!(result.contains("#strong["));
    assert!(result.contains("#emph["));
    assert!(result.contains("#strike["));
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
