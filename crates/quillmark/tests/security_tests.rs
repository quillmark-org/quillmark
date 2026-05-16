//! Security attack scenario tests for markdown parsing and conversion
//!
//! These tests verify that the system properly handles malicious input
//! and prevents common attack vectors like injection, DoS, and path traversal.

use quillmark_core::Document;

/// Test deeply nested YAML structures hit the depth limit
#[test]
fn test_yaml_depth_limit_attack() {
    // Create deeply nested YAML structure (exceeds MAX_YAML_DEPTH)
    let mut deep_yaml = String::new();
    for i in 0..150 {
        deep_yaml.push_str(&"  ".repeat(i));
        deep_yaml.push_str("a:\n");
    }
    let markdown = format!("---\nQUILL: test_quill\n{}---\n\nBody", deep_yaml);
    let result = Document::from_markdown(&markdown);

    // Should fail with YAML depth limit error
    assert!(result.is_err(), "Should reject deeply nested YAML");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("depth") || err_msg.contains("YAML") || err_msg.contains("limit"),
        "Error should mention depth/limit: {}",
        err_msg
    );
}

/// Test card count limit prevents DoS
#[test]
fn test_card_count_limit_attack() {
    // Generate more than MAX_CARD_COUNT (1000) card blocks
    let mut markdown = String::from("---\nQUILL: test_quill\ntitle: Test\n---\n\nBody\n\n");
    for i in 0..1002 {
        markdown.push_str(&format!("```card item{}\nvalue: {}\n```\n\n", i, i));
    }
    let result = Document::from_markdown(&markdown);

    // Should fail with card count limit error
    assert!(result.is_err(), "Should reject excessive card blocks");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("too large") || err_msg.contains("max"),
        "Error should mention limit: {}",
        err_msg
    );
}

/// Test that Typst special characters are properly escaped (injection prevention)
#[test]
fn test_typst_injection_via_special_chars() {
    let malicious_inputs = vec![
        r#"**"; eval("malicious")""#,
        r#"$x$ math injection"#,
        r#"#eval("danger")"#,
        r#"@dangerous"#,
        r#"~strike~`code`"#,
    ];

    for input in malicious_inputs {
        let markdown = format!("---\nQUILL: test_quill\n---\n\n{}", input);
        let result = Document::from_markdown(&markdown);
        // Should parse without error (escaping happens during conversion)
        assert!(
            result.is_ok(),
            "Should handle special chars in input: {}",
            input
        );
    }
}

/// Test large input size limit
#[test]
fn test_input_size_limit() {
    let large_content = "a".repeat(11 * 1024 * 1024); // 11 MB
    let markdown = format!(
        "---\nQUILL: test_quill\ntitle: Large\n---\n\n{}",
        large_content
    );
    let result = Document::from_markdown(&markdown);

    assert!(result.is_err(), "Should reject oversized input");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("too large") || err_msg.contains("bytes"),
        "Error should mention size limit: {}",
        err_msg
    );
}

/// Test YAML size limit
#[test]
fn test_yaml_size_limit() {
    let large_value = "x".repeat(1024 * 1024 + 100);
    let markdown = format!("---\nQUILL: test_quill\ndata: {}\n---\n\nBody", large_value);
    let result = Document::from_markdown(&markdown);

    assert!(result.is_err(), "Should reject oversized YAML");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("too large") || err_msg.contains("YAML"),
        "Error should mention YAML size: {}",
        err_msg
    );
}

/// Test reserved field names are rejected
#[test]
fn test_reserved_field_injection() {
    let reserved_tests = vec![
        (
            "---\nQUILL: test_quill\nBODY: injected\n---\n\nBody",
            "BODY",
        ),
        ("---\nQUILL: test_quill\nCARDS: []\n---\n\nBody", "CARDS"),
    ];

    for (markdown, reserved) in reserved_tests {
        let result = Document::from_markdown(markdown);
        assert!(
            result.is_err(),
            "Should reject reserved field '{}' in YAML",
            reserved
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Reserved") || err_msg.contains(reserved),
            "Error should mention reserved field: {}",
            err_msg
        );
    }
}

/// Test that card kind-token validation prevents invalid names
#[test]
fn test_card_name_validation() {
    let invalid_names = vec![
        "---\nQUILL: test_quill\n---\n\n```card Invalid-Name\n```\n\n",
        "---\nQUILL: test_quill\n---\n\n```card 123start\n```\n\n",
        "---\nQUILL: test_quill\n---\n\n```card UPPERCASE\n```\n\n",
        "---\nQUILL: test_quill\n---\n\n```card spaces here\n```\n\n",
    ];

    for markdown in invalid_names {
        let result = Document::from_markdown(markdown);
        assert!(
            result.is_err(),
            "Should reject invalid card name in: {}",
            markdown
        );
    }
}

/// Test YAML error includes line number context
#[test]
fn test_yaml_error_location() {
    let markdown =
        "---\nQUILL: test_quill\ntitle: Test\n---\n\nBody\n\n```card test\ninvalid yaml: {\n```\n\n";
    let result = Document::from_markdown(markdown);

    assert!(result.is_err(), "Should reject invalid YAML");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("line") || err_msg.contains("YAML"),
        "Error should include location context: {}",
        err_msg
    );
}

/// Test that `KIND` as an input body key is rejected as a reserved key
#[test]
fn test_quill_card_conflict() {
    let markdown = "---\nQUILL: template\nKIND: item\n---\n\n";
    let result = Document::from_markdown(markdown);

    assert!(result.is_err(), "Should reject KIND as an input body key");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Reserved field name") && err_msg.contains("KIND"),
        "Error should flag KIND as a reserved key: {}",
        err_msg
    );
}

/// Test that CommonMark 4+ backtick fences hide `---` lines from metadata parsing
#[test]
fn test_strict_fence_detection() {
    let markdown =
        "---\nQUILL: test_quill\ntitle: Test\n---\n\n````\n```card test\nvalue: 1\n```\n````";
    let result = Document::from_markdown(markdown);

    assert!(result.is_ok(), "Should parse successfully");
    let doc = result.unwrap();
    assert_eq!(
        doc.cards().len(),
        0,
        "--- inside ```` fence should not be parsed as metadata"
    );
}

/// Test that CommonMark tilde fences hide `---` lines from metadata parsing
#[test]
fn test_tilde_fence_hides_metadata() {
    let markdown =
        "---\nQUILL: test_quill\ntitle: Test\n---\n\n~~~\n```card test\nvalue: 1\n```\n~~~";
    let result = Document::from_markdown(markdown);

    assert!(result.is_ok(), "Should parse successfully");
    let doc = result.unwrap();
    assert_eq!(
        doc.cards().len(),
        0,
        "--- inside ~~~ fence should not be parsed as metadata"
    );
}
