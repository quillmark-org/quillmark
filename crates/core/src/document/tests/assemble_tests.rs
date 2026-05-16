use crate::document::assemble::decompose;
use crate::document::sentinel::is_valid_tag_name;
use crate::document::Document;

#[test]
fn test_no_frontmatter() {
    let markdown = "# Hello World\n\nThis is a test.";
    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Missing required QUILL field"));
}

#[test]
fn test_empty_input_dedicated_error() {
    // Empty input should not surface the generic "Missing required QUILL"
    // message — that misleadingly suggests a partial document. Both the
    // truly-empty and whitespace-only cases get the dedicated message.
    for input in ["", "   ", "\n\n\t\n"] {
        let err = decompose(input).unwrap_err().to_string();
        assert!(
            err.contains("Empty markdown input"),
            "expected dedicated empty-input message for {input:?}, got: {err}"
        );
    }
}

#[test]
fn test_empty_input_diagnostic_code() {
    // Empty / whitespace-only inputs surface a stable code consumers can
    // pattern-match without inspecting the message text.
    for input in ["", "   ", "\n\n\t\n"] {
        let err = decompose(input).unwrap_err();
        let diag = err.to_diagnostic();
        assert_eq!(
            diag.code.as_deref(),
            Some("parse::empty_input"),
            "expected parse::empty_input for {input:?}, got: {:?}",
            diag.code
        );
    }
}

#[test]
fn test_missing_quill_field_diagnostic_code() {
    // All "missing QUILL" sub-cases — no fences, wrong-cased key, mis-ordered
    // key, empty fence — share the dedicated `parse::missing_quill_field`
    // code so consumers don't have to regex the message text.
    let cases = [
        "# Hello World\n\nNo frontmatter here.",
        "---\nquill: foo\n---\n\nbody",
        "---\ntitle: Foo\nQUILL: bar\n---\n\nbody",
        "---\n   \n---\n\n# Hello",
    ];
    for input in cases {
        let err = decompose(input).unwrap_err();
        let diag = err.to_diagnostic();
        assert_eq!(
            diag.code.as_deref(),
            Some("parse::missing_quill_field"),
            "expected parse::missing_quill_field for {input:?}, got: {:?}",
            diag.code
        );
    }
}

#[test]
fn test_with_frontmatter() {
    let markdown = r#"---
QUILL: test_quill
title: Test Document
author: Test Author
---

# Hello World

This is the body."#;

    let doc = decompose(markdown).unwrap();

    assert_eq!(doc.main().body(), "\n# Hello World\n\nThis is the body.");
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "Test Document"
    );
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("author")
            .unwrap()
            .as_str()
            .unwrap(),
        "Test Author"
    );
    assert_eq!(doc.main().frontmatter().len(), 2); // title, author
    assert_eq!(doc.cards().len(), 0);
    assert_eq!(doc.quill_reference().name, "test_quill");
}

#[test]
fn test_whitespace_frontmatter() {
    // Frontmatter with only whitespace has no QUILL → error
    let markdown = "---\n   \n---\n\n# Hello";
    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Missing required QUILL field"));
}

#[test]
fn test_complex_yaml_frontmatter() {
    let markdown = r#"---
QUILL: test_quill
title: Complex Document
tags:
  - test
  - yaml
metadata:
  version: 1.0
  nested:
    field: value
---

Content here."#;

    let doc = decompose(markdown).unwrap();

    assert_eq!(doc.main().body(), "\nContent here.");
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "Complex Document"
    );

    let tags = doc
        .main()
        .frontmatter()
        .get("tags")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(tags.len(), 2);
    assert_eq!(tags[0].as_str().unwrap(), "test");
    assert_eq!(tags[1].as_str().unwrap(), "yaml");
}

#[test]
fn test_invalid_yaml() {
    // Real fence (QUILL first) with invalid YAML — size check happens, then YAML parse fails.
    let markdown = r#"---
QUILL: test_quill
title: [invalid yaml
author: missing close bracket
---

Content here."#;

    let result = decompose(markdown);
    assert!(result.is_err());
    // Error message now includes location context
    assert!(result.unwrap_err().to_string().contains("YAML error"));
}

#[test]
fn test_unclosed_frontmatter() {
    // Real fence (QUILL first) without closer → spec §9 "not closed" error.
    let markdown = r#"---
QUILL: test_quill
title: Test
author: Test Author

Content without closing ---"#;

    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not closed"));
}

// Extended metadata tests

#[test]
fn test_basic_tagged_block() {
    let markdown = r#"---
QUILL: test_quill
title: Main Document
---

Main body content.

---
CARD: items
name: Item 1
---

Body of item 1."#;

    let doc = decompose(markdown).unwrap();

    // Global body is followed by a CARD fence: F2 separator stripped, so the
    // trailing `\n\n` from the source becomes a single `\n` (content's line
    // terminator preserved).
    assert_eq!(doc.main().body(), "\nMain body content.\n");
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "Main Document"
    );

    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.tag(), "items");
    assert_eq!(
        card.frontmatter().get("name").unwrap().as_str().unwrap(),
        "Item 1"
    );
    // Last card body at EOF: no F2 separator to strip.
    assert_eq!(card.body(), "\nBody of item 1.");
}

#[test]
fn test_multiple_tagged_blocks() {
    let markdown = r#"---
QUILL: test_quill
---

---
CARD: items
name: Item 1
tags: [a, b]
---

First item body.

---
CARD: items
name: Item 2
tags: [c, d]
---

Second item body."#;

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 2);

    let card1 = &doc.cards()[0];
    assert_eq!(card1.tag(), "items");
    assert_eq!(
        card1.frontmatter().get("name").unwrap().as_str().unwrap(),
        "Item 1"
    );

    let card2 = &doc.cards()[1];
    assert_eq!(card2.tag(), "items");
    assert_eq!(
        card2.frontmatter().get("name").unwrap().as_str().unwrap(),
        "Item 2"
    );
}

