//! Actionable-hint enrichment for YAML parse errors.
//!
//! The underlying YAML parser (`serde_saphyr` / its `saphyr-parser-bw` backend)
//! surfaces messages in YAML jargon ("alias references unknown anchor",
//! "mapping values are not allowed in this context", "multiple YAML documents
//! detected; use from_multiple or from_multiple_with_options") that LLM
//! callers in a tool-use loop cannot translate into a content edit.
//!
//! This module post-processes a parser error string + the offending YAML
//! content into:
//!
//! - a **sanitized** message with Rust API names (`from_multiple`,
//!   `DuplicateKeyPolicy`, `Options`) stripped, and
//! - an optional **hint** that names the concrete textual fix.
//!
//! The hint is attached to the resulting [`crate::Diagnostic`] in the `hint`
//! field, so every binding (CLI, Python, MCP) surfaces the same advice — no
//! per-binding error enrichment.

/// Output of [`enrich_yaml_error`]: a cleaned message plus an optional hint.
#[derive(Debug, Clone)]
pub(crate) struct EnrichedYamlError {
    /// Parser message with Rust API names stripped and prose normalized.
    pub message: String,
    /// Actionable hint suggesting the concrete fix, when one is recognized.
    pub hint: Option<String>,
}

/// Inspect a serde_saphyr error string against the YAML content it came from
/// and return an [`EnrichedYamlError`].
///
/// The content slice is the YAML payload of a single `~~~` card-yaml block
/// (the same string passed to the parser). The function never panics on
/// non-UTF8 byte offsets — all inspection is over `&str` / `chars()`.
pub(crate) fn enrich_yaml_error(raw: &str, content: &str) -> EnrichedYamlError {
    let sanitized = sanitize_message(raw);
    let hint = derive_hint(&sanitized, content);
    EnrichedYamlError {
        message: sanitized,
        hint,
    }
}

/// Strip Rust-API-name leakage from the parser message.
///
/// `serde_saphyr` appends advice like
/// `"use from_multiple or from_multiple_with_options"` or
/// `"set DuplicateKeyPolicy in Options if acceptable"` to certain errors.
/// Those identifiers point at the Rust crate's API surface and mean nothing
/// to a non-Rust caller (LLM, CLI user, Python consumer).
fn sanitize_message(raw: &str) -> String {
    // Patterns to remove outright, including the leading `;` or `,` separator
    // when present so we don't leave a trailing comma.
    const STRIPS: &[&str] = &[
        "; use from_multiple_with_options",
        "; use from_multiple or from_multiple_with_options",
        ", use from_multiple_with_options",
        ", use from_multiple or from_multiple_with_options",
        "; set DuplicateKeyPolicy in Options if acceptable",
        ", set DuplicateKeyPolicy in Options if acceptable",
        " use from_multiple or from_multiple_with_options",
        " use from_multiple_with_options",
        " set DuplicateKeyPolicy in Options if acceptable",
    ];

    let mut out = raw.to_string();
    for p in STRIPS {
        if let Some(idx) = out.find(p) {
            out.replace_range(idx..idx + p.len(), "");
        }
    }
    // Tidy any leftover ", ." or "; ." after removal.
    out = out.replace(" ; .", ".").replace(" , .", ".");
    out.trim_end_matches([',', ';', ' ']).to_string()
}

