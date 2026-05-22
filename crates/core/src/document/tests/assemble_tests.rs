use crate::document::assemble::decompose;
use crate::document::meta::is_valid_kind_name;
use crate::document::Document;

#[test]
fn test_no_payload() {
    let markdown = "# Hello World\n\nThis is a test.";
    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Missing required root card-yaml block"));
}

#[test]
fn test_empty_input_dedicated_error() {
    // Empty input gets a dedicated message distinct from the missing-root one.
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
fn test_missing_quill_diagnostic_code() {
    // Documents with no `~~~card-yaml` block at all surface the dedicated
    // `parse::missing_quill` code.
    let cases = [
        "# Hello World\n\nNo payload here.",
        "Just prose, no card-yaml block.",
    ];
    for input in cases {
        let err = decompose(input).unwrap_err();
        let diag = err.to_diagnostic();
        assert_eq!(
            diag.code.as_deref(),
            Some("parse::missing_quill"),
            "expected parse::missing_quill for {input:?}, got: {:?}",
            diag.code
        );
    }
}

#[test]
fn test_with_payload() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test Document
author: Test Author
~~~

# Hello World

This is the body.";

    let doc = decompose(markdown).unwrap();

    assert_eq!(doc.main().body(), "\n# Hello World\n\nThis is the body.");
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Test Document"
    );
    assert_eq!(
        doc.main()
            .payload()
            .get("author")
            .unwrap()
            .as_str()
            .unwrap(),
        "Test Author"
    );
    assert_eq!(doc.main().payload().len(), 2); // title, author
    assert_eq!(doc.cards().len(), 0);
    assert_eq!(doc.quill_reference().name, "test_quill");
}

#[test]
fn test_complex_yaml_payload() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Complex Document
tags:
  - test
  - yaml
metadata:
  version: 1.0
  nested:
    field: value
~~~

Content here.";

    let doc = decompose(markdown).unwrap();

    assert_eq!(doc.main().body(), "\nContent here.");
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Complex Document"
    );

    let tags = doc
        .main()
        .payload()
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
    // Root card-yaml block with invalid YAML payload.
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: [invalid yaml
author: missing close bracket
~~~

Content here.";

    let result = decompose(markdown);
    assert!(result.is_err());
    // Error message now includes location context
    assert!(result.unwrap_err().to_string().contains("YAML error"));
}

#[test]
fn test_unclosed_payload() {
    // Root card-yaml block without a closing `~~~` fence.
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test
author: Test Author

Content without closing fence";

    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("never closed with `~~~`"));
}

// Extended metadata tests

#[test]
fn test_basic_card_block() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Main Document
~~~

Main body content.

~~~card-yaml
$kind: items
name: Item 1
~~~

Body of item 1.";

    let doc = decompose(markdown).unwrap();

    // Global body is followed by a card block: blank-line separator stripped,
    // so the trailing `\n\n` from the source becomes a single `\n`.
    assert_eq!(doc.main().body(), "\nMain body content.\n");
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Main Document"
    );

    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.kind(), Some("items"));
    assert_eq!(
        card.payload().get("name").unwrap().as_str().unwrap(),
        "Item 1"
    );
    // Last card body at EOF: no blank-line separator to strip.
    assert_eq!(card.body(), "\nBody of item 1.");
}

#[test]
fn test_multiple_card_blocks() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: items
name: Item 1
tags: [a, b]
~~~

First item body.

~~~card-yaml
$kind: items
name: Item 2
tags: [c, d]
~~~

Second item body.";

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 2);

    let card1 = &doc.cards()[0];
    assert_eq!(card1.kind(), Some("items"));
    assert_eq!(
        card1.payload().get("name").unwrap().as_str().unwrap(),
        "Item 1"
    );

    let card2 = &doc.cards()[1];
    assert_eq!(card2.kind(), Some("items"));
    assert_eq!(
        card2.payload().get("name").unwrap().as_str().unwrap(),
        "Item 2"
    );
}

#[test]
fn test_mixed_global_and_cards() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Global
author: John Doe
~~~

Global body.

~~~card-yaml
$kind: sections
title: Section 1
~~~

Section 1 content.

~~~card-yaml
$kind: sections
title: Section 2
~~~

Section 2 content.";

    let doc = decompose(markdown).unwrap();

    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Global"
    );
    assert_eq!(doc.main().body(), "\nGlobal body.\n");
    assert_eq!(doc.cards().len(), 2);
    assert_eq!(doc.cards()[0].kind(), Some("sections"));
}

#[test]
fn test_empty_card_metadata() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: items
~~~

Body without metadata.";

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.kind(), Some("items"));
    assert!(card.payload().is_empty());
    assert_eq!(card.body(), "\nBody without metadata.");
}

#[test]
fn test_card_block_without_body() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: items
name: Item
~~~";

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.kind(), Some("items"));
    assert_eq!(card.body(), ""); // empty, not absent
}

