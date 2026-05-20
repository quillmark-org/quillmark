//! Canonical Markdown emission for [`Document`].
//!
//! This module implements [`Document::to_markdown`], which converts a typed
//! in-memory `Document` back into canonical Quillmark Markdown.
//!
//! ## YAML emission strategy
//!
//! Scalar emission (quoting, escaping, multi-line handling) is delegated to
//! `serde-saphyr` — the same library used for parsing. This makes the emit
//! and parse sides of the wire symmetric by construction: anything saphyr
//! decides to quote on emit, saphyr will read back as a string on parse.
//! The hand-written quoting heuristics that used to live here couldn't
//! keep pace with YAML 1.1 edge cases (`on`/`yes`/`off`, leading-zero
//! integers, `1.0`-style numerics) — saphyr already handles them all.
//!
//! `prefer_block_scalars: false` keeps multi-line strings inline as
//! double-quoted scalars with `\n` escapes, so the emitter never produces
//! `|` or `>` block forms in v1.
//!
//! This module owns the surrounding structure — `~~~card-yaml` fences,
//! `#@` system-metadata headers, field ordering, indentation, comment
//! interleaving — and calls saphyr only for the scalar leaves.

use serde_json::Value as JsonValue;
use serde_saphyr::{FlowMap, FlowSeq, SerializerOptions};

use super::payload::PayloadItem;
use super::prescan::{CommentPathSegment, NestedComment};
use super::{Card, Document};

// ── Public entry point ────────────────────────────────────────────────────────

impl Document {
    /// Emit canonical Quillmark Markdown from this document.
    ///
    /// # Contract
    ///
    /// 1. **Type-fidelity round-trip.** `Document::from_markdown(&doc.to_markdown())`
    ///    returns a `Document` equal to `doc` by value *and* by type variant.
    ///    `QuillValue::String("on")` round-trips as a string, never as a bool.
    ///    `QuillValue::String("01234")` round-trips as a string, never as an
    ///    integer.  This guarantee is the whole point of owning emission.
    ///
    /// 2. **Emit-idempotent.** `to_markdown` is a pure function of `doc`; two
    ///    calls on the same `doc` return byte-equal strings.
    ///
    /// Byte-equality with the *original source* is **not** guaranteed.
    ///
    /// # Emission rules (§5.2)
    ///
    /// - Line endings: `\n` only.  CRLF normalization happens on import.
    /// - Every block is emitted as a `~~~card-yaml` fence: a `~~~card-yaml`
    ///   opener, the `#@` system-metadata header (`#@quill: <ref>` for the
    ///   root block, `#@kind: <kind>` for composable cards), the block's YAML
    ///   payload, then a closing `~~~`.
    /// - Cards: one blank line before each, then the block, then the card body.
    /// - Body: emitted verbatim after the root block (and after each card).
    /// - Mappings and sequences: **block style** at every nesting level.
    /// - Scalars (booleans, null, numbers, strings): delegated to
    ///   `serde-saphyr`, which emits the type-canonical form (`true`/
    ///   `false`, `null`, bare numeric literal) and quotes strings only
    ///   when the unquoted form would be misread (`on`/`yes`/`off`,
    ///   `null`/`~`, numeric-looking strings, leading flow indicators,
    ///   `: ` runs, …).  Quoting form is not stable — what matters is
    ///   that the emitted scalar round-trips to the same `QuillValue`
    ///   variant. This is the type-fidelity guarantee.
    /// - Multi-line strings: emitted as inline double-quoted scalars with
    ///   `\n` escapes; no `|` / `>` block forms.
    ///
    /// # Open decisions (resolved)
    ///
    /// - **Nested-map order.** `QuillValue` is backed by `serde_json::Value`
    ///   whose object type (`serde_json::Map`) preserves insertion order when the
    ///   `serde_json/preserve_order` feature is enabled (it is in this workspace).
    ///   Insertion order is therefore preserved for nested maps at emit time.
    ///
    /// - **Empty containers.**
    ///   - Empty object (`{}`) → the key is **omitted** from emit entirely.
    ///   - Empty array (`[]`) → emitted as `key: []\n`.
    ///
    /// # What is preserved
    ///
    /// - **YAML comments**: own-line and inline trailing comments round-trip
    ///   at their source position. Comments whose host disappears at emit time
    ///   (empty-mapping omission, programmatic field removal) degrade to
    ///   own-line comments at the same indent so the comment text is preserved
    ///   even when its position shifts.
    /// - **`!fill` tags**: round-trip via the `fill` flag on `PayloadItem::Field`.
    ///
    /// # What is lost
    ///
    /// - **Other custom tags** (`!include`, `!env`, …): the tag is dropped;
    ///   the scalar value is preserved.
    /// - **Original quoting style**: strings are re-emitted in saphyr's
    ///   canonical form (plain when safe, quoted when ambiguous). The
    ///   form chosen for emit may not match the form in the source.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();

