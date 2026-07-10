//! Markdown-string input normalization — the boundary preprocessing corpus
//! import runs before parsing. Converts line endings to `\n`, strips invisible
//! Unicode bidi controls (which sit adjacent to `**`/`_` and defeat delimiter
//! recognition), and repairs `<!-- ... -->` HTML-comment fences that would
//! otherwise swallow trailing text.
//!
//! The pure string primitive [`from_markdown`](crate::import::from_markdown)
//! applies at its boundary. It carries no dependency on the document engine, so
//! this crate is a leaf `quillmark-core` depends on.

#[inline]
pub(crate) fn is_bidi_char(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' // ARABIC LETTER MARK (ALM)
        | '\u{200E}' // LEFT-TO-RIGHT MARK (LRM)
        | '\u{200F}' // RIGHT-TO-LEFT MARK (RLM)
        | '\u{202A}' // LEFT-TO-RIGHT EMBEDDING (LRE)
        | '\u{202B}' // RIGHT-TO-LEFT EMBEDDING (RLE)
        | '\u{202C}' // POP DIRECTIONAL FORMATTING (PDF)
        | '\u{202D}' // LEFT-TO-RIGHT OVERRIDE (LRO)
        | '\u{202E}' // RIGHT-TO-LEFT OVERRIDE (RLO)
        | '\u{2066}' // LEFT-TO-RIGHT ISOLATE (LRI)
        | '\u{2067}' // RIGHT-TO-LEFT ISOLATE (RLI)
        | '\u{2068}' // FIRST STRONG ISOLATE (FSI)
        | '\u{2069}' // POP DIRECTIONAL ISOLATE (PDI)
    )
}

/// Strips Unicode bidirectional formatting characters that can interfere with markdown parsing.
///
/// Removes all of ALM (U+061C), LRM/RLM (U+200E/F), LRE/RLE/PDF/LRO/RLO
/// (U+202A–202E), and LRI/RLI/FSI/PDI (U+2066–2069).
pub fn strip_bidi_formatting(s: &str) -> String {
    if !s.chars().any(is_bidi_char) {
        return s.to_string();
    }

    s.chars().filter(|c| !is_bidi_char(*c)).collect()
}

/// Inserts a newline after `-->` when followed by non-whitespace content.
///
/// CommonMark HTML block type 2 ends with the line containing `-->`, so any
/// text on the same line after `-->` would be swallowed. This function is
/// context-aware: only closing fences inside a `<!-- ... -->` pair are fixed;
/// bare `-->` outside a comment is left untouched.
pub fn fix_html_comment_fences(s: &str) -> String {
    if !s.contains("-->") {
        return s.to_string();
    }

    let mut result = String::with_capacity(s.len() + 16);
    let mut current_pos = 0;

    while let Some(open_idx) = s[current_pos..].find("<!--") {
        let abs_open = current_pos + open_idx;

        if let Some(close_idx) = s[abs_open..].find("-->") {
            let abs_close = abs_open + close_idx;
            let mut after_fence = abs_close + 3;

            // Handle `<!--- ... --->` style fences: the extra hyphen is part of
            // the fence, not leaked trailing text.
            let opener_has_extra_hyphen = s
                .get(abs_open + 4..)
                .is_some_and(|rest| rest.starts_with('-'));
            if opener_has_extra_hyphen
                && s.get(after_fence..)
                    .is_some_and(|rest| rest.starts_with('-'))
            {
                after_fence += 1;
            }

            result.push_str(&s[current_pos..after_fence]);

            let after_content = &s[after_fence..];

            let needs_newline = if after_content.is_empty()
                || after_content.starts_with('\n')
                || after_content.starts_with("\r\n")
            {
                false
            } else {
                let next_newline = after_content.find('\n');
                let until_newline = match next_newline {
                    Some(pos) => &after_content[..pos],
                    None => after_content,
                };
                !until_newline.trim().is_empty()
            };

            if needs_newline {
                result.push('\n');
            }

            current_pos = after_fence;
        } else {
            // Unclosed comment — append the rest and stop.
            result.push_str(&s[current_pos..]);
            current_pos = s.len();
            break;
        }
    }

    if current_pos < s.len() {
        result.push_str(&s[current_pos..]);
    }

    result
}

/// Applies all markdown normalizations in order: CRLF → LF, bidi strip,
/// HTML comment fence repair.
pub fn normalize_markdown(markdown: &str) -> String {
    let cleaned = normalize_line_endings(markdown);
    let cleaned = strip_bidi_formatting(&cleaned);
    fix_html_comment_fences(&cleaned)
}

