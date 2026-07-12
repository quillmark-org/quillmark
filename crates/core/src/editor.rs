//! Schema-bound typed editor — the front door for typed field writes.
//!
//! [`Card::commit_field`](crate::Card::commit_field) still asks the caller to
//! fetch a [`FieldSchema`] per write. Every consumer that wants typed writes (a
//! form editor, an MCP server) already holds the resolved [`QuillConfig`] — it
//! renders with it. [`Quill::editor`](crate::Quill::editor) binds the schema
//! once, so callers issue one verb (`set`) and never pass a type token or an
//! `inline` flag: the editor resolves each field's type itself, strict-commits
//! a schema field, and rejects a name the schema does not declare with
//! [`EditError::UnknownField`] — on the typed path an undeclared name is a typo,
//! not a fallback. Opaque storage stays available on purpose through the raw
//! [`Card::set_field`](crate::Card::set_field) verb.
//!
//! ```ignore
//! let mut ed = quill.editor(&mut doc);
//! ed.set("subject", "Q3 results")?;       // richtext(inline) → strict corpus commit
//! ed.set("qty", "3")?;                    // integer → strict coerce, stores 3
//! ed.card(2)?.set("desc", corpus_json)?;  // card kind → CardSchema → field type
//! ed.set_all([("a", "1"), ("b", "2")])?;  // batched, all-or-nothing
//! ```
//!
//! The editor holds `&mut Document` and `&QuillConfig`, so a bound `TypedEditor`
//! cannot cross a binding boundary that carries no lifetimes (wasm-bindgen /
//! pyo3 objects); those surfaces construct one per call from the quill handle.

use std::collections::BTreeMap;

use crate::document::edit::resolve_field_write;
use crate::document::{Card, Document, EditError};
use crate::quill::{CardSchema, FieldSchema, QuillConfig};
use crate::value::QuillValue;

/// A [`Document`] bound to its [`QuillConfig`] for typed writes. Construct with
/// [`Quill::editor`](crate::Quill::editor). Writes target the main card; use
/// [`card`](Self::card) for a composable card.
pub struct TypedEditor<'a> {
    config: &'a QuillConfig,
    doc: &'a mut Document,
}

impl<'a> TypedEditor<'a> {
    /// Bind `doc` to `config`. Prefer [`Quill::editor`](crate::Quill::editor).
    pub fn new(config: &'a QuillConfig, doc: &'a mut Document) -> Self {
        Self { config, doc }
    }

    /// Write a field on the main card. Resolves the field's schema type and
    /// strict-commits it; a name the schema does not declare fails with
    /// [`EditError::UnknownField`] rather than falling to the opaque store — on
    /// the typed path it is a typo. For deliberate opaque storage use the raw
    /// [`Card::set_field`](crate::Card::set_field). Other errors are those of
    /// [`Card::commit_field`](crate::Card::commit_field).
    pub fn set(&mut self, name: &str, value: impl Into<QuillValue>) -> Result<(), EditError> {
        let config = self.config;
        match config.main.fields.get(name) {
            Some(schema) => self.doc.main_mut().commit_field(name, value, schema),
            None => Err(EditError::UnknownField(name.to_string())),
        }
    }

    /// Write several main-card fields atomically — the typed twin of
    /// [`Card::set_fields`](crate::Card::set_fields). Every field is resolved
    /// (strict conform, or [`EditError::UnknownField`] for a name the schema does
    /// not declare) before any is applied; on any violation nothing is written
    /// and every offending field is returned as a `(name, error)` pair, so a
    /// caller submitting a whole form sees every typo in one pass the way
    /// [`Card::set_fields`](crate::Card::set_fields) does.
    pub fn set_all<K, V, I>(&mut self, fields: I) -> Result<(), Vec<(String, EditError)>>
    where
        K: Into<String>,
        V: Into<QuillValue>,
        I: IntoIterator<Item = (K, V)>,
    {
        let schema = Some(&self.config.main.fields);
        set_all_impl(self.doc.main_mut(), schema, fields)
    }