        // ── Root block (card-yaml fence + global body) ────────────────────────
        emit_block(&mut out, self.main());
        out.push_str(self.main().body());

        // ── Composable cards ──────────────────────────────────────────────────
        // `ensure_blank_before_fence` normalises the separator before each
        // block, so edited bodies (which may lack a trailing blank line) still
        // round-trip.
        for card in self.cards() {
            ensure_blank_before_fence(&mut out);
            emit_block(&mut out, card);
            if !card.body().is_empty() {
                out.push_str(card.body());
            }
        }

        out
    }
}

// ── Block emission ────────────────────────────────────────────────────────────

/// Emit a card-yaml block: `~~~card-yaml`, the `#@` system-metadata header,
/// the YAML payload, then a closing `~~~`.
///
/// The `#@` header is emitted in the canonical key order `quill`, `kind`,
/// `id` — only keys the block declares appear. There is no `#@` line for an
/// inline comment to attach to, so an inline comment carried over at
/// `items[0]` degrades to an own-line comment as the first payload line,
/// preserving its text.
///
/// Three tildes are always a safe fence length: canonically emitted payload
/// lines never begin with `~` (keys are identifiers, sequence items start with
/// `-`, saphyr quotes any string that would, comments start with `#`), so the
/// fence can never be closed early.
fn emit_meta_line(out: &mut String, key: &str, value: &str) {
    out.push_str("#@");
    out.push_str(key);
    out.push_str(": ");
    out.push_str(value);
    out.push('\n');
}

fn emit_block(out: &mut String, card: &Card) {
    out.push_str("~~~card-yaml\n");

    let meta = card.meta();
    if let Some(quill) = &meta.quill {
        emit_meta_line(out, "quill", &quill.to_string());
    }
    if let Some(kind) = &meta.kind {
        emit_meta_line(out, "kind", kind);
    }
    if let Some(id) = &meta.id {
        emit_meta_line(out, "id", id);
    }

    emit_fence_items(
        out,
        card.payload().items(),
        card.payload().nested_comments(),
    );

    out.push_str("~~~\n");
}

/// Emit the ordered YAML items (fields and comments) of a fence body.
///
/// ## Inline-comment handling
///
/// - **Field + trailing inline.** A `Field` peeks at its successor: if the
///   next item is `Comment{inline:true}`, the comment text is passed to
///   [`emit_field`] as a trailer and consumed here. The trailer lands on the
///   field's key/value line.
/// - **Own-line / orphan.** A `Comment` that is not consumed by a field's
///   lookahead — own-line, or an inline orphan — renders as an own-line
///   `# text` comment.
fn emit_fence_items(out: &mut String, items: &[PayloadItem], nested: &[NestedComment]) {
    let mut i = 0;
    while i < items.len() {
        match &items[i] {
            PayloadItem::Field { key, value, fill } => {
                let trailer = items.get(i + 1).and_then(|next| match next {
                    PayloadItem::Comment { text, inline: true } => Some(text.as_str()),
                    _ => None,
                });
                let path = vec![CommentPathSegment::Key(key.clone())];
                emit_field(out, key, value.as_json(), 0, *fill, &path, nested, trailer);
                i += if trailer.is_some() { 2 } else { 1 };
            }
            PayloadItem::Comment { text, .. } => {
                out.push_str("# ");
                out.push_str(text);
                out.push('\n');
                i += 1;
            }
        }
    }
}