#[test]
fn test_name_collision_global_and_card() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
items: \"global value\"
~~~

Body

~~~card-yaml
$kind: items
name: Item
~~~

Item body";

    let result = decompose(markdown);
    assert!(result.is_ok(), "Name collision should be allowed now");
}

#[test]
fn test_card_name_collision_with_array_field() {
    // Card kind names CAN now conflict with payload field names.
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
items:
  - name: Global Item 1
    value: 100
~~~

Global body

~~~card-yaml
$kind: items
name: Scope Item 1
~~~

Scope item 1 body";

    let result = decompose(markdown);
    assert!(
        result.is_ok(),
        "Collision with array field should be allowed"
    );
}

#[test]
fn test_empty_global_array_with_card() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
items: []
~~~

Global body

~~~card-yaml
$kind: items
name: Item 1
~~~

Item 1 body";

    let result = decompose(markdown);
    assert!(
        result.is_ok(),
        "Collision with empty array field should be allowed"
    );
}

#[test]
fn test_uppercase_payload_keys_pass_parser() {
    // Uppercase YAML keys (e.g. legacy `BODY`/`CARDS`) used to be rejected
    // because they collided with the flat plate-JSON wire format. The wire
    // format now namespaces metadata under `$`-prefixed keys, so user
    // payload keys can be arbitrary — they're filtered downstream by the
    // editor surface's `[a-z_][a-z0-9_]*` rule, not the parser.
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: section
BODY: Test
~~~";

    let result = decompose(markdown);
    assert!(result.is_ok(), "uppercase payload keys parse fine");
}

#[test]
fn test_delimiter_inside_fenced_code_block_backticks() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test
~~~

Here is some code:

```yaml
~~~card-yaml
$kind: code_example
fake: payload
~~~
```

More content.
";

    let doc = decompose(markdown).unwrap();
    // The card-yaml inside the code block should NOT be parsed as metadata.
    assert!(doc.main().body().contains("fake: payload"));
    assert!(doc.main().payload().get("fake").is_none());
    assert_eq!(doc.cards().len(), 0);
}

#[test]
fn test_four_backticks_are_fences() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test
~~~

Here is some code:

````yaml
~~~card-yaml
$kind: code_example
fake: payload
~~~
````

More content.
";

    let doc = decompose(markdown).unwrap();
    assert!(doc.main().body().contains("fake: payload"));
    assert!(doc.main().payload().get("fake").is_none());
    assert_eq!(doc.cards().len(), 0);
}

#[test]
fn test_adjacent_blocks_different_kinds() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: items
name: Item 1
~~~

Item 1 body

~~~card-yaml
$kind: sections
title: Section 1
~~~

Section 1 body";

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 2);

    let card0 = &doc.cards()[0];
    assert_eq!(card0.kind(), Some("items"));
    assert_eq!(
        card0.payload().get("name").unwrap().as_str().unwrap(),
        "Item 1"
    );

    let card1 = &doc.cards()[1];
    assert_eq!(card1.kind(), Some("sections"));
    assert_eq!(
        card1.payload().get("title").unwrap().as_str().unwrap(),
        "Section 1"
    );
}

#[test]
fn test_order_preservation() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: items
id: 1
~~~

First

~~~card-yaml
$kind: items
id: 2
~~~

Second

~~~card-yaml
$kind: items
id: 3
~~~

Third";

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 3);

    for (i, card) in doc.cards().iter().enumerate() {
        assert_eq!(card.kind(), Some("items"));
        let id = card.payload().get("id").unwrap().as_i64().unwrap();
        assert_eq!(id, (i + 1) as i64);
    }
}

#[test]
fn test_product_catalog_integration() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Product Catalog
author: John Doe
date: 2024-01-01
~~~

This is the main catalog description.

~~~card-yaml
$kind: products
name: Widget A
price: 19.99
sku: WID-001
~~~

The **Widget A** is our most popular product.

~~~card-yaml
$kind: products
name: Gadget B
price: 29.99
sku: GAD-002
~~~

The **Gadget B** is perfect for professionals.

~~~card-yaml
$kind: reviews
product: Widget A
rating: 5
~~~

\"Excellent product! Highly recommended.\"

~~~card-yaml
$kind: reviews
product: Gadget B
rating: 4
~~~