    /// A schema-bound editor for the composable card at `index`. The card's
    /// `$kind` resolves its [`CardSchema`]; an unknown kind carries no schema, so
    /// every field on it is undeclared and its typed writes fail with
    /// [`EditError::UnknownField`] (write such a card opaquely through
    /// [`Card::set_field`](crate::Card::set_field)). Returns
    /// [`EditError::IndexOutOfRange`] when `index` is out of range.
    pub fn card(&mut self, index: usize) -> Result<CardEditor<'_>, EditError> {
        let config = self.config;
        let len = self.doc.cards().len();
        let card = self
            .doc
            .card_mut(index)
            .ok_or(EditError::IndexOutOfRange { index, len })?;
        let schema = card.kind().and_then(|k| config.card_kind(k));
        Ok(CardEditor { schema, card })
    }
}

/// A single composable card bound to its [`CardSchema`], from
/// [`TypedEditor::card`]. Same `set` / `set_all` verbs as [`TypedEditor`].
pub struct CardEditor<'a> {
    schema: Option<&'a CardSchema>,
    card: &'a mut Card,
}

impl CardEditor<'_> {
    /// The card's `$kind`, if any.
    pub fn kind(&self) -> Option<&str> {
        self.card.kind()
    }

    /// Write a field on this card. Resolves the field against the card's
    /// [`CardSchema`] and strict-commits it; a field the schema does not declare
    /// — or any field when the card kind is unknown — fails with
    /// [`EditError::UnknownField`] rather than storing opaquely.
    pub fn set(&mut self, name: &str, value: impl Into<QuillValue>) -> Result<(), EditError> {
        match self.schema.and_then(|s| s.fields.get(name)) {
            Some(schema) => self.card.commit_field(name, value, schema),
            None => Err(EditError::UnknownField(name.to_string())),
        }
    }

    /// Write several fields on this card atomically — see
    /// [`TypedEditor::set_all`]; an undeclared name aborts the whole batch with
    /// [`EditError::UnknownField`].
    pub fn set_all<K, V, I>(&mut self, fields: I) -> Result<(), Vec<(String, EditError)>>
    where
        K: Into<String>,
        V: Into<QuillValue>,
        I: IntoIterator<Item = (K, V)>,
    {
        set_all_impl(self.card, self.schema.map(|s| &s.fields), fields)
    }
}