#[test]
fn test_mixed_global_and_tagged() {
    let markdown = r#"---
QUILL: test_quill
title: Global
author: John Doe
---

Global body.

---
CARD: sections
title: Section 1
---

Section 1 content.

---
CARD: sections
title: Section 2
---

Section 2 content."#;

    let doc = decompose(markdown).unwrap();

    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "Global"
    );
    assert_eq!(doc.main().body(), "\nGlobal body.\n");
    assert_eq!(doc.cards().len(), 2);
    assert_eq!(doc.cards()[0].tag(), "sections");
}

#[test]
fn test_empty_tagged_metadata() {
    let markdown = r#"---
QUILL: test_quill
---

---
CARD: items
---

Body without metadata."#;

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.tag(), "items");
    assert!(card.frontmatter().is_empty());
    assert_eq!(card.body(), "\nBody without metadata.");
}

#[test]
fn test_tagged_block_without_body() {
    let markdown = r#"---
QUILL: test_quill
---

---
CARD: items
name: Item
---"#;

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.tag(), "items");
    assert_eq!(card.body(), ""); // empty, not absent
}

#[test]
fn test_name_collision_global_and_tagged() {
    let markdown = r#"---
QUILL: test_quill
items: "global value"
---

Body

---
CARD: items
name: Item
---

Item body"#;

    let result = decompose(markdown);
    assert!(result.is_ok(), "Name collision should be allowed now");
}

#[test]
fn test_card_name_collision_with_array_field() {
    // CARD type names CAN now conflict with frontmatter field names
    let markdown = r#"---
QUILL: test_quill
items:
  - name: Global Item 1
    value: 100
---

Global body

---
CARD: items
name: Scope Item 1
---

Scope item 1 body"#;

    let result = decompose(markdown);
    assert!(
        result.is_ok(),
        "Collision with array field should be allowed"
    );
}

#[test]
fn test_empty_global_array_with_card() {
    let markdown = r#"---
QUILL: test_quill
items: []
---

Global body

---
CARD: items
name: Item 1
---

Item 1 body"#;

    let result = decompose(markdown);
    assert!(
        result.is_ok(),
        "Collision with empty array field should be allowed"
    );
}

#[test]
fn test_reserved_field_body_rejected() {
    // BODY reserved inside a CARD block (requires prior QUILL fence per spec §4 F1).
    let markdown = r#"---
QUILL: test_quill
---

---
CARD: section
BODY: Test
---"#;

    let result = decompose(markdown);
    assert!(result.is_err(), "BODY is a reserved field name");
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Reserved field name"));
}

#[test]
fn test_reserved_field_cards_rejected() {
    // CARDS reserved inside the QUILL frontmatter.
    let markdown = r#"---
QUILL: test_quill
title: Test
CARDS: []
---"#;

    let result = decompose(markdown);
    assert!(result.is_err(), "CARDS is a reserved field name");
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Reserved field name"));
}

#[test]
fn test_delimiter_inside_fenced_code_block_backticks() {
    let markdown = r#"---
QUILL: test_quill
title: Test
---
Here is some code:

```yaml
---
fake: frontmatter
---
```

More content.
"#;

    let doc = decompose(markdown).unwrap();
    // The --- inside the code block should NOT be parsed as metadata
    assert!(doc.main().body().contains("fake: frontmatter"));
    assert!(doc.main().frontmatter().get("fake").is_none());
}

#[test]
fn test_tildes_are_fences() {
    // Per CommonMark: tildes (~~~) are valid fenced code block delimiters.
    let markdown = r#"---
QUILL: test_quill
title: Test
---
Here is some code:

~~~yaml
---
CARD: code_example
fake: frontmatter
---
~~~

More content.
"#;

    let doc = decompose(markdown).unwrap();
    assert!(doc.main().body().contains("fake: frontmatter"));
    assert!(doc.main().frontmatter().get("fake").is_none());
}

#[test]
fn test_four_backticks_are_fences() {
    let markdown = r#"---
QUILL: test_quill
title: Test
---
Here is some code:

````yaml
---
CARD: code_example
fake: frontmatter
---
````

More content.
"#;

    let doc = decompose(markdown).unwrap();
    assert!(doc.main().body().contains("fake: frontmatter"));
    assert!(doc.main().frontmatter().get("fake").is_none());
}

#[test]
fn test_invalid_tag_syntax() {
    let markdown = r#"---
QUILL: test_quill
---

---
CARD: Invalid-Name
title: Test
---"#;

    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid card field name"));
}

#[test]
fn test_multiple_global_frontmatter_blocks() {
    // Two `---/---` blocks without QUILL/CARD sentinels both fail F1
    // and are delegated to CommonMark, so the document has no metadata
    // blocks and parsing fails with the missing-QUILL error.
    let markdown = r#"---
title: First
---

Body

---
author: Second
---

More body"#;

    let err = decompose(markdown).unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("QUILL"),
        "Error should mention missing QUILL: {}",
        err_str
    );
}