/// Convert CRLF (`\r\n`) and bare CR (`\r`) line endings to LF (`\n`).
///
/// Applied only to the Markdown body (spec §7); YAML scalars are unaffected.
/// Necessary because YAML parsing normalizes its own scalars but passes the
/// body verbatim, and some Windows/clipboard sources leave bare `\r` bytes.
fn normalize_line_endings(s: &str) -> String {
    if !s.contains('\r') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_bidi_no_change() {
        assert_eq!(strip_bidi_formatting("hello world"), "hello world");
        assert_eq!(strip_bidi_formatting(""), "");
        assert_eq!(strip_bidi_formatting("**bold** text"), "**bold** text");
    }

    #[test]
    fn test_strip_bidi_lro() {
        assert_eq!(strip_bidi_formatting("he\u{202D}llo"), "hello");
        assert_eq!(
            strip_bidi_formatting("**asdf** or \u{202D}**(1234**"),
            "**asdf** or **(1234**"
        );
    }

    #[test]
    fn test_strip_bidi_marks() {
        assert_eq!(strip_bidi_formatting("a\u{200E}b\u{200F}c"), "abc");
    }

    #[test]
    fn test_strip_bidi_embeddings() {
        assert_eq!(
            strip_bidi_formatting("\u{202A}text\u{202B}more\u{202C}"),
            "textmore"
        );
    }

    #[test]
    fn test_strip_bidi_isolates() {
        assert_eq!(
            strip_bidi_formatting("\u{2066}a\u{2067}b\u{2068}c\u{2069}"),
            "abc"
        );
    }

    #[test]
    fn test_strip_bidi_all_chars() {
        let all_bidi = "\u{061C}\u{200E}\u{200F}\u{202A}\u{202B}\u{202C}\u{202D}\u{202E}\u{2066}\u{2067}\u{2068}\u{2069}";
        assert_eq!(strip_bidi_formatting(all_bidi), "");
    }

    #[test]
    fn test_strip_bidi_arabic_letter_mark() {
        assert_eq!(strip_bidi_formatting("hello\u{061C}world"), "helloworld");
        assert_eq!(strip_bidi_formatting("\u{061C}**bold**"), "**bold**");
    }

    #[test]
    fn test_strip_bidi_unicode_preserved() {
        assert_eq!(strip_bidi_formatting("你好世界"), "你好世界");
        assert_eq!(strip_bidi_formatting("مرحبا"), "مرحبا");
        assert_eq!(strip_bidi_formatting("🎉"), "🎉");
    }

    #[test]
    fn test_normalize_markdown_basic() {
        assert_eq!(normalize_markdown("hello"), "hello");
        assert_eq!(
            normalize_markdown("**bold** \u{202D}**more**"),
            "**bold** **more**"
        );
    }

    #[test]
    fn test_normalize_markdown_html_comment() {
        assert_eq!(
            normalize_markdown("<!-- comment -->Some text"),
            "<!-- comment -->\nSome text"
        );
    }

    #[test]
    fn test_fix_html_comment_no_comment() {
        assert_eq!(fix_html_comment_fences("hello world"), "hello world");
        assert_eq!(fix_html_comment_fences("**bold** text"), "**bold** text");
        assert_eq!(fix_html_comment_fences(""), "");
    }

    #[test]
    fn test_fix_html_comment_single_line_trailing_text() {
        assert_eq!(
            fix_html_comment_fences("<!-- comment -->Same line text"),
            "<!-- comment -->\nSame line text"
        );
    }

    #[test]
    fn test_fix_html_comment_already_newline() {
        assert_eq!(
            fix_html_comment_fences("<!-- comment -->\nNext line text"),
            "<!-- comment -->\nNext line text"
        );
    }

    #[test]
    fn test_fix_html_comment_only_whitespace_after() {
        assert_eq!(
            fix_html_comment_fences("<!-- comment -->   \nSome text"),
            "<!-- comment -->   \nSome text"
        );
    }

    #[test]
    fn test_fix_html_comment_multiline_trailing_text() {
        assert_eq!(
            fix_html_comment_fences("<!--\nmultiline\ncomment\n-->Trailing text"),
            "<!--\nmultiline\ncomment\n-->\nTrailing text"
        );
    }

    #[test]
    fn test_fix_html_comment_multiline_proper() {
        assert_eq!(
            fix_html_comment_fences("<!--\nmultiline\n-->\n\nParagraph text"),
            "<!--\nmultiline\n-->\n\nParagraph text"
        );
    }

    #[test]
    fn test_fix_html_comment_multiple_comments() {
        assert_eq!(
            fix_html_comment_fences("<!-- first -->Text\n\n<!-- second -->More text"),
            "<!-- first -->\nText\n\n<!-- second -->\nMore text"
        );
    }

    #[test]
    fn test_fix_html_comment_end_of_string() {
        assert_eq!(
            fix_html_comment_fences("Some text before <!-- comment -->"),
            "Some text before <!-- comment -->"
        );
    }

    #[test]
    fn test_fix_html_comment_arrow_not_comment() {
        assert_eq!(fix_html_comment_fences("-->some text"), "-->some text");
    }

    #[test]
    fn test_fix_html_comment_nested_opener() {
        // The first <!-- opens, the first --> closes; inner <!-- is just text.
        assert_eq!(
            fix_html_comment_fences("<!-- <!-- -->Trailing"),
            "<!-- <!-- -->\nTrailing"
        );
    }

    #[test]
    fn test_fix_html_comment_multiple_valid_invalid() {
        let input = "<!-- valid -->FixMe\ntext --> Ignore\n<!-- valid2 -->FixMe2";
        let expected = "<!-- valid -->\nFixMe\ntext --> Ignore\n<!-- valid2 -->\nFixMe2";
        assert_eq!(fix_html_comment_fences(input), expected);
    }

    #[test]
    fn test_fix_html_comment_crlf() {
        assert_eq!(
            fix_html_comment_fences("<!-- comment -->\r\nSome text"),
            "<!-- comment -->\r\nSome text"
        );
    }

    #[test]
    fn test_fix_html_comment_triple_hyphen_single_line() {
        assert_eq!(
            fix_html_comment_fences("<!--- comment --->Trailing text"),
            "<!--- comment --->\nTrailing text"
        );
    }

}
