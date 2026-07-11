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
//! Delegation also covers the YAML 1.1 edge cases that ad-hoc quoting
//! heuristics miss (`on`/`yes`/`off`, leading-zero integers, `1.0`-style
//! numerics) — saphyr handles them all.
//!
//! `prefer_block_scalars: false` keeps multi-line strings inline as
//! double-quoted scalars with `\n` escapes, so the emitter never produces
//! `|` or `>` block forms in v1.
//!
//! This module owns the surrounding structure — `~~~` card-yaml fences,
//! `$`-prefixed system-metadata lines, field ordering, indentation, comment
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
    /// # Emission rules (§9)
    ///
    /// - Line endings: `\n` only.  CRLF normalization happens on import.
    /// - Every block is emitted as a `~~~` card-yaml fence: a bare `~~~`
    ///   opener, the `$`-prefixed system-metadata lines (`$quill: <ref>` for
    ///   the root block, `$kind: <kind>` for composable cards) leading the
    ///   YAML payload, the user-defined data fields, then a closing `~~~`.
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
    /// # Design notes
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
    /// - **`!must_fill` tags**: round-trip via the `fill` flag on `PayloadItem::Field`.
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
        // Bodies are corpora; the markdown surface is their export projection,
        // so a `Document` → markdown → `Document` round-trip canonicalizes the body markdown (leading blank
        // lines dropped, a single trailing `\n`). A blank line separates the
        // closing fence from a non-empty body, the conventional card-yaml shape.
        emit_block(&mut out, self.main());
        append_body(&mut out, &self.main().body_markdown());

        // ── Composable cards ──────────────────────────────────────────────────
        // `ensure_blank_before_fence` normalises the separator before each
        // block, so edited bodies (which may lack a trailing blank line) still
        // round-trip.
        for card in self.cards() {
            ensure_blank_before_fence(&mut out);
            emit_block(&mut out, card);
            append_body(&mut out, &card.body_markdown());
        }

        out
    }
}

/// Append a card's markdown body after its closing fence, separated by one
/// blank line (the conventional card-yaml shape). Empty bodies append nothing —
/// the fence closes and the next block (or EOF) follows.
fn append_body(out: &mut String, body: &str) {
    if !body.is_empty() {
        out.push('\n');
        out.push_str(body);
    }
}

// ── Block emission ────────────────────────────────────────────────────────────

fn emit_meta_line(out: &mut String, key: &str, value: &str, trailer: Option<&str>) {
    out.push('$');
    out.push_str(key);
    out.push_str(": ");
    out.push_str(&saphyr_emit_scalar(&JsonValue::String(value.to_string())));
    push_trailer(out, trailer);
    out.push('\n');
}

/// Emit an out-of-band meta block (`$ext` / `$seed`). An empty map emits inline
/// as `<key>: {}` so the declaration survives the round-trip; a non-empty map
/// emits as a `<key>:` header followed by indented block-style children.
/// `nested` carries comments with paths relative to the value tree (the meta
/// key itself is not in the path) — the child mapping walker re-injects them at
/// the matching positions. Meta maps are out-of-band data and never carry
/// `!must_fill`.
fn emit_meta_block(
    out: &mut String,
    key: &str,
    value: &serde_json::Map<String, JsonValue>,
    trailer: Option<&str>,
    nested: &[NestedComment],
) {
    if value.is_empty() {
        out.push_str(key);
        out.push_str(": {}");
        push_trailer(out, trailer);
        out.push('\n');
        return;
    }
    out.push_str(key);
    out.push(':');
    push_trailer(out, trailer);
    out.push('\n');
    let path: Vec<CommentPathSegment> = Vec::new();
    emit_mapping_children(out, value, 2, &path, nested, &[]);
}

/// `true` when `path` (relative to a field value) carries a `!must_fill`
/// marker. Fill sets are small (one entry per placeholder), so a linear
/// scan is cheaper than building a hash set per field.
fn path_is_fill(fills: &[Vec<CommentPathSegment>], path: &[CommentPathSegment]) -> bool {
    fills.iter().any(|p| p.as_slice() == path)
}

