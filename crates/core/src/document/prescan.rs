//! Pre-scan of a metadata fence's YAML content to recover features that
//! serde_saphyr discards.
//!
//! Three features are recovered here:
//!
//! 1. **Top-level comments.** YAML comments are dropped by the YAML parser.
//!    To round-trip them as [`super::FrontmatterItem::Comment`], we extract them
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
/// `Comment` stands alone; `Field` captures only the `fill` flag because the
/// value is produced by serde_saphyr parsing the cleaned text. The matching
/// YAML key is the lookup key into the parsed map.
///
/// `Comment.inline` distinguishes own-line comments (`# text` on a line by
/// itself) from inline trailing comments (`field: value # text`). Inline
/// top-level comments always immediately follow their host `Field` in the
/// item stream; the emitter peeks ahead by one slot to attach them.
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

/// A comment that appears inside a nested mapping or sequence.
///
/// `container_path` locates the immediate parent container.
///
/// Position semantics depend on `inline`:
/// - **Own-line (`inline = false`)**: `position` is the slot ordinal within
///   the container's child list, ranging `0..=child_count`. The comment is
///   rendered before the child at this position. `position == child_count`
///   means "after all children".
/// - **Inline (`inline = true`)**: `position` is the host child's index,
///   ranging `0..child_count`. The comment is attached to that child's
///   trailing line. An inline comment whose host is missing at emit time
///   (orphan) degrades to an own-line comment at the same indent.
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
    /// YAML text with `!fill` tags stripped and all comment lines removed.
    /// Suitable for feeding into serde_saphyr.
    pub cleaned_yaml: String,
    /// Ordered items discovered at the top level — fields (with fill flags)
    /// and own-line top-level comments, in source order.
    pub items: Vec<PreItem>,
    /// Comments inside nested containers, with structural paths.
    pub nested_comments: Vec<NestedComment>,
    /// Warnings produced during the scan.
    pub warnings: Vec<Diagnostic>,
    /// Unsupported-fill-target errors. The parser turns these into
    /// `ParseError::InvalidStructure` rejections (`!fill` on mappings).
    pub fill_target_errors: Vec<String>,
}

/// Tracks one open YAML container while scanning lines.
#[derive(Debug)]
struct Frame {
    /// Indent (in columns) of children of this container.
    indent: usize,
    /// Path to this container from the fence root.
    path: Vec<CommentPathSegment>,
    /// Container kind. `None` until the first child line determines it.
    kind: Option<FrameKind>,
    /// Number of children seen so far.
    child_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Mapping,
    Sequence,
}

