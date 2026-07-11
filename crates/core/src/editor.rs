//! Schema-bound typed editor — the front door for typed field writes.
//!
//! [`Card::commit_field`](crate::Card::commit_field) still asks the caller to
//! fetch a [`FieldSchema`] per write. Every consumer that wants typed writes (a
//! form editor, an MCP server) already holds the resolved [`QuillConfig`] — it
//! renders with it. [`Quill::editor`](crate::Quill::editor) binds the schema
//! once, so callers issue one verb (`set`) and never pass a type token or an
//! `inline` flag: the editor resolves each field's type itself and routes a
//! schema field through the strict commit and an unknown field through the
//! opaque store.
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

/// Which store a [`TypedEditor::set`] / [`CardEditor::set`] used: `Typed` when
/// the field is declared in the schema (strict commit), `Opaque` when it is not
/// (verbatim store, mirroring [`Card::set_field`](crate::Card::set_field)).
///
/// An editor's caller usually meant a schema field, so a typo'd name silently
/// storing opaque is otherwise invisible until validation — the return value
/// surfaces which path ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Committed {
    /// The field is declared in the schema; the value was strict-committed to
    /// its canonical typed form.
    Typed,
    /// The field is not in the schema; the value was stored opaquely (the field
    /// bag stays a bag).
    Opaque,
}

impl Committed {
    /// `true` for [`Committed::Typed`].
    pub fn is_typed(self) -> bool {
        matches!(self, Committed::Typed)
    }

    /// `"typed"` or `"opaque"` — the discriminant a binding surfaces to its
    /// caller.
    pub fn as_str(self) -> &'static str {
        match self {
            Committed::Typed => "typed",
            Committed::Opaque => "opaque",
        }
    }
}

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
    /// strict-commits it ([`Committed::Typed`]); an unknown field stores
    /// opaquely ([`Committed::Opaque`]). Error surface is that of
    /// [`Card::commit_field`](crate::Card::commit_field) /
    /// [`Card::set_field`](crate::Card::set_field).
    pub fn set(
        &mut self,
        name: &str,
        value: impl Into<QuillValue>,
    ) -> Result<Committed, EditError> {
        let config = self.config;
        match config.main.fields.get(name) {
            Some(schema) => {
                self.doc.main_mut().commit_field(name, value, schema)?;
                Ok(Committed::Typed)
            }
            None => {
                self.doc.main_mut().set_field(name, value)?;
                Ok(Committed::Opaque)
            }
        }
    }

    /// Write several main-card fields atomically — the typed twin of
    /// [`Card::set_fields`](crate::Card::set_fields). Every field is resolved
    /// (typed conform or opaque store) before any is applied; on any violation
    /// nothing is written and every offending field is returned as a
    /// `(name, error)` pair.
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
    /// `$kind` resolves its [`CardSchema`]; an unknown kind degrades the whole
    /// card to opaque writes (mirroring `coerce_card`). Returns
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
    /// [`CardSchema`] (typed commit) or stores opaquely when the field — or the
    /// whole card kind — is unknown.
    pub fn set(
        &mut self,
        name: &str,
        value: impl Into<QuillValue>,
    ) -> Result<Committed, EditError> {
        match self.schema.and_then(|s| s.fields.get(name)) {
            Some(schema) => {
                self.card.commit_field(name, value, schema)?;
                Ok(Committed::Typed)
            }
            None => {
                self.card.set_field(name, value)?;
                Ok(Committed::Opaque)
            }
        }
    }

    /// Write several fields on this card atomically — see
    /// [`TypedEditor::set_all`].
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
/// apply none on failure, apply all on success.
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
        let schema = fields_schema.and_then(|m| m.get(&name));
        match resolve_field_write(&name, value, schema) {
            Ok(stored) => resolved.push((name, stored)),
            Err(e) => errors.push((name, e)),
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
        assert_eq!(ed.set("qty", "3").unwrap(), Committed::Typed);
        assert_eq!(ed.set("subject", "Hello").unwrap(), Committed::Typed);
        assert_eq!(
            doc.main().payload().get("qty").unwrap().as_json(),
            &serde_json::json!(3)
        );
        assert_eq!(doc.main().field_markdown("subject").unwrap(), "Hello\n");
    }

    #[test]
    fn set_stores_unknown_field_opaque() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedEditor::new(&config, &mut doc);
        // Unknown field → opaque store, and the caller can see it happened.
        assert_eq!(ed.set("notafield", "x").unwrap(), Committed::Opaque);
        assert_eq!(
            doc.main().payload().get("notafield").unwrap().as_str(),
            Some("x")
        );
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
    fn card_editor_resolves_card_kind_schema() {
        let config = config();
        let mut doc = blank_doc();
        doc.push_card(Card::new("note").unwrap()).unwrap();

        let mut ed = TypedEditor::new(&config, &mut doc);
        let mut card_ed = ed.card(0).unwrap();
        assert_eq!(card_ed.set("body", "**hi**").unwrap(), Committed::Typed);
        // Unknown field on a known card → opaque.
        assert_eq!(card_ed.set("stray", "v").unwrap(), Committed::Opaque);

        assert_eq!(doc.cards()[0].field_markdown("body").unwrap(), "**hi**\n");

        // Out-of-range card index errors.
        let mut ed = TypedEditor::new(&config, &mut doc);
        assert!(matches!(
            ed.card(9),
            Err(EditError::IndexOutOfRange { .. })
        ));
    }
}