fn emit_block(out: &mut String, card: &Card) {
    out.push_str("~~~\n");
    emit_payload_items(out, card.payload().items());
    out.push_str("~~~\n");
}

/// Walk the unified item list and emit each entry. An `inline: true` comment
/// immediately following a non-comment item is consumed as that item's trailer.
///
/// Each `Field` / `Ext` item carries its own `nested_comments` slice with
/// paths relative to the field's value tree, so emission of nested
/// structures starts with an empty container path.
fn emit_payload_items(out: &mut String, items: &[PayloadItem]) {
    let mut i = 0;
    while i < items.len() {
        // Peek for a trailing inline comment to use as the line trailer.
        let trailer = items.get(i + 1).and_then(|next| match next {
            PayloadItem::Comment { text, inline: true } => Some(text.as_str()),
            _ => None,
        });
        let mut consumed_trailer = trailer.is_some();

        match &items[i] {
            PayloadItem::Quill { reference } => {
                emit_meta_line(out, "quill", &reference.to_string(), trailer);
            }
            PayloadItem::Kind { value } => {
                emit_meta_line(out, "kind", value, trailer);
            }
            PayloadItem::Id { value } => {
                emit_meta_line(out, "id", value, trailer);
            }
            PayloadItem::Meta {
                key,
                value,
                nested_comments,
            } => {
                emit_meta_block(out, key.as_str(), value, trailer, nested_comments);
            }
            PayloadItem::Field {
                key,
                value,
                fill,
                nested_comments,
            } => {
                // A richtext field stores its value as a canonical corpus
                // object (via `set_field_richtext`); card-yaml is the
                // human-authored surface, so it projects back to a markdown
                // string here — the field-level twin of the `$body` projection,
                // lossy per the corpus's island loss class (the DTO stays the
                // lossless carrier). A corpus field is never `!must_fill` and
                // its content carries no user nested-comments/fills, so the
                // projected scalar routes through the plain string path.
                if !*fill {
                    if let Some(markdown) = project_corpus_field(value.as_json()) {
                        emit_field(
                            out,
                            key,
                            &JsonValue::String(markdown),
                            0,
                            false,
                            &[],
                            &[],
                            &[],
                            trailer,
                        );
                        i += if consumed_trailer { 2 } else { 1 };
                        continue;
                    }
                }
                // Paths in `nested_comments` are relative to this field's
                // value, so the container path starts empty.
                let path: Vec<CommentPathSegment> = Vec::new();
                // `!must_fill` markers on nested nodes, as paths relative to
                // this field's value; the top-level marker rides on `*fill`.
                let fills = value.fill_paths();
                emit_field(
                    out,
                    key,
                    value.as_json(),
                    0,
                    *fill,
                    &path,
                    nested_comments,
                    &fills,
                    trailer,
                );
            }
            PayloadItem::Comment { text, .. } => {
                out.push_str("# ");
                out.push_str(text);
                out.push('\n');
                consumed_trailer = false;
            }
        }
        i += if consumed_trailer { 2 } else { 1 };
    }
}

/// The markdown projection of a richtext-valued field, or `None` when `value` is
/// not a canonical corpus object.
///
/// A richtext field written via [`Card::set_field_richtext`](super::Card::set_field_richtext)
/// stores the canonical corpus object; emit projects it to a markdown string so
/// card-yaml — the human-authored surface — stays markdown-clean rather than
/// carrying a nested `{text, lines, marks, islands}` tree. Projection is lossy
/// per the corpus's island loss class (the same tradeoff `$body` makes): island
/// ids and corpus-only marks do not survive a `.qmd` round-trip, so on-disk
/// identity is markdown-lossy by design; the storage DTO is the lossless carrier.
///
/// The guard requires the object to round-trip *byte-for-byte* as a canonical
/// corpus, so a user object field that merely resembles one (extra keys,
/// non-canonical shape) stays structural. A corpus object only ever arises from
/// the programmatic corpus writer — `from_markdown` is schema-less and stores a
/// richtext field authored as markdown as a plain string — so this never fires
/// on a parse-originated document, leaving the emit round-trip property intact.
fn project_corpus_field(value: &JsonValue) -> Option<String> {
    if !value.is_object() {
        return None;
    }
    let rt = quillmark_richtext::serial::from_canonical_value(value).ok()?;
    if quillmark_richtext::serial::to_canonical_value(&rt) != *value {
        return None;
    }
    Some(quillmark_richtext::export::to_markdown(&rt))
}

