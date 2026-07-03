//! Generates the virtual `@local/quillmark-helper:0.1.0` package that
//! provides document data and helper functions to Typst plates.
//! The package exports `data` — a dictionary of document fields with markdown
//! and date fields auto-converted to Typst values.

use std::ops::Range;

use crate::convert::escape_string;

/// Exposed for fuzzing tests.
#[doc(hidden)]
pub fn inject_json(bytes: &str) -> String {
    format!("json(bytes(\"{}\"))", escape_string(bytes))
}

pub const HELPER_VERSION: &str = "0.1.0";
pub const HELPER_NAMESPACE: &str = "local";
pub const HELPER_NAME: &str = "quillmark-helper";

const LIB_TYP_TEMPLATE: &str = include_str!("lib.typ.template");

/// A generated eval call site's byte window in the produced `lib.typ`: the
/// range of the string-literal argument (quotes included — the span `eval`
/// stamps on its result), keyed by the schema address of the content it
/// carries. The world layer pairs these with the helper's `FileId` for the
/// span scan.
pub struct ContentWindow {
    pub path: String,
    pub range: Range<usize>,
}

/// Generate `lib.typ` for the quillmark-helper package from JSON data plus
/// the per-render content entries — `(schema address, converted markup)`
/// pairs from `content_entries`, one per content field / `markdown[]` element
/// / card content field present in the data. Each entry becomes a textually
/// distinct `eval()` call site in the generated `_qm-content` dictionary, and
/// the returned windows record where.
pub fn generate_lib_typ(json_data: &str, content: &[(String, String)]) -> (String, Vec<ContentWindow>) {
    // Every placeholder is located in the *raw template* — trusted static
    // text — never in a string that already carries substituted document
    // data. Locating a slot after substitution would let data containing the
    // literal placeholder text hijack the splice point (the JSON payload
    // precedes the content slot in the template, and `escape_string` leaves
    // braces verbatim).
    let json_at = LIB_TYP_TEMPLATE
        .find("{escaped_json}")
        .expect("lib.typ.template carries the {escaped_json} slot");
    let slot_at = LIB_TYP_TEMPLATE
        .find("{content_evals}")
        .expect("lib.typ.template carries the {content_evals} slot");
    debug_assert!(json_at < slot_at, "template slot order");

    let escaped_json = escape_string(json_data);
    let (block, rel_windows) = content_evals(content);

    let mut src = String::with_capacity(LIB_TYP_TEMPLATE.len() + escaped_json.len() + block.len());
    src.push_str(&LIB_TYP_TEMPLATE[..json_at].replace("{version}", HELPER_VERSION));
    src.push_str(&escaped_json);
    src.push_str(&LIB_TYP_TEMPLATE[json_at + "{escaped_json}".len()..slot_at]);
    let block_at = src.len();
    src.push_str(&block);
    src.push_str(&LIB_TYP_TEMPLATE[slot_at + "{content_evals}".len()..]);

    let windows = rel_windows
        .into_iter()
        .map(|(path, r)| ContentWindow {
            path,
            range: (r.start + block_at)..(r.end + block_at),
        })
        .collect();
    (src, windows)
}

/// The generated `_qm-content` dictionary source, with each entry's
/// string-literal window relative to the block's own start. The window covers
/// the literal *including* both quotes — confirmed empirically: the uniform
/// span `eval` assigns is the argument expression's, which is the quoted
/// literal, not its contents.
fn content_evals(content: &[(String, String)]) -> (String, Vec<(String, Range<usize>)>) {
    if content.is_empty() {
        return ("#let _qm-content = (:)".to_string(), Vec::new());
    }
    let mut out = String::from("#let _qm-content = (\n");
    let mut windows = Vec::with_capacity(content.len());
    for (path, value) in content {
        out.push_str("  \"");
        out.push_str(&escape_string(path));
        out.push_str("\": eval(");
        let start = out.len();
        out.push('"');
        out.push_str(&escape_string(value));
        out.push('"');
        windows.push((path.clone(), start..out.len()));
        out.push_str(", mode: \"markup\"),\n");
    }
    out.push(')');
    (out, windows)
}