/// Ensures `out` ends with a `\n\n` suffix so the next card-yaml block has the
/// required blank line above it.
///
/// Under the separator-never-stored invariant, stored bodies may end with
/// their content (no newline), a content line terminator (`\n`), or an
/// author-intended blank line (`\n\n`, `\n\n\n`, …). In every case we append
/// exactly one `\n` to produce the blank line. If the body doesn't already
/// end in `\n`, we also append a line terminator first so content lines are
/// terminated in the emitted markdown.
///
/// Empty `out` needs no separator — a block at line 1 has no line above it.
fn ensure_blank_before_fence(out: &mut String) {
    if out.is_empty() {
        return;
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
}

// ── YAML value emission ───────────────────────────────────────────────────────

/// Emit own-line nested comments at `position` in `path` as `# text` lines
/// indented by `indent` spaces. Inline comments are skipped here — they are
/// consumed by `find_inline_trailer` at the host's emission site.
fn emit_own_line_pending(
    out: &mut String,
    path: &[CommentPathSegment],
    position: usize,
    indent: usize,
    nested: &[NestedComment],
) {
    for c in nested {
        if c.position == position && !c.inline && c.container_path.as_slice() == path {
            push_indent(out, indent);
            out.push_str("# ");
            out.push_str(&c.text);
            out.push('\n');
        }
    }
}

/// Look up the inline trailer for the child at `position` in `path`. If
/// multiple inline comments share this slot (programmatic edge case), the
/// first one is returned and the rest are emitted as own-line comments at
/// `indent` to preserve their text.
fn find_inline_trailer<'a>(
    out: &mut String,
    path: &[CommentPathSegment],
    position: usize,
    indent: usize,
    nested: &'a [NestedComment],
) -> Option<&'a str> {
    let mut chosen: Option<&str> = None;
    for c in nested {
        if c.position == position && c.inline && c.container_path.as_slice() == path {
            if chosen.is_none() {
                chosen = Some(c.text.as_str());
            } else {
                push_indent(out, indent);
                out.push_str("# ");
                out.push_str(&c.text);
                out.push('\n');
            }
        }
    }
    chosen
}

/// Emit any orphan inline comments (`inline=true` with `position >=
/// container_len`) as own-line comments at the trailing slot. These are
/// programmatic edge cases — well-formed prescan output never produces them.
fn emit_orphan_inlines(
    out: &mut String,
    path: &[CommentPathSegment],
    container_len: usize,
    indent: usize,
    nested: &[NestedComment],
) {
    for c in nested {
        if c.inline && c.position >= container_len && c.container_path.as_slice() == path {
            push_indent(out, indent);
            out.push_str("# ");
            out.push_str(&c.text);
            out.push('\n');
        }
    }
}

/// Append ` # trailer` to `out` if `trailer` is `Some`. Caller writes the
/// terminating `\n` afterwards.
fn push_trailer(out: &mut String, trailer: Option<&str>) {
    if let Some(t) = trailer {
        out.push_str(" # ");
        out.push_str(t);
    }
}