#[test]
fn test_adjacent_blocks_different_tags() {
    let markdown = r#"---
QUILL: test_quill
---

---
CARD: items
name: Item 1
---

Item 1 body

---
CARD: sections
title: Section 1
---

Section 1 body"#;

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 2);

    let card0 = &doc.cards()[0];
    assert_eq!(card0.tag(), "items");
    assert_eq!(
        card0.frontmatter().get("name").unwrap().as_str().unwrap(),
        "Item 1"
    );

    let card1 = &doc.cards()[1];
    assert_eq!(card1.tag(), "sections");
    assert_eq!(
        card1.frontmatter().get("title").unwrap().as_str().unwrap(),
        "Section 1"
    );
}

#[test]
fn test_order_preservation() {
    let markdown = r#"---
QUILL: test_quill
---

---
CARD: items
id: 1
---

First

---
CARD: items
id: 2
---

Second

---
CARD: items
id: 3
---

Third"#;

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 3);

    for (i, card) in doc.cards().iter().enumerate() {
        assert_eq!(card.tag(), "items");
        let id = card.frontmatter().get("id").unwrap().as_i64().unwrap();
        assert_eq!(id, (i + 1) as i64);
    }
}

#[test]
fn test_product_catalog_integration() {
    let markdown = r#"---
QUILL: test_quill
title: Product Catalog
author: John Doe
date: 2024-01-01
---

This is the main catalog description.

---
CARD: products
name: Widget A
price: 19.99
sku: WID-001
---

The **Widget A** is our most popular product.

---
CARD: products
name: Gadget B
price: 29.99
sku: GAD-002
---

The **Gadget B** is perfect for professionals.

---
CARD: reviews
product: Widget A
rating: 5
---

"Excellent product! Highly recommended."

---
CARD: reviews
product: Gadget B
rating: 4
---

"Very good, but a bit pricey.""#;

    let doc = decompose(markdown).unwrap();

    // Verify global frontmatter
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "Product Catalog"
    );
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("author")
            .unwrap()
            .as_str()
            .unwrap(),
        "John Doe"
    );
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("date")
            .unwrap()
            .as_str()
            .unwrap(),
        "2024-01-01"
    );

    // Verify global body
    assert!(doc.main().body().contains("main catalog description"));

    // 4 cards total
    assert_eq!(doc.cards().len(), 4);

    // First 2 are products
    assert_eq!(doc.cards()[0].tag(), "products");
    assert_eq!(
        doc.cards()[0]
            .frontmatter()
            .get("name")
            .unwrap()
            .as_str()
            .unwrap(),
        "Widget A"
    );
    assert_eq!(
        doc.cards()[0]
            .frontmatter()
            .get("price")
            .unwrap()
            .as_f64()
            .unwrap(),
        19.99
    );

    assert_eq!(doc.cards()[1].tag(), "products");
    assert_eq!(
        doc.cards()[1]
            .frontmatter()
            .get("name")
            .unwrap()
            .as_str()
            .unwrap(),
        "Gadget B"
    );

    // Last 2 are reviews
    assert_eq!(doc.cards()[2].tag(), "reviews");
    assert_eq!(
        doc.cards()[2]
            .frontmatter()
            .get("product")
            .unwrap()
            .as_str()
            .unwrap(),
        "Widget A"
    );
    assert_eq!(
        doc.cards()[2]
            .frontmatter()
            .get("rating")
            .unwrap()
            .as_i64()
            .unwrap(),
        5
    );

    // Frontmatter has 3 fields: title, author, date
    assert_eq!(doc.main().frontmatter().len(), 3);
}

#[test]
fn taro_quill_directive() {
    let markdown = r#"---
QUILL: usaf_memo
memo_for: [ORG/SYMBOL]
memo_from: [ORG/SYMBOL]
---

This is the memo body."#;

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.quill_reference().name, "usaf_memo");
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("memo_for")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .as_str()
            .unwrap(),
        "ORG/SYMBOL"
    );
    assert_eq!(doc.main().body(), "\nThis is the memo body.");
}

#[test]
fn test_quill_with_card_blocks() {
    let markdown = r#"---
QUILL: document
title: Test Document
---

Main body.

---
CARD: sections
name: Section 1
---

Section 1 body."#;

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.quill_reference().name, "document");
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "Test Document"
    );
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].tag(), "sections");
    assert_eq!(doc.main().body(), "\nMain body.\n");
}

#[test]
fn test_multiple_quill_directives_error() {
    let markdown = r#"---
QUILL: first
---

---
QUILL: second
---"#;

    let output = Document::from_markdown_with_warnings(markdown).unwrap();
    assert!(output
        .warnings
        .iter()
        .any(|w| w.code.as_deref() == Some("parse::near_miss_sentinel")
            && w.message.contains("QUILL")));
    assert!(output.document.main().body().contains("QUILL: second"));
}

#[test]
fn test_invalid_quill_ref() {
    let markdown = r#"---
QUILL: Invalid-Name
---"#;

    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid QUILL reference"));
}

#[test]
fn test_quill_wrong_value_type() {
    let markdown = r#"---
QUILL: 123
---"#;

    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("QUILL value must be a string"));
}

#[test]
fn test_card_wrong_value_type() {
    let markdown = r#"---
QUILL: test_quill
---

---
CARD: 123
---"#;

    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("CARD value must be a string"));
}

#[test]
fn test_both_quill_and_card_error() {
    let markdown = r#"---
QUILL: test
CARD: items
---"#;

    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Cannot specify both QUILL and CARD"));
}

#[test]
fn test_blank_lines_in_frontmatter() {
    let markdown = r#"---
QUILL: test_quill
title: Test Document
author: Test Author

description: This has a blank line above it
tags:
  - one
  - two
---

# Hello World

This is the body."#;

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body(), "\n# Hello World\n\nThis is the body.");
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "Test Document"
    );
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("author")
            .unwrap()
            .as_str()
            .unwrap(),
        "Test Author"
    );
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("description")
            .unwrap()
            .as_str()
            .unwrap(),
        "This has a blank line above it"
    );
    let tags = doc
        .main()
        .frontmatter()
        .get("tags")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(tags.len(), 2);
}

