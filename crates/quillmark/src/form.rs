//! Schema-aware form views for form editors.
//!
//! This module provides [`Form`] — a read-only snapshot of a [`Document`]
//! viewed through its [`Quill`] schema — and [`FormLeaf`], the per-leaf view.
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
//! // A blank form for a fresh leaf the user is about to add:
//! if let Some(blank) = quill.blank_leaf("indorsement") {
//!     // render `blank.values`...
//!     # let _ = blank;
//! }
//! # }
//! ```
//!
//! # Snapshot semantics
//!
//! A [`Form`] (or [`FormLeaf`]) is a read-only snapshot built at the moment
//! the call returned. Subsequent edits to the document are not reflected;
//! call `quill.form(doc)` again to obtain an updated snapshot.
//!
//! # Unknown leaf tags
//!
//! Leaves whose tag is not declared in the schema are dropped from
//! [`Form::leaves`]. Each such leaf produces one [`Diagnostic`] in
//! [`Form::diagnostics`] with code `"form::unknown_leaf_kind"`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use quillmark_core::quill::LeafSchema;
use quillmark_core::{Diagnostic, Document, QuillValue, Severity};

use crate::Quill;

/// Source of a field's effective value in a form view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormFieldSource {
    /// Value was present in the document's frontmatter or leaf fields.
    Document,
    /// Value was absent from the document; the schema provides a default.
    Default,
    /// Value was absent from the document and the schema has no default.
    Missing,
}

/// A single field's view within a [`FormLeaf`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormFieldValue {
    /// Current value from the document, if present.
    pub value: Option<QuillValue>,
    /// Schema default value, if declared.
    pub default: Option<QuillValue>,
    /// Where the effective value comes from.
    pub source: FormFieldSource,
}

/// A leaf viewed through its schema — either the main document leaf or a
/// named leaf block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormLeaf {
    /// The schema that governs this leaf.
    pub schema: LeafSchema,
    /// View of each schema-declared field.
    ///
    /// Keys follow `IndexMap` insertion order: schema field definition order,
    /// stably re-sorted by `ui.order` when present.
    pub values: IndexMap<String, FormFieldValue>,
}

impl FormLeaf {
    /// A blank form leaf for `schema` — no document values supplied. Every
    /// declared field's source is [`FormFieldSource::Default`] (when the
    /// schema declares a default) or [`FormFieldSource::Missing`].
    ///
    /// This is the "user is about to add a new leaf" view. Reach it through
    /// [`Quill::blank_leaf`] or [`Quill::blank_main`] when you have a tag in
    /// hand instead of a [`LeafSchema`].
    pub fn blank(schema: &LeafSchema) -> Self {
        project_leaf(schema, &IndexMap::new())
    }
}

/// Read-only snapshot of a [`Document`] viewed through a [`Quill`]'s schema.
///
/// Produced by [`Quill::form`]. Subsequent edits to the document are **not**
/// reflected here — call `quill.form(doc)` again after editing.
///
/// # Unknown leaves
///
/// Document leaves whose tag is not declared in the schema are dropped and
/// each produces a [`Diagnostic`] with code `"form::unknown_leaf_kind"` in
/// [`Form::diagnostics`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Form {
    /// View of the main document leaf (frontmatter fields).
    pub main: FormLeaf,
    /// View of each recognised leaf, in document order.
    ///
    /// Leaves with unknown tags are excluded; see [`Form::diagnostics`].
    pub leaves: Vec<FormLeaf>,
    /// Diagnostics from unknown leaf tags and validation.
    pub diagnostics: Vec<Diagnostic>,
}

// ── Internal projection ─────────────────────────────────────────────────────
//
// `project_leaf`, `build_form`, and `blank_leaf_for_kind` are the internal
// machinery used by `Quill::form`, `Quill::blank_main`, and
// `Quill::blank_leaf`. They are **deliberately not public**: consumers
// reach the form module through methods on `Quill`, never by holding a
// `LeafSchema` and a field map directly.

/// Build the [`Form`] for a document. Composes:
/// - `QuillConfig::main` — the main leaf schema.
/// - `QuillConfig::leaf_kind` — to look up leaf schemas by tag.
/// - `QuillConfig::validate_document` — to gather validation diagnostics.
///
/// Coercion (`coerce_frontmatter` / `coerce_leaf`) is **not** applied here:
/// the form view is the document as-is so the editor sees what the user typed.
/// Validation diagnostics already inform the consumer when values are
/// type-mismatched.
pub(crate) fn build_form(quill: &Quill, doc: &Document) -> Form {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    let main_schema = &quill.source().config().main;
    let main_fields = doc.main().frontmatter().to_index_map();
    let main = project_leaf(main_schema, &main_fields);

    let mut leaves: Vec<FormLeaf> = Vec::new();
    for (index, leaf) in doc.leaves().iter().enumerate() {
        let tag = leaf.tag();
        match quill.source().config().leaf_kind(&tag) {
            Some(leaf_schema) => {
                let leaf_fields = leaf.frontmatter().to_index_map();
                leaves.push(project_leaf(leaf_schema, &leaf_fields));
            }
            None => {
                diagnostics.push(
                    Diagnostic::new(
                        Severity::Warning,
                        format!(
                            "leaf at index {index} has unknown tag \"{tag}\"; \
                             it is not declared in the quill schema and has been \
                             excluded from the form view"
                        ),
                    )
                    .with_code("form::unknown_leaf_kind".to_string()),
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
        leaves,
        diagnostics,
    }
}

/// Build a blank [`FormLeaf`] for a leaf type by tag, or `None` if the tag
/// isn't declared in the quill's schema.
pub(crate) fn blank_leaf_for_kind(quill: &Quill, leaf_kind: &str) -> Option<FormLeaf> {
    quill
        .source()
        .config()
        .leaf_kind(leaf_kind)
        .map(FormLeaf::blank)
}

/// Build a [`FormLeaf`] by walking each schema-declared field and looking up
/// its value in `fields`.
fn project_leaf(schema: &LeafSchema, fields: &IndexMap<String, QuillValue>) -> FormLeaf {
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

    FormLeaf {
        schema: schema.clone(),
        values,
    }
}

#[cfg(test)]
mod tests;