/// All-or-nothing batched write shared by [`TypedEditor::set_all`] and
/// [`CardEditor::set_all`]: resolve every field first (collecting every error),
/// apply none on failure, apply all on success. A name absent from
/// `fields_schema` (or every name, when the whole schema is `None` — an unknown
/// card kind) is an [`EditError::UnknownField`], the batch form of the scalar
/// `set`'s reject-the-typo decision.
fn set_all_impl<K, V, I>(
    card: &mut Card,
    fields_schema: Option<&BTreeMap<String, FieldSchema>>,
    fields: I,
) -> Result<(), Vec<(String, EditError)>>
where
    K: Into<String>,
    V: Into<QuillValue>,
    I: IntoIterator<Item = (K, V)>,
{
    let fields: Vec<(String, QuillValue)> = fields
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();

    let mut resolved: Vec<(String, QuillValue)> = Vec::with_capacity(fields.len());
    let mut errors: Vec<(String, EditError)> = Vec::new();
    for (name, value) in fields {
        match fields_schema.and_then(|m| m.get(&name)) {
            Some(schema) => match resolve_field_write(&name, value, schema) {
                Ok(stored) => resolved.push((name, stored)),
                Err(e) => errors.push((name, e)),
            },
            None => errors.push((name.clone(), EditError::UnknownField(name))),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    for (name, stored) in resolved {
        card.payload_mut().insert(name, stored);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Card, Document};
    use crate::version::QuillReference;
    use std::str::FromStr;

    const QUILL_YAML: &str = "\
quill:
  name: memo
  backend: typst
  version: 1.0.0
  description: Editor test quill
main:
  fields:
    subject:
      type: richtext
      inline: true
    qty:
      type: integer
card_kinds:
  note:
    fields:
      body:
        type: richtext
";

    fn config() -> QuillConfig {
        QuillConfig::from_yaml(QUILL_YAML).expect("valid quill")
    }

    fn blank_doc() -> Document {
        Document::new(QuillReference::from_str("memo@1.0.0").unwrap())
    }

    #[test]
    fn set_resolves_schema_field_as_typed_commit() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedEditor::new(&config, &mut doc);

        // A schema field commits typed: "3" → 3, richtext string → corpus.
        ed.set("qty", "3").unwrap();
        ed.set("subject", "Hello").unwrap();
        assert_eq!(
            doc.main().payload().get("qty").unwrap().as_json(),
            &serde_json::json!(3)
        );
        assert_eq!(doc.main().field_markdown("subject").unwrap(), "Hello\n");
    }

    #[test]
    fn set_rejects_unknown_field() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedEditor::new(&config, &mut doc);
        // Unknown field on the typed path is a typo, not a fallback: it fails
        // here and nothing is written. Opaque storage is the raw `set_field`.
        let err = ed.set("notafield", "x").unwrap_err();
        assert_eq!(err.variant_name(), "UnknownField");
        assert!(doc.main().payload().get("notafield").is_none());
    }

    #[test]
    fn set_reports_strict_conform_failure() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedEditor::new(&config, &mut doc);
        let err = ed.set("qty", "not-a-number").unwrap_err();
        assert_eq!(err.variant_name(), "FieldConform");
        // A richtext(inline) violation surfaces through the richtext variant.
        let err = ed.set("subject", "line one\n\nline two").unwrap_err();
        assert_eq!(err.variant_name(), "FieldRichtextNotInline");
    }

    #[test]
    fn set_all_is_all_or_nothing() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedEditor::new(&config, &mut doc);
        // One bad field aborts the whole batch; nothing is applied.
        let errs = ed
            .set_all([("qty", "5"), ("subject", "bad\n\nblock")])
            .unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].0, "subject");
        assert!(doc.main().payload().get("qty").is_none());

        // A clean batch applies every field.
        let mut ed = TypedEditor::new(&config, &mut doc);
        ed.set_all([("qty", "5"), ("subject", "ok")]).unwrap();
        assert_eq!(
            doc.main().payload().get("qty").unwrap().as_json(),
            &serde_json::json!(5)
        );
    }

    #[test]
    fn set_all_rejects_unknown_field() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedEditor::new(&config, &mut doc);
        // A whole-form submit with a typo'd name: `qty` is a schema field, `titel`
        // is not. The undeclared name aborts the all-or-nothing batch — nothing is
        // written and the typo is reported.
        let errs = ed.set_all([("qty", "3"), ("titel", "oops")]).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].0, "titel");
        assert_eq!(errs[0].1.variant_name(), "UnknownField");
        assert!(doc.main().payload().get("qty").is_none());
    }

    #[test]
    fn card_editor_resolves_card_kind_schema() {
        let config = config();
        let mut doc = blank_doc();
        doc.push_card(Card::new("note").unwrap()).unwrap();

        let mut ed = TypedEditor::new(&config, &mut doc);
        let mut card_ed = ed.card(0).unwrap();
        card_ed.set("body", "**hi**").unwrap();
        // Unknown field on a known card → rejected as a typo.
        let err = card_ed.set("stray", "v").unwrap_err();
        assert_eq!(err.variant_name(), "UnknownField");

        assert_eq!(doc.cards()[0].field_markdown("body").unwrap(), "**hi**\n");

        // Out-of-range card index errors.
        let mut ed = TypedEditor::new(&config, &mut doc);
        assert!(matches!(
            ed.card(9),
            Err(EditError::IndexOutOfRange { .. })
        ));
    }
}