/// Scan the body of a YAML metadata fence.
///
/// `content` is the text between the opening and closing `---` markers
/// (exclusive), with leading/trailing whitespace preserved.
pub fn prescan_fence_content(content: &str) -> PreScan {
    let mut out = PreScan::default();

    // We operate on the raw text to preserve positions. `lines()` strips
    // line endings; we rebuild with `\n` which is what serde_saphyr expects.
    let lines: Vec<&str> = content.split('\n').collect();
    let mut cleaned_lines: Vec<String> = Vec::with_capacity(lines.len());

    // Stack of open containers. The root frame is the frontmatter mapping
    // itself; children appear at indent 0.
    let mut stack: Vec<Frame> = vec![Frame {
        indent: 0,
        path: Vec::new(),
        kind: Some(FrameKind::Mapping),
        child_count: 0,
    }];

    for raw_line in &lines {
        let line = *raw_line;
        let indent = leading_space_count(line);
        let trimmed = &line[indent..];

        // Skip blank lines (no structural meaning, no comment).
        if trimmed.is_empty() {
            cleaned_lines.push(line.to_string());
            continue;
        }

        // Pop frames that this line has dedented out of. A line at indent
        // `indent` belongs to the deepest frame whose `indent <= indent`.
        // (Equality means the line is a child at this frame's level.)
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

            // Determine the deepest frame that contains this line.
            // For a comment at indent N, the containing frame is the one
            // with the largest indent <= N. The stack is ordered shallow
            // to deep; the last frame is the deepest. After the dedent
            // pop above, the top frame's indent is <= indent, which is
            // what we want.
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
            // Don't emit the line into the cleaned YAML — serde_saphyr
            // ignores comments either way, but omitting the line avoids
            // ambiguity with `!fill` rewriting.
            continue;
        }

        // Case 2: sequence item line (`- ...`).
        if trimmed == "-" || trimmed.starts_with("- ") {
            // The frame at this indent must be a sequence. If the deepest
            // frame's indent matches this line's indent, claim it; if it
            // doesn't, push a fresh sequence frame at this indent under
            // the deepest container.
            let frame_idx = ensure_frame_at_indent(&mut stack, indent, FrameKind::Sequence);
            let frame = &mut stack[frame_idx];
            let item_index = frame.child_count;
            frame.child_count += 1;
            let parent_path: Vec<CommentPathSegment> = frame.path.clone();
            // Snapshot the item path before borrowing mutably again below.
            let item_path: Vec<CommentPathSegment> = {
                let mut p = parent_path.clone();
                p.push(CommentPathSegment::Index(item_index));
                p
            };
            // Drop frames deeper than this sequence; the new item starts
            // a fresh nested context.
            while stack.len() > frame_idx + 1 {
                stack.pop();
            }

            // Detach a possible trailing comment on the item line.
            let after_dash_full = if trimmed == "-" { "" } else { &trimmed[2..] };
            let (after_dash, trailing_comment) = split_trailing_comment(after_dash_full);
            let after_dash_trimmed = after_dash.trim_start();
            let inline_indent_offset = indent + 2 + (after_dash.len() - after_dash_trimmed.len());

            if after_dash_trimmed.is_empty() {
                // No inline value. Children, if any, will appear on the
                // following lines with indent > this line's indent. Push a
                // placeholder frame so when those children arrive, the
                // sequence-item frame is already on the stack.
                //
                // We push a frame with indent = indent + 2; the actual
                // child kind/indent gets resolved when the next non-empty
                // line arrives.
                stack.push(Frame {
                    indent: indent + 2,
                    path: item_path,
                    kind: None,
                    child_count: 0,
                });
            } else if split_key(after_dash_trimmed).is_some() {
                // Inline mapping start (`- key: ...`). The key is the first
                // child of an implicit mapping whose siblings sit at the
                // same column as the key.
                stack.push(Frame {
                    indent: inline_indent_offset,
                    path: item_path,
                    kind: Some(FrameKind::Mapping),
                    child_count: 1,
                });
            }
            // Otherwise: inline scalar value, no further nesting.

            // Rebuild the line with the trailing comment stripped, and
            // capture it as an inline NestedComment attached to this item.
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
            continue;
        }

        // Case 3: top-level field line with possible `!fill` tag and/or
        // trailing comment. Top-level only — `is_top_level` mirrors the
        // pre-existing semantics.
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

                // Update the structural stack for this top-level key.
                // The root frame is at index 0; children appear at indent 0.
                let root = &mut stack[0];
                root.child_count += 1;
                let key_path = vec![CommentPathSegment::Key(key.clone())];

                // Pop everything but the root.
                while stack.len() > 1 {
                    stack.pop();
                }

                // If the value is empty (block style: `key:` followed by
                // indented children), push a frame so nested comments can
                // be attached. Otherwise (inline scalar/flow), no nested
                // children come from this key.
                if has_empty_inline_value(&value_without_tag) {
                    stack.push(Frame {
                        indent: 2,
                        path: key_path,
                        kind: None,
                        child_count: 0,
                    });
                }

                // Rebuild the line without the `!fill` tag (and without
                // the trailing comment, since that goes on its own
                // line now).
                let cleaned = format!("{}:{}", key, value_without_tag);
                cleaned_lines.push(cleaned);

                if let Some(c) = trailing_comment {
                    out.items.push(PreItem::Comment {
                        text: strip_comment_marker(&c).to_string(),
                        inline: true,
                    });
                }

                continue;
            }
        }

        // Case 4: nested key line (`key:` or `key: value`) inside a block
        // mapping. We recognise simple `key:` patterns; unusual forms fall
        // through to verbatim pass-through.
        if let Some((key, after_colon)) = split_key(trimmed) {
            // The frame at this indent must be a mapping.
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
            // Drop frames deeper than this mapping; siblings reset nesting.
            while stack.len() > frame_idx + 1 {
                stack.pop();
            }

            // Detach a possible trailing comment on the line. We keep the
            // value (sans comment) in the cleaned YAML and capture the
            // comment as an inline NestedComment attached to this key.
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

            // If the value is empty (block style) push a frame for nested
            // children at indent + 2.
            if has_empty_inline_value(&after_colon) {
                stack.push(Frame {
                    indent: indent + 2,
                    path: key_path,
                    kind: None,
                    child_count: 0,
                });
            }
            continue;
        }

        // Everything else: pass through verbatim.
        cleaned_lines.push(line.to_string());
    }

    out.cleaned_yaml = cleaned_lines.join("\n");
    out
}

