//! # Input Normalization
//!
//! This module provides input normalization for markdown content before parsing.
//! Normalization ensures that invisible control characters and other artifacts
//! that can interfere with markdown parsing are handled consistently.
//!
//! ## Overview
//!
//! Input text may contain invisible Unicode characters (especially from copy-paste)
//! that interfere with markdown parsing. This module provides functions to:
//!
//! - Strip Unicode bidirectional formatting characters that break delimiter recognition
//! - Fix HTML comment fences to preserve trailing text
//! - Apply all normalizations in the correct order
//!
//! Double chevrons (`<<` and `>>`) are passed through unchanged without conversion.
//!
//! ## Functions
//!
//! - [`strip_bidi_formatting`] - Remove Unicode bidi control characters
//! - [`normalize_markdown`] - Apply all markdown-specific normalizations
//! - [`normalize_fields`] - Normalize document frontmatter fields (bidi stripping on body only)
//! - [`normalize_document`] - Normalize a typed [`crate::document::Document`] in-place
//!
//! ## Why Normalize?
//!
//! Unicode bidirectional formatting characters (LRO, RLO, LRE, RLE, etc.) are invisible
//! control characters used for bidirectional text layout. When placed adjacent to markdown
//! delimiters like `**`, they can prevent parsers from recognizing the delimiters:
//!
//! ```text
//! **bold** or <U+202D>**(1234**
//!             ^^^^^^^^ invisible LRO here prevents second ** from being recognized as bold
//! ```
//!
//! These characters commonly appear when copying text from:
//! - Web pages with mixed LTR/RTL content
//! - PDF documents
//! - Word processors
//! - Some clipboard managers
//!
//! ## Examples
//!
//! ```
//! use quillmark_core::normalize::strip_bidi_formatting;
//!
//! // Input with invisible U+202D (LRO) before second **
//! let input = "**asdf** or \u{202D}**(1234**";
//! let cleaned = strip_bidi_formatting(input);
//! assert_eq!(cleaned, "**asdf** or **(1234**");
//! ```

use crate::document::Leaf;
use crate::value::QuillValue;
use indexmap::IndexMap;
use unicode_normalization::UnicodeNormalization;

/// Errors that can occur during normalization
#[derive(Debug, thiserror::Error)]
pub enum NormalizationError {
    /// JSON nesting depth exceeded maximum allowed
    #[error("JSON nesting too deep: {depth} levels (max: {max} levels)")]
    NestingTooDeep {
        /// Actual depth
        depth: usize,
        /// Maximum allowed depth
        max: usize,
    },
}

