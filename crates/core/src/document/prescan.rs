//! Pre-scan of a card-yaml block's YAML payload to recover features that
//! serde_saphyr discards.
//!
//! Three features are recovered here:
//!
//! 1. **Top-level comments.** YAML comments are dropped by the YAML parser.
//!    To round-trip them as [`super::PayloadItem::Comment`], we extract them
//!    before parsing.
//!
//! 2. **Nested comments.** Comments inside block mappings/sequences are
//!    captured with their structural path (sequence of keys/indices) and an
//!    ordinal indicating where in the container they sit. The emitter
//!    re-injects them at the matching position. See [`NestedComment`].
//!
//! 3. **`!fill` tags.** Custom YAML tags are accepted and dropped by
//!    serde_saphyr; the value survives but the tag annotation is lost. We
//!    detect `!fill` on top-level scalar fields, strip the tag from the
//!    cleaned YAML (so serde_saphyr sees a plain scalar), and record a
//!    `fill: true` marker on the resulting `Field` item.
//!
//! Other custom tags (`!include`, `!env`, …) are stripped with a
//! `parse::unsupported_yaml_tag` warning.

use crate::Diagnostic;
use crate::Severity;

/// One ordered hint extracted from the fence body.
///
/// `Field` captures only the `fill` flag; the value comes from serde_saphyr.
/// `Comment.inline` distinguishes own-line from trailing inline comments;
/// inline comments immediately follow their host `Field` in the item stream.
#[derive(Debug, Clone, PartialEq)]
pub enum PreItem {
    Field { key: String, fill: bool },
    Comment { text: String, inline: bool },
}

/// One segment of a path into the parsed YAML structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentPathSegment {
    Key(String),
    Index(usize),
}

/// A comment inside a nested mapping or sequence.
///
/// `container_path` locates the immediate parent. For own-line comments
/// (`inline = false`), `position` is the child slot ordinal (`0..=child_count`,
/// where `child_count` means "after all children"). For inline comments
/// (`inline = true`), `position` is the host child's index; orphaned inlines
/// degrade to own-line at emit time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedComment {
    pub container_path: Vec<CommentPathSegment>,
    pub position: usize,
    pub text: String,
    pub inline: bool,
}

/// Output of [`prescan_fence_content`].
#[derive(Debug, Clone, Default)]
pub struct PreScan {
    /// YAML with `!fill` tags stripped and comment lines removed; fed to serde_saphyr.
    pub cleaned_yaml: String,
    /// Top-level fields and comments in source order.
    pub items: Vec<PreItem>,
    pub nested_comments: Vec<NestedComment>,
    pub warnings: Vec<Diagnostic>,
    /// `!fill` on mappings — turned into `ParseError::InvalidStructure` by the parser.
    pub fill_target_errors: Vec<String>,
}

#[derive(Debug)]
struct Frame {
    indent: usize,
    path: Vec<CommentPathSegment>,
    kind: Option<FrameKind>,
    child_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Mapping,
    Sequence,
}