\"Very good, but a bit pricey.\"";

    let doc = decompose(markdown).unwrap();

    // Verify global payload
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Product Catalog"
    );
    assert_eq!(
        doc.main()
            .payload()
            .get("author")
            .unwrap()
            .as_str()
            .unwrap(),
        "John Doe"
    );
    assert_eq!(
        doc.main().payload().get("date").unwrap().as_str().unwrap(),
        "2024-01-01"
    );

    // Verify global body
    assert!(doc.main().body().contains("main catalog description"));

    // 4 cards total
    assert_eq!(doc.cards().len(), 4);

    // First 2 are products
    assert_eq!(doc.cards()[0].kind(), Some("products"));
    assert_eq!(
        doc.cards()[0]
            .payload()
            .get("name")
            .unwrap()
            .as_str()
            .unwrap(),
        "Widget A"
    );
    assert_eq!(
        doc.cards()[0]
            .payload()
            .get("price")
            .unwrap()
            .as_f64()
            .unwrap(),
        19.99
    );

    assert_eq!(doc.cards()[1].kind(), Some("products"));
    assert_eq!(
        doc.cards()[1]
            .payload()
            .get("name")
            .unwrap()
            .as_str()
            .unwrap(),
        "Gadget B"
    );

    // Last 2 are reviews
    assert_eq!(doc.cards()[2].kind(), Some("reviews"));
    assert_eq!(
        doc.cards()[2]
            .payload()
            .get("product")
            .unwrap()
            .as_str()
            .unwrap(),
        "Widget A"
    );
    assert_eq!(
        doc.cards()[2]
            .payload()
            .get("rating")
            .unwrap()
            .as_i64()
            .unwrap(),
        5
    );

    // Payload has 3 fields: title, author, date
    assert_eq!(doc.main().payload().len(), 3);
}

#[test]
fn taro_quill_directive() {
    let markdown = "~~~card-yaml
$quill: usaf_memo
$kind: main
memo_for: [ORG/SYMBOL]
memo_from: [ORG/SYMBOL]
~~~

This is the memo body.";

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.quill_reference().name, "usaf_memo");
    assert_eq!(
        doc.main()
            .payload()
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
    let markdown = "~~~card-yaml
$quill: document
$kind: main
title: Test Document
~~~

Main body.

~~~card-yaml
$kind: sections
name: Section 1
~~~

Section 1 body.";

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.quill_reference().name, "document");
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Test Document"
    );
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), Some("sections"));
    assert_eq!(doc.main().body(), "\nMain body.\n");
}

#[test]
fn test_non_root_block_declaring_quill_is_error() {
    // Only the root block binds the document to a quill. A composable card
    // declaring `$quill` is a structural parse error.
    let markdown = "~~~card-yaml
$quill: first
$kind: main
~~~

~~~card-yaml
$quill: second
$kind: note
~~~";

    let err = decompose(markdown).unwrap_err().to_string();
    assert!(err.contains("must not declare `$quill`"), "got: {err}");
}

#[test]
fn test_invalid_quill_ref() {
    let markdown = "~~~card-yaml
$quill: Invalid-Name
$kind: main
~~~";

    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid $quill reference"));
}

#[test]
fn test_quill_empty_value() {
    let markdown = "~~~card-yaml
$quill:
~~~";

    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid $quill reference"));
}

#[test]
fn test_card_with_unknown_meta_key_is_error() {
    // `$`-prefixed metadata keys are a closed set `{quill, kind, id}`. Any other `$key`
    // is a parse error.
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$foo: bar
$kind: note
~~~";

    let err = decompose(markdown).unwrap_err().to_string();
    assert!(
        err.contains("Unknown `$foo`"),
        "expected unknown-key parse error, got: {err}"
    );
}

#[test]
fn dollar_keys_at_any_position_in_payload_work() {
    // `$`-prefixed reserved keys are ordinary YAML; they may appear at any
    // position in the block's mapping. Emit preserves source order so that
    // any comments adjacent to a `$` line round-trip in place.
    let markdown = "~~~card-yaml
title: First
$quill: test_quill
author: Bob
$kind: main
~~~

Body.";

    let doc = decompose(markdown).expect("payload with $-keys mid-mapping should parse");
    assert_eq!(doc.main().quill().unwrap().to_string(), "test_quill");
    assert_eq!(doc.main().kind(), Some("main"));
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str(),
        Some("First")
    );
    assert_eq!(
        doc.main().payload().get("author").unwrap().as_str(),
        Some("Bob")
    );
    // `$` keys do not appear in the user-field accessors.
    assert!(doc.main().payload().get("$quill").is_none());
    assert!(doc.main().payload().get("$kind").is_none());

    // Round-trip stability: emit then re-parse produces an equal Document.
    let emitted = doc.to_markdown();
    let reparsed = decompose(&emitted).expect("round-trip should re-parse");
    assert_eq!(doc, reparsed);
}

#[test]
fn fill_on_dollar_key_is_rejected() {
    // `!fill` is not permitted on `$` metadata keys — they are extracted
    // into typed metadata and have no placeholder semantics.
    let markdown = "~~~card-yaml
$quill: !fill test_quill
$kind: main
~~~";
    let err = decompose(markdown).unwrap_err().to_string();
    assert!(
        err.contains("`!fill`") && err.contains("$quill"),
        "expected !fill-on-$ rejection, got: {err}"
    );
}

#[test]
fn test_blank_lines_in_payload() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test Document
author: Test Author

description: This has a blank line above it
tags:
  - one
  - two
~~~

# Hello World

