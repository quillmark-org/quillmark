//! Schema-aware form views for form editors.
//!
//! This module provides [`Form`] — a read-only snapshot of a [`Document`]
//! viewed through its [`Quill`] schema — and [`FormCard`], the per-card view.
//! For each schema-declared field the view records the current value, the
//! schema default, and the source of the effective value.
//!
//! # Entry points
//!
//! Consumers reach this module through methods on [`Quill`]:
//!
//! ```rust,no_run
//! # use quillmark::{Quill, Document};
//! # use quillmark::form::FormFieldSource;
//! # fn example(quill: &Quill, doc: &Document) {
//! let form = quill.form(doc);
//!
//! for (name, fv) in &form.main.values {
//!     match fv.source {
//!         FormFieldSource::Document => println!("{name}: {:?}", fv.value),
//!         FormFieldSource::Default  => println!("{name}: (default) {:?}", fv.default),
//!         FormFieldSource::Missing  => println!("{name}: MISSING"),
//!     }
//! }
//!
//! // A blank form for a fresh card the user is about to add:
//! if let Some(blank) = quill.blank_card("indorsement") {
//!     // render `blank.values`...
//!     # let _ = blank;
//! }
//! # }
//! ```
//!
//! # Snapshot semantics
//!
//! A [`Form`] (or [`FormCard`]) is a read-only snapshot built at the moment
//! the call returned. Subsequent edits to the document are not reflected;
//! call `quill.form(doc)` again to obtain an updated snapshot.
//!
//! # Unknown card tags
//!
//! Cards whose tag is not declared in the schema are dropped from
//! [`Form::cards`]. Each such card produces one [`Diagnostic`] in
//! [`Form::diagnostics`] with code `"form::unknown_card_tag"`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use quillmark_core::quill::CardSchema;
use quillmark_core::{Diagnostic, Document, QuillValue, Severity};

use crate::Quill;

/// Source of a field's effective value in a form view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormFieldSource {
    /// Value was present in the document's frontmatter or card fields.
    Document,
    /// Value was absent from the document; the schema provides a default.
    Default,
    /// Value was absent from the document and the schema has no default.
    Missing,
}

/// A single field's view within a [`FormCard`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormFieldValue {
    /// Current value from the document, if present.
    pub value: Option<QuillValue>,
    /// Schema default value, if declared.
    pub default: Option<QuillValue>,
    /// Where the effective value comes from.
    pub source: FormFieldSource,
}

/// A card viewed through its schema — either the main document card or a
/// named card block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormCard {
    /// The schema that governs this card.
    pub schema: CardSchema,
    /// View of each schema-declared field.
    ///
    /// Keys follow `IndexMap` insertion order: schema field definition order,
    /// stably re-sorted by `ui.order` when present.
    pub values: IndexMap<String, FormFieldValue>,
}

impl FormCard {
    /// A blank form card for `schema` — no document values supplied. Every
    /// declared field's source is [`FormFieldSource::Default`] (when the
    /// schema declares a default) or [`FormFieldSource::Missing`].
    ///
    /// This is the "user is about to add a new card" view. Reach it through
    /// [`Quill::blank_card`] or [`Quill::blank_main`] when you have a tag in
    /// hand instead of a [`CardSchema`].
    pub fn blank(schema: &CardSchema) -> Self {
        project_card(schema, &IndexMap::new())
    }
}

/// Read-only snapshot of a [`Document`] viewed through a [`Quill`]'s schema.
///
/// Produced by [`Quill::form`]. Subsequent edits to the document are **not**
/// reflected here — call `quill.form(doc)` again after editing.
///
/// # Unknown cards
///
/// Document cards whose tag is not declared in the schema are dropped and
/// each produces a [`Diagnostic`] with code `"form::unknown_card_tag"` in
/// [`Form::diagnostics`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Form {
    /// View of the main document card (frontmatter fields).
    pub main: FormCard,
    /// View of each recognised card, in document order.
    ///
    /// Cards with unknown tags are excluded; see [`Form::diagnostics`].
    pub cards: Vec<FormCard>,
    /// Diagnostics from unknown card tags and validation.
    pub diagnostics: Vec<Diagnostic>,
}