pub fn prescan_fence_content(content: &str) -> PreScan {
    let mut out = PreScan::default();

    let lines: Vec<&str> = content.split('\n').collect();
    let mut cleaned_lines: Vec<String> = Vec::with_capacity(lines.len());

    let mut stack: Vec<Frame> = vec![Frame {
        indent: 0,
        path: Vec::new(),
        kind: Some(FrameKind::Mapping),
        child_count: 0,
    }];

    // Indent of the `key:` line that opened the current YAML block scalar
    // (`|`/`>`), if any. While set, deeper-indented lines are literal scalar
    // content and bypass structural prescanning.
    let mut block_scalar_indent: Option<usize> = None;

    for raw_line in &lines {
        let line = *raw_line;
        let indent = leading_space_count(line);
        let trimmed = &line[indent..];

        if trimmed.is_empty() {
            cleaned_lines.push(line.to_string());
            continue;
        }

        // Inside a block scalar: lines indented deeper than the opening key
        // are literal text — a markdown heading (`## …`), a `- ` bullet, or a
        // `key: value` line in the content must pass through verbatim, never
        // parsed as a comment, sequence item, or nested key. A line at or
        // below the key's indent ends the scalar and is reprocessed normally.
        if let Some(key_indent) = block_scalar_indent {
            if indent > key_indent {
                cleaned_lines.push(line.to_string());
                continue;
            }
            block_scalar_indent = None;
        }

        while let Some(frame) = stack.last() {
            if frame.indent > indent {
                stack.pop();
            } else {
                break;
            }
        }

        // Case 1: own-line comment.
        if trimmed.starts_with('#') {
            let text = strip_comment_marker(trimmed);
            let frame = stack.last().expect("root frame always present");

            if frame.path.is_empty() {
                // Top-level comment — preserve via PreItem::Comment.
                out.items.push(PreItem::Comment {
                    text: text.to_string(),
                    inline: false,
                });
            } else {
                out.nested_comments.push(NestedComment {
                    container_path: frame.path.clone(),
                    position: frame.child_count,
                    text: text.to_string(),
                    inline: false,
                });
            }
            continue;
        }

        // Case 2: sequence item line (`- ...`).
        if trimmed == "-" || trimmed.starts_with("- ") {
            let frame_idx = ensure_frame_at_indent(&mut stack, indent, FrameKind::Sequence);
            let frame = &mut stack[frame_idx];
            let item_index = frame.child_count;
            frame.child_count += 1;
            let parent_path: Vec<CommentPathSegment> = frame.path.clone();
            let item_path: Vec<CommentPathSegment> = {
                let mut p = parent_path.clone();
                p.push(CommentPathSegment::Index(item_index));
                p
            };
            while stack.len() > frame_idx + 1 {
                stack.pop();
            }

            // `trimmed` is either `"-"` or starts with `"- "` (case 2 guard).
            // `strip_prefix` keeps this categorically free of byte-range
            // slicing on user content even though `"- "` is two ASCII bytes.
            let after_dash_full = trimmed.strip_prefix("- ").unwrap_or("");
            let (after_dash, trailing_comment) = split_trailing_comment(after_dash_full);
            let after_dash_trimmed = after_dash.trim_start();
            let inline_indent_offset = indent + 2 + (after_dash.len() - after_dash_trimmed.len());

            if after_dash_trimmed.is_empty() {
                stack.push(Frame {
                    indent: indent + 2,
                    path: item_path,
                    kind: None,
                    child_count: 0,
                });
            } else if split_key(after_dash_trimmed).is_some() {
                stack.push(Frame {
                    indent: inline_indent_offset,
                    path: item_path,
                    kind: Some(FrameKind::Mapping),
                    child_count: 1,
                });
            }

            if let Some(c) = trailing_comment {
                out.nested_comments.push(NestedComment {
                    container_path: parent_path,
                    position: item_index,
                    text: strip_comment_marker(&c).to_string(),
                    inline: true,
                });
                let head = format!("{:width$}", "", width = indent);
                let body = if after_dash.trim_end().is_empty() {
                    "-".to_string()
                } else {
                    format!("- {}", after_dash.trim_end())
                };
                cleaned_lines.push(format!("{}{}", head, body));
            } else {
                cleaned_lines.push(line.to_string());
            }

            // A sequence item whose value is itself a block scalar (`- |-`):
            // content lines are indented past the dash, so the dash line's
            // indent is the boundary. Without this, headings / bullets / `key:`
            // lines inside a `markdown[]` item would be mis-parsed as structure.
            if is_block_scalar_header(after_dash_trimmed) {
                block_scalar_indent = Some(indent);
            }
            continue;
        }

        // Case 3: top-level field line.
        let is_top_level = indent == 0;
        if is_top_level {
            if let Some((key, after_colon)) = split_key(line) {
                let (value_part, trailing_comment) = split_trailing_comment(&after_colon);

                let (fill, value_without_tag, had_non_fill_tag, fill_target_err) =
                    inspect_fill_and_tags(&value_part, &key);

                if had_non_fill_tag {
                    out.warnings.push(
                        Diagnostic::new(
                            Severity::Warning,
                            format!(
                                "YAML tag on key `{}` is not supported; the tag has been dropped and the value kept",
                                key
                            ),
                        )
                        .with_code("parse::unsupported_yaml_tag".to_string()),
                    );
                }
                if let Some(err) = fill_target_err {
                    out.fill_target_errors.push(err);
                }

                out.items.push(PreItem::Field {
                    key: key.clone(),
                    fill,
                });

                let root = &mut stack[0];
                root.child_count += 1;
                let key_path = vec![CommentPathSegment::Key(key.clone())];

                while stack.len() > 1 {
                    stack.pop();
                }

                if has_empty_inline_value(&value_without_tag) {
                    stack.push(Frame {
                        indent: 2,
                        path: key_path,
                        kind: None,
                        child_count: 0,
                    });
                }

                let cleaned = format!("{}:{}", key, value_without_tag);
                cleaned_lines.push(cleaned);

                if let Some(c) = trailing_comment {
                    out.items.push(PreItem::Comment {
                        text: strip_comment_marker(&c).to_string(),
                        inline: true,
                    });
                }

                if is_block_scalar_header(&value_without_tag) {
                    block_scalar_indent = Some(indent);
                }

                continue;
            }
        }

        // Case 4: nested key line inside a block mapping.
        if let Some((key, after_colon)) = split_key(trimmed) {
            let frame_idx = ensure_frame_at_indent(&mut stack, indent, FrameKind::Mapping);
            let frame = &mut stack[frame_idx];
            let key_index = frame.child_count;
            frame.child_count += 1;
            let parent_path: Vec<CommentPathSegment> = frame.path.clone();
            let key_path: Vec<CommentPathSegment> = {
                let mut p = parent_path.clone();
                p.push(CommentPathSegment::Key(key.clone()));
                p
            };
            while stack.len() > frame_idx + 1 {
                stack.pop();
            }

            let (value_part, trailing_comment) = split_trailing_comment(&after_colon);
            if let Some(c) = trailing_comment {
                out.nested_comments.push(NestedComment {
                    container_path: parent_path,
                    position: key_index,
                    text: strip_comment_marker(&c).to_string(),
                    inline: true,
                });
                let head = format!("{:width$}", "", width = indent);
                cleaned_lines.push(format!("{}{}:{}", head, key, value_part));
            } else {
                cleaned_lines.push(line.to_string());
            }

            if has_empty_inline_value(&after_colon) {
                stack.push(Frame {
                    indent: indent + 2,
                    path: key_path,
                    kind: None,
                    child_count: 0,
                });
            }

            if is_block_scalar_header(&value_part) {
                block_scalar_indent = Some(indent);
            }
            continue;
        }

        cleaned_lines.push(line.to_string());
    }

    out.cleaned_yaml = cleaned_lines.join("\n");
    out
}