This is the body.";

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body(), "\n# Hello World\n\nThis is the body.");
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Test Document"
    );
    assert_eq!(
        doc.main()
            .payload()
            .get("author")
            .unwrap()
            .as_str()
            .unwrap(),
        "Test Author"
    );
    assert_eq!(
        doc.main()
            .payload()
            .get("description")
            .unwrap()
            .as_str()
            .unwrap(),
        "This has a blank line above it"
    );
    let tags = doc
        .main()
        .payload()
        .get("tags")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(tags.len(), 2);
}

#[test]
fn test_blank_lines_in_scope_blocks() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: items
name: Item 1

price: 19.99

tags:
  - electronics
  - gadgets
~~~

Body of item 1.";

    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 1);
    let card = &doc.cards()[0];
    assert_eq!(card.kind(), Some("items"));
    assert_eq!(
        card.payload().get("name").unwrap().as_str().unwrap(),
        "Item 1"
    );
    assert_eq!(
        card.payload().get("price").unwrap().as_f64().unwrap(),
        19.99
    );
    let tags = card.payload().get("tags").unwrap().as_array().unwrap();
    assert_eq!(tags.len(), 2);
}

#[test]
fn test_triple_dash_between_paragraphs_is_delegated() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test
~~~

First paragraph.

---

Second paragraph.";

    let doc = decompose(markdown).unwrap();
    let body = doc.main().body();
    assert!(body.contains("First paragraph."));
    assert!(body.contains("Second paragraph."));
    assert!(body.contains("---"));
}

#[test]
fn test_lone_triple_dash_in_body_is_delegated() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test
~~~

First paragraph.
---

Second paragraph.";

    let doc = decompose(markdown).unwrap();
    let body = doc.main().body();
    assert!(body.contains("First paragraph."));
    assert!(body.contains("Second paragraph."));
    assert!(body.contains("---"));
}

#[test]
fn test_multiple_blank_lines_in_yaml() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test


author: John Doe


version: 1.0
~~~

Body content.";

    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Test"
    );
    assert_eq!(
        doc.main()
            .payload()
            .get("author")
            .unwrap()
            .as_str()
            .unwrap(),
        "John Doe"
    );
    assert_eq!(
        doc.main()
            .payload()
            .get("version")
            .unwrap()
            .as_f64()
            .unwrap(),
        1.0
    );
}

// --- demo_file_test ---

#[test]
fn test_extended_metadata_demo_file() {
    let markdown = include_str!("../../../../fixtures/resources/extended_metadata_demo.md");
    let doc = decompose(markdown).unwrap();

    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Extended Metadata Demo"
    );
    assert_eq!(
        doc.main()
            .payload()
            .get("author")
            .unwrap()
            .as_str()
            .unwrap(),
        "Quillmark Team"
    );
    // version is parsed as a number by YAML
    assert_eq!(
        doc.main()
            .payload()
            .get("version")
            .unwrap()
            .as_f64()
            .unwrap(),
        1.0
    );

    // Verify body
    assert!(doc.main().body().contains("card-yaml metadata format"));

    // 5 cards total: 3 features + 2 use_cases
    assert_eq!(doc.cards().len(), 5);

    let features_count = doc
        .cards()
        .iter()
        .filter(|c| c.kind() == Some("features"))
        .count();
    let use_cases_count = doc
        .cards()
        .iter()
        .filter(|c| c.kind() == Some("use_cases"))
        .count();
    assert_eq!(features_count, 3);
    assert_eq!(use_cases_count, 2);

    // Check first card is a feature
    assert_eq!(doc.cards()[0].kind(), Some("features"));
    assert_eq!(
        doc.cards()[0]
            .payload()
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
    let mut markdown = String::from("~~~card-yaml\n$quill: test_quill\n$kind: main\n");
    let size = crate::error::MAX_YAML_SIZE + 1;
    markdown.push_str("data: \"");
    markdown.push_str(&"x".repeat(size));
    markdown.push_str("\"\n~~~\n\nBody");

    let result = decompose(&markdown);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Input too large"));
}

#[test]
fn test_input_within_size_limit() {
    let size = 1000;
    let markdown = format!(
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Test\n~~~\n\n{}",
        "a".repeat(size)
    );

    let result = decompose(&markdown);
    assert!(result.is_ok());
}

#[test]
fn test_yaml_within_size_limit() {
    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Test\nauthor: John Doe\n~~~\n\nBody content";
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

    let markdown = format!(
        "~~~card-yaml\n$quill: test_quill\n$kind: main\n{}~~~\n\nBody",
        yaml_content
    );
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
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
level1:
  level2:
    level3:
      level4:
        value: test
~~~

Body content";

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
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
title: Test <<with chevrons>>
items:
  - \"<<first>>\"
  - \"<<second>>\"
metadata:
  description: \"<<nested value>>\"
~~~

<<body>> text.

```
<<in code block>>
```

`<<inline code>>` and <<plain>>

~~~card-yaml
$kind: items
description: \"<<card yaml>>\"
~~~

Use <<card body>> here.";

    let doc = decompose(markdown).unwrap();

    // Payload scalar, array, nested map.
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Test <<with chevrons>>"
    );
    let items = doc
        .main()
        .payload()
        .get("items")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(items[0].as_str().unwrap(), "<<first>>");
    assert_eq!(items[1].as_str().unwrap(), "<<second>>");
    let metadata = doc
        .main()
        .payload()
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
        card.payload().get("description").unwrap().as_str().unwrap(),
        "<<card yaml>>"
    );
    assert!(card.body().contains("<<card body>>"));
}