// ── Internal projection ─────────────────────────────────────────────────────
//
// `project_card`, `build_form`, and `blank_card_for_tag` are the internal
// machinery used by `Quill::form`, `Quill::blank_main`, and
// `Quill::blank_card`. They are **deliberately not public**: consumers
// reach the form module through methods on `Quill`, never by holding a
// `CardSchema` and a field map directly.

/// Build the [`Form`] for a document. Composes:
/// - `QuillConfig::main` — the main card schema.
/// - `QuillConfig::card_type` — to look up card schemas by tag.
/// - `QuillConfig::validate_document` — to gather validation diagnostics.
///
/// Coercion (`coerce_frontmatter` / `coerce_card`) is **not** applied here:
/// the form view is the document as-is so the editor sees what the user typed.
/// Validation diagnostics already inform the consumer when values are
/// type-mismatched.
pub(crate) fn build_form(quill: &Quill, doc: &Document) -> Form {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    let main_schema = &quill.source().config().main;
    let main_fields = doc.main().frontmatter().to_index_map();
    let main = project_card(main_schema, &main_fields);

    let mut cards: Vec<FormCard> = Vec::new();
    for (index, card) in doc.cards().iter().enumerate() {
        let tag = card.tag();
        match quill.source().config().card_type(&tag) {
            Some(card_schema) => {
                let card_fields = card.frontmatter().to_index_map();
                cards.push(project_card(card_schema, &card_fields));
            }
            None => {
                diagnostics.push(
                    Diagnostic::new(
                        Severity::Warning,
                        format!(
                            "card at index {index} has unknown tag \"{tag}\"; \
                             it is not declared in the quill schema and has been \
                             excluded from the form view"
                        ),
                    )
                    .with_code("form::unknown_card_tag".to_string()),
                );
            }
        }
    }

    if let Err(validation_errors) = quill.source().config().validate_document(doc) {
        for err in validation_errors {
            diagnostics.push(
                Diagnostic::new(Severity::Error, err.to_string())
                    .with_code("form::validation_error".to_string()),
            );
        }
    }

    Form {
        main,
        cards,
        diagnostics,
    }
}

/// Build a blank [`FormCard`] for a card type by tag, or `None` if the tag
/// isn't declared in the quill's schema.
pub(crate) fn blank_card_for_tag(quill: &Quill, card_type: &str) -> Option<FormCard> {
    quill
        .source()
        .config()
        .card_type(card_type)
        .map(FormCard::blank)
}

/// Build a [`FormCard`] by walking each schema-declared field and looking up
/// its value in `fields`.
fn project_card(schema: &CardSchema, fields: &IndexMap<String, QuillValue>) -> FormCard {
    let mut values: IndexMap<String, FormFieldValue> = IndexMap::new();

    let mut field_names: Vec<&str> = schema.fields.keys().map(String::as_str).collect();
    field_names.sort_by_key(|name| {
        schema
            .fields
            .get(*name)
            .and_then(|fs| fs.ui.as_ref())
            .and_then(|ui| ui.order)
            .unwrap_or(i32::MAX)
    });

    for field_name in field_names {
        let field_schema = &schema.fields[field_name];
        let default = field_schema.default.clone();

        let ffv = match fields.get(field_name) {
            Some(v) => FormFieldValue {
                value: Some(v.clone()),
                default,
                source: FormFieldSource::Document,
            },
            None => match default {
                Some(ref d) => FormFieldValue {
                    value: None,
                    default: Some(d.clone()),
                    source: FormFieldSource::Default,
                },
                None => FormFieldValue {
                    value: None,
                    default: None,
                    source: FormFieldSource::Missing,
                },
            },
        };

        values.insert(field_name.to_string(), ffv);
    }

    FormCard {
        schema: schema.clone(),
        values,
    }
}

#[cfg(test)]
mod tests;
