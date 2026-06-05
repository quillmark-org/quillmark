//! Document seeding from a quill schema.
//!
//! [`Quill::seed_document`], [`Quill::seed_main`], and [`Quill::seed_card`]
//! build a starter document by committing each schema field's `example` value
//! and leaving **every other field absent**. Absent fields are interpolated at
//! render time — schema `default`, else type-empty zero — by the zero-filled
//! render in [`Quill::compile_data`]; they are never written into the document.
//!
//! This is the committed, structured counterpart of
//! [`QuillConfig::example`](quillmark_core::quill::QuillConfig::example)'s
//! illustrative Markdown *string*: the same `example`-first intent, materialized
//! as real [`Document`] content with no `<must-fill>` sentinels and no
//! default/zero values persisted. Because only `example` values are committed,
//! the seed never collides with the render layer (no editor/preview drift) and
//! preserves the absence-based completeness signal for fields that have no
//! `example` to seed.
//!
//! Provenance — distinguishing a still-untouched seeded `example` from authored
//! content — is intentionally out of scope here; correctness and renderability
//! do not depend on it. A field carrying its seeded `example` reads as ordinary
//! authored content until that distinction is added.
//!
//! Composable cards (`card_kinds`, multiplicity `0..N`) are seeded as **one**
//! instance per declared kind, mirroring
//! [`QuillConfig::example`](quillmark_core::quill::QuillConfig::example).

use indexmap::IndexMap;

use quillmark_core::quill::CardSchema;
use quillmark_core::{Card, Document, Payload, QuillReference, QuillValue};

use crate::Quill;

/// Build the seeded `(payload, body)` for one card schema: each field that
/// declares an `example` is committed, ordered by `ui.order` (matching the
/// form view and blueprint); fields without an `example` are omitted. The
/// `$quill` / `$kind` system metadata is attached by the caller.
fn seed_parts(schema: &CardSchema) -> (Payload, String) {
    let mut names: Vec<&str> = schema.fields.keys().map(String::as_str).collect();
    names.sort_by_key(|name| schema.fields[*name].ui_order());

    let mut fields: IndexMap<String, QuillValue> = IndexMap::new();
    for name in names {
        if let Some(example) = &schema.fields[name].example {
            fields.insert(name.to_string(), example.clone());
        }
    }

    // Body region carries `body.example` when declared and bodies are enabled,
    // mirroring the blueprint/example document; otherwise it is empty.
    let body = if schema.body_enabled() {
        schema
            .body
            .as_ref()
            .and_then(|b| b.example.clone())
            .unwrap_or_default()
    } else {
        String::new()
    };

    (Payload::from_index_map(fields), body)
}

/// `$quill` reference for the main card, as `name@version`. Falls back to a
/// versionless reference if the configured version is unparseable (it is
/// validated at quill load, so the fallback is defensive only).
fn main_reference(quill: &Quill) -> QuillReference {
    let config = quill.source().config();
    format!("{}@{}", config.name, config.version)
        .parse()
        .unwrap_or_else(|_| QuillReference::latest(config.name.clone()))
}

pub(crate) fn seed_main(quill: &Quill) -> Card {
    let (mut payload, body) = seed_parts(&quill.source().config().main);
    payload.set_quill(main_reference(quill));
    // The root block carries `$kind: main` alongside `$quill` (see the
    // markdown spec); set it so a seeded main card round-trips through
    // `to_markdown()` exactly as the parser and blueprint emit it.
    payload.set_kind("main");
    Card::from_parts(payload, body)
}

pub(crate) fn seed_card_for_kind(quill: &Quill, card_kind: &str) -> Option<Card> {
    let schema = quill.source().config().card_kind(card_kind)?;
    Some(seed_composable(schema))
}

/// Seed a single composable card from its schema (sets `$kind`, never `$quill`).
fn seed_composable(schema: &CardSchema) -> Card {
    let (mut payload, body) = seed_parts(schema);
    payload.set_kind(schema.name.clone());
    Card::from_parts(payload, body)
}

pub(crate) fn seed_document(quill: &Quill) -> Document {
    let main = seed_main(quill);
    let cards = quill
        .source()
        .config()
        .card_kinds
        .iter()
        .map(seed_composable)
        .collect();
    Document::from_main_and_cards(main, cards, Vec::new())
}

#[cfg(test)]
mod tests;