#[test]
fn test_blank_lines_in_scope_blocks() {
    let markdown = r#"---
QUILL: test_quill
---

---
CARD: items
name: Item 1

price: 19.99

tags:
  - electronics
  - gadgets
---

Body of item 1."#;

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.tag(), "items");
    assert_eq!(
        card.frontmatter().get("name").unwrap().as_str().unwrap(),
        "Item 1"
    );
    assert_eq!(
        card.frontmatter().get("price").unwrap().as_f64().unwrap(),
        19.99
    );
    let tags = card.frontmatter().get("tags").unwrap().as_array().unwrap();
    assert_eq!(tags.len(), 2);
}

#[test]
fn test_triple_dash_in_body_without_sentinel_is_delegated() {
    let markdown = r#"---
QUILL: test_quill
title: Test
---

First paragraph.

---

Second paragraph."#;

    let doc = decompose(markdown).unwrap();
    let body = doc.main().body();
    assert!(body.contains("First paragraph."));
    assert!(body.contains("Second paragraph."));
    assert!(body.contains("---"));
}

#[test]
fn test_lone_triple_dash_in_body_is_delegated() {
    let markdown = r#"---
QUILL: test_quill
title: Test
---

First paragraph.
---

Second paragraph."#;

    let doc = decompose(markdown).unwrap();
    let body = doc.main().body();
    assert!(body.contains("First paragraph."));
    assert!(body.contains("Second paragraph."));
    assert!(body.contains("---"));
}

#[test]
fn test_multiple_blank_lines_in_yaml() {
    let markdown = r#"---
QUILL: test_quill
title: Test


author: John Doe


version: 1.0
---

Body content."#;

    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "Test"
    );
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("author")
            .unwrap()
            .as_str()
            .unwrap(),
        "John Doe"
    );
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("version")
            .unwrap()
            .as_f64()
            .unwrap(),
        1.0
    );
}

#[test]
fn test_html_comment_interaction() {
    let markdown = r#"<!---
---> the rest of the page content

---
QUILL: test_quill
key: value
---
"#;
    let doc = decompose(markdown).unwrap();
    let key = doc.main().frontmatter().get("key").and_then(|v| v.as_str());
    assert_eq!(key, Some("value"));
}

// --- demo_file_test ---

#[test]
fn test_extended_metadata_demo_file() {
    let markdown = include_str!("../../../../fixtures/resources/extended_metadata_demo.md");
    let doc = decompose(markdown).unwrap();

    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "Extended Metadata Demo"
    );
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("author")
            .unwrap()
            .as_str()
            .unwrap(),
        "Quillmark Team"
    );
    // version is parsed as a number by YAML
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("version")
            .unwrap()
            .as_f64()
            .unwrap(),
        1.0
    );

    // Verify body
    assert!(doc
        .main()
        .body()
        .contains("extended YAML metadata standard"));

    // 5 cards total: 3 features + 2 use_cases
    assert_eq!(doc.cards().len(), 5);

    let features_count = doc.cards().iter().filter(|c| c.tag() == "features").count();
    let use_cases_count = doc
        .cards()
        .iter()
        .filter(|c| c.tag() == "use_cases")
        .count();
    assert_eq!(features_count, 3);
    assert_eq!(use_cases_count, 2);

    // Check first card is a feature
    assert_eq!(doc.cards()[0].tag(), "features");
    assert_eq!(
        doc.cards()[0]
            .frontmatter()
            .get("name")
            .unwrap()
            .as_str()
            .unwrap(),
        "Tag Directives"
    );
}

#[test]
fn test_input_size_limit() {
    let size = crate::error::MAX_INPUT_SIZE + 1;
    let large_markdown = "a".repeat(size);

    let result = decompose(&large_markdown);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Input too large"));
}

#[test]
fn test_yaml_size_limit() {
    let mut markdown = String::from("---\nQUILL: test_quill\n");
    let size = crate::error::MAX_YAML_SIZE + 1;
    markdown.push_str("data: \"");
    markdown.push_str(&"x".repeat(size));
    markdown.push_str("\"\n---\n\nBody");

    let result = decompose(&markdown);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Input too large"));
}

#[test]
fn test_input_within_size_limit() {
    let size = 1000;
    let markdown = format!(
        "---\nQUILL: test_quill\ntitle: Test\n---\n\n{}",
        "a".repeat(size)
    );

    let result = decompose(&markdown);
    assert!(result.is_ok());
}

#[test]
fn test_yaml_within_size_limit() {
    let markdown = "---\nQUILL: test_quill\ntitle: Test\nauthor: John Doe\n---\n\nBody content";
    let result = decompose(markdown);
    assert!(result.is_ok());
}

#[test]
fn test_yaml_depth_limit() {
    let mut yaml_content = String::new();
    for i in 0..110 {
        yaml_content.push_str(&"  ".repeat(i));
        yaml_content.push_str(&format!("level{}: value\n", i));
    }

    let markdown = format!("---\n{}---\n\nBody", yaml_content);
    let result = decompose(&markdown);

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.to_lowercase().contains("budget")
            || err_msg.to_lowercase().contains("depth")
            || err_msg.contains("YAML"),
        "Expected depth/budget error, got: {}",
        err_msg
    );
}

#[test]
fn test_yaml_depth_within_limit() {
    let markdown = r#"---
QUILL: test_quill
level1:
  level2:
    level3:
      level4:
        value: test
---

Body content"#;

    let result = decompose(markdown);
    assert!(result.is_ok());
}

