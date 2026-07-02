# Quillmark Fuzzing Tests

This crate contains property-based fuzzing tests for Quillmark using the `proptest` framework. These tests validate the security of Quillmark's escaping functions, markdown parser, and filter inputs.

**Note:** This crate is not published to crates.io and is only used for internal testing.

## Quickstart

Run all property-based fuzzing tests:

```bash
cargo test --package quillmark-fuzz
```

Or from the `quillmark-fuzz` directory:

```bash
cd crates/fuzz
cargo test
```

Run a specific test module:

```bash
cargo test --package quillmark-fuzz convert_fuzz
cargo test --package quillmark-fuzz filter_fuzz
cargo test --package quillmark-fuzz parse_fuzz
cargo test --package quillmark-fuzz emit_roundtrip_fuzz
```

**Note:** This crate is excluded from `default-members` to avoid running expensive fuzzing tests on every `cargo test`. Use `cargo test --workspace` to run all tests including fuzzing. This crate uses `proptest` for property-based testing, not `cargo-fuzz`.

## Test Coverage

### Type Coercion (`coerce_fuzz`)

Property tests for `QuillConfig::coerce_payload`: no panics on arbitrary `(FieldSchema, Value)` pairs, well-formed error paths, and idempotence of successful coercions.

### Escaping Function Security (`convert_fuzz`)

Tests for `escape_string` and `escape_markup` functions in `quillmark-typst`:
- Injection attack vectors with quotes and eval patterns
- Control character handling (null bytes, ASCII control chars)
- Property tests ensuring no unescaped quotes can break out of string context
- Dangerous patterns like `\"); eval(...)` that could enable code injection
- Validation that all Typst special characters are properly escaped
- Backslash handling to prevent double-escaping vulnerabilities

### Markdown Parser Fuzzing (`convert_fuzz`)

DoS attack prevention:
- Deeply nested structures (blockquotes, lists up to 20 levels deep)
- Large input handling (up to 10,000 characters)
- Ensures parser doesn't panic on malicious inputs

### Filter Input Fuzzing (`filter_fuzz`)

Tests for the `inject_json` helper function:
- Validates proper escaping in JSON injection contexts
- Tests dangerous character combinations (`\`, `"`, control chars)
- Ensures no unescaped quotes that could break out of `json(bytes("..."))` wrapper
- Tests Unicode handling and various input sizes

### YAML Parser Fuzzing (`parse_fuzz`)

card-yaml payload security:
- Tests malformed YAML handling
- Validates composable card-kind parsing with random inputs
- Tests nested YAML structures for stability
- Unicode and special character handling

## Security Properties Validated

The fuzzing tests validate critical security properties:

1. **No injection vulnerabilities**: Quotes are always escaped in string contexts
2. **Control character safety**: ASCII control characters are properly escaped as `\u{...}`
3. **Backslash handling**: Backslashes are escaped first to prevent double-escaping
4. **DoS resistance**: Parser handles deeply nested and large inputs without panicking
5. **Unicode safety**: Handles arbitrary Unicode input without crashes

## Architecture

The fuzzing tests are organized into five modules:

- `coerce_fuzz.rs` - Property tests for `QuillConfig::coerce_payload` (no-panic, well-formed error paths, idempotence)
- `convert_fuzz.rs` - Tests for markdown to Typst conversion and escaping functions
- `emit_roundtrip_fuzz.rs` - Round-trip stability tests (parse → emit → re-parse)
- `filter_fuzz.rs` - Tests for filter input validation and injection safety
- `parse_fuzz.rs` - Tests for card-yaml and markdown parsing

All fuzzing tests use `proptest` for property-based testing, which generates random inputs to validate that security properties hold across a wide range of inputs.

## Contributing

When adding new features to Quillmark, consider adding corresponding fuzzing tests to this crate to ensure security properties are maintained.

## References

- [proptest documentation](https://docs.rs/proptest/) for property-based testing guidelines
