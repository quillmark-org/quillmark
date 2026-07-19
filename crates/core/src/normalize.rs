//! # Document Normalization
//!
//! Post-parse normalization of a [`Document`](crate::document::Document): payload
//! field names to Unicode NFC (so composed `"café"` and decomposed `"cafe\u{0301}"`
//! compare equal). YAML field *values* pass through verbatim.
//!
//! Card bodies are **not** normalized here: a body is already a normalized
//! [`Content`](quillmark_content::Content) content, established once at import
//! (`import::from_markdown` runs `normalize_markdown` — line endings, bidi strip,
//! HTML-comment fence repair — before parsing). This pass only touches field
//! names and carries each body through unchanged.

use crate::document::Card;
use unicode_normalization::UnicodeNormalization;

/// Normalize field name to Unicode NFC, so visually identical keys
/// (e.g., composed `"café"` vs decomposed `"cafe\u{0301}"`) are treated as equal.
pub fn normalize_field_name(name: &str) -> String {
    name.nfc().collect()
}

/// Primary entry point for normalizing a [`crate::document::Document`] after parsing.
///
/// Per-card normalization:
/// 1. Payload field names → Unicode NFC.
///
/// Card bodies are already-normalized content (import-time); they carry through
/// unchanged. YAML field *values* pass through verbatim. Idempotent.
pub fn normalize_document(
    doc: crate::document::Document,
) -> Result<crate::document::Document, crate::error::ParseError> {
    use crate::document::Document;

    let main = normalize_card(doc.main());
    let normalized_cards: Vec<Card> = doc.cards().iter().map(normalize_card).collect();

    Ok(Document::from_main_and_cards(main, normalized_cards))
}

/// Build a new `Card` with NFC-normalized field names, carrying the (already
/// normalized) body content through unchanged.
fn normalize_card(card: &Card) -> Card {
    use crate::document::PayloadItem;
    let mut payload = card.payload().clone();
    for item in payload.items_mut() {
        if let PayloadItem::Field { key, .. } = item {
            let normalized = normalize_field_name(key);
            if normalized != *key {
                *key = normalized;
            }
        }
    }
    Card::from_parts(payload, card.body().clone())
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_normalize_document_basic() {
        use crate::document::Document;

        let doc = Document::parse(
            "~~~card-yaml\n$quill: test\n$kind: main\ntitle: <<placeholder>>\n~~~\n\n<<content>> \u{202D}**bold**",
        )
        .unwrap()
        .document;
        let normalized = super::normalize_document(doc).unwrap();

        assert_eq!(
            normalized
                .main()
                .payload()
                .get("title")
                .unwrap()
                .as_str()
                .unwrap(),
            "<<placeholder>>"
        );

        assert_eq!(normalized.main().body_markdown(), "\\<> **bold**");
    }

    #[test]
    fn test_normalize_document_preserves_quill_tag() {
        use crate::document::Document;

        let doc = Document::parse("~~~card-yaml\n$quill: custom_quill\n$kind: main\n~~~\n")
            .unwrap()
            .document;
        let normalized = super::normalize_document(doc).unwrap();

        assert_eq!(normalized.quill_reference().name, "custom_quill");
    }

    #[test]
    fn test_normalize_document_idempotent() {
        use crate::document::Document;

        let doc =
            Document::parse("~~~card-yaml\n$quill: test\n$kind: main\n~~~\n\n<<content>>")
                .unwrap()
                .document;
        let normalized_once = super::normalize_document(doc).unwrap();
        let normalized_twice = super::normalize_document(normalized_once.clone()).unwrap();

        assert_eq!(
            normalized_once.main().body_markdown(),
            normalized_twice.main().body_markdown()
        );
    }

    #[test]
    fn test_normalize_document_yaml_field_bidi_preserved() {
        use crate::document::Document;

        let doc = Document::parse(
            "~~~card-yaml\n$quill: test\n$kind: main\ntitle: a\u{202D}b\n~~~\n",
        )
        .unwrap()
        .document;
        let normalized = super::normalize_document(doc).unwrap();
        assert_eq!(
            normalized
                .main()
                .payload()
                .get("title")
                .unwrap()
                .as_str()
                .unwrap(),
            "a\u{202D}b"
        );
    }

    #[test]
    fn test_normalize_document_card_body_bidi_stripped() {
        use crate::document::Document;

        let md = "~~~card-yaml\n$quill: test\n$kind: main\n~~~\n\nbody\n\n~~~card-yaml\n$kind: note\n~~~\ncard\u{202D}body\n";
        let doc = Document::parse(md).unwrap().document;
        assert_eq!(doc.cards().len(), 1, "expected 1 card");
        let normalized = super::normalize_document(doc).unwrap();
        assert_eq!(normalized.cards()[0].body_markdown(), "cardbody");
    }

    #[test]
    fn test_normalize_document_card_field_bidi_preserved() {
        use crate::document::Document;

        let md = "~~~card-yaml\n$quill: test\n$kind: main\n~~~\n\nbody\n\n~~~card-yaml\n$kind: note\nname: Ali\u{202D}ce\n~~~\n";
        let doc = Document::parse(md).unwrap().document;
        assert_eq!(doc.cards().len(), 1, "expected 1 card");
        let normalized = super::normalize_document(doc).unwrap();
        assert_eq!(
            normalized.cards()[0]
                .payload()
                .get("name")
                .unwrap()
                .as_str()
                .unwrap(),
            "Ali\u{202D}ce"
        );
    }

    #[test]
    fn test_normalize_document_card_body_html_comment_repair() {
        use crate::document::Document;

        let md = "~~~card-yaml\n$quill: test\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: note\n~~~\n<!-- comment -->Trailing text\n";
        let doc = Document::parse(md).unwrap().document;
        let normalized = super::normalize_document(doc).unwrap();
        assert_eq!(normalized.cards()[0].body_markdown(), "Trailing text");
    }

    #[test]
    fn test_normalize_document_toplevel_body_html_comment_repair() {
        use crate::document::Document;

        let md = "~~~card-yaml\n$quill: test\n$kind: main\n~~~\n\n<!-- note -->Content here";
        let doc = Document::parse(md).unwrap().document;
        let normalized = super::normalize_document(doc).unwrap();
        assert_eq!(normalized.main().body_markdown(), "Content here");
    }
}