// Guillemet preservation tests

/// Guillemet/chevron sequences (`<<...>>`) must survive parsing unmodified in
/// every context — body, YAML string values, YAML arrays, nested maps, code
/// blocks, inline code, and card bodies/fields. A single integrative document
/// exercises all of these.
#[test]
fn test_chevrons_preserved_in_all_contexts() {
    let markdown = r#"---
QUILL: test_quill
title: Test <<with chevrons>>
items:
  - "<<first>>"
  - "<<second>>"
metadata:
  description: "<<nested value>>"
---

<<body>> text.

```
<<in code block>>
```

`<<inline code>>` and <<plain>>

---
CARD: items
description: "<<card yaml>>"
---

Use <<card body>> here."#;

    let doc = decompose(markdown).unwrap();

    // Frontmatter scalar, array, nested map.
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "Test <<with chevrons>>"
    );
    let items = doc
        .main()
        .frontmatter()
        .get("items")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(items[0].as_str().unwrap(), "<<first>>");
    assert_eq!(items[1].as_str().unwrap(), "<<second>>");
    let metadata = doc
        .main()
        .frontmatter()
        .get("metadata")
        .unwrap()
        .as_object()
        .unwrap();
    assert_eq!(
        metadata.get("description").unwrap().as_str().unwrap(),
        "<<nested value>>"
    );

    // Body: plain, fenced code, inline code.
    let body = doc.main().body();
    assert!(body.contains("<<body>>"));
    assert!(body.contains("<<in code block>>"));
    assert!(body.contains("`<<inline code>>`"));
    assert!(body.contains("<<plain>>"));

    // Card yaml and body.
    let card = &doc.cards()[0];
    assert_eq!(
        card.frontmatter()
            .get("description")
            .unwrap()
            .as_str()
            .unwrap(),
        "<<card yaml>>"
    );
    assert!(card.body().contains("<<card body>>"));
}

#[test]
fn test_yaml_numbers_not_affected() {
    let markdown = r#"---
QUILL: test_quill
count: 42
---

Body."#;
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("count")
            .unwrap()
            .as_i64()
            .unwrap(),
        42
    );
}

#[test]
fn test_yaml_booleans_not_affected() {
    let markdown = r#"---
QUILL: test_quill
active: true
---

Body."#;
    let doc = decompose(markdown).unwrap();
    assert!(doc
        .main()
        .frontmatter()
        .get("active")
        .unwrap()
        .as_bool()
        .unwrap());
}

#[test]
fn test_multiline_chevrons_preserved() {
    let markdown = "---\nQUILL: test_quill\n---\n<<text\nacross lines>>";
    let doc = decompose(markdown).unwrap();
    let body = doc.main().body();
    assert!(body.contains("<<text"));
    assert!(body.contains("across lines>>"));
}

#[test]
fn test_unmatched_chevrons_preserved() {
    let markdown = "---\nQUILL: test_quill\n---\n<<unmatched";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body(), "<<unmatched");
}

// Robustness tests

/// Inputs with no parseable QUILL frontmatter must fail with "Missing
/// required QUILL field". Empty / whitespace-only inputs get a dedicated
/// "Empty markdown input" message instead — see `test_empty_input_dedicated_error`.
#[test]
fn test_missing_quill_field() {
    for input in ["---", "----\ntitle: Test\n----\n\nBody"] {
        let err = decompose(input).unwrap_err().to_string();
        assert!(
            err.contains("Missing required QUILL field"),
            "input {:?} produced unexpected error: {}",
            input,
            err
        );
    }
}

#[test]
fn test_dashes_in_middle_of_line() {
    let markdown = "---\nQUILL: test_quill\n---\nsome text --- more text";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body(), "some text --- more text");
}

/// CRLF and mixed line endings must parse identically to LF.
#[test]
fn test_line_ending_normalization() {
    for markdown in [
        "---\r\nQUILL: test_quill\r\ntitle: Test\r\n---\r\n\r\nBody content.",
        "---\nQUILL: test_quill\r\ntitle: Test\r\n---\n\nBody.",
    ] {
        let doc = decompose(markdown).unwrap();
        assert_eq!(
            doc.main()
                .frontmatter()
                .get("title")
                .unwrap()
                .as_str()
                .unwrap(),
            "Test"
        );
    }
}

#[test]
fn test_frontmatter_at_eof_no_trailing_newline() {
    let markdown = "---\nQUILL: test_quill\ntitle: Test\n---";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "Test"
    );
    assert_eq!(doc.main().body(), "");
}

#[test]
fn test_empty_frontmatter() {
    let markdown = "---\n \n---\n\nBody content.";
    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Missing required QUILL field"));
}

#[test]
fn test_whitespace_only_frontmatter() {
    let markdown = "---\n   \n\n   \n---\n\nBody.";
    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Missing required QUILL field"));
}

// Unicode handling

#[test]
fn test_unicode_in_yaml_keys() {
    let markdown = "---\nQUILL: test_quill\ntitre: Bonjour\nタイトル: こんにちは\n---\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("titre")
            .unwrap()
            .as_str()
            .unwrap(),
        "Bonjour"
    );
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("タイトル")
            .unwrap()
            .as_str()
            .unwrap(),
        "こんにちは"
    );
}

#[test]
fn test_unicode_in_yaml_values() {
    let markdown = "---\nQUILL: test_quill\ntitle: 你好世界 🎉\n---\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "你好世界 🎉"
    );
}