#[test]
fn test_yaml_numbers_not_affected() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
count: 42
~~~

Body.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main().payload().get("count").unwrap().as_i64().unwrap(),
        42
    );
}

#[test]
fn test_yaml_booleans_not_affected() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
active: true
~~~

Body.";
    let doc = decompose(markdown).unwrap();
    assert!(doc
        .main()
        .payload()
        .get("active")
        .unwrap()
        .as_bool()
        .unwrap());
}

#[test]
fn test_multiline_chevrons_preserved() {
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\n~~~\n\n<<text\nacross lines>>";
    let doc = decompose(markdown).unwrap();
    let body = doc.main().body();
    assert!(body.contains("<<text"));
    assert!(body.contains("across lines>>"));
}

#[test]
fn test_unmatched_chevrons_preserved() {
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\n~~~\n\n<<unmatched";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body(), "\n<<unmatched");
}

// Robustness tests

/// Inputs with no `~~~card-yaml` block must fail with the missing-root error.
#[test]
fn test_missing_quill() {
    for input in ["plain text", "# Heading\n\nbody"] {
        let err = decompose(input).unwrap_err().to_string();
        assert!(
            err.contains("Missing required root card-yaml block"),
            "input {:?} produced unexpected error: {}",
            input,
            err
        );
    }
}

#[test]
fn test_dashes_in_middle_of_line() {
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\n~~~\n\nsome text --- more text";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body(), "\nsome text --- more text");
}

/// CRLF and mixed line endings must parse identically to LF.
#[test]
fn test_line_ending_normalization() {
    for markdown in [
        "~~~card-yaml\r\n$quill: test_quill\r\n$kind: main\r\ntitle: Test\r\n~~~\r\n\r\nBody content.",
        "~~~card-yaml\n$quill: test_quill\r\n$kind: main\r\ntitle: Test\r\n~~~\n\nBody.",
    ] {
        let doc = decompose(markdown).unwrap();
        assert_eq!(
            doc.main().payload().get("title").unwrap().as_str().unwrap(),
            "Test"
        );
    }
}

#[test]
fn test_payload_at_eof_no_trailing_newline() {
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Test\n~~~";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Test"
    );
    assert_eq!(doc.main().body(), "");
}

// Unicode handling

#[test]
fn test_unicode_in_yaml_keys() {
    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitre: Bonjour\nタイトル: こんにちは\n~~~\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main().payload().get("titre").unwrap().as_str().unwrap(),
        "Bonjour"
    );
    assert_eq!(
        doc.main()
            .payload()
            .get("タイトル")
            .unwrap()
            .as_str()
            .unwrap(),
        "こんにちは"
    );
}

#[test]
fn test_unicode_in_yaml_values() {
    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: 你好世界 🎉\n~~~\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "你好世界 🎉"
    );
}

#[test]
fn test_unicode_in_body() {
    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Test\n~~~\n\n日本語テキスト with emoji 🚀";
    let doc = decompose(markdown).unwrap();
    assert!(doc.main().body().contains("日本語テキスト"));
    assert!(doc.main().body().contains("🚀"));
}

// YAML edge cases

#[test]
fn test_yaml_multiline_string() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
description: |
  This is a
  multiline string
  with preserved newlines.
~~~

Body.";
    let doc = decompose(markdown).unwrap();
    let desc = doc
        .main()
        .payload()
        .get("description")
        .unwrap()
        .as_str()
        .unwrap();
    assert!(desc.contains("multiline string"));
    assert!(desc.contains('\n'));
}

#[test]
fn test_yaml_folded_string() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
description: >
  This is a folded
  string that becomes
  a single line.
~~~

Body.";
    let doc = decompose(markdown).unwrap();
    let desc = doc
        .main()
        .payload()
        .get("description")
        .unwrap()
        .as_str()
        .unwrap();
    assert!(desc.contains("folded"));
}

#[test]
fn test_yaml_null_value() {
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\noptional: null\n~~~\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert!(doc.main().payload().get("optional").unwrap().is_null());
}

#[test]
fn test_yaml_empty_string_value() {
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\nempty: \"\"\n~~~\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main().payload().get("empty").unwrap().as_str().unwrap(),
        ""
    );
}