/// Derive an actionable hint for `message`, given the YAML `content`.
fn derive_hint(message: &str, content: &str) -> Option<String> {
    let m = message.to_ascii_lowercase();

    // Gap 2: a plain scalar starting with `*` or `&` is read as a YAML alias
    // or anchor indicator. LLMs writing `field: **bold**` trip this.
    if m.contains("alias references unknown anchor")
        || m.contains("anchor") && m.contains("not found")
    {
        if let Some(field) = first_field_with_unquoted_prefix(content, &['*', '&']) {
            return Some(format!(
                "Plain-scalar values cannot start with `*` or `&` (reserved as YAML \
                 alias/anchor indicators). For markdown emphasis or a literal `*`/`&`, \
                 wrap the value in single quotes: `{field}: '**bold text**'`"
            ));
        }
        return Some(
            "Plain-scalar values cannot start with `*` or `&` (reserved as YAML \
             alias/anchor indicators). For markdown emphasis or a literal `*`/`&`, \
             wrap the value in single quotes — e.g. `field: '**bold text**'`."
                .to_string(),
        );
    }

    // Gap 3: an unquoted value containing `:` is read as a nested mapping key.
    if m.contains("mapping values are not allowed") {
        if let Some((field, value)) = first_field_with_unquoted_colon(content) {
            return Some(format!(
                "Unquoted values cannot contain `:` (it starts a nested mapping key). \
                 Quote the value: `{field}: \"{value}\"`"
            ));
        }
        return Some(
            "Unquoted values cannot contain `:` (it starts a nested mapping key). \
             Wrap the value in double quotes — e.g. `field: \"value: with colon\"`."
                .to_string(),
        );
    }

    // Gap 4 (the `---` separator case): a stray YAML document separator
    // inside a card-yaml block.
    if m.contains("multiple yaml documents") {
        if content.lines().any(|l| l.trim_end() == "---") {
            return Some(
                "`---` is not a valid separator inside a card-yaml block (YAML \
                 reads it as a new-document marker). Close the metadata block with a \
                 line containing exactly `~~~` (three tildes) before starting the \
                 prose body."
                    .to_string(),
            );
        }
        return Some(
            "Only one YAML document is allowed per card-yaml block. Remove the \
             stray `---` separator and close the block with `~~~` before any prose."
                .to_string(),
        );
    }

    // Gap 4 (duplicate keys): a field declared twice in the same block.
    if m.contains("duplicate mapping key") || m.contains("duplicate key") {
        return Some(
            "Each field may appear at most once inside a card-yaml block. \
             Remove the duplicate line, or move it to a separate composable card."
                .to_string(),
        );
    }

    // S2-1: a `- item` list line where a mapping key was expected. Either the
    // sequence is mis-indented, or the field was meant to be a scalar.
    if m.contains("block sequence entries are not allowed") {
        return Some(
            "A `- item` list was found where a mapping key was expected. Either \
             indent the sequence two spaces under the key it belongs to \
             (`field:` newline `  - item`), or — if this field expects a single \
             scalar value — drop the `-` and put the value on the same line: \
             `field: value`."
                .to_string(),
        );
    }

    // S2-2: a continuation line of a plain-scalar value is being read as a new
    // key. Either quote / block-scalar the multi-line value, or indent the
    // continuation.
    if m.contains("simple key expected") || m.contains("simple key expect") {
        return Some(
            "A second line of a value was read as a new mapping key (YAML \
             plain-scalar values stop at the next unindented line). For \
             multi-line text, use a block scalar: `field: |` then put each \
             line indented two spaces below. For a single-line value, keep it \
             on one line."
                .to_string(),
        );
    }

    // S2-3: anchor-scan failure — same root cause as the alias case above
    // (unquoted value starts with `&`), matched via different message wording:
    // "scanning an anchor or alias" rather than "anchor" + "not found".
    if m.contains("scanning an anchor") || m.contains("scanning an alias") {
        if let Some(field) = first_field_with_unquoted_prefix(content, &['*', '&']) {
            return Some(format!(
                "Plain-scalar values cannot start with `*` or `&` (reserved as YAML \
                 alias/anchor indicators). Wrap the value in single quotes: \
                 `{field}: '&literal value'`"
            ));
        }
        return Some(
            "Plain-scalar values cannot start with `*` or `&` (reserved as YAML \
             alias/anchor indicators). Wrap the value in single quotes — e.g. \
             `field: '&literal value'`."
                .to_string(),
        );
    }

    // Gap 5: a multi-line double-quoted scalar — block scalars are friendlier.
    if m.contains("invalid indentation in multiline quoted scalar")
        || (m.contains("indentation") && m.contains("quoted scalar"))
    {
        if let Some(field) = first_field_with_unterminated_dquote(content) {
            return Some(format!(
                "Multi-line text is easier to write as a block scalar:\n\
                 `{field}: |\\n  line one\\n  line two`"
            ));
        }
        return Some(
            "Multi-line text is easier to write as a block scalar: \
             `field: |` then put each line indented two spaces below."
                .to_string(),
        );
    }

    None
}

/// Find the first `key: <scalar>` line whose scalar's first non-whitespace
/// character matches `prefixes` (e.g. `*` or `&`). Scans only the first line
/// of each plain mapping entry — multi-line values are not relevant for
/// alias/anchor diagnostics.
fn first_field_with_unquoted_prefix(content: &str, prefixes: &[char]) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        let (key, value) = match trimmed.split_once(':') {
            Some(parts) => parts,
            None => continue,
        };
        if key.is_empty() || key.contains(' ') {
            continue;
        }
        let value = value.trim_start();
        let Some(first) = value.chars().next() else {
            continue;
        };
        // Skip quoted scalars — they wouldn't trigger the anchor/alias error.
        if first == '\'' || first == '"' {
            continue;
        }
        if prefixes.contains(&first) {
            return Some(key.trim().to_string());
        }
    }
    None
}

/// Find the first `key: <value>` line whose unquoted value contains an
/// additional `:` (the second colon — first triggers the parser's
/// "mapping values are not allowed in this context" error).
fn first_field_with_unquoted_colon(content: &str) -> Option<(String, String)> {
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.starts_with('-') {
            continue;
        }
        let (key, rest) = match trimmed.split_once(':') {
            Some(parts) => parts,
            None => continue,
        };
        if key.is_empty() || key.contains(' ') {
            continue;
        }
        let value = rest.trim_start();
        let first = value.chars().next();
        if matches!(first, Some('\'') | Some('"') | Some('|') | Some('>')) {
            continue;
        }
        if value.contains(':') {
            // Strip a trailing comment if any.
            let value_clean = match value.split_once(" #") {
                Some((v, _)) => v.trim_end(),
                None => value.trim_end(),
            };
            return Some((key.trim().to_string(), value_clean.to_string()));
        }
    }
    None
}