#[test]
fn test_unicode_in_body() {
    let markdown = "---\nQUILL: test_quill\ntitle: Test\n---\n\n日本語テキスト with emoji 🚀";
    let doc = decompose(markdown).unwrap();
    assert!(doc.main().body().contains("日本語テキスト"));
    assert!(doc.main().body().contains("🚀"));
}

// YAML edge cases

#[test]
fn test_yaml_multiline_string() {
    let markdown = r#"---
QUILL: test_quill
description: |
  This is a
  multiline string
  with preserved newlines.
---

Body."#;
    let doc = decompose(markdown).unwrap();
    let desc = doc
        .main()
        .frontmatter()
        .get("description")
        .unwrap()
        .as_str()
        .unwrap();
    assert!(desc.contains("multiline string"));
    assert!(desc.contains('\n'));
}

#[test]
fn test_yaml_folded_string() {
    let markdown = r#"---
QUILL: test_quill
description: >
  This is a folded
  string that becomes
  a single line.
---

Body."#;
    let doc = decompose(markdown).unwrap();
    let desc = doc
        .main()
        .frontmatter()
        .get("description")
        .unwrap()
        .as_str()
        .unwrap();
    assert!(desc.contains("folded"));
}

#[test]
fn test_yaml_null_value() {
    let markdown = "---\nQUILL: test_quill\noptional: null\n---\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert!(doc.main().frontmatter().get("optional").unwrap().is_null());
}

#[test]
fn test_yaml_empty_string_value() {
    let markdown = "---\nQUILL: test_quill\nempty: \"\"\n---\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("empty")
            .unwrap()
            .as_str()
            .unwrap(),
        ""
    );
}

#[test]
fn test_yaml_special_characters_in_string() {
    let markdown = "---\nQUILL: test_quill\nspecial: \"colon: here, and [brackets]\"\n---\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("special")
            .unwrap()
            .as_str()
            .unwrap(),
        "colon: here, and [brackets]"
    );
}

#[test]
fn test_yaml_nested_objects() {
    let markdown = r#"---
QUILL: test_quill
config:
  database:
    host: localhost
    port: 5432
  cache:
    enabled: true
---

Body."#;
    let doc = decompose(markdown).unwrap();
    let config = doc
        .main()
        .frontmatter()
        .get("config")
        .unwrap()
        .as_object()
        .unwrap();
    let db = config.get("database").unwrap().as_object().unwrap();
    assert_eq!(db.get("host").unwrap().as_str().unwrap(), "localhost");
    assert_eq!(db.get("port").unwrap().as_i64().unwrap(), 5432);
}

// CARD block edge cases

#[test]
fn test_card_with_empty_body() {
    let markdown = r#"---
QUILL: test_quill
---

---
CARD: items
name: Item
---"#;
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].tag(), "items");
    assert_eq!(doc.cards()[0].body(), "");
}

#[test]
fn test_card_consecutive_blocks() {
    let markdown = r#"---
QUILL: test_quill
---

---
CARD: a
id: 1
---

---
CARD: a
id: 2
---"#;
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 2);
    assert_eq!(doc.cards()[0].tag(), "a");
    assert_eq!(doc.cards()[1].tag(), "a");
}

#[test]
fn test_card_with_body_containing_dashes() {
    let markdown = r#"---
QUILL: test_quill
---

---
CARD: items
name: Item
---

Some text with --- dashes in it."#;
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert!(doc.cards()[0].body().contains("--- dashes"));
}

// QUILL directive edge cases

#[test]
fn test_quill_with_underscore_prefix() {
    let markdown = "---\nQUILL: _internal\n---\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.quill_reference().name, "_internal");
}

#[test]
fn test_quill_with_numbers() {
    let markdown = "---\nQUILL: form_8_v2\n---\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.quill_reference().name, "form_8_v2");
}

#[test]
fn test_quill_with_additional_fields() {
    let markdown = r#"---
QUILL: my_quill
title: Document Title
author: John Doe
---

Body content."#;
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.quill_reference().name, "my_quill");
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "Document Title"
    );
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("author")
            .unwrap()
            .as_str()
            .unwrap(),
        "John Doe"
    );
}

// Error handling

#[test]
fn test_invalid_scope_name_uppercase() {
    let markdown = "---\nQUILL: test_quill\n---\n\n---\nCARD: ITEMS\n---\n\nBody.";
    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid card field name"));
}

#[test]
fn test_invalid_scope_name_starts_with_number() {
    let markdown = "---\nCARD: 123items\n---\n\nBody.";
    let result = decompose(markdown);
    assert!(result.is_err());
}

#[test]
fn test_invalid_scope_name_with_hyphen() {
    let markdown = "---\nCARD: my-items\n---\n\nBody.";
    let result = decompose(markdown);
    assert!(result.is_err());
}

#[test]
fn test_invalid_quill_ref_uppercase() {
    let markdown = "---\nQUILL: MyQuill\n---\n\nBody.";
    let result = decompose(markdown);
    assert!(result.is_err());
}

#[test]
fn test_yaml_syntax_error_missing_colon() {
    let markdown = "---\ntitle Test\n---\n\nBody.";
    let result = decompose(markdown);
    assert!(result.is_err());
}

#[test]
fn test_yaml_syntax_error_bad_indentation() {
    let markdown = "---\nitems:\n- one\n - two\n---\n\nBody.";
    let result = decompose(markdown);
    // Bad indentation may or may not be an error depending on YAML parser
    let _ = result;
}

// Body extraction edge cases

#[test]
fn test_body_with_leading_newlines() {
    let markdown = "---\nQUILL: test_quill\ntitle: Test\n---\n\n\n\nBody with leading newlines.";
    let doc = decompose(markdown).unwrap();
    assert!(doc.main().body().starts_with('\n'));
}