#[test]
fn test_yaml_special_characters_in_string() {
    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\nspecial: \"colon: here, and [brackets]\"\n~~~\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main()
            .payload()
            .get("special")
            .unwrap()
            .as_str()
            .unwrap(),
        "colon: here, and [brackets]"
    );
}

#[test]
fn test_yaml_nested_objects() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
config:
  database:
    host: localhost
    port: 5432
  cache:
    enabled: true
~~~

Body.";
    let doc = decompose(markdown).unwrap();
    let config = doc
        .main()
        .payload()
        .get("config")
        .unwrap()
        .as_object()
        .unwrap();
    let db = config.get("database").unwrap().as_object().unwrap();
    assert_eq!(db.get("host").unwrap().as_str().unwrap(), "localhost");
    assert_eq!(db.get("port").unwrap().as_i64().unwrap(), 5432);
}

// Card block edge cases

#[test]
fn test_card_with_empty_body() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: items
name: Item
~~~";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), Some("items"));
    assert_eq!(doc.cards()[0].body(), "");
}

#[test]
fn test_card_consecutive_blocks() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: a
id: 1
~~~

~~~card-yaml
$kind: a
id: 2
~~~";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 2);
    assert_eq!(doc.cards()[0].kind(), Some("a"));
    assert_eq!(doc.cards()[1].kind(), Some("a"));
}

#[test]
fn test_card_with_body_containing_dashes() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
~~~

~~~card-yaml
$kind: items
name: Item
~~~

Some text with --- dashes in it.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert!(doc.cards()[0].body().contains("--- dashes"));
}

// `$quill` reference edge cases

#[test]
fn test_quill_with_underscore_prefix() {
    let markdown = "~~~card-yaml\n$quill: _internal\n$kind: main\n~~~\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.quill_reference().name, "_internal");
}

#[test]
fn test_quill_with_numbers() {
    let markdown = "~~~card-yaml\n$quill: form_8_v2\n$kind: main\n~~~\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.quill_reference().name, "form_8_v2");
}

#[test]
fn test_quill_with_additional_fields() {
    let markdown = "~~~card-yaml
$quill: my_quill
$kind: main
title: Document Title
author: John Doe
~~~

Body content.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.quill_reference().name, "my_quill");
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Document Title"
    );
    assert_eq!(
        doc.main()
            .payload()
            .get("author")
            .unwrap()
            .as_str()
            .unwrap(),
        "John Doe"
    );
}

// Error handling

#[test]
fn test_invalid_card_kind_names_are_rejected() {
    // `$kind` is name-validated at parse time against `[a-z_][a-z0-9_]*`.
    for kind in ["ITEMS", "123items", "my-items", "Invalid-Name", ""] {
        let markdown = format!(
            "~~~card-yaml\n$quill: test_quill\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: {kind}\n~~~\n\nBody."
        );
        let err = decompose(&markdown).unwrap_err().to_string();
        assert!(
            err.contains("Invalid `$kind`"),
            "kind {kind:?} should be rejected; got: {err}"
        );
    }
}

#[test]
fn test_invalid_quill_ref_uppercase() {
    let markdown = "~~~card-yaml\n$quill: MyQuill\n$kind: main\n~~~\n\nBody.";
    let result = decompose(markdown);
    assert!(result.is_err());
}

#[test]
fn test_yaml_syntax_error_missing_colon() {
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle Test\n~~~\n\nBody.";
    let result = decompose(markdown);
    assert!(result.is_err());
}

#[test]
fn test_yaml_syntax_error_bad_indentation() {
    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\nitems:\n- one\n - two\n~~~\n\nBody.";
    let result = decompose(markdown);
    // Bad indentation may or may not be an error depending on YAML parser
    let _ = result;
}

// Body extraction edge cases

#[test]
fn test_body_with_leading_newlines() {
    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Test\n~~~\n\n\n\nBody with leading newlines.";
    let doc = decompose(markdown).unwrap();
    assert!(doc.main().body().starts_with('\n'));
}

#[test]
fn test_body_with_trailing_newlines() {
    // Body at EOF: no blank-line separator to strip, source's trailing
    // newlines are preserved verbatim as authored content.
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Test\n~~~\n\nBody.\n\n\n";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body(), "\nBody.\n\n\n");
}

// ── Blank-line separator stripping: parse-side normalisation ─────────────────
// See `assemble.rs::strip_blank_separator` and `MARKDOWN.md §4` (rule D1).

#[test]
fn test_blank_separator_strip_global_body_followed_by_card_lf() {
    // Global body followed by a card block: the source's tail `\n\n` is
    // (content line terminator) + (blank-line separator). Strip exactly the
    // separator `\n`, leaving `\n` as the content terminator.
    let markdown =
        "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\nbody\n\n~~~card-yaml\n$kind: x\n~~~\n";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body(), "\nbody\n");
}

