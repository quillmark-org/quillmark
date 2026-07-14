//! Document seeding from a quill schema.
//!
//! [`Quill::seed_document`](super::Quill::seed_document),
//! [`seed_main`](super::Quill::seed_main), and
//! [`seed_card`](super::Quill::seed_card) build a starter document by committing
//! each schema field's `example` value and leaving **every other field absent**.
//! Absent fields are interpolated at render time — schema `default`, else
//! type-empty zero — by the zero-filled render in
//! [`Quill::compile_data`](super::Quill::compile_data); they are never written
//! into the document.
//!
//! This is the **filled-out twin of the blueprint**
//! ([`QuillConfig::blueprint`](crate::quill::QuillConfig::blueprint)): the
//! blueprint is the annotated authoring surface (`!must_fill` placeholders,
//! `# e.g.` hints), while the seed is its `example`-first intent materialized as
//! real [`Document`] content with no `!must_fill` markers and no default/zero
//! values persisted. Because only `example` values are committed, the seed
//! never collides with the render layer (no editor/preview drift) and
//! preserves the absence-based completeness signal for fields that have no
//! `example` to seed.
//!
//! Provenance — distinguishing a still-untouched seeded `example` from authored
//! content — is intentionally out of scope here; correctness and renderability
//! do not depend on it. A field carrying its seeded `example` reads as ordinary
//! authored content until that distinction is added.
//!
//! Composable cards (`card_kinds`, multiplicity `0..N`) are seeded as **one**
//! instance per declared kind.

use indexmap::IndexMap;
use quillmark_richtext::RichText;

use super::Quill;
use crate::quill::CardSchema;
use crate::{Card, Document, Payload, QuillReference, QuillValue, SeedOverlay};

/// Build the seeded `(payload, body)` for one card schema, layering an optional
/// [`SeedOverlay`] over the schema-example base. Per field the precedence is
/// `overlay › example › absent`; the overlay may also add a field the base
/// omits (a `default`-only field with no `example`). The final fields are
/// ordered by schema declaration order (matching the blueprint). Only fields declared on the
/// schema are included — an overlay key naming no schema field is ignored here
/// (the editor-surface validator flags it). Body: `overlay › body.example ›
/// empty`, honored only when the kind enables bodies. The `$quill` / `$kind`
/// system metadata is attached by the caller.
///
/// **Seed-commits-corpus**: a seeded richtext field commits the *corpus* form
/// (the load-time [`example_corpus`](crate::quill::FieldSchema::example_corpus)
/// cache), and the body is the corpus, so a seeded document is canonical from
/// birth rather than a markdown string re-imported at render. Overlay values
/// (from a document's `$seed`) pass through as authored and canonicalize at the
/// next `compile_data`.
fn seed_parts(schema: &CardSchema, overlay: Option<&SeedOverlay>) -> (Payload, RichText) {
    // Drive by `schema.fields` (declaration order), so the result is in
    // declaration order natively — no merge-then-sort. Per field the precedence
    // is `overlay › example_corpus › example › absent`: a richtext-bearing field
    // seeds from its pre-validated corpus companion, every other from its raw
    // `example`, and the overlay overrides either (and can supply a value for a
    // `default`-only field the base omits). An overlay key naming no schema
    // field is skipped — it is never iterated here.
    let mut fields: IndexMap<String, QuillValue> = IndexMap::new();
    for (name, field) in &schema.fields {
        let value = overlay
            .and_then(|o| o.fields.get(name))
            .or(field.example_corpus.as_ref())
            .or(field.example.as_ref());
        if let Some(value) = value {
            fields.insert(name.clone(), value.clone());
        }
    }

    // Body region as a corpus: an overlay body (authored markdown) is imported;
    // otherwise the `body.example` corpus cache is used; else empty — and only
    // when bodies are enabled for the kind.
    let body = if schema.body_enabled() {
        if let Some(overlay_body) = overlay.and_then(|o| o.body.clone()) {
            crate::document::import_body(&overlay_body).unwrap_or_else(|_| RichText::empty())
        } else if let Some(corpus) = schema.body.as_ref().and_then(|b| b.example_corpus.as_ref()) {
            quillmark_richtext::serial::from_canonical_value(corpus.as_json())
                .unwrap_or_else(|_| RichText::empty())
        } else if let Some(example) = schema.body.as_ref().and_then(|b| b.example.as_ref()) {
            // Fallback for a schema built outside the loader (no cached corpus).
            crate::document::import_body(example).unwrap_or_else(|_| RichText::empty())
        } else {
            RichText::empty()
        }
    } else {
        RichText::empty()
    };

    (Payload::from_index_map(fields), body)
}

/// `$quill` reference for the main card, as `name@version`. Falls back to a
/// versionless reference if the configured version is unparseable (it is
/// validated at quill load, so the fallback is defensive only).
fn main_reference(quill: &Quill) -> QuillReference {
    let config = quill.config();
    format!("{}@{}", config.name, config.version)
        .parse()
        .unwrap_or_else(|_| QuillReference::latest(config.name.clone()))
}

pub(crate) fn seed_main(quill: &Quill) -> Card {
    // The main card is never seeded from an overlay — `$seed` keys range over
    // composable `card_kinds`, and `main` is not one of them.
    let (mut payload, body) = seed_parts(&quill.config().main, None);
    payload.set_quill(main_reference(quill));
    // The root block carries `$kind: main` alongside `$quill` (see the
    // markdown spec); set it so a seeded main card round-trips through
    // `to_markdown()` exactly as the parser and blueprint emit it.
    payload.set_kind("main");
    Card::from_parts(payload, body)
}

pub(crate) fn seed_card_for_kind(
    quill: &Quill,
    card_kind: &str,
    overlay: Option<&SeedOverlay>,
) -> Option<Card> {
    let schema = quill.config().card_kind(card_kind)?;
    Some(seed_composable(schema, overlay))
}

/// Seed a single composable card from its schema and an optional overlay (sets
/// `$kind`, never `$quill`).
fn seed_composable(schema: &CardSchema, overlay: Option<&SeedOverlay>) -> Card {
    let (mut payload, body) = seed_parts(schema, overlay);
    payload.set_kind(schema.name.clone());
    Card::from_parts(payload, body)
}

pub(crate) fn seed_document(quill: &Quill) -> Document {
    // A fresh document carries no `$seed`, so every kind seeds from its schema
    // example base (overlay = `None`).
    let main = seed_main(quill);
    let cards = quill
        .config()
        .card_kinds
        .iter()
        .map(|schema| seed_composable(schema, None))
        .collect();
    Document::from_main_and_cards(main, cards, Vec::new())
}

#[cfg(test)]
mod tests;