/// Ensure the deepest frame on the stack matches the given `indent` and
/// kind, pushing a new frame if necessary. Returns the index of the matched
/// or freshly-pushed frame.
fn ensure_frame_at_indent(stack: &mut Vec<Frame>, indent: usize, kind: FrameKind) -> usize {
    // After dedent popping, the top frame has `indent <= indent`. If it
    // matches exactly, claim it. Otherwise, push a new child frame under
    // it that has the requested indent.
    let top_idx = stack.len() - 1;
    let top = &mut stack[top_idx];

    if top.indent == indent {
        if top.kind.is_none() {
            top.kind = Some(kind);
        }
        return top_idx;
    }

    // The top frame is shallower (its indent < indent). Push a new frame
    // at this indent, parented under the top frame. The new frame's path
    // is a continuation: for a sequence at deeper indent under a mapping,
    // the path is the same as the parent's `path` (because the sequence
    // is the value of the parent's most recent key).
    //
    // Concretely, when we encounter `- foo` at indent 2 and the stack top
    // is the root mapping with indent 0, the parent frame's most-recent
    // child path was already pushed when we saw `key:` in case 3 (we
    // pushed a placeholder frame at indent 2 with `path = [Key(key)]` and
    // unknown kind). So usually we won't reach this branch — the
    // placeholder is already there. This branch is a safety net for
    // unusual layouts.
    let parent_path = top.path.clone();
    stack.push(Frame {
        indent,
        path: parent_path,
        kind: Some(kind),
        child_count: 0,
    });
    stack.len() - 1
}

/// Strip a YAML comment marker (`# `) from the start of a string.
///
/// Strips all leading `#` characters, then one optional space.
fn strip_comment_marker(raw: &str) -> &str {
    let after = raw.trim_start_matches('#');
    after.strip_prefix(' ').unwrap_or(after)
}

/// Number of leading ASCII spaces. Tabs are not expanded; they don't appear
/// in canonical Quillmark YAML and would be a separate problem.
fn leading_space_count(line: &str) -> usize {
    line.bytes().take_while(|b| *b == b' ').count()
}

/// `true` when the value portion of a `key:` line is empty (after trimming
/// whitespace). Trailing comments are ignored. An empty value means the
/// real value is on subsequent indented lines (block mapping or sequence).
fn has_empty_inline_value(after_colon: &str) -> bool {
    let (v, _) = split_trailing_comment(after_colon);
    v.trim().is_empty()
}

/// Split a line into `(key, rest_after_colon)`. Returns `None` if the line
/// does not start with a bare YAML key.
fn split_key(line: &str) -> Option<(String, String)> {
    // Identifier-like keys only. YAML allows more, but Quillmark's schema
    // restricts field names to `[a-zA-Z_][a-zA-Z0-9_]*` (and reserved
    // uppercase sentinels). Anything more exotic falls through to the
    // unmodified path and will be parsed (or rejected) by serde_saphyr.
    let bytes = line.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    if !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_') {
        return None;
    }
    let mut i = 1;
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