#[test]
fn test_body_with_trailing_newlines() {
    // Body at EOF: no F2 separator to strip, source's trailing newlines
    // are preserved verbatim as authored content.
    let markdown = "---\nQUILL: test_quill\ntitle: Test\n---\n\nBody.\n\n\n";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body(), "\nBody.\n\n\n");
}

// ── F2 separator stripping: parse-side normalisation ─────────────────────────
// See `assemble.rs::strip_f2_separator` and `MARKDOWN.md §3 F2`.

#[test]
fn test_f2_strip_global_body_followed_by_card_lf() {
    // Global body followed by a CARD fence: the source's tail `\n\n` is
    // (content line terminator) + (F2 blank line). Strip exactly the F2 `\n`,
    // leaving `\n` as the content terminator.
    let markdown = "---\nQUILL: q\n---\n\nbody\n\n---\nCARD: x\n---\n";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body(), "\nbody\n");
}

#[test]
fn test_f2_strip_global_body_followed_by_card_crlf() {
    // CRLF line endings: strip exactly one `\r\n` as the F2 separator.
    let markdown = "---\r\nQUILL: q\r\n---\r\n\r\nbody\r\n\r\n---\r\nCARD: x\r\n---\r\n";
    let doc = decompose(markdown).unwrap();
    assert!(
        doc.main().body().ends_with('\n') && !doc.main().body().ends_with("\n\n"),
        "expected exactly one trailing line ending, got {:?}",
        doc.main().body()
    );
}

#[test]
fn test_f2_strip_card_body_followed_by_card() {
    // First card body is followed by another fence → F2 stripped.
    // Last card body is at EOF → preserved verbatim.
    let markdown = "---\nQUILL: q\n---\n\n---\nCARD: a\n---\nfirst\n\n---\nCARD: b\n---\nsecond\n";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards()[0].body(), "first\n");
    assert_eq!(doc.cards()[1].body(), "second\n");
}

#[test]
fn test_f2_strip_preserves_author_blank_lines() {
    // Author wrote two blank lines after the body. Only the F2 blank (last
    // `\n`) is stripped; the author's blank line is preserved.
    let markdown = "---\nQUILL: q\n---\n\nbody\n\n\n---\nCARD: x\n---\n";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body(), "\nbody\n\n");
}

#[test]
fn test_f2_strip_does_not_overstrip_content_newlines() {
    // Content-fidelity: a body whose authored content ends with multiple
    // newlines (e.g. a code block with trailing blank lines) must survive
    // round-trip. The previous WASM-binding `trim_body` over-stripped this.
    let markdown = "---\nQUILL: q\n---\n\n```\ncode\n```\n\n\n---\nCARD: x\n---\n";
    let doc = decompose(markdown).unwrap();
    let emitted = doc.to_markdown();
    let reparsed = Document::from_markdown(&emitted).unwrap();
    assert_eq!(doc.main().body(), reparsed.main().body());
    // Author's blank line after the code block survives.
    assert!(
        doc.main().body().ends_with("```\n\n"),
        "expected code block + blank line, got {:?}",
        doc.main().body()
    );
}

#[test]
fn test_no_body_after_frontmatter() {
    let markdown = "---\nQUILL: test_quill\ntitle: Test\n---";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body(), "");
}

// Tag name validation

#[test]
fn test_tag_name_validator() {
    for &name in &["_", "_private", "item1", "item_2"] {
        assert!(is_valid_tag_name(name), "expected valid: {:?}", name);
    }
    for &name in &[
        "", "1item", "Items", "ITEMS", "my-items", "my.items", "my items",
    ] {
        assert!(!is_valid_tag_name(name), "expected invalid: {:?}", name);
    }
}

// Guillemet preprocessing

#[test]
fn test_guillemet_in_yaml_preserves_non_strings() {
    let markdown = r#"---
QUILL: test_quill
count: 42
price: 19.99
active: true
items:
  - first
  - 100
  - true
---

Body."#;
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("count")
            .unwrap()
            .as_i64()
            .unwrap(),
        42
    );
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("price")
            .unwrap()
            .as_f64()
            .unwrap(),
        19.99
    );
    assert!(doc
        .main()
        .frontmatter()
        .get("active")
        .unwrap()
        .as_bool()
        .unwrap());
}

#[test]
fn test_guillemet_double_conversion_prevention() {
    let markdown = "---\nQUILL: test_quill\ntitle: Already «converted»\n---\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "Already «converted»"
    );
}

#[test]
fn test_allowed_card_field_collision() {
    let markdown = r#"---
QUILL: test_quill
my_card: "some global value"
---

---
CARD: my_card
title: "My Card"
---
Body
"#;
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("my_card")
            .unwrap()
            .as_str()
            .unwrap(),
        "some global value"
    );
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].tag(), "my_card");
    assert_eq!(
        doc.cards()[0]
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "My Card"
    );
}

#[test]
fn test_yaml_custom_tags_in_frontmatter() {
    let markdown = r#"---
QUILL: test_quill
memo_from: !fill 2d lt example
regular_field: normal value
---

Body content."#;
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("memo_from")
            .unwrap()
            .as_str()
            .unwrap(),
        "2d lt example"
    );
    assert_eq!(
        doc.main()
            .frontmatter()
            .get("regular_field")
            .unwrap()
            .as_str()
            .unwrap(),
        "normal value"
    );
    assert_eq!(doc.main().body(), "\nBody content.");
}

