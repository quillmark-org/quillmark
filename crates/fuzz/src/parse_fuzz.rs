use proptest::prelude::*;
use quillmark_core::Document;

proptest! {
    #[test]
    fn fuzz_decompose_no_panic(s in "\\PC{0,1000}") {
        // Test that decompose doesn't panic on arbitrary input
        let _ = Document::from_markdown(&s);
        // We don't care about the result, just that it doesn't panic
    }

    #[test]
    fn fuzz_decompose_with_fences(s in "~~~card-yaml[\\s\\S]*~~~[\\s\\S]*") {
        // Test inputs that look like card-yaml blocks
        let result = Document::from_markdown(&s);
        // Should either succeed or return an error, but not panic
        match result {
            Ok(doc) => {
                // If it parsed, we should be able to access the document safely
                let _ = doc.main().body();
                let _ = doc.main().payload();
                let _ = doc.cards();
            }
            Err(_) => {
                // Error is fine - malformed YAML or other issues
            }
        }
    }

    #[test]
    fn fuzz_decompose_valid_payload(
        title in "[a-zA-Z0-9 ]{1,50}",
        author in "[a-zA-Z ]{1,30}",
        content in "\\PC{0,200}"
    ) {
        // Test with a well-formed root card-yaml block
        let markdown = format!(
            "~~~card-yaml\n#@quill: test_quill\n#@kind: main\ntitle: {}\nauthor: {}\n~~~\n\n{}",
            title, author, content
        );

        let result = Document::from_markdown(&markdown);
        // Result may be Ok or Err (e.g. an ambiguous YAML scalar); never panics
        let _ = result;
    }

    #[test]
    fn fuzz_decompose_card_kinds(kind_name in "[a-z][a-z0-9_]{0,19}") {
        // Test composable card parsing with a `#@kind` discriminator
        let markdown = format!(
            "~~~card-yaml\n#@quill: test_quill\n#@kind: main\n~~~\n\n\
             ~~~card-yaml\n#@kind: {}\nfield: data\n~~~\n\nContent",
            kind_name
        );

        let result = Document::from_markdown(&markdown);
        // Should handle composable cards without panic
        if let Ok(doc) = result {
            let _ = doc.cards();
            let _ = doc.main().payload();
        }
    }

    #[test]
    fn fuzz_decompose_malformed_yaml(s in "[^a-zA-Z0-9\\s]{1,50}") {
        // Test with potentially malformed YAML in the payload
        let markdown = format!("~~~card-yaml\n#@quill: test_quill\n#@kind: main\n{}\n~~~\n\nContent", s);
        let _ = Document::from_markdown(&markdown);
        // Should handle errors gracefully
    }

    #[test]
    fn fuzz_decompose_large_payload(size in 1usize..100) {
        // Test with large payload blocks
        let fields: Vec<String> = (0..size)
            .map(|i| format!("field{}: value{}", i, i))
            .collect();
        let payload = fields.join("\n");
        let markdown =
            format!("~~~card-yaml\n#@quill: test_quill\n#@kind: main\n{}\n~~~\n\nContent", payload);

        let result = Document::from_markdown(&markdown);
        if let Ok(doc) = result {
            // payload has exactly the fields we provided (no BODY or CARDS keys)
            assert!(doc.main().payload().len() <= size);
        }
    }

    #[test]
    fn fuzz_decompose_nested_structures(depth in 1usize..5) {
        // Test with nested YAML structures
        let mut yaml = String::from("root:\n");
        for i in 0..depth {
            let indent = "  ".repeat(i + 1);
            yaml.push_str(&format!("{}level{}:\n", indent, i));
        }
        yaml.push_str(&format!("{}value: data", "  ".repeat(depth + 1)));

        let markdown =
            format!("~~~card-yaml\n#@quill: test_quill\n#@kind: main\n{}\n~~~\n\nContent", yaml);
        let _ = Document::from_markdown(&markdown);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn fuzz_decompose_special_characters(s in "[\\\\\"'`$#*_\\[\\]<>@\\n\\r\\t]{0,100}") {
        // Test with special characters in body content
        let markdown =
            format!("~~~card-yaml\n#@quill: test_quill\n#@kind: main\ntitle: Test\n~~~\n\n{}", s);
        let result = Document::from_markdown(&markdown);

        if let Ok(doc) = result {
            // Should be able to retrieve body with special chars
            let body = doc.main().body();
            let _ = body;
        }
    }

    #[test]
    fn fuzz_decompose_unicode(s in "\\PC{0,100}") {
        // Test with Unicode body content
        let markdown =
            format!("~~~card-yaml\n#@quill: test_quill\n#@kind: main\ntitle: Test\n~~~\n\n{}", s);
        let result = Document::from_markdown(&markdown);

        if let Ok(doc) = result {
            let _ = doc.main().body();
        }
    }

    #[test]
    fn fuzz_decompose_multiple_cards(count in 1usize..10) {
        // Test with multiple composable card blocks
        let mut markdown = String::from("~~~card-yaml\n#@quill: test_quill\n#@kind: main\n~~~\n\n");

        for i in 0..count {
            markdown.push_str(&format!(
                "~~~card-yaml\n#@kind: section{}\ndata: value{}\n~~~\n\nContent {}\n\n",
                i, i, i
            ));
        }

        let result = Document::from_markdown(&markdown);
        if let Ok(doc) = result {
            // Should handle multiple cards
            let _ = doc.main().payload();
            let _ = doc.cards();
        }
    }
}