/// Ensure `out` ends with `\n\n` so the next fence has a blank line above it.
/// Appends a line terminator first if `out` doesn't already end with `\n`.
/// No-op on empty `out` (block at line 1 needs no separator).
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

/// Emit own-line nested comments at `position` in `path` (inline comments are
/// handled by `find_inline_trailer`).
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

/// Return the inline trailer for `position` in `path`. If multiple inline
/// comments share the slot, returns the first and emits the rest as own-line.
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

/// Emit orphan inline comments (`position >= container_len`) as own-line.
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

fn push_trailer(out: &mut String, trailer: Option<&str>) {
    if let Some(t) = trailer {
        out.push_str(" # ");
        out.push_str(t);
    }
}

/// Emit a `key: <value>\n` pair at `indent` spaces.
///
/// `path` is the container path for nested-comment interleaving. Empty objects
/// are omitted; their inline trailer degrades to an own-line comment to
/// preserve the text. Empty arrays emit `key: []\n`. When `fill` is `true`:
/// scalars → `key: !must_fill <value>`, empty seqs → `key: !must_fill []`, null →
/// `key: !must_fill`, non-empty seqs → `key: !must_fill\n  - …`. Mappings with `fill`
/// are rejected at parse and never reach this path.
#[allow(clippy::too_many_arguments)]
fn emit_field(
    out: &mut String,
    key: &str,
    value: &JsonValue,
    indent: usize,
    fill: bool,
    path: &[CommentPathSegment],
    nested: &[NestedComment],
    fills: &[Vec<CommentPathSegment>],
    inline_trailer: Option<&str>,
) {
    if fill {
        push_indent(out, indent);
        emit_key_at(out, key, indent);
        match value {
            JsonValue::Null => {
                out.push_str(": !must_fill");
                push_trailer(out, inline_trailer);
                out.push('\n');
            }
            JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => {
                out.push_str(": !must_fill ");
                emit_scalar(out, value);
                push_trailer(out, inline_trailer);
                out.push('\n');
            }
            JsonValue::Array(items) if items.is_empty() => {
                out.push_str(": !must_fill []");
                push_trailer(out, inline_trailer);
                out.push('\n');
            }
            JsonValue::Array(items) => {
                out.push_str(": !must_fill");
                push_trailer(out, inline_trailer);
                out.push('\n');
                emit_sequence_children(out, items, indent + 2, path, nested, fills);
            }
            JsonValue::Object(_) => {
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
            if let Some(t) = inline_trailer {
                push_indent(out, indent);
                out.push_str("# ");
                out.push_str(t);
                out.push('\n');
            }
        }
        JsonValue::Object(map) => {
            push_indent(out, indent);
            emit_key_at(out, key, indent);
            out.push(':');
            push_trailer(out, inline_trailer);
            out.push('\n');
            emit_mapping_children(out, map, indent + 2, path, nested, fills);
        }
        JsonValue::Array(items) if items.is_empty() => {
            push_indent(out, indent);
            emit_key_at(out, key, indent);
            out.push_str(": []");
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
        JsonValue::Array(items) => {
            push_indent(out, indent);
            emit_key_at(out, key, indent);
            out.push(':');
            push_trailer(out, inline_trailer);
            out.push('\n');
            emit_sequence_children(out, items, indent + 2, path, nested, fills);
        }
        _ => {
            push_indent(out, indent);
            emit_key_at(out, key, indent);
            out.push_str(": ");
            emit_scalar(out, value);
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
    }
}

fn emit_mapping_children(
    out: &mut String,
    map: &serde_json::Map<String, JsonValue>,
    child_indent: usize,
    path: &[CommentPathSegment],
    nested: &[NestedComment],
    fills: &[Vec<CommentPathSegment>],
) {
    for (i, (k, v)) in map.iter().enumerate() {
        emit_own_line_pending(out, path, i, child_indent, nested);
        let trailer = find_inline_trailer(out, path, i, child_indent, nested);
        let mut child_path = path.to_vec();
        child_path.push(CommentPathSegment::Key(k.clone()));
        let child_fill = path_is_fill(fills, &child_path);
        emit_field(
            out,
            k,
            v,
            child_indent,
            child_fill,
            &child_path,
            nested,
            fills,
            trailer,
        );
    }
    emit_own_line_pending(out, path, map.len(), child_indent, nested);
    emit_orphan_inlines(out, path, map.len(), child_indent, nested);
}

fn emit_sequence_children(
    out: &mut String,
    items: &[JsonValue],
    base_indent: usize,
    path: &[CommentPathSegment],
    nested: &[NestedComment],
    fills: &[Vec<CommentPathSegment>],
) {
    for (i, item) in items.iter().enumerate() {
        emit_own_line_pending(out, path, i, base_indent, nested);
        let trailer = find_inline_trailer(out, path, i, base_indent, nested);
        let mut child_path = path.to_vec();
        child_path.push(CommentPathSegment::Index(i));
        emit_sequence_item(out, item, base_indent, &child_path, nested, fills, trailer);
    }
    emit_own_line_pending(out, path, items.len(), base_indent, nested);
    emit_orphan_inlines(out, path, items.len(), base_indent, nested);
}

/// Emit a single `- <value>\n` sequence item. When the item is a mapping,
/// if both the seq-item trailer and the first key's trailer are present,
/// the inner one degrades to an own-line comment.
#[allow(clippy::too_many_arguments)]
fn emit_sequence_item(
    out: &mut String,
    value: &JsonValue,
    base_indent: usize,
    path: &[CommentPathSegment],
    nested: &[NestedComment],
    fills: &[Vec<CommentPathSegment>],
    inline_trailer: Option<&str>,
) {
    match value {
        JsonValue::Object(map) if map.is_empty() => {
            push_indent(out, base_indent);
            out.push_str("- {}");
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
        JsonValue::Object(map) => {
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
                    let line_trailer = inline_trailer.or(inner_trailer);
                    push_indent(out, base_indent);
                    out.push_str("- ");
                    emit_field_inline(
                        out,
                        k,
                        v,
                        base_indent + 2,
                        path_is_fill(fills, &child_path),
                        &child_path,
                        nested,
                        fills,
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
                        path_is_fill(fills, &child_path),
                        &child_path,
                        nested,
                        fills,
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
            push_indent(out, base_indent);
            out.push('-');
            push_trailer(out, inline_trailer);
            out.push('\n');
            emit_sequence_children(out, inner, base_indent + 2, path, nested, fills);
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

/// Emit `key: <value>\n` where the caller already wrote `- ` on the current line.
#[allow(clippy::too_many_arguments)]
fn emit_field_inline(
    out: &mut String,
    key: &str,
    value: &JsonValue,
    child_indent: usize,
    fill: bool,
    path: &[CommentPathSegment],
    nested: &[NestedComment],
    fills: &[Vec<CommentPathSegment>],
    inline_trailer: Option<&str>,
) {
    if fill {
        emit_key(out, key);
        match value {
            JsonValue::Null => out.push_str(": !must_fill"),
            JsonValue::Array(items) if items.is_empty() => out.push_str(": !must_fill []"),
            JsonValue::Array(items) => {
                out.push_str(": !must_fill");
                push_trailer(out, inline_trailer);
                out.push('\n');
                emit_sequence_children(out, items, child_indent + 2, path, nested, fills);
                return;
            }
            JsonValue::Object(_) => {
                // `!must_fill` on a mapping is rejected at parse; emit plainly.
                out.push(':');
                push_trailer(out, inline_trailer);
                out.push('\n');
                if let JsonValue::Object(map) = value {
                    emit_mapping_children(out, map, child_indent, path, nested, fills);
                }
                return;
            }
            _ => {
                out.push_str(": !must_fill ");
                emit_scalar(out, value);
            }
        }
        push_trailer(out, inline_trailer);
        out.push('\n');
        return;
    }
    match value {
        JsonValue::Object(map) if map.is_empty() => {
            emit_key(out, key);
            out.push_str(": {}");
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
        JsonValue::Object(map) => {
            emit_key(out, key);
            out.push(':');
            push_trailer(out, inline_trailer);
            out.push('\n');
            emit_mapping_children(out, map, child_indent, path, nested, fills);
        }
        JsonValue::Array(items) if items.is_empty() => {
            emit_key(out, key);
            out.push_str(": []");
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
        JsonValue::Array(items) => {
            emit_key(out, key);
            out.push(':');
            push_trailer(out, inline_trailer);
            out.push('\n');
            emit_sequence_children(out, items, child_indent + 2, path, nested, fills);
        }
        _ => {
            emit_key(out, key);
            out.push_str(": ");
            emit_scalar(out, value);
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
    }
}

fn emit_scalar(out: &mut String, value: &JsonValue) {
    let s = saphyr_emit_scalar(value);
    out.push_str(&s);
}

/// Emit a *nested* mapping key, quoting it through the same scalar path as
/// values. Nested keys are arbitrary user data (never name-validated) and are
/// re-parsed by serde_saphyr, so a key containing `:`/`#`, a leading YAML
/// indicator (`*`, `&`, `?`, `-`, …), edge whitespace, or a type-ambiguous form
/// (`n`, `true`, `123`) must be quoted or the emitted document re-parses to a
/// different key — breaking the round-trip/idempotence contract.
fn emit_key(out: &mut String, key: &str) {
    out.push_str(&saphyr_emit_scalar(&JsonValue::String(key.to_string())));
}

/// Emit a mapping key at `indent`. Top-level field names (indent 0) are emitted
/// verbatim: the line-oriented prescan accepts only bare `[A-Za-z_][A-Za-z0-9_]*`
/// field names there, so quoting one would make it unparseable. Nested keys
/// (indent > 0) route through [`emit_key`] for correct YAML quoting.
fn emit_key_at(out: &mut String, key: &str, indent: usize) {
    if indent == 0 {
        out.push_str(key);
    } else {
        emit_key(out, key);
    }
}

/// `prefer_block_scalars: false` forces multi-line strings to double-quoted
/// inline scalars (no `|` / `>` block forms in v1).
fn saphyr_opts() -> SerializerOptions {
    SerializerOptions {
        prefer_block_scalars: false,
        ..SerializerOptions::default()
    }
}

pub(crate) fn saphyr_emit_scalar(value: &JsonValue) -> String {
    let mut buf = String::new();
    serde_saphyr::to_fmt_writer_with_options(&mut buf, value, saphyr_opts())
        .expect("saphyr scalar emission is infallible for JsonValue scalars");
    while buf.ends_with('\n') {
        buf.pop();
    }

    // Saphyr 0.0.23's emitter and parser disagree about which plain scalars
    // are string-safe: it emits some `String`s unquoted that its own parser
    // reads back as a non-string (`_0` → integer 0) or as a different string.
    // Edge-whitespace strings are one class — the plain-safety check inspects
    // only the leading ASCII byte, missing a leading/trailing Unicode-
    // whitespace char (U+2000…) or a trailing ASCII space, and YAML strips
    // such whitespace from plain scalars on parse. `_0`-style numeric-looking
    // strings are another. Both lose the original string on round-trip. When
    // saphyr emits a `String` unquoted, re-parse the emitted plain scalar with
    // the same library the parser uses and, unless it round-trips to the exact
    // same string, emit double-quoted ourselves. Edge whitespace stays an
    // explicit guard: a trailing Unicode-whitespace char survives the isolated
    // re-parse yet is still stripped in the real block context.
    if let JsonValue::String(s) = value {
        let unquoted = !buf.starts_with('"')
            && !buf.starts_with('\'')
            && !buf.starts_with('|')
            && !buf.starts_with('>');
        if unquoted {
            let has_edge_whitespace = !s.is_empty()
                && (s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace));
            // A parse error counts as "must quote", conservatively.
            let reparses_same = matches!(
                serde_saphyr::from_str::<JsonValue>(&buf),
                Ok(JsonValue::String(ref s2)) if s2 == s
            );
            if has_edge_whitespace || !reparses_same {
                return double_quote_string(s);
            }
        }
    }
    buf
}

/// JSON-style double-quoted fallback for strings saphyr would emit in a form
/// that loses bytes on parse (e.g. trailing-whitespace plain scalars).
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

/// Render a `JsonValue` as a one-line YAML flow form (`[a, b]` / `{k: v}` /
/// flow-quoted scalar). Used for `# e.g.` hint lines in blueprint output.
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
            // Wrap in FlowSeq so saphyr applies flow-context quoting, then strip `[`/`]`.
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

    fn assert_scalar_round_trips(value: serde_json::Value) {
        let mut yaml = String::from("~~~card-yaml\n$quill: q\n$kind: main\nv: ");
        yaml.push_str(&saphyr_emit_scalar(&value));
        yaml.push_str("\n~~~\n");
        let doc = crate::document::Document::from_markdown(&yaml).unwrap_or_else(|e| {
            panic!(
                "failed to parse emitted scalar {:?}: {}\n{}",
                value, e, yaml
            )
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
        for ambiguous in &[
            "on", "off", "yes", "no", "true", "false", "null", "~", "01234", "1e10",
        ] {
            assert_scalar_round_trips(serde_json::json!(*ambiguous));
        }
    }

    #[test]
    fn saphyr_scalar_round_trips_numeric_looking_strings() {
        // Saphyr's emitter treats a leading-underscore-then-digits scalar as
        // plain-safe, but its parser reads the plain form back as an integer
        // (underscores are digit separators, leading ones ignored): `_0` → 0,
        // `-_0` → 0, `__0` → 0. `_0` is the minimal fuzz-shrunk case. Each is
        // emitted unquoted and re-parsed as `Number` before the fix; the
        // general round-trip check must quote every one.
        for numericish in &["_0", "_1", "-_0", "__0"] {
            assert_scalar_round_trips(serde_json::json!(*numericish));
        }
    }

    #[test]
    fn string_underscore_zero_round_trips_via_document() {
        // Full `to_markdown` → `from_markdown` path for the reported bug:
        // `String("_0")` must return as a `String`, still equal to `"_0"`.
        let src = "~~~card-yaml\n$quill: q\n$kind: main\na: \"_0\"\n~~~\n\nBody.\n";
        let doc = crate::document::Document::from_markdown(src).expect("parse src");
        let emitted = doc.to_markdown();
        let reparsed =
            crate::document::Document::from_markdown(&emitted).expect("re-parse emitted markdown");
        let value = reparsed
            .main()
            .payload()
            .get("a")
            .expect("field 'a'")
            .as_json();
        assert_eq!(
            value,
            &serde_json::Value::String("_0".to_string()),
            "String(\"_0\") must round-trip as a string; emitted:\n{}",
            emitted
        );
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
            &[],
            None,
        );
        assert_eq!(out, "");
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
            &[],
            Some("orphan"),
        );
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
            &[],
            Some("greeting"),
        );
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
            &[],
            Some("note"),
        );
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
            &[],
            None,
        );
        assert_eq!(out, "recipient: !must_fill\n");
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
            &[],
            None,
        );
        assert_eq!(out, "dept: !must_fill placeholder\n");
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
            &[],
            Some("note"),
        );
        assert_eq!(out, "dept: !must_fill placeholder # note\n");
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
            &[],
            None,
        );
        assert_eq!(out, "count: !must_fill 42\n");
    }
}
