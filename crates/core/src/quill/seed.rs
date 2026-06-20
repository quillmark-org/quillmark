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
//! blueprint is the annotated authoring surface (sentinels, `# e.g.` hints),
//! while the seed is its `example`-first intent materialized as real
//! [`Document`] content with no `<must-fill>` sentinels and no default/zero
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

use super::Quill;
use crate::quill::CardSchema;
use crate::{Card, Document, Payload, QuillReference, QuillValue, SeedOverlay};

/// Build the seeded `(payload, body)` for one card schema, layering an optional
/// [`SeedOverlay`] over the schema-example base. Per field the precedence is
/// `overlay › example › absent`; the overlay may also add a field the base
/// omits (a `default`-only field with no `example`). The final fields are
/// ordered by `ui.order` (matching the blueprint). Only fields declared on the
/// schema are included — an overlay key naming no schema field is ignored here
/// (the editor-surface validator flags it). Body: `overlay › body.example ›
/// empty`, honored only when the kind enables bodies. The `$quill` / `$kind`
/// system metadata is attached by the caller.
fn seed_parts(schema: &CardSchema, overlay: Option<&SeedOverlay>) -> (Payload, String) {
    // Merge the example base with the overlay (schema-declared fields only).
    let mut merged: IndexMap<String, QuillValue> = IndexMap::new();
    for (name, field) in &schema.fields {
        if let Some(example) = &field.example {
            merged.insert(name.clone(), example.clone());
        }
    }
    if let Some(overlay) = overlay {
        for (name, value) in &overlay.fields {
            if schema.fields.contains_key(name) {
                merged.insert(name.clone(), value.clone());
            }
        }
    }

    // Order by `ui.order`. Every key is a declared field here, so the lookup
    // always resolves; the `i32::MAX` fallback is defensive.
    let mut entries: Vec<(String, QuillValue)> = merged.into_iter().collect();
    entries.sort_by_key(|(name, _)| {
        schema
            .fields
            .get(name)
            .map(|f| f.ui_order())
            .unwrap_or(i32::MAX)
    });
    let fields: IndexMap<String, QuillValue> = entries.into_iter().collect();

    // Body region: an overlay body wins, else `body.example`, else empty —
    // and only when bodies are enabled for the kind.
    let body = if schema.body_enabled() {
        overlay
            .and_then(|o| o.body.clone())
            .or_else(|| schema.body.as_ref().and_then(|b| b.example.clone()))
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