#[test]
fn test_blank_separator_strip_global_body_followed_by_card_crlf() {
    // CRLF line endings: strip exactly one `\r\n` as the blank-line separator.
    let markdown =
        "~~~card-yaml\r\n$quill: q\r\n$kind: main\r\n~~~\r\n\r\nbody\r\n\r\n~~~card-yaml\r\n$kind: x\r\n~~~\r\n";
    let doc = decompose(markdown).unwrap();
    assert!(
        doc.main().body().ends_with('\n') && !doc.main().body().ends_with("\n\n"),
        "expected exactly one trailing line ending, got {:?}",
        doc.main().body()
    );
}

#[test]
fn test_blank_separator_strip_card_body_followed_by_card() {
    // First card body is followed by another fence → separator stripped.
    // Last card body is at EOF → preserved verbatim.
    let markdown = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n~~~card-yaml\n$kind: a\n~~~\n\nfirst\n\n~~~card-yaml\n$kind: b\n~~~\n\nsecond\n";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.cards()[0].body(), "\nfirst\n");
    assert_eq!(doc.cards()[1].body(), "\nsecond\n");
}

#[test]
fn test_blank_separator_strip_preserves_author_blank_lines() {
    // Author wrote two blank lines after the body. Only the blank-line
    // separator (last `\n`) is stripped; the author's blank line is preserved.
    let markdown =
        "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\nbody\n\n\n~~~card-yaml\n$kind: x\n~~~\n";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body(), "\nbody\n\n");
}

#[test]
fn test_f2_strip_does_not_overstrip_content_newlines() {
    // Content-fidelity: a body whose authored content ends with multiple
    // newlines (e.g. a code block with trailing blank lines) must survive
    // round-trip.
    let markdown =
        "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\n```\ncode\n```\n\n\n~~~card-yaml\n$kind: x\n~~~\n";
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
fn test_no_body_after_payload() {
    let markdown = "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Test\n~~~";
    let doc = decompose(markdown).unwrap();
    assert_eq!(doc.main().body(), "");
}

// Kind name validation

#[test]
fn test_kind_name_validator() {
    for &name in &["_", "_private", "item1", "item_2"] {
        assert!(is_valid_kind_name(name), "expected valid: {:?}", name);
    }
    for &name in &[
        "", "1item", "Items", "ITEMS", "my-items", "my.items", "my items",
    ] {
        assert!(!is_valid_kind_name(name), "expected invalid: {:?}", name);
    }
}

// Guillemet preprocessing

#[test]
fn test_guillemet_in_yaml_preserves_non_strings() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
count: 42
price: 19.99
active: true
items:
  - first
  - 100
  - true
~~~

Body.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main().payload().get("count").unwrap().as_i64().unwrap(),
        42
    );
    assert_eq!(
        doc.main().payload().get("price").unwrap().as_f64().unwrap(),
        19.99
    );
    assert!(doc
        .main()
        .payload()
        .get("active")
        .unwrap()
        .as_bool()
        .unwrap());
}

#[test]
fn test_guillemet_double_conversion_prevention() {
    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Already «converted»\n~~~\n\nBody.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "Already «converted»"
    );
}

#[test]
fn test_allowed_card_field_collision() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
my_card: \"some global value\"
~~~

~~~card-yaml
$kind: my_card
title: \"My Card\"
~~~

Body
";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main()
            .payload()
            .get("my_card")
            .unwrap()
            .as_str()
            .unwrap(),
        "some global value"
    );
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), Some("my_card"));
    assert_eq!(
        doc.cards()[0]
            .payload()
            .get("title")
            .unwrap()
            .as_str()
            .unwrap(),
        "My Card"
    );
}

#[test]
fn test_yaml_custom_tags_in_payload() {
    let markdown = "~~~card-yaml
$quill: test_quill
$kind: main
memo_from: !fill 2d lt example
regular_field: normal value
~~~

Body content.";
    let doc = decompose(markdown).unwrap();
    assert_eq!(
        doc.main()
            .payload()
            .get("memo_from")
            .unwrap()
            .as_str()
            .unwrap(),
        "2d lt example"
    );
    assert_eq!(
        doc.main()
            .payload()
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
    let markdown = "~~~card-yaml
$quill: blog_post
$kind: main
title: My Document
~~~

Main document body.

***

More content after horizontal rule.

~~~card-yaml
$kind: section
heading: Introduction
~~~

Introduction content.

~~~card-yaml
$kind: section
heading: Conclusion
~~~

Conclusion content.
";

    let doc = decompose(markdown).unwrap();

    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str().unwrap(),
        "My Document"
    );
    assert_eq!(doc.quill_reference().name, "blog_post");

    let body = doc.main().body();
    assert!(body.contains("Main document body."));
    assert!(body.contains("***"));
    assert!(body.contains("More content after horizontal rule."));

    assert_eq!(doc.cards().len(), 2);
    assert_eq!(doc.cards()[0].kind(), Some("section"));
    assert_eq!(
        doc.cards()[0]
            .payload()
            .get("heading")
            .unwrap()
            .as_str()
            .unwrap(),
        "Introduction"
    );
    assert_eq!(doc.cards()[0].body(), "\nIntroduction content.\n");
    assert_eq!(doc.cards()[1].kind(), Some("section"));
    assert_eq!(
        doc.cards()[1]
            .payload()
            .get("heading")
            .unwrap()
            .as_str()
            .unwrap(),
        "Conclusion"
    );
    assert_eq!(doc.cards()[1].body(), "\nConclusion content.\n");
}

