use proptest::prelude::*;
use quillmark_core::Document;

proptest! {
    #[test]
    fn fuzz_decompose_no_panic(s in "\\PC{0,1000}") {
        let _ = Document::from_markdown(&s);
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
    fn fuzz_decompose_card_kinds(kind_name in "[a-z][a-z0-9_]{0,19}") {
        // Test composable card parsing with a `$kind` discriminator
        let markdown = format!(
            "~~~card-yaml\n$quill: test_quill\n$kind: main\n~~~\n\n\
             ~~~card-yaml\n$kind: {}\nfield: data\n~~~\n\nContent",
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
    fn fuzz_decompose_large_payload(size in 1usize..100) {
        // Test with large payload blocks
        let fields: Vec<String> = (0..size)
            .map(|i| format!("field{}: value{}", i, i))
            .collect();
        let payload = fields.join("\n");
        let markdown =
            format!("~~~card-yaml\n$quill: test_quill\n$kind: main\n{}\n~~~\n\nContent", payload);

        let result = Document::from_markdown(&markdown);
        if let Ok(doc) = result {
            // payload has exactly the fields we provided (no BODY or CARDS keys)
            assert!(doc.main().payload().len() <= size);
        }
    }

}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn fuzz_decompose_multiple_cards(count in 1usize..10) {
        // Test with multiple composable card blocks
        let mut markdown = String::from("~~~card-yaml\n$quill: test_quill\n$kind: main\n~~~\n\n");

        for i in 0..count {
            markdown.push_str(&format!(
                "~~~card-yaml\n$kind: section{}\ndata: value{}\n~~~\n\nContent {}\n\n",
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