/// Return the index of the deepest frame matching `indent` and `kind`,
/// pushing a new frame if the current top is shallower (safety net for
/// unusual layouts; the placeholder frame from case 3 usually covers this).
fn ensure_frame_at_indent(stack: &mut Vec<Frame>, indent: usize, kind: FrameKind) -> usize {
    let top_idx = stack.len() - 1;
    let top = &mut stack[top_idx];

    if top.indent == indent {
        if top.kind.is_none() {
            top.kind = Some(kind);
        }
        return top_idx;
    }

    let parent_path = top.path.clone();
    stack.push(Frame {
        indent,
        path: parent_path,
        kind: Some(kind),
        child_count: 0,
    });
    stack.len() - 1
}

fn strip_comment_marker(raw: &str) -> &str {
    let after = raw.trim_start_matches('#');
    after.strip_prefix(' ').unwrap_or(after)
}

fn leading_space_count(line: &str) -> usize {
    line.bytes().take_while(|b| *b == b' ').count()
}

/// `true` when a field value is a YAML block-scalar header (`|` or `>`, with
/// optional chomping/indent indicators). Unquoted plain scalars cannot begin
/// with these characters, so a leading `|`/`>` unambiguously opens a literal/
/// folded block whose following content lines are text, not YAML structure.
fn is_block_scalar_header(value: &str) -> bool {
    let t = value.trim_start();
    t.starts_with('|') || t.starts_with('>')
}

/// `true` when the value portion of a `key:` line is empty — real value is on
/// subsequent indented lines.
fn has_empty_inline_value(after_colon: &str) -> bool {
    let (v, _) = split_trailing_comment(after_colon);
    v.trim().is_empty()
}