#[test]
fn test_missing_quill_errors() {
    let markdown = "# Body\n\nNo card-yaml block here.";
    let result = decompose(markdown);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Missing required root card-yaml block"));
}

// ── to_plate_json round-trip snapshot ─────────────────────────────────────────

/// Verify to_plate_json produces the correct shape for a simple document.
#[test]
fn test_to_plate_json_simple() {
    let doc = Document::from_markdown(
        "~~~card-yaml\n$quill: my_quill\n$kind: main\ntitle: Hello\n~~~\n\nBody text.\n",
    )
    .unwrap();
    let json = doc.to_plate_json();

    assert_eq!(json["$quill"], "my_quill");
    assert_eq!(json["title"], "Hello");
    assert_eq!(json["$body"], "\nBody text.\n");
    assert!(json["$cards"].is_array());
    assert_eq!(json["$cards"].as_array().unwrap().len(), 0);
}

/// to_plate_json with cards produces a `$cards` array containing `$kind`,
/// fields, and `$body`.
#[test]
fn test_to_plate_json_with_cards() {
    let markdown = "~~~card-yaml
$quill: usaf_memo
$kind: main
title: Test
~~~

Global body.

~~~card-yaml
$kind: indorsement
for: ORG
~~~

Card body here.
";
    let doc = Document::from_markdown(markdown).unwrap();
    let json = doc.to_plate_json();

    assert_eq!(json["$quill"], "usaf_memo");
    assert_eq!(json["title"], "Test");
    // Blank-line separator stripped on parse; plate `$body` reflects the same
    // content-only string as `Document::body()`.
    assert_eq!(json["$body"], "\nGlobal body.\n");

    let cards = json["$cards"].as_array().unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["$kind"], "indorsement");
    assert_eq!(cards[0]["for"], "ORG");
    assert_eq!(cards[0]["$body"], "\nCard body here.\n");
}

/// to_plate_json parity: the `$quill` key appears first.
#[test]
fn test_to_plate_json_quill_first() {
    let doc = Document::from_markdown(
        "~~~card-yaml\n$quill: my_quill\n$kind: main\nfoo: bar\nbaz: qux\n~~~\n",
    )
    .unwrap();
    let json = doc.to_plate_json();
    let obj = json.as_object().unwrap();
    let keys: Vec<&String> = obj.keys().collect();
    assert_eq!(keys[0], "$quill");
}

/// Snapshot test over a representative usaf_memo-shaped document.
#[test]
fn test_to_plate_json_fixture_snapshot() {
    let markdown = "~~~card-yaml
$quill: usaf_memo@0.1
$kind: main
memo_for:
  - ORG/SYMBOL
date: 2504-10-05
subject: Subject of the Memorandum
~~~

The body of the memorandum.

~~~card-yaml
$kind: indorsement
for: ORG/SYMBOL
from: ORG/SYMBOL
~~~

This body and the metadata above are an indorsement card.
";
    let doc = Document::from_markdown(markdown).unwrap();
    let json = doc.to_plate_json();

    // `$quill` is present
    assert_eq!(json["$quill"], "usaf_memo@0.1");
    // payload fields are present at top level
    assert!(json.get("memo_for").is_some());
    assert!(json.get("date").is_some());
    // `$body` and `$cards` are present
    assert!(json.get("$body").is_some());
    assert!(json["$cards"].is_array());
    // One indorsement card
    let cards = json["$cards"].as_array().unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["$kind"], "indorsement");
    // Card has `$body`
    assert!(cards[0].get("$body").is_some());
}

/// Regression test for the `serde_json::Map::remove` / `shift_remove` bug.
///
/// `serde_json::Map::remove` with `preserve_order` uses `swap_remove` under
/// the hood (O(1), moves last element into removed slot) — NOT the order-
/// preserving `shift_remove` (O(n)).  Payload field order must be
/// preserved.
#[test]
fn payload_field_order_preserved_after_quill_removal() {
    let md = "~~~card-yaml\n$quill: q\n$kind: main\nsender: Alice\nrecipient: Bob\ndate: March 15\nsubject: hi\n~~~\n";
    let doc = Document::from_markdown(md).unwrap();
    let keys: Vec<&str> = doc.main().payload().keys().map(|s| s.as_str()).collect();
    // Fields must appear in YAML document order, not alphabetical or swap-order.
    assert_eq!(
        keys,
        vec!["sender", "recipient", "date", "subject"],
        "Payload fields must preserve insertion order"
    );
}