/// Split a value string into `(value, trailing_comment)`.
///
/// Trailing comments begin with ` #` or `\t#` outside of any quoted string.
/// This is a simple scanner: it respects `"..."` and `'...'` quoting.
fn split_trailing_comment(value: &str) -> (String, Option<String>) {
    let bytes = value.as_bytes();
    let mut i = 0;
    let mut prev_was_ws = true; // allow `key:#` edge case to NOT be a comment
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

/// Inspect the value portion of a field line for `!fill` and other tags.
///
/// Returns `(fill, value_without_tag, had_other_tag, fill_target_err)`.
///
/// - `fill`: `true` when the value starts with `!fill`.
/// - `value_without_tag`: the same text with the `!fill` tag stripped;
///   leading whitespace is preserved so YAML parsing still sees a clean
///   scalar.
/// - `had_other_tag`: `true` when a non-`!fill` `!tag` was found at the
///   start of the value. The tag is *not* stripped (serde_saphyr tolerates
///   and drops unknown tags), so callers get a warning only.
/// - `fill_target_err`: populated when `!fill` is applied to a mapping
///   (flow `{...}` or block form). `!fill` on mappings is rejected because
///   top-level `type: object` is not a supported schema type in Quillmark;
///   `!fill` on scalars and sequences is allowed.
fn inspect_fill_and_tags(value: &str, key: &str) -> (bool, String, bool, Option<String>) {
    let trimmed = value.trim_start();
    let leading_ws_len = value.len() - trimmed.len();

    // Exactly empty / null (e.g. `key:` with nothing) — not a fill target.
    if trimmed.is_empty() {
        return (false, value.to_string(), false, None);
    }

    // `!fill` alone on the line (bare tag, no value) → placeholder. The
    // value may be null (no continuation) or a block sequence on the
    // following indented lines. serde_saphyr produces the actual value.
    if trimmed == "!fill" {
        // Replace the tag with nothing; leave the leading whitespace so the
        // line shape is preserved (serde_saphyr treats `key: ` as null,
        // and if a block sequence follows on indented lines, it parses as
        // a sequence).
        let reconstructed = value[..leading_ws_len].to_string();
        return (true, reconstructed, false, None);
    }

    // `!fill <value>` → strip tag, record fill=true.
    if let Some(rest) = trimmed.strip_prefix("!fill") {
        // Must be followed by whitespace or end-of-value to count; otherwise
        // it's `!fillwhatever` which is a non-`!fill` tag.
        if rest.starts_with(' ') || rest.starts_with('\t') || rest.is_empty() {
            let rest_trim = rest.trim_start();
            // Reject flow-mappings (`!fill {...}`); top-level `type: object`
            // isn't supported by the schema. Flow sequences (`!fill [...]`)
            // and scalars are allowed.
            let err = if rest_trim.starts_with('{') {
                Some(format!(
                    "`!fill` on key `{}` targets a mapping; `!fill` is supported on scalars and sequences only",
                    key
                ))
            } else {
                None
            };
            // Reconstruct: one space + the rest (trimmed) so the cleaned
            // text reads `key: rest`.
            let reconstructed = if rest_trim.is_empty() {
                value[..leading_ws_len].to_string()
            } else {
                format!(" {}", rest_trim)
            };
            return (true, reconstructed, false, err);
        }
    }

    // Any other `!tag` prefix is a non-fill custom tag. Leave the value
    // alone; serde_saphyr will strip the tag.
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
            "no dropped-comment warning expected; nested comments are now preserved"
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
        // Each sequence item is a mapping. A comment between keys of the
        // first item belongs to that item's mapping.
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
        // `- a # tail` attaches an inline comment to item 0 (host index, not
        // the slot after).
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
        // `inner: 1 # tail` inside `outer:` attaches inline at host index 0.
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
    fn fill_on_flow_mapping_errors() {
        let input = "x: !fill {a: 1}\n";
        let out = prescan_fence_content(input);
        assert!(
            !out.fill_target_errors.is_empty(),
            "expected error; !fill on mappings is rejected"
        );
    }
}