/// Split a line into `(key, rest_after_colon)`, or `None` for non-key lines.
/// Handles `[a-zA-Z_][a-zA-Z0-9_]*` and `$`-prefixed system keys.
fn split_key(line: &str) -> Option<(String, String)> {
    let bytes = line.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut i;
    if bytes[0] == b'$' {
        if bytes.len() < 2 || !(bytes[1].is_ascii_alphabetic() || bytes[1] == b'_') {
            return None;
        }
        i = 2;
    } else if bytes[0].is_ascii_alphabetic() || bytes[0] == b'_' {
        i = 1;
    } else {
        return None;
    }
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b':' {
        return None;
    }
    let key = line[..i].to_string();
    let rest = line[i + 1..].to_string();
    Some((key, rest))
}

/// Split `value` into `(value_without_comment, trailing_comment)`.
/// Respects `"..."` and `'...'` quoting; ` #` or `\t#` outside quotes
/// starts a comment.
fn split_trailing_comment(value: &str) -> (String, Option<String>) {
    let bytes = value.as_bytes();
    let mut i = 0;
    let mut prev_was_ws = true;
    let mut in_dq = false;
    let mut in_sq = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_dq {
            if b == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_dq = false;
            }
        } else if in_sq {
            if b == b'\'' {
                in_sq = false;
            }
        } else {
            if b == b'"' {
                in_dq = true;
            } else if b == b'\'' {
                in_sq = true;
            } else if b == b'#' && prev_was_ws {
                let v = value[..i].trim_end().to_string();
                let c = value[i..].to_string();
                return (v, Some(c));
            }
        }
        prev_was_ws = matches!(b, b' ' | b'\t');
        i += 1;
    }
    (value.to_string(), None)
}