pub fn generate_typst_toml() -> String {
    format!(
        r#"[package]
name = "{name}"
version = "{version}"
namespace = "{namespace}"
entrypoint = "lib.typ"
"#,
        name = HELPER_NAME,
        version = HELPER_VERSION,
        namespace = HELPER_NAMESPACE
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_lib_typ_basic() {
        let json = r#"{"title":"Test","$body":"Hello","date":"2025-01-15","__meta__":{"content_fields":["$body"],"card_content_fields":{},"date_fields":["date"],"card_date_fields":{}}}"#;
        let (lib, windows) = generate_lib_typ(json, &[("$body".to_string(), "Hello".to_string())]);

        assert!(lib.contains("Version: 0.1.0"));
        assert!(lib.contains("json(bytes("));
        // The template must expose only the private `_parse-date` helper —
        // no public `parse-date` and no `eval-markup` symbol.
        assert!(!lib.contains("eval-markup"));
        assert!(lib.contains("#let _parse-date(s)"));
        assert!(!lib.contains("#let parse-date(s)"));
        assert!(lib.contains("meta.date_fields"));
        assert!(lib.contains("meta.card_date_fields"));
        // The template exports no tagging surface — no `tagged`, no
        // `_qm-tag`; cards carry their `$path` prefix for form-field
        // address composition.
        assert!(!lib.contains("#let tagged"));
        assert!(!lib.contains("_qm-tag"));
        assert!(lib.contains("card.insert(\"$path\", prefix)"));
        // Each content entry becomes its own eval call site whose recorded
        // window is exactly the quoted literal.
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].path, "$body");
        assert_eq!(&lib[windows[0].range.clone()], "\"Hello\"");
    }

    #[test]
    fn windows_cover_escaped_literals_exactly() {
        let entries = vec![
            ("a".to_string(), "line one\nline \"two\"".to_string()),
            ("b.0".to_string(), "plain".to_string()),
        ];
        let (lib, windows) = generate_lib_typ("{}", &entries);
        assert_eq!(windows.len(), 2);
        assert_eq!(
            &lib[windows[0].range.clone()],
            "\"line one\\nline \\\"two\\\"\"",
            "the window covers the escaped literal, quotes included"
        );
        assert_eq!(&lib[windows[1].range.clone()], "\"plain\"");
    }

    #[test]
    fn data_containing_placeholder_text_cannot_hijack_the_splice() {
        // The JSON payload precedes the content slot in the template; a
        // document whose data contains the literal slot text must not move
        // the splice point into the payload.
        let json = r#"{"body":"quoting the template: {content_evals} and {escaped_json}"}"#;
        let (lib, windows) = generate_lib_typ(json, &[("body".to_string(), "hi".to_string())]);

        assert_eq!(windows.len(), 1);
        assert_eq!(&lib[windows[0].range.clone()], "\"hi\"");
        // The payload text is intact and the generated dict sits at the real
        // slot, after it.
        let payload = lib.find("quoting the template").expect("payload present");
        let dict = lib.find("#let _qm-content").expect("dict present");
        assert!(payload < dict, "content dict spliced at the template slot, not into the payload");
    }

    #[test]
    fn no_content_entries_yields_an_empty_dict() {
        let (lib, windows) = generate_lib_typ("{}", &[]);
        assert!(lib.contains("#let _qm-content = (:)"));
        assert!(windows.is_empty());
    }

    #[test]
    fn test_generate_lib_typ_escapes_json() {
        let json = r#"{"title": "Test \"quoted\""}"#;
        let (lib, _) = generate_lib_typ(json, &[]);

        assert!(lib.contains("\\\""));
    }

    #[test]
    fn test_generate_lib_typ_handles_newlines() {
        let json = "{\n\"title\": \"Test\"\n}";
        let (lib, _) = generate_lib_typ(json, &[]);

        assert!(lib.contains("\\n"));
    }

    #[test]
    fn test_generate_typst_toml() {
        let toml = generate_typst_toml();

        assert!(toml.contains("name = \"quillmark-helper\""));
        assert!(toml.contains("version = \"0.1.0\""));
        assert!(toml.contains("namespace = \"local\""));
        assert!(toml.contains("entrypoint = \"lib.typ\""));
    }
}
