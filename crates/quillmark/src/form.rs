//! Schema-aware form views for form editors.
//!
//! Provides [`Form`] — a read-only snapshot of a [`Document`] viewed through
//! its [`Quill`] schema — and [`FormCard`], the per-card view. Each
//! schema-declared field records the current value, schema default, and source.
//!
//! Consumers reach this module through [`Quill::form`], [`Quill::blank_main`],
//! and [`Quill::blank_card`]. A [`Form`] is a snapshot — re-call after edits.
//!
//! Cards whose kind is not declared in the schema are dropped from
//! [`Form::cards`]; each produces a [`Diagnostic`] with code
//! `"form::unknown_card_kind"` in [`Form::diagnostics`].

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use quillmark_core::quill::CardSchema;
use quillmark_core::{Diagnostic, Document, QuillValue, Severity};

use crate::Quill;

/// Source of a field's effective value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormFieldSource {
    /// Value was present in the document's payload or card fields.
    Document,
    /// Value was absent from the document; the schema provides a default.
    Default,
    /// Value was absent from the document and the schema has no default.
    Missing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormFieldValue {
    pub value: Option<QuillValue>,
    pub default: Option<QuillValue>,
    pub source: FormFieldSource,
}

/// A card viewed through its schema — either the main document card or a
/// named card block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormCard {
    pub schema: CardSchema,
    /// Keys follow schema field definition order, re-sorted by `ui.order` when present.
    pub values: IndexMap<String, FormFieldValue>,
}

impl FormCard {
    /// Blank form for `schema` with no document values — fields are
    /// [`FormFieldSource::Default`] or [`FormFieldSource::Missing`].
    /// Prefer [`Quill::blank_card`] / [`Quill::blank_main`] over this.
    pub fn blank(schema: &CardSchema) -> Self {
        project_card(schema, &IndexMap::new())
    }
}

/// Read-only snapshot of a [`Document`] viewed through a [`Quill`]'s schema.
/// Produced by [`Quill::form`]; re-call after editing the document.
/// Unknown card kinds are dropped; each surfaces in [`Form::diagnostics`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Form {
    pub main: FormCard,
    /// Cards with unknown kinds are excluded; see [`Form::diagnostics`].
    pub cards: Vec<FormCard>,
    pub diagnostics: Vec<Diagnostic>,
}

// Internal: `project_card`, `build_form`, and `blank_card_for_kind` are not
// public — consumers go through `Quill` methods, never raw `CardSchema`.

/// Build the [`Form`] for a document.
///
/// Coercion is **not** applied — the view shows the document as-is so the
/// editor sees what the user typed; validation diagnostics flag mismatches.
pub(crate) fn build_form(quill: &Quill, doc: &Document) -> Form {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    let main_schema = &quill.source().config().main;
    let main_fields = doc.main().payload().to_index_map();
    let main = project_card(main_schema, &main_fields);

    let mut cards: Vec<FormCard> = Vec::new();
    for (index, card) in doc.cards().iter().enumerate() {
        let kind = card.kind().unwrap_or("").to_string();
        match quill.source().config().card_kind(&kind) {
            Some(card_schema) => {
                let card_fields = card.payload().to_index_map();
                cards.push(project_card(card_schema, &card_fields));
            }
            None => {
                diagnostics.push(
                    Diagnostic::new(
                        Severity::Warning,
                        format!(
                            "card at index {index} has unknown kind \"{kind}\"; \
                             it is not declared in the quill schema and has been \
                             excluded from the form view"
                        ),
                    )
                    .with_code("form::unknown_card_kind".to_string()),
                );
            }
        }
    }

    if let Err(validation_errors) = quill.source().config().validate_document(doc) {
        // Forward the structured diagnostic produced by `ValidationError`
        // verbatim — same `validation::*` code, same path, same hint —
        // instead of wrapping in `form::validation_error` and dropping
        // those fields. Consumers can route on the code without parsing
        // the message text.
        for err in validation_errors {
            diagnostics.push(err.to_diagnostic());
        }
    }

    Form {
        main,
        cards,
        diagnostics,
    }
}

pub(crate) fn blank_card_for_kind(quill: &Quill, card_kind: &str) -> Option<FormCard> {
    quill
        .source()
        .config()
        .card_kind(card_kind)
        .map(FormCard::blank)
}

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