/// Inspect a field value for `!fill` and other tags.
///
/// Returns `(fill, value_without_tag, had_other_tag, fill_target_err)`.
/// `fill_target_err` is set when `!fill` targets a mapping (rejected;
/// scalars and sequences are allowed).
fn inspect_fill_and_tags(value: &str, key: &str) -> (bool, String, bool, Option<String>) {
    let trimmed = value.trim_start();
    let leading_ws_len = value.len() - trimmed.len();

    if trimmed.is_empty() {
        return (false, value.to_string(), false, None);
    }

    if trimmed == "!fill" {
        let reconstructed = value[..leading_ws_len].to_string();
        return (true, reconstructed, false, None);
    }

    if let Some(rest) = trimmed.strip_prefix("!fill") {
        if rest.starts_with(' ') || rest.starts_with('\t') || rest.is_empty() {
            let rest_trim = rest.trim_start();
            let err = if rest_trim.starts_with('{') {
                Some(format!(
                    "`!fill` on key `{}` targets a mapping; `!fill` is supported on scalars and sequences only",
                    key
                ))
            } else {
                None
            };
            let reconstructed = if rest_trim.is_empty() {
                value[..leading_ws_len].to_string()
            } else {
                format!(" {}", rest_trim)
            };
            return (true, reconstructed, false, err);
        }
    }

    if trimmed.starts_with('!') {
        return (false, value.to_string(), true, None);
    }

    (false, value.to_string(), false, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_own_line_comments() {
        let input = "# top\ntitle: foo\n# mid\nauthor: bar\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.items,
            vec![
                PreItem::Comment {
                    text: "top".to_string(),
                    inline: false,
                },
                PreItem::Field {
                    key: "title".to_string(),
                    fill: false,
                },
                PreItem::Comment {
                    text: "mid".to_string(),
                    inline: false,
                },
                PreItem::Field {
                    key: "author".to_string(),
                    fill: false,
                },
            ]
        );
        assert!(out.nested_comments.is_empty());
    }

    #[test]
    fn splits_trailing_comments() {
        let input = "title: foo # inline\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.items,
            vec![
                PreItem::Field {
                    key: "title".to_string(),
                    fill: false,
                },
                PreItem::Comment {
                    text: "inline".to_string(),
                    inline: true,
                },
            ]
        );
        assert!(out.cleaned_yaml.contains("title: foo"));
        assert!(!out.cleaned_yaml.contains("inline"));
    }

    #[test]
    fn detects_fill_on_scalar() {
        let input = "dept: !fill Department\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.items,
            vec![PreItem::Field {
                key: "dept".to_string(),
                fill: true,
            }]
        );
        assert!(out.cleaned_yaml.contains("dept: Department"));
        assert!(!out.cleaned_yaml.contains("!fill"));
    }

    #[test]
    fn detects_bare_fill() {
        let input = "dept: !fill\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.items,
            vec![PreItem::Field {
                key: "dept".to_string(),
                fill: true,
            }]
        );
        assert!(!out.cleaned_yaml.contains("!fill"));
    }

    #[test]
    fn unknown_tag_warns() {
        let input = "x: !custom value\n";
        let out = prescan_fence_content(input);
        assert!(
            out.warnings
                .iter()
                .any(|w| w.code.as_deref() == Some("parse::unsupported_yaml_tag")),
            "expected unsupported_yaml_tag warning"
        );
    }

    #[test]
    fn nested_comment_in_sequence_captured() {
        let input = "arr:\n  # before-first\n  - a\n  # between\n  - b\n  # after-last\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.nested_comments,
            vec![
                NestedComment {
                    container_path: vec![CommentPathSegment::Key("arr".to_string())],
                    position: 0,
                    text: "before-first".to_string(),
                    inline: false,
                },
                NestedComment {
                    container_path: vec![CommentPathSegment::Key("arr".to_string())],
                    position: 1,
                    text: "between".to_string(),
                    inline: false,
                },
                NestedComment {
                    container_path: vec![CommentPathSegment::Key("arr".to_string())],
                    position: 2,
                    text: "after-last".to_string(),
                    inline: false,
                },
            ]
        );
        assert!(
            !out.warnings
                .iter()
                .any(|w| w.code.as_deref() == Some("parse::comments_in_nested_yaml_dropped")),
            "nested comments are preserved, so no dropped-comment warning is emitted"
        );
    }

    #[test]
    fn nested_comment_in_mapping_captured() {
        let input = "outer:\n  # comment\n  inner: 1\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.nested_comments,
            vec![NestedComment {
                container_path: vec![CommentPathSegment::Key("outer".to_string())],
                position: 0,
                text: "comment".to_string(),
                inline: false,
            }]
        );
    }

    #[test]
    fn deep_nested_comment_path() {
        let input = "outer:\n  inner:\n    # deep\n    leaf: 1\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.nested_comments,
            vec![NestedComment {
                container_path: vec![
                    CommentPathSegment::Key("outer".to_string()),
                    CommentPathSegment::Key("inner".to_string()),
                ],
                position: 0,
                text: "deep".to_string(),
                inline: false,
            }]
        );
    }

    #[test]
    fn comment_inside_seq_of_maps() {
        let input = "items:\n  - name: a\n    # inside-first\n    val: 1\n  - name: b\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.nested_comments,
            vec![NestedComment {
                container_path: vec![
                    CommentPathSegment::Key("items".to_string()),
                    CommentPathSegment::Index(0),
                ],
                position: 1,
                text: "inside-first".to_string(),
                inline: false,
            }]
        );
    }

    #[test]
    fn nested_inline_on_sequence_item() {
        let input = "arr:\n  - a # tail\n  - b\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.nested_comments,
            vec![NestedComment {
                container_path: vec![CommentPathSegment::Key("arr".to_string())],
                position: 0,
                text: "tail".to_string(),
                inline: true,
            }]
        );
        assert!(out.cleaned_yaml.contains("- a\n"));
        assert!(!out.cleaned_yaml.contains("tail"));
    }

    #[test]
    fn nested_inline_on_mapping_field() {
        let input = "outer:\n  inner: 1 # tail\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.nested_comments,
            vec![NestedComment {
                container_path: vec![CommentPathSegment::Key("outer".to_string())],
                position: 0,
                text: "tail".to_string(),
                inline: true,
            }]
        );
    }

    #[test]
    fn fill_on_flow_sequence_allowed() {
        let input = "x: !fill [1, 2]\n";
        let out = prescan_fence_content(input);
        assert!(
            out.fill_target_errors.is_empty(),
            "expected no error; !fill on sequences is supported"
        );
        assert_eq!(
            out.items,
            vec![PreItem::Field {
                key: "x".to_string(),
                fill: true,
            }]
        );
    }

    #[test]
    fn sequence_with_multibyte_after_dash_does_not_panic() {
        // En-dash (3 bytes), em-dash (3 bytes), smart quote (3 bytes), and emoji
        // (4 bytes) appearing immediately after `- ` or as a sibling bullet
        // marker. Earlier versions sliced `&trimmed[2..]` here; if that ever
        // regresses to indexing inside a multi-byte codepoint, this test will
        // panic with `"byte index 2 is not a char boundary"`.
        let inputs = [
            "arr:\n  - – en-dash\n  - — em-dash\n",
            "arr:\n  - \u{2013}line\n  - \u{2014}line\n",
            "arr:\n  - \u{201C}smart-quoted\u{201D}\n",
            "arr:\n  - \u{1F600} emoji\n",
            // A literal block scalar holding mixed dashes — mirrors the eval
            // payload (`bullets: |` with `–` substituted for `-`).
            "bullets: |\n  - (U) **A:** text\n  – (U) **B:** text\n",
        ];
        for input in inputs {
            let out = prescan_fence_content(input);
            // We don't care about the exact items; just that no panic occurred
            // and that the cleaned YAML round-trips line count.
            assert_eq!(out.cleaned_yaml.lines().count(), input.lines().count());
        }
    }

    #[test]
    fn block_scalar_content_is_not_parsed_as_structure() {
        // A markdown block scalar whose content contains a `#` heading, a
        // `- ` bullet, and a `key:` line. None of these are YAML structure —
        // they must survive verbatim in the cleaned YAML, and the field after
        // the block must still parse as a top-level field.
        let input =
            "bio: |-\n  ## About me\n\n  - point one\n  role: engineer\n  Done.\nname: jane\n";
        let out = prescan_fence_content(input);

        // The heading is content, not a stripped comment.
        assert!(
            out.cleaned_yaml.contains("## About me"),
            "block-scalar heading must survive: {:?}",
            out.cleaned_yaml
        );
        assert!(out.cleaned_yaml.contains("- point one"));
        assert!(out.cleaned_yaml.contains("role: engineer"));

        // Nothing from inside the block leaked into items as a comment/field.
        assert!(
            !out.items.iter().any(|i| matches!(
                i,
                PreItem::Comment { text, .. } if text.contains("About")
            )),
            "block-scalar `#` line must not become a comment"
        );
        assert!(
            !out.items
                .iter()
                .any(|i| matches!(i, PreItem::Field { key, .. } if key == "role")),
            "block-scalar `key:` line must not become a field"
        );

        // The two real top-level fields are `bio` then `name`, in order.
        let fields: Vec<&str> = out
            .items
            .iter()
            .filter_map(|i| match i {
                PreItem::Field { key, .. } => Some(key.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(fields, vec!["bio", "name"]);
    }

    #[test]
    fn sequence_item_block_scalar_content_is_not_parsed_as_structure() {
        // A `markdown[]` array authored as `- |-` block-scalar items. Content
        // lines (heading, bullet, `key:`) must survive verbatim, and the next
        // item at the dash indent must still parse as a sequence item.
        let input = "items:\n  - |-\n    ## Heading\n    - inner bullet\n    role: x\n  - second\n";
        let out = prescan_fence_content(input);

        assert!(
            out.cleaned_yaml.contains("## Heading"),
            "block-scalar heading inside a sequence item must survive: {:?}",
            out.cleaned_yaml
        );
        assert!(out.cleaned_yaml.contains("- inner bullet"));
        assert!(out.cleaned_yaml.contains("role: x"));
        // The heading must not have been captured as a comment.
        assert!(
            !out.nested_comments
                .iter()
                .any(|c| c.text.contains("Heading")),
            "block-scalar `#` line must not become a nested comment"
        );
        // `second` is preserved (the block ended at the next dash).
        assert!(out.cleaned_yaml.contains("- second"));
    }

    #[test]
    fn fill_on_flow_mapping_errors() {
        let input = "x: !fill {a: 1}\n";
        let out = prescan_fence_content(input);
        assert!(
            !out.fill_target_errors.is_empty(),
            "expected error; !fill on mappings is rejected"
        );
    }
}
