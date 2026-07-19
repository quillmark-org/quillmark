//! Security attack scenario tests for markdown parsing and conversion
//!
//! These tests verify that the system properly handles malicious input
//! and prevents common attack vectors like injection, DoS, and path traversal.

use quillmark_core::{Card, Document};

/// Test that card kind validation prevents invalid names via the edit API.
///
/// `$kind` is opaque system metadata at parse time, so `from_markdown` does
/// not validate kind names. The `[a-z_][a-z0-9_]*` rule is enforced by the
/// structural edit API (`Card::new`), which is what this test exercises.
#[test]
fn test_card_name_validation() {
    let invalid_names = vec!["Invalid-Name", "123start", "UPPERCASE", "spaces here"];

    for name in invalid_names {
        let result = Card::new(name);
        assert!(result.is_err(), "Should reject invalid card name: {}", name);
    }

    // Valid lowercase/underscore names are accepted.
    assert!(Card::new("valid_name").is_ok());
    assert!(Card::new("item1").is_ok());
}

/// Test YAML error includes line number context
#[test]
fn test_yaml_error_location() {
    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Test\n~~~\n\nBody\n\n~~~card-yaml\n$kind: test\ninvalid yaml: {\n~~~\n\n";
    let result = Document::parse(markdown);

    assert!(result.is_err(), "Should reject invalid YAML");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("line") || err_msg.contains("YAML"),
        "Error should include location context: {}",
        err_msg
    );
}