/// Find the first `key: "...` line whose double-quoted scalar does not close
/// on the same line. A useful proxy for "the model wrote a multi-line
/// double-quoted scalar" — relevant for gap 5.
fn first_field_with_unterminated_dquote(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim_start();
        let (key, rest) = match trimmed.split_once(':') {
            Some(parts) => parts,
            None => continue,
        };
        if key.is_empty() || key.contains(' ') {
            continue;
        }
        let value = rest.trim_start();
        if !value.starts_with('"') {
            continue;
        }
        // Walk the value counting unescaped quotes. A single opening quote
        // with no closer on the same line is an unterminated double-quote.
        let body = &value[1..];
        let mut closed = false;
        let mut prev_backslash = false;
        for ch in body.chars() {
            if prev_backslash {
                prev_backslash = false;
                continue;
            }
            if ch == '\\' {
                prev_backslash = true;
                continue;
            }
            if ch == '"' {
                closed = true;
                break;
            }
        }
        if !closed {
            return Some(key.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_from_multiple_advice() {
        let raw =
            "multiple YAML documents detected; use from_multiple or from_multiple_with_options";
        let out = sanitize_message(raw);
        assert_eq!(out, "multiple YAML documents detected");
    }

    #[test]
    fn strips_duplicate_key_policy_advice() {
        let raw =
            "duplicate mapping key: organizations, set DuplicateKeyPolicy in Options if acceptable";
        let out = sanitize_message(raw);
        assert_eq!(out, "duplicate mapping key: organizations");
    }

    #[test]
    fn hint_for_alias_unknown_anchor_names_field() {
        let content = "title: Doc\nbluf: **Increased maritime activity**\n";
        let enriched = enrich_yaml_error("alias references unknown anchor", content);
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("bluf"), "hint did not name the field: {hint}");
        assert!(hint.contains("single quotes"));
    }

    #[test]
    fn hint_for_mapping_values_names_field_and_value() {
        let content = "system_name: Node.js Service: Order Processing API\n";
        let enriched = enrich_yaml_error("mapping values are not allowed in this context", content);
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("system_name"));
        assert!(hint.contains("Node.js Service: Order Processing API"));
        assert!(hint.contains("Quote"));
    }

    #[test]
    fn hint_for_multiple_documents_calls_out_dash_separator() {
        let content = "title: Doc\n---\n";
        let enriched = enrich_yaml_error("multiple YAML documents detected", content);
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("`---`"));
        assert!(hint.contains("`~~~`"));
    }

    #[test]
    fn hint_for_duplicate_key_is_actionable() {
        let enriched = enrich_yaml_error("duplicate mapping key: organizations", "");
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("at most once"));
    }

    #[test]
    fn hint_for_multiline_dquote_suggests_block_scalar() {
        let content = "bullets: \"- one\n- two\n- three\"\n";
        let enriched = enrich_yaml_error("invalid indentation in multiline quoted scalar", content);
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("bullets"));
        assert!(hint.contains("block scalar"));
    }

    #[test]
    fn returns_no_hint_for_unrecognized_messages() {
        let enriched = enrich_yaml_error("something unrelated", "");
        assert!(enriched.hint.is_none());
        assert_eq!(enriched.message, "something unrelated");
    }

    #[test]
    fn hint_for_block_sequence_in_mapping_context() {
        let enriched = enrich_yaml_error(
            "block sequence entries are not allowed in this context",
            "section_headers:\n- Title\n",
        );
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("`- item` list"));
        assert!(hint.contains("indent"));
    }

    #[test]
    fn hint_for_simple_key_expected_suggests_block_scalar() {
        let enriched = enrich_yaml_error(
            "simple key expected at line 17, column 1",
            "summary: This is a long\nsummary across multiple lines\n",
        );
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("block scalar"));
        assert!(hint.contains("|"));
    }

    #[test]
    fn hint_for_simple_key_expect_colon_variant_also_matches() {
        let enriched = enrich_yaml_error("simple key expect ':'", "");
        assert!(enriched.hint.is_some());
    }

    #[test]
    fn hint_for_scanning_anchor_names_field() {
        let content = "title: Doc\nbluf: &unquoted ampersand\n";
        let enriched = enrich_yaml_error(
            "while scanning an anchor or alias, did not find expected alphabetic or numeric character",
            content,
        );
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("bluf"));
        assert!(hint.contains("single quotes"));
    }

    #[test]
    fn does_not_panic_on_multibyte_content() {
        // Em-dash and curly quotes are multibyte in UTF-8 — must not panic.
        let content = "briefer: Maj Sarah Chen — INDOPACOM/A2\nbluf: **\u{201c}peer\u{201d}**\n";
        let _ = enrich_yaml_error("alias references unknown anchor", content);
        let _ = enrich_yaml_error("mapping values are not allowed in this context", content);
    }
}