/// Check if a character is a Unicode bidirectional formatting character
#[inline]
fn is_bidi_char(c: char) -> bool {
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
/// These invisible control characters are used for bidirectional text layout but can
/// break markdown delimiter recognition when placed adjacent to `**`, `*`, `_`, etc.
///
/// # Characters Stripped
///
/// - U+061C (ARABIC LETTER MARK, ALM)
/// - U+200E (LEFT-TO-RIGHT MARK, LRM)
/// - U+200F (RIGHT-TO-LEFT MARK, RLM)
/// - U+202A (LEFT-TO-RIGHT EMBEDDING, LRE)
/// - U+202B (RIGHT-TO-LEFT EMBEDDING, RLE)
/// - U+202C (POP DIRECTIONAL FORMATTING, PDF)
/// - U+202D (LEFT-TO-RIGHT OVERRIDE, LRO)
/// - U+202E (RIGHT-TO-LEFT OVERRIDE, RLO)
/// - U+2066 (LEFT-TO-RIGHT ISOLATE, LRI)
/// - U+2067 (RIGHT-TO-LEFT ISOLATE, RLI)
/// - U+2068 (FIRST STRONG ISOLATE, FSI)
/// - U+2069 (POP DIRECTIONAL ISOLATE, PDI)
///
/// # Examples
///
/// ```
/// use quillmark_core::normalize::strip_bidi_formatting;
///
/// // Normal text is unchanged
/// assert_eq!(strip_bidi_formatting("hello"), "hello");
///
/// // LRO character is stripped
/// assert_eq!(strip_bidi_formatting("he\u{202D}llo"), "hello");
///
/// // All bidi characters are stripped
/// let input = "\u{200E}\u{200F}\u{202A}\u{202B}\u{202C}\u{202D}\u{202E}";
/// assert_eq!(strip_bidi_formatting(input), "");
/// ```
pub fn strip_bidi_formatting(s: &str) -> String {
    // Early return optimization: avoid allocation if no bidi characters present
    if !s.chars().any(is_bidi_char) {
        return s.to_string();
    }

    s.chars().filter(|c| !is_bidi_char(*c)).collect()
}

/// Fixes HTML comment closing fences to prevent content loss.
///
/// According to CommonMark, HTML block type 2 (comments) ends with the line containing `-->`.
/// This means any text on the same line after `-->` is included in the HTML block and would
/// be discarded by markdown parsers that ignore HTML blocks.
///
/// This function inserts a newline after `-->` when followed by non-whitespace content,
/// ensuring the trailing text is parsed as regular markdown.
///
/// # Examples
///
/// ```
/// use quillmark_core::normalize::fix_html_comment_fences;
///
/// // Text on same line as --> is moved to next line
/// assert_eq!(
///     fix_html_comment_fences("<!-- comment -->Some text"),
///     "<!-- comment -->\nSome text"
/// );
///
/// // Already on separate line - no change
/// assert_eq!(
///     fix_html_comment_fences("<!-- comment -->\nSome text"),
///     "<!-- comment -->\nSome text"
/// );
///
/// // Only whitespace after --> - no change needed
/// assert_eq!(
///     fix_html_comment_fences("<!-- comment -->   \nSome text"),
///     "<!-- comment -->   \nSome text"
/// );
///
/// // Multi-line comments with trailing text
/// assert_eq!(
///     fix_html_comment_fences("<!--\nmultiline\n-->Trailing text"),
///     "<!--\nmultiline\n-->\nTrailing text"
/// );
/// ```
pub fn fix_html_comment_fences(s: &str) -> String {
    // Early return if no HTML comment closing fence present
    if !s.contains("-->") {
        return s.to_string();
    }

    // Context-aware processing: only fix `-->` if we are inside a comment started by `<!--`
    let mut result = String::with_capacity(s.len() + 16);
    let mut current_pos = 0;

    // Find first opener
    while let Some(open_idx) = s[current_pos..].find("<!--") {
        let abs_open = current_pos + open_idx;

        // Find matching closer AFTER the opener
        if let Some(close_idx) = s[abs_open..].find("-->") {
            let abs_close = abs_open + close_idx;
            let mut after_fence = abs_close + 3;

            // Handle `<!--- ... --->` style fences by treating the extra
            // hyphen as part of the comment content, not leaked trailing text.
            // 4 == "<!--".len(); check whether opener is `<!---` (extra hyphen).
            let opener_has_extra_hyphen = s
                .get(abs_open + 4..)
                .is_some_and(|rest| rest.starts_with('-'));
            if opener_has_extra_hyphen
                && s.get(after_fence..)
                    .is_some_and(|rest| rest.starts_with('-'))
            {
                after_fence += 1;
            }

            // Append everything up to and including the closing fence
            result.push_str(&s[current_pos..after_fence]);

            // Check what comes after the fence
            let after_content = &s[after_fence..];

            // Determine if we need to insert a newline
            let needs_newline = if after_content.is_empty()
                || after_content.starts_with('\n')
                || after_content.starts_with("\r\n")
            {
                false
            } else {
                // Check if there's only whitespace until end of line
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

            // Move position to after the fence (we'll process the rest in next iteration)
            current_pos = after_fence;
        } else {
            // Unclosed comment at end of string - just append the rest and break
            // The opener was found but no closer exists.
            result.push_str(&s[current_pos..]);
            current_pos = s.len();
            break;
        }
    }

    // Append remaining content (text after last closed comment, or text if no comments found)
    if current_pos < s.len() {
        result.push_str(&s[current_pos..]);
    }

    result
}

/// Normalizes markdown content by applying all preprocessing steps.
///
/// This function applies normalizations in the correct order:
/// 1. Strip Unicode bidirectional formatting characters
/// 2. Fix HTML comment closing fences (ensure text after `-->` is preserved)
///
/// Note: Guillemet preprocessing (`<<text>>` → `«text»`) is handled separately
/// in [`normalize_fields`] because it needs to be applied after schema defaults
/// and coercion.
///
/// # Examples
///
/// ```
/// use quillmark_core::normalize::normalize_markdown;
///
/// // Bidi characters are stripped
/// let input = "**bold** \u{202D}**more**";
/// let normalized = normalize_markdown(input);
/// assert_eq!(normalized, "**bold** **more**");
///
/// // HTML comment trailing text is preserved
/// let with_comment = "<!-- comment -->Some text";
/// let normalized = normalize_markdown(with_comment);
/// assert_eq!(normalized, "<!-- comment -->\nSome text");
/// ```
pub fn normalize_markdown(markdown: &str) -> String {
    let cleaned = normalize_line_endings(markdown);
    let cleaned = strip_bidi_formatting(&cleaned);
    fix_html_comment_fences(&cleaned)
}

/// Convert CRLF (`\r\n`) and bare CR (`\r`) line endings to LF (`\n`).
///
/// YAML parsing already normalizes line endings inside scalar values, but the
/// Markdown body is passed through verbatim. Authoring on Windows or pasting
/// from some clipboard sources leaves `\r` bytes in the body which some
/// backends render as visible garbage. This canonicalization is performed
/// only on the Markdown body (see §7); YAML scalars are unaffected.
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

/// Normalizes document frontmatter fields per the Quillmark §7 spec.
///
/// This is an internal helper used by [`normalize_document`]. It operates on
/// the typed `IndexMap<String, QuillValue>` frontmatter; it does **not** touch
/// `body` or `leaves` (those are normalized separately by the caller).
///
/// Field names at the top level are NFC-normalized (see [`normalize_field_name`]).
/// Only **body regions** receive content normalization (bidi stripping + HTML comment
/// fence repair). All other field values pass through verbatim.
///
/// # Examples
///
/// ```
/// use quillmark_core::normalize::normalize_fields;
/// use quillmark_core::QuillValue;
/// use indexmap::IndexMap;
///
/// let mut fields = IndexMap::new();
/// fields.insert("title".to_string(), QuillValue::from_json(serde_json::json!("<<hello>>")));
///
/// let result = normalize_fields(fields);
///
/// // Title passes through verbatim
/// assert_eq!(result.get("title").unwrap().as_str().unwrap(), "<<hello>>");
/// ```
pub fn normalize_fields(fields: IndexMap<String, QuillValue>) -> IndexMap<String, QuillValue> {
    fields
        .into_iter()
        .map(|(key, value)| {
            // Normalize field name to NFC form for consistent key comparison.
            let normalized_key = normalize_field_name(&key);
            // All top-level frontmatter fields pass through verbatim — body
            // regions are handled separately in normalize_document.
            (normalized_key, value)
        })
        .collect()
}

/// Normalize field name to Unicode NFC (Canonical Decomposition, followed by Canonical Composition)
///
/// This ensures that equivalent Unicode strings (e.g., "café" composed vs decomposed)
/// are treated as identical field names, preventing subtle bugs where visually
/// identical keys are treated as different.
///
/// # Examples
///
/// ```
/// use quillmark_core::normalize::normalize_field_name;
///
/// // Composed form (single code point for é)
/// let composed = "café";
/// // Decomposed form (e + combining acute accent)
/// let decomposed = "cafe\u{0301}";
///
/// // Both normalize to the same NFC form
/// assert_eq!(normalize_field_name(composed), normalize_field_name(decomposed));
/// ```
pub fn normalize_field_name(name: &str) -> String {
    name.nfc().collect()
}

/// Normalizes a typed [`crate::document::Document`] by applying all field-level normalizations.
///
/// This is the **primary entry point** for normalizing documents after parsing.
/// It ensures consistent processing regardless of how the document was created.
///
/// # Normalization Steps
///
/// 1. **Unicode NFC normalization** — Frontmatter field names are normalized to NFC form.
/// 2. **Bidi stripping** — Invisible bidirectional control characters are removed from
///    body regions (each `Leaf::body`). YAML field values in every
///    `Leaf::frontmatter` pass through verbatim (spec §7).
/// 3. **HTML comment fence fixing** — Trailing text after `-->` is preserved in body
///    regions only.
///
/// Double chevrons (`<<` and `>>`) are passed through unchanged without conversion.
///
/// # Idempotency
///
/// This function is idempotent — calling it multiple times produces the same result.
///
/// # Example
///
/// ```no_run
/// use quillmark_core::{Document, normalize::normalize_document};
///
/// let markdown = "---\nQUILL: my_quill\ntitle: Example\n---\n\nBody with <<placeholder>>";
/// let doc = Document::from_markdown(markdown).unwrap();
/// let normalized = normalize_document(doc).unwrap();
/// ```
pub fn normalize_document(
    doc: crate::document::Document,
) -> Result<crate::document::Document, crate::error::ParseError> {
    use crate::document::{Document, Sentinel};

    // NFC-normalize main-leaf field names; values pass through verbatim.
    let normalized_main_fm_map = normalize_fields(doc.main().frontmatter().to_index_map());
    let normalized_main_body = normalize_markdown(doc.main().body());
    let main_sentinel = doc.main().sentinel().clone();
    let main = Leaf::new_with_sentinel(
        main_sentinel,
        crate::document::Frontmatter::from_index_map(normalized_main_fm_map),
        normalized_main_body,
    );

    // Normalize each composable leaf's body; NFC-normalize its field names;
    // values pass through verbatim.
    let normalized_leaves: Vec<Leaf> = doc
        .leaves()
        .iter()
        .map(|leaf| {
            let normalized_leaf_fields: IndexMap<String, QuillValue> = leaf
                .frontmatter()
                .iter()
                .map(|(k, v)| (normalize_field_name(k), v.clone()))
                .collect();
            let normalized_leaf_body = normalize_markdown(leaf.body());
            Leaf::new_with_sentinel(
                Sentinel::Leaf(leaf.tag()),
                crate::document::Frontmatter::from_index_map(normalized_leaf_fields),
                normalized_leaf_body,
            )
        })
        .collect();

    Ok(Document::from_main_and_leaves(
        main,
        normalized_leaves,
        doc.warnings().to_vec(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for strip_bidi_formatting

    #[test]
    fn test_strip_bidi_no_change() {
        assert_eq!(strip_bidi_formatting("hello world"), "hello world");
        assert_eq!(strip_bidi_formatting(""), "");
        assert_eq!(strip_bidi_formatting("**bold** text"), "**bold** text");
    }

    #[test]
    fn test_strip_bidi_lro() {
        // U+202D (LEFT-TO-RIGHT OVERRIDE)
        assert_eq!(strip_bidi_formatting("he\u{202D}llo"), "hello");
        assert_eq!(
            strip_bidi_formatting("**asdf** or \u{202D}**(1234**"),
            "**asdf** or **(1234**"
        );
    }

    #[test]
    fn test_strip_bidi_rlo() {
        // U+202E (RIGHT-TO-LEFT OVERRIDE)
        assert_eq!(strip_bidi_formatting("he\u{202E}llo"), "hello");
    }

    #[test]
    fn test_strip_bidi_marks() {
        // U+200E (LRM) and U+200F (RLM)
        assert_eq!(strip_bidi_formatting("a\u{200E}b\u{200F}c"), "abc");
    }

    #[test]
    fn test_strip_bidi_embeddings() {
        // U+202A (LRE), U+202B (RLE), U+202C (PDF)
        assert_eq!(
            strip_bidi_formatting("\u{202A}text\u{202B}more\u{202C}"),
            "textmore"
        );
    }

    #[test]
    fn test_strip_bidi_isolates() {
        // U+2066 (LRI), U+2067 (RLI), U+2068 (FSI), U+2069 (PDI)
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
        // U+061C ARABIC LETTER MARK (ALM) should be stripped
        assert_eq!(strip_bidi_formatting("hello\u{061C}world"), "helloworld");
        assert_eq!(strip_bidi_formatting("\u{061C}**bold**"), "**bold**");
    }

    #[test]
    fn test_strip_bidi_unicode_preserved() {
        // Non-bidi unicode should be preserved
        assert_eq!(strip_bidi_formatting("你好世界"), "你好世界");
        assert_eq!(strip_bidi_formatting("مرحبا"), "مرحبا");
        assert_eq!(strip_bidi_formatting("🎉"), "🎉");
    }

    // Tests for normalize_markdown

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

    // Tests for fix_html_comment_fences

    #[test]
    fn test_fix_html_comment_no_comment() {
        assert_eq!(fix_html_comment_fences("hello world"), "hello world");
        assert_eq!(fix_html_comment_fences("**bold** text"), "**bold** text");
        assert_eq!(fix_html_comment_fences(""), "");
    }

    #[test]
    fn test_fix_html_comment_single_line_trailing_text() {
        // Text on same line as --> should be moved to next line
        assert_eq!(
            fix_html_comment_fences("<!-- comment -->Same line text"),
            "<!-- comment -->\nSame line text"
        );
    }

    #[test]
    fn test_fix_html_comment_already_newline() {
        // Already has newline after --> - no change
        assert_eq!(
            fix_html_comment_fences("<!-- comment -->\nNext line text"),
            "<!-- comment -->\nNext line text"
        );
    }

    #[test]
    fn test_fix_html_comment_only_whitespace_after() {
        // Only whitespace after --> until newline - no change needed
        assert_eq!(
            fix_html_comment_fences("<!-- comment -->   \nSome text"),
            "<!-- comment -->   \nSome text"
        );
    }

    #[test]
    fn test_fix_html_comment_multiline_trailing_text() {
        // Multi-line comment with text on closing line
        assert_eq!(
            fix_html_comment_fences("<!--\nmultiline\ncomment\n-->Trailing text"),
            "<!--\nmultiline\ncomment\n-->\nTrailing text"
        );
    }

    #[test]
    fn test_fix_html_comment_multiline_proper() {
        // Multi-line comment with proper newline after -->
        assert_eq!(
            fix_html_comment_fences("<!--\nmultiline\n-->\n\nParagraph text"),
            "<!--\nmultiline\n-->\n\nParagraph text"
        );
    }

    #[test]
    fn test_fix_html_comment_multiple_comments() {
        // Multiple comments in the same document
        assert_eq!(
            fix_html_comment_fences("<!-- first -->Text\n\n<!-- second -->More text"),
            "<!-- first -->\nText\n\n<!-- second -->\nMore text"
        );
    }

    #[test]
    fn test_fix_html_comment_end_of_string() {
        // Comment at end of string - no trailing content
        assert_eq!(
            fix_html_comment_fences("Some text before <!-- comment -->"),
            "Some text before <!-- comment -->"
        );
    }

    #[test]
    fn test_fix_html_comment_only_comment() {
        // Just a comment with nothing after
        assert_eq!(
            fix_html_comment_fences("<!-- comment -->"),
            "<!-- comment -->"
        );
    }

    #[test]
    fn test_fix_html_comment_arrow_not_comment() {
        // --> that's not part of a comment (standalone)
        // Should NOT be touched by the context-aware fixer
        assert_eq!(fix_html_comment_fences("-->some text"), "-->some text");
    }

    #[test]
    fn test_fix_html_comment_nested_opener() {
        // Nested openers are just text inside the comment
        // <!-- <!-- -->Trailing
        // The first <!-- opens, the first --> closes.
        assert_eq!(
            fix_html_comment_fences("<!-- <!-- -->Trailing"),
            "<!-- <!-- -->\nTrailing"
        );
    }

    #[test]
    fn test_fix_html_comment_unmatched_closer() {
        // Closer without opener
        assert_eq!(
            fix_html_comment_fences("text --> more text"),
            "text --> more text"
        );
    }

    #[test]
    fn test_fix_html_comment_multiple_valid_invalid() {
        // Mixed valid and invalid comments
        // <!-- valid -->FixMe
        // text --> Ignore
        // <!-- valid2 -->FixMe2
        let input = "<!-- valid -->FixMe\ntext --> Ignore\n<!-- valid2 -->FixMe2";
        let expected = "<!-- valid -->\nFixMe\ntext --> Ignore\n<!-- valid2 -->\nFixMe2";
        assert_eq!(fix_html_comment_fences(input), expected);
    }

    #[test]
    fn test_fix_html_comment_crlf() {
        // CRLF line endings
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

    #[test]
    fn test_fix_html_comment_triple_hyphen_multiline() {
        assert_eq!(
            fix_html_comment_fences("<!---\ncomment\n--->Trailing text"),
            "<!---\ncomment\n--->\nTrailing text"
        );
    }

    // Tests for normalize_fields (frontmatter only)

    #[test]
    fn test_normalize_fields_other_field_chevrons_preserved() {
        let mut fields = IndexMap::new();
        fields.insert(
            "title".to_string(),
            QuillValue::from_json(serde_json::json!("<<hello>>")),
        );

        let result = normalize_fields(fields);
        // Chevrons are passed through unchanged
        assert_eq!(result.get("title").unwrap().as_str().unwrap(), "<<hello>>");
    }

    #[test]
    fn test_normalize_fields_other_field_bidi_preserved() {
        // Per spec §7: bidi stripping is NOT applied to YAML field values.
        // Only body regions are normalized.
        let mut fields = IndexMap::new();
        fields.insert(
            "title".to_string(),
            QuillValue::from_json(serde_json::json!("a\u{202D}b")),
        );

        let result = normalize_fields(fields);
        // Bidi character must be PRESERVED in non-body fields
        assert_eq!(result.get("title").unwrap().as_str().unwrap(), "a\u{202D}b");
    }

    #[test]
    fn test_normalize_fields_non_string_unchanged() {
        let mut fields = IndexMap::new();
        fields.insert(
            "count".to_string(),
            QuillValue::from_json(serde_json::json!(42)),
        );
        fields.insert(
            "enabled".to_string(),
            QuillValue::from_json(serde_json::json!(true)),
        );

        let result = normalize_fields(fields);
        assert_eq!(result.get("count").unwrap().as_i64().unwrap(), 42);
        assert!(result.get("enabled").unwrap().as_bool().unwrap());
    }

    // Tests for normalize_document

    #[test]
    fn test_normalize_document_basic() {
        use crate::document::Document;

        let doc = Document::from_markdown(
            "---\nQUILL: test\ntitle: <<placeholder>>\n---\n\n<<content>> \u{202D}**bold**",
        )
        .unwrap();
        let normalized = super::normalize_document(doc).unwrap();

        // Title has chevrons preserved (only bidi stripped on body)
        assert_eq!(
            normalized
                .main()
                .frontmatter()
                .get("title")
                .unwrap()
                .as_str()
                .unwrap(),
            "<<placeholder>>"
        );

        // Body has bidi stripped, chevrons preserved
        assert_eq!(normalized.main().body(), "\n<<content>> **bold**");
    }

    #[test]
    fn test_normalize_document_preserves_quill_tag() {
        use crate::document::Document;

        let doc = Document::from_markdown("---\nQUILL: custom_quill\n---\n").unwrap();
        let normalized = super::normalize_document(doc).unwrap();

        assert_eq!(normalized.quill_reference().name, "custom_quill");
    }

    #[test]
    fn test_normalize_document_idempotent() {
        use crate::document::Document;

        let doc = Document::from_markdown("---\nQUILL: test\n---\n\n<<content>>").unwrap();
        let normalized_once = super::normalize_document(doc).unwrap();
        let normalized_twice = super::normalize_document(normalized_once.clone()).unwrap();

        assert_eq!(
            normalized_once.main().body(),
            normalized_twice.main().body()
        );
    }

    #[test]
    fn test_normalize_document_body_bidi_stripped() {
        use crate::document::Document;

        let doc = Document::from_markdown("---\nQUILL: test\n---\n\nhello\u{202D}world").unwrap();
        let normalized = super::normalize_document(doc).unwrap();
        assert_eq!(normalized.main().body(), "\nhelloworld");
    }

    #[test]
    fn test_normalize_document_yaml_field_bidi_preserved() {
        use crate::document::Document;

        let doc = Document::from_markdown("---\nQUILL: test\ntitle: a\u{202D}b\n---\n").unwrap();
        let normalized = super::normalize_document(doc).unwrap();
        // Bidi preserved in YAML fields
        assert_eq!(
            normalized
                .main()
                .frontmatter()
                .get("title")
                .unwrap()
                .as_str()
                .unwrap(),
            "a\u{202D}b"
        );
    }

    #[test]
    fn test_normalize_document_leaf_body_bidi_stripped() {
        use crate::document::Document;

        let md = "---\nQUILL: test\n---\n\nbody\n\n```leaf\nKIND: note\n```\nleaf\u{202D}body\n";
        let doc = Document::from_markdown(md).unwrap();
        assert_eq!(doc.leaves().len(), 1, "expected 1 leaf");
        let normalized = super::normalize_document(doc).unwrap();
        assert_eq!(normalized.leaves()[0].body(), "leafbody\n");
    }

    #[test]
    fn test_normalize_document_leaf_field_bidi_preserved() {
        use crate::document::Document;

        let md = "---\nQUILL: test\n---\n\nbody\n\n```leaf\nKIND: note\nname: Ali\u{202D}ce\n```\n";
        let doc = Document::from_markdown(md).unwrap();
        assert_eq!(doc.leaves().len(), 1, "expected 1 leaf");
        let normalized = super::normalize_document(doc).unwrap();
        assert_eq!(
            normalized.leaves()[0]
                .frontmatter()
                .get("name")
                .unwrap()
                .as_str()
                .unwrap(),
            "Ali\u{202D}ce"
        );
    }

    #[test]
    fn test_normalize_document_leaf_body_html_comment_repair() {
        use crate::document::Document;

        let md =
            "---\nQUILL: test\n---\n\n```leaf\nKIND: note\n```\n<!-- comment -->Trailing text\n";
        let doc = Document::from_markdown(md).unwrap();
        let normalized = super::normalize_document(doc).unwrap();
        assert_eq!(
            normalized.leaves()[0].body(),
            "<!-- comment -->\nTrailing text\n"
        );
    }

    #[test]
    fn test_normalize_document_toplevel_body_html_comment_repair() {
        use crate::document::Document;

        let md = "---\nQUILL: test\n---\n\n<!-- note -->Content here";
        let doc = Document::from_markdown(md).unwrap();
        let normalized = super::normalize_document(doc).unwrap();
        assert_eq!(normalized.main().body(), "\n<!-- note -->\nContent here");
    }
}