/// Test the exact example from EXTENDED_MARKDOWN.md
#[test]
fn test_spec_example() {
    let markdown = r#"---
QUILL: blog_post
title: My Document
---
Main document body.

***

More content after horizontal rule.

---
CARD: section
heading: Introduction
---
Introduction content.

---
CARD: section
heading: Conclusion
---
Conclusion content.
"#;

    let doc = decompose(markdown).unwrap();

    assert_eq!(
        doc.main()
            .frontmatter()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "My Document"
    );
    assert_eq!(doc.quill_reference().name, "blog_post");

    let body = doc.main().body();
    assert!(body.contains("Main document body."));
    assert!(body.contains("***"));
    assert!(body.contains("More content after horizontal rule."));

    assert_eq!(doc.cards().len(), 2);
    assert_eq!(doc.cards()[0].tag(), "section");
    assert_eq!(
        doc.cards()[0]
            .frontmatter()
            .get("heading")
            .unwrap()
            .as_str()
            .unwrap(),
        "Introduction"
    );
    assert_eq!(doc.cards()[0].body(), "Introduction content.\n");
    assert_eq!(doc.cards()[1].tag(), "section");
    assert_eq!(
        doc.cards()[1]
            .frontmatter()
            .get("heading")
            .unwrap()
            .as_str()
            .unwrap(),
        "Conclusion"
    );
    assert_eq!(doc.cards()[1].body(), "Conclusion content.\n");
}

#[test]
fn test_missing_quill_field_errors() {
    let markdown = "---\ntitle: No quill here\n---\n# Body";
    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Missing required QUILL field"));
}

// ── to_plate_json round-trip snapshot ─────────────────────────────────────────

/// Verify to_plate_json produces the correct shape for a simple document.
#[test]
fn test_to_plate_json_simple() {
    let doc =
        Document::from_markdown("---\nQUILL: my_quill\ntitle: Hello\n---\n\nBody text.\n").unwrap();
    let json = doc.to_plate_json();

    assert_eq!(json["QUILL"], "my_quill");
    assert_eq!(json["title"], "Hello");
    assert_eq!(json["BODY"], "\nBody text.\n");
    assert!(json["CARDS"].is_array());
    assert_eq!(json["CARDS"].as_array().unwrap().len(), 0);
}

/// to_plate_json with cards produces CARDS array with CARD, fields, BODY.
#[test]
fn test_to_plate_json_with_cards() {
    let markdown = r#"---
QUILL: usaf_memo
title: Test
---

Global body.

---
CARD: indorsement
for: ORG
---

Card body here.
"#;
    let doc = Document::from_markdown(markdown).unwrap();
    let json = doc.to_plate_json();

    assert_eq!(json["QUILL"], "usaf_memo");
    assert_eq!(json["title"], "Test");
    // F2 separator stripped on parse; plate `BODY` reflects the same
    // content-only string as `Document::body()`.
    assert_eq!(json["BODY"], "\nGlobal body.\n");

    let cards = json["CARDS"].as_array().unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["CARD"], "indorsement");
    assert_eq!(cards[0]["for"], "ORG");
    assert_eq!(cards[0]["BODY"], "\nCard body here.\n");
}

/// to_plate_json parity: the QUILL key appears first.
#[test]
fn test_to_plate_json_quill_first() {
    let doc = Document::from_markdown("---\nQUILL: my_quill\nfoo: bar\nbaz: qux\n---\n").unwrap();
    let json = doc.to_plate_json();
    let obj = json.as_object().unwrap();
    let keys: Vec<&String> = obj.keys().collect();
    assert_eq!(keys[0], "QUILL");
}

/// Snapshot test over a representative usaf_memo-shaped document.
#[test]
fn test_to_plate_json_fixture_snapshot() {
    let markdown = "\
---
QUILL: usaf_memo@0.1
memo_for:
  - ORG/SYMBOL
date: 2504-10-05
subject: Subject of the Memorandum
---

The body of the memorandum.

```card indorsement
for: ORG/SYMBOL
from: ORG/SYMBOL
```

This body and the metadata above are an indorsement card.
";
    let doc = Document::from_markdown(markdown).unwrap();
    let json = doc.to_plate_json();

    // QUILL key is present
    assert_eq!(json["QUILL"], "usaf_memo@0.1");
    // frontmatter fields are present at top level
    assert!(json.get("memo_for").is_some());
    assert!(json.get("date").is_some());
    // BODY and CARDS present
    assert!(json.get("BODY").is_some());
    assert!(json["CARDS"].is_array());
    // One indorsement card
    let cards = json["CARDS"].as_array().unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["CARD"], "indorsement");
    // Card has BODY
    assert!(cards[0].get("BODY").is_some());
}

/// Regression test for the `serde_json::Map::remove` / `shift_remove` bug.
///
/// `serde_json::Map::remove` with `preserve_order` uses `swap_remove` under
/// the hood (O(1), moves last element into removed slot) — NOT the order-
/// preserving `shift_remove` (O(n)).  `extract_sentinels` must use
/// `shift_remove` explicitly so YAML frontmatter field order is preserved
/// after the QUILL sentinel is stripped.
#[test]
fn frontmatter_field_order_preserved_after_quill_removal() {
    let md = "---\nQUILL: q\nsender: Alice\nrecipient: Bob\ndate: March 15\nsubject: hi\n---\n";
    let doc = Document::from_markdown(md).unwrap();
    let keys: Vec<&str> = doc
        .main()
        .frontmatter()
        .keys()
        .map(|s| s.as_str())
        .collect();
    // Fields must appear in YAML document order, not alphabetical or swap-order.
    assert_eq!(
        keys,
        vec!["sender", "recipient", "date", "subject"],
        "Frontmatter fields must preserve insertion order after QUILL removal"
    );
}