/// Emit a `key: <value>\n` pair at `indent` spaces.
///
/// `path` is the path to *this* field (parent path + this key). It's used as
/// the *container* path when recursing into the value: nested comments
/// captured at this path are interleaved between the value's children.
///
/// `inline_trailer`, when `Some`, is rendered as ` # text` on the field's
/// key/value line. For scalars this trails the value; for containers it
/// trails the `key:` line (before the indented children).
///
/// - Empty objects are **omitted** (caller skips them). An empty-object
///   field with an inline trailer degrades the trailer to an own-line
///   comment at `indent`, so the comment text is preserved even though its
///   host disappears.
/// - Empty arrays emit `key: []\n`.
/// - All other values follow the block-style rules.
/// - When `fill` is `true`, the emitted form is `key: !fill <value>` for
///   scalars, `key: !fill\n  - …` for non-empty sequences,
///   `key: !fill []` for empty sequences, and `key: !fill` for null.
///   Mappings are rejected at parse and never reach this path.
#[allow(clippy::too_many_arguments)]
fn emit_field(
    out: &mut String,
    key: &str,
    value: &JsonValue,
    indent: usize,
    fill: bool,
    path: &[CommentPathSegment],
    nested: &[NestedComment],
    inline_trailer: Option<&str>,
) {
    if fill {
        push_indent(out, indent);
        out.push_str(key);
        match value {
            JsonValue::Null => {
                out.push_str(": !fill");
                push_trailer(out, inline_trailer);
                out.push('\n');
            }
            JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => {
                out.push_str(": !fill ");
                emit_scalar(out, value);
                push_trailer(out, inline_trailer);
                out.push('\n');
            }
            JsonValue::Array(items) if items.is_empty() => {
                out.push_str(": !fill []");
                push_trailer(out, inline_trailer);
                out.push('\n');
            }
            JsonValue::Array(items) => {
                out.push_str(": !fill");
                push_trailer(out, inline_trailer);
                out.push('\n');
                emit_sequence_children(out, items, indent + 2, path, nested);
            }
            JsonValue::Object(_) => {
                // Parser rejects !fill on mappings; recovery path only.
                out.push_str(": ");
                emit_scalar(out, value);
                push_trailer(out, inline_trailer);
                out.push('\n');
            }
        }
        return;
    }
    match value {
        JsonValue::Object(map) if map.is_empty() => {
            // Empty object → omit the key entirely. If there's an inline
            // trailer, degrade it to an own-line comment so its text isn't
            // lost.
            if let Some(t) = inline_trailer {
                push_indent(out, indent);
                out.push_str("# ");
                out.push_str(t);
                out.push('\n');
            }
        }
        JsonValue::Object(map) => {
            push_indent(out, indent);
            out.push_str(key);
            out.push(':');
            push_trailer(out, inline_trailer);
            out.push('\n');
            emit_mapping_children(out, map, indent + 2, path, nested);
        }
        JsonValue::Array(items) if items.is_empty() => {
            push_indent(out, indent);
            out.push_str(key);
            out.push_str(": []");
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
        JsonValue::Array(items) => {
            push_indent(out, indent);
            out.push_str(key);
            out.push(':');
            push_trailer(out, inline_trailer);
            out.push('\n');
            emit_sequence_children(out, items, indent + 2, path, nested);
        }
        _ => {
            push_indent(out, indent);
            out.push_str(key);
            out.push_str(": ");
            emit_scalar(out, value);
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
    }
}

/// Emit the children of a mapping value with comment interleaving.
///
/// `child_indent` is the indent at which each child key sits; nested
/// comments inside this mapping are emitted at the same indent. `path` is
/// the path to the mapping container (its key in the parent).
fn emit_mapping_children(
    out: &mut String,
    map: &serde_json::Map<String, JsonValue>,
    child_indent: usize,
    path: &[CommentPathSegment],
    nested: &[NestedComment],
) {
    for (i, (k, v)) in map.iter().enumerate() {
        emit_own_line_pending(out, path, i, child_indent, nested);
        let trailer = find_inline_trailer(out, path, i, child_indent, nested);
        let mut child_path = path.to_vec();
        child_path.push(CommentPathSegment::Key(k.clone()));
        emit_field(out, k, v, child_indent, false, &child_path, nested, trailer);
    }
    emit_own_line_pending(out, path, map.len(), child_indent, nested);
    emit_orphan_inlines(out, path, map.len(), child_indent, nested);
}

/// Emit the children of a sequence value with comment interleaving.
///
/// `base_indent` is the indent at which each `- ` sits; nested comments
/// inside this sequence are emitted at the same indent.
fn emit_sequence_children(
    out: &mut String,
    items: &[JsonValue],
    base_indent: usize,
    path: &[CommentPathSegment],
    nested: &[NestedComment],
) {
    for (i, item) in items.iter().enumerate() {
        emit_own_line_pending(out, path, i, base_indent, nested);
        let trailer = find_inline_trailer(out, path, i, base_indent, nested);
        let mut child_path = path.to_vec();
        child_path.push(CommentPathSegment::Index(i));
        emit_sequence_item(out, item, base_indent, &child_path, nested, trailer);
    }
    emit_own_line_pending(out, path, items.len(), base_indent, nested);
    emit_orphan_inlines(out, path, items.len(), base_indent, nested);
}

/// Emit a single `- <value>\n` sequence item at `base_indent` spaces.
///
/// `path` is the path to *this* item (parent path + item index).
///
/// `inline_trailer`, when `Some`, is rendered as ` # text` on the `-` line.
/// For mapping items the trailer co-exists with any inline trailer at index
/// 0 of the inner mapping (the latter would be on the same physical line);
/// in well-formed input only one of them is present, but if both appear
/// the inner one degrades to an own-line comment beneath the `- ` line.
fn emit_sequence_item(
    out: &mut String,
    value: &JsonValue,
    base_indent: usize,
    path: &[CommentPathSegment],
    nested: &[NestedComment],
    inline_trailer: Option<&str>,
) {
    match value {
        JsonValue::Object(map) if map.is_empty() => {
            // Empty nested object in a sequence: emit as `- {}`
            push_indent(out, base_indent);
            out.push_str("- {}");
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
        JsonValue::Object(map) => {
            // Block mapping inside a sequence. First key on the same line
            // as `- `; subsequent keys indented by 2. Comments inside this
            // mapping use this item's path as the container.
            emit_own_line_pending(out, path, 0, base_indent, nested);

            let mut first = true;
            for (i, (k, v)) in map.iter().enumerate() {
                if !first {
                    emit_own_line_pending(out, path, i, base_indent + 2, nested);
                }
                let inner_trailer = find_inline_trailer(out, path, i, base_indent + 2, nested);
                let mut child_path = path.to_vec();
                child_path.push(CommentPathSegment::Key(k.clone()));
                if first {
                    // The seq-item's trailer and the first key's trailer
                    // both target the `- key: ...` line. Prefer the
                    // seq-item's; degrade the loser to own-line.
                    let line_trailer = inline_trailer.or(inner_trailer);
                    push_indent(out, base_indent);
                    out.push_str("- ");
                    emit_field_inline(
                        out,
                        k,
                        v,
                        base_indent + 2,
                        &child_path,
                        nested,
                        line_trailer,
                    );
                    if let (Some(_), Some(loser)) = (inline_trailer, inner_trailer) {
                        push_indent(out, base_indent + 2);
                        out.push_str("# ");
                        out.push_str(loser);
                        out.push('\n');
                    }
                    first = false;
                } else {
                    emit_field(
                        out,
                        k,
                        v,
                        base_indent + 2,
                        false,
                        &child_path,
                        nested,
                        inner_trailer,
                    );
                }
            }
            emit_own_line_pending(out, path, map.len(), base_indent + 2, nested);
            emit_orphan_inlines(out, path, map.len(), base_indent + 2, nested);
        }
        JsonValue::Array(inner) if inner.is_empty() => {
            push_indent(out, base_indent);
            out.push_str("- []");
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
        JsonValue::Array(inner) => {
            // Nested sequence: `-` line then recurse.
            push_indent(out, base_indent);
            out.push('-');
            push_trailer(out, inline_trailer);
            out.push('\n');
            emit_sequence_children(out, inner, base_indent + 2, path, nested);
        }
        _ => {
            push_indent(out, base_indent);
            out.push_str("- ");
            emit_scalar(out, value);
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
    }
}

/// Emit a `key: <value>\n` pair where the key is already on a `- ` line.
/// The key/value go on the same line as the `- ` prefix (caller already wrote it).
fn emit_field_inline(
    out: &mut String,
    key: &str,
    value: &JsonValue,
    child_indent: usize,
    path: &[CommentPathSegment],
    nested: &[NestedComment],
    inline_trailer: Option<&str>,
) {
    match value {
        JsonValue::Object(map) if map.is_empty() => {
            out.push_str(key);
            out.push_str(": {}");
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
        JsonValue::Object(map) => {
            out.push_str(key);
            out.push(':');
            push_trailer(out, inline_trailer);
            out.push('\n');
            emit_mapping_children(out, map, child_indent, path, nested);
        }
        JsonValue::Array(items) if items.is_empty() => {
            out.push_str(key);
            out.push_str(": []");
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
        JsonValue::Array(items) => {
            out.push_str(key);
            out.push(':');
            push_trailer(out, inline_trailer);
            out.push('\n');
            emit_sequence_children(out, items, child_indent + 2, path, nested);
        }
        _ => {
            out.push_str(key);
            out.push_str(": ");
            emit_scalar(out, value);
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
    }
}

/// Emit a scalar value (no key, no newline) onto `out`.
///
/// Delegates to saphyr: booleans, null, and numbers emit as their
/// type-canonical bare form; strings emit plain when the unquoted form
/// is unambiguous, or double-quoted with `\n`/`\t`/`\uXXXX` escapes when
/// the value would otherwise be misread (YAML 1.1 booleans like
/// `on`/`yes`, numeric-looking strings like `01234`, leading flow
/// indicators, control characters, `: ` runs, …).
fn emit_scalar(out: &mut String, value: &JsonValue) {
    let s = saphyr_emit_scalar(value);
    out.push_str(&s);
}

/// Saphyr options shared by every scalar / flow emission in this module.
///
/// `prefer_block_scalars: false` is the load-bearing setting: it forces
/// multi-line strings to emit as double-quoted inline scalars with `\n`
/// escapes instead of `|` / `>` block forms (no block scalars in v1).
fn saphyr_opts() -> SerializerOptions {
    SerializerOptions {
        prefer_block_scalars: false,
        ..SerializerOptions::default()
    }
}

/// Serialize `value` with saphyr and strip the trailing newline. Caller
/// is responsible for ensuring `value` is a scalar — arrays and objects
/// would emit as multi-line block YAML, which is not what callers want.
pub(crate) fn saphyr_emit_scalar(value: &JsonValue) -> String {
    let mut buf = String::new();
    serde_saphyr::to_fmt_writer_with_options(&mut buf, value, saphyr_opts())
        .expect("saphyr scalar emission is infallible for JsonValue scalars");
    while buf.ends_with('\n') {
        buf.pop();
    }

    // Saphyr 0.0.23's plain-safety check inspects only the leading byte
    // as ASCII whitespace, missing strings whose first or last char is
    // Unicode whitespace (U+2000…) or whose last char is an ASCII space.
    // YAML strips such whitespace from plain scalars on parse, losing
    // the data. Detect and emit double-quoted ourselves so the round-
    // trip is preserved.
    if let JsonValue::String(s) = value {
        let unquoted = !buf.starts_with('"')
            && !buf.starts_with('\'')
            && !buf.starts_with('|')
            && !buf.starts_with('>');
        let has_edge_whitespace = !s.is_empty()
            && (s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace));
        if unquoted && has_edge_whitespace {
            return double_quote_string(s);
        }
    }
    buf
}

/// JSON-style double-quoted YAML scalar — used only as a fallback for
/// strings that saphyr would emit in a form that loses bytes on parse
/// (currently: trailing-whitespace plain scalars).
fn double_quote_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (0x7F..=0x9F).contains(&(c as u32)) => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render any `JsonValue` as a one-line YAML flow form — arrays as
/// `[a, b, c]`, objects as `{k: v, …}`, scalars in flow context (so
/// items containing `,`/`[`/`]`/`{`/`}` get quoted). Used for `# e.g.`
/// hint lines in blueprint output where multi-element shape needs to
/// fit on a single comment.
pub(crate) fn saphyr_emit_flow(value: &JsonValue) -> String {
    let mut buf = String::new();
    let opts = saphyr_opts();
    match value {
        JsonValue::Array(items) => {
            let wrapped = FlowSeq(items.clone());
            serde_saphyr::to_fmt_writer_with_options(&mut buf, &wrapped, opts)
                .expect("saphyr flow seq emission");
        }
        JsonValue::Object(map) => {
            let wrapped = FlowMap(map.clone());
            serde_saphyr::to_fmt_writer_with_options(&mut buf, &wrapped, opts)
                .expect("saphyr flow map emission");
        }
        scalar => {
            // Single scalar in flow context: wrap in a one-element FlowSeq
            // so saphyr applies flow-context quoting (commas, brackets,
            // braces become flow indicators), then strip the `[`/`]`.
            let wrapped = FlowSeq(vec![scalar.clone()]);
            serde_saphyr::to_fmt_writer_with_options(&mut buf, &wrapped, opts)
                .expect("saphyr flow scalar emission");
            while buf.ends_with('\n') {
                buf.pop();
            }
            return buf
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .unwrap_or(&buf)
                .to_string();
        }
    }
    while buf.ends_with('\n') {
        buf.pop();
    }
    buf
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn push_indent(out: &mut String, spaces: usize) {
    for _ in 0..spaces {
        out.push(' ');
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::QuillValue;

    /// Round-trip helper: emit a scalar as a single-field document, then
    /// parse it back with the same machinery the rest of the pipeline
    /// uses. The point of saphyr-backed emission is type-fidelity, not
    /// any specific quoting form, so the test asserts the value survives
    /// parse — not the byte layout.
    fn assert_scalar_round_trips(value: serde_json::Value) {
        let mut yaml = String::from("~~~card-yaml\n#@quill: q\n#@kind: main\nv: ");
        yaml.push_str(&saphyr_emit_scalar(&value));
        yaml.push_str("\n~~~\n");
        let doc = crate::document::Document::from_markdown(&yaml).unwrap_or_else(|e| {
            panic!("failed to parse emitted scalar {:?}: {}\n{}", value, e, yaml)
        });
        let parsed = doc.main().payload().get("v").expect("field 'v'").as_json();
        assert_eq!(
            parsed, &value,
            "scalar round-trip mismatch for {:?}: emitted as {:?}",
            value, yaml
        );
    }

    #[test]
    fn saphyr_scalar_round_trips_plain_string() {
        assert_scalar_round_trips(serde_json::json!("hello"));
    }

    #[test]
    fn saphyr_scalar_round_trips_ambiguous_strings() {
        // Saphyr quotes whatever needs quoting to stay a string on re-parse.
        for ambiguous in &[
            "on", "off", "yes", "no", "true", "false", "null", "~", "01234", "1e10",
        ] {
            assert_scalar_round_trips(serde_json::json!(*ambiguous));
        }
    }

    #[test]
    fn saphyr_scalar_round_trips_escapes() {
        assert_scalar_round_trips(serde_json::json!("a\\b\"c\nd\te"));
    }

    #[test]
    fn saphyr_scalar_round_trips_control_chars() {
        assert_scalar_round_trips(serde_json::json!("\x01\x1F"));
    }

    fn p(key: &str) -> Vec<CommentPathSegment> {
        vec![CommentPathSegment::Key(key.to_string())]
    }

    #[test]
    fn empty_object_omitted() {
        let value = QuillValue::from_json(serde_json::json!({}));
        let mut out = String::new();
        emit_field(
            &mut out,
            "empty_map",
            value.as_json(),
            0,
            false,
            &p("empty_map"),
            &[],
            None,
        );
        assert_eq!(out, ""); // omitted
    }

    #[test]
    fn empty_object_with_inline_trailer_degrades() {
        let value = QuillValue::from_json(serde_json::json!({}));
        let mut out = String::new();
        emit_field(
            &mut out,
            "empty_map",
            value.as_json(),
            0,
            false,
            &p("empty_map"),
            &[],
            Some("orphan"),
        );
        // Host omitted; trailer survives as own-line at the same indent.
        assert_eq!(out, "# orphan\n");
    }

    #[test]
    fn empty_array_emitted() {
        let value = QuillValue::from_json(serde_json::json!([]));
        let mut out = String::new();
        emit_field(
            &mut out,
            "empty_seq",
            value.as_json(),
            0,
            false,
            &p("empty_seq"),
            &[],
            None,
        );
        assert_eq!(out, "empty_seq: []\n");
    }

    #[test]
    fn scalar_field_with_inline_trailer() {
        let value = QuillValue::from_json(serde_json::json!("Hello"));
        let mut out = String::new();
        emit_field(
            &mut out,
            "title",
            value.as_json(),
            0,
            false,
            &p("title"),
            &[],
            Some("greeting"),
        );
        // Saphyr emits "Hello" as a plain scalar — the inline trailer is
        // what the test cares about: it lands on the same line, after the
        // value, separated by ` # `.
        assert_eq!(out, "title: Hello # greeting\n");
    }

    #[test]
    fn container_field_with_inline_trailer_lands_on_key_line() {
        let value = QuillValue::from_json(serde_json::json!({"inner": 1}));
        let mut out = String::new();
        emit_field(
            &mut out,
            "outer",
            value.as_json(),
            0,
            false,
            &p("outer"),
            &[],
            Some("note"),
        );
        // Trailer lands on the key line, not after the children.
        assert_eq!(out, "outer: # note\n  inner: 1\n");
    }

    #[test]
    fn fill_null_emits_bare_tag() {
        let value = QuillValue::from_json(serde_json::Value::Null);
        let mut out = String::new();
        emit_field(
            &mut out,
            "recipient",
            value.as_json(),
            0,
            true,
            &p("recipient"),
            &[],
            None,
        );
        assert_eq!(out, "recipient: !fill\n");
    }

    #[test]
    fn fill_string_emits_tag_with_value() {
        let value = QuillValue::from_json(serde_json::json!("placeholder"));
        let mut out = String::new();
        emit_field(
            &mut out,
            "dept",
            value.as_json(),
            0,
            true,
            &p("dept"),
            &[],
            None,
        );
        assert_eq!(out, "dept: !fill placeholder\n");
    }

    #[test]
    fn fill_with_inline_trailer() {
        let value = QuillValue::from_json(serde_json::json!("placeholder"));
        let mut out = String::new();
        emit_field(
            &mut out,
            "dept",
            value.as_json(),
            0,
            true,
            &p("dept"),
            &[],
            Some("note"),
        );
        assert_eq!(out, "dept: !fill placeholder # note\n");
    }

    #[test]
    fn fill_integer_emits_tag_with_value() {
        let value = QuillValue::from_json(serde_json::json!(42));
        let mut out = String::new();
        emit_field(
            &mut out,
            "count",
            value.as_json(),
            0,
            true,
            &p("count"),
            &[],
            None,
        );
        assert_eq!(out, "count: !fill 42\n");
    }
}
