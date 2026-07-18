//! Schema-bound typed writer — the front door for typed field writes.
//!
//! [`Card::commit_field`](crate::Card::commit_field) still asks the caller to
//! fetch a [`FieldSchema`] per write. Every consumer that wants typed writes (a
//! form editor, an MCP server) already holds the resolved [`QuillConfig`] — it
//! renders with it. [`Quill::writer`](crate::Quill::writer) binds the schema
//! once, so callers issue one verb (`set`) and never pass a type token or an
//! `inline` flag: the writer resolves each field's type itself, strict-commits
//! a schema field, and rejects a name the schema does not declare with
//! [`EditError::UnknownField`] — on the typed path an undeclared name is a typo,
//! not a fallback. Opaque storage stays available on purpose through the raw
//! [`Card::store_field`](crate::Card::store_field) verb.
//!
//! ```ignore
//! let mut w = quill.writer(&mut doc);
//! w.set("subject", "Q3 results")?;       // richtext(inline) → strict content commit
//! w.set("qty", "3")?;                    // integer → strict coerce, stores 3
//! w.card(2)?.set("desc", corpus_json)?;  // card kind → CardSchema → field type
//! w.set_all([("a", "1"), ("b", "2")])?;  // batched, all-or-nothing
//! ```
//!
//! The writer holds `&mut Document` and `&QuillConfig`, so a bound `TypedWriter`
//! cannot cross a binding boundary that carries no lifetimes (wasm-bindgen /
//! pyo3 objects); those surfaces construct one per call from the quill handle.

use indexmap::IndexMap;

use crate::document::edit::resolve_field_write;
use crate::document::{Card, Document, EditError};
use crate::quill::{CardSchema, FieldSchema, QuillConfig};
use crate::value::QuillValue;
use crate::Delta;

/// A [`Document`] bound to its [`QuillConfig`] for typed writes. Construct with
/// [`Quill::writer`](crate::Quill::writer). Writes target the main card; use
/// [`card`](Self::card) for a composable card.
pub struct TypedWriter<'a> {
    config: &'a QuillConfig,
    doc: &'a mut Document,
}

impl<'a> TypedWriter<'a> {
    /// Bind `doc` to `config`. Prefer [`Quill::writer`](crate::Quill::writer).
    pub fn new(config: &'a QuillConfig, doc: &'a mut Document) -> Self {
        Self { config, doc }
    }

    /// Write a field on the main card. Resolves the field's schema type and
    /// strict-commits it; a name the schema does not declare fails with
    /// [`EditError::UnknownField`] rather than falling to the opaque store — on
    /// the typed path it is a typo. For deliberate opaque storage use the raw
    /// [`Card::store_field`](crate::Card::store_field). Other errors are those of
    /// [`Card::commit_field`](crate::Card::commit_field).
    pub fn set(&mut self, name: &str, value: impl Into<QuillValue>) -> Result<(), EditError> {
        let config = self.config;
        match config.main.fields.get(name) {
            Some(schema) => self.doc.main_mut().commit_field(name, value, schema),
            None => Err(EditError::UnknownField(name.to_string())),
        }
    }

    /// Write several main-card fields atomically — the typed twin of
    /// [`Card::store_fields`](crate::Card::store_fields). Every field is resolved
    /// (strict conform, or [`EditError::UnknownField`] for a name the schema does
    /// not declare) before any is applied; on any violation nothing is written
    /// and every offending field is returned as a `(name, error)` pair, so a
    /// caller submitting a whole form sees every typo in one pass the way
    /// [`Card::store_fields`](crate::Card::store_fields) does.
    pub fn set_all<K, V, I>(&mut self, fields: I) -> Result<(), Vec<(String, EditError)>>
    where
        K: Into<String>,
        V: Into<QuillValue>,
        I: IntoIterator<Item = (K, V)>,
    {
        let schema = Some(&self.config.main.fields);
        set_all_impl(self.doc.main_mut(), schema, fields)
    }

    /// Revise the main card's body from markdown (edit semantics: surviving
    /// anchors rebase), discarding the text delta — the receipt-free body write.
    /// Call [`Card::revise_body`](crate::Card::revise_body) on `doc.main_mut()`
    /// for the [`Delta`] receipt.
    pub fn set_body(&mut self, markdown: &str) -> Result<(), EditError> {
        self.doc.main_mut().revise_body(markdown).map(|_| ())
    }

    /// Revise a richtext field on the main card from markdown — typed *and*
    /// anchor-preserving. Resolves the field's schema and defers to
    /// [`Card::revise_field_checked`](crate::Card::revise_field_checked), so
    /// surviving anchors rebase and the diffed result is schema-conformed
    /// (`richtext(inline)` rejects a multi-block result). Returns the text
    /// [`Delta`]. A name the schema does not declare fails with
    /// [`EditError::UnknownField`], as [`set`](Self::set).
    pub fn revise_field(&mut self, name: &str, markdown: &str) -> Result<Delta, EditError> {
        match self.config.main.fields.get(name) {
            Some(schema) => self.doc.main_mut().revise_field_checked(name, markdown, schema),
            None => Err(EditError::UnknownField(name.to_string())),
        }
    }

    /// Build a composable card of `kind`, typed-commit `fields` onto it,
    /// optionally set its body from markdown, and place it — the fused
    /// [`Card::new`](crate::Card::new) + typed writes + insertion. `at` picks the
    /// position: `None` appends ([`push_card`]), `Some(i)` inserts at index `i`
    /// ([`insert_card`]), so a positioned typed insert is one atomic call rather
    /// than `add_card` + [`move_card`](Document::move_card). The card is committed
    /// in full *before* it joins the document, so it is transactional by
    /// construction: a rejected field (or an invalid kind, body, or out-of-range
    /// `at`) leaves the document untouched. Field errors use the all-or-nothing
    /// bundle of [`set_all`](Self::set_all); an invalid kind or body, or an
    /// out-of-range position, surfaces as a single-entry bundle keyed `$kind` /
    /// `$body`.
    ///
    /// [`push_card`]: Document::push_card
    /// [`insert_card`]: Document::insert_card
    pub fn add_card<K, V, I>(
        &mut self,
        kind: &str,
        fields: I,
        body: Option<&str>,
        at: Option<usize>,
    ) -> Result<(), Vec<(String, EditError)>>
    where
        K: Into<String>,
        V: Into<QuillValue>,
        I: IntoIterator<Item = (K, V)>,
    {
        let mut card = Card::new(kind).map_err(|e| vec![("$kind".to_string(), e)])?;
        let schema = self.config.card_kind(kind).map(|s| &s.fields);
        set_all_impl(&mut card, schema, fields)?;
        if let Some(md) = body {
            card.revise_body(md)
                .map_err(|e| vec![("$body".to_string(), e)])?;
        }
        match at {
            Some(index) => self.doc.insert_card(index, card),
            None => self.doc.push_card(card),
        }
        .map_err(|e| vec![("$kind".to_string(), e)])?;
        Ok(())
    }

    /// Remove the composable card at `index`, returning it — the writer
    /// spelling of [`Document::remove_card`], mirroring the JS `writer.removeCard`
    /// sugar. `None` when `index` is out of range.
    pub fn remove_card(&mut self, index: usize) -> Option<Card> {
        self.doc.remove_card(index)
    }

    /// A schema-bound writer for the composable card at `index`. The card's
    /// `$kind` resolves its [`CardSchema`]; an unknown kind carries no schema, so
    /// every field on it is undeclared and its typed writes fail with
    /// [`EditError::UnknownField`] (write such a card opaquely through
    /// [`Card::store_field`](crate::Card::store_field)). Returns
    /// [`EditError::IndexOutOfRange`] when `index` is out of range.
    pub fn card(&mut self, index: usize) -> Result<CardWriter<'_>, EditError> {
        let config = self.config;
        let len = self.doc.cards().len();
        let card = self
            .doc
            .card_mut(index)
            .ok_or(EditError::IndexOutOfRange { index, len })?;
        let schema = card.kind().and_then(|k| config.card_kind(k));
        Ok(CardWriter { schema, card })
    }
}

/// A single composable card bound to its [`CardSchema`], from
/// [`TypedWriter::card`]. Same `set` / `set_all` verbs as [`TypedWriter`].
pub struct CardWriter<'a> {
    schema: Option<&'a CardSchema>,
    card: &'a mut Card,
}

impl CardWriter<'_> {
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

    /// Revise this card's body from markdown (edit semantics), discarding the
    /// delta — the card twin of [`TypedWriter::set_body`].
    pub fn set_body(&mut self, markdown: &str) -> Result<(), EditError> {
        self.card.revise_body(markdown).map(|_| ())
    }

    /// Revise a richtext field on this card from markdown — typed *and*
    /// anchor-preserving; the card twin of [`TypedWriter::revise_field`].
    /// Resolves the field against the card's [`CardSchema`]; an undeclared name —
    /// or any field when the card kind is unknown — fails with
    /// [`EditError::UnknownField`].
    pub fn revise_field(&mut self, name: &str, markdown: &str) -> Result<Delta, EditError> {
        match self.schema.and_then(|s| s.fields.get(name)) {
            Some(schema) => self.card.revise_field_checked(name, markdown, schema),
            None => Err(EditError::UnknownField(name.to_string())),
        }
    }

    /// Write several fields on this card atomically — see
    /// [`TypedWriter::set_all`]; an undeclared name aborts the whole batch with
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

/// All-or-nothing batched write shared by [`TypedWriter::set_all`] and
/// [`CardWriter::set_all`]: resolve every field first (collecting every error),
/// apply none on failure, apply all on success. A name absent from
/// `fields_schema` (or every name, when the whole schema is `None` — an unknown
/// card kind) is an [`EditError::UnknownField`], the batch form of the scalar
/// `set`'s reject-the-typo decision.
fn set_all_impl<K, V, I>(
    card: &mut Card,
    fields_schema: Option<&IndexMap<String, FieldSchema>>,
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
    // Every entry validated by `resolve_field_write` above; apply unchecked.
    for (name, stored) in resolved {
        card.payload_mut().insert_unchecked(name, stored);
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
        let mut ed = TypedWriter::new(&config, &mut doc);

        // A schema field commits typed: "3" → 3, richtext string → content.
        ed.set("qty", "3").unwrap();
        ed.set("subject", "Hello").unwrap();
        assert_eq!(
            doc.main().payload().get("qty").unwrap().as_json(),
            &serde_json::json!(3)
        );
        assert_eq!(doc.main().field_markdown("subject").unwrap().unwrap(), "Hello\n");
    }

    #[test]
    fn set_rejects_unknown_field() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedWriter::new(&config, &mut doc);
        // Unknown field on the typed path is a typo, not a fallback: it fails
        // here and nothing is written. Opaque storage is the raw `store_field`.
        let err = ed.set("notafield", "x").unwrap_err();
        assert_eq!(err.variant_name(), "UnknownField");
        assert!(doc.main().payload().get("notafield").is_none());
    }

    #[test]
    fn set_all_is_all_or_nothing() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedWriter::new(&config, &mut doc);
        // One bad field aborts the whole batch; nothing is applied.
        let errs = ed
            .set_all([("qty", "5"), ("subject", "bad\n\nblock")])
            .unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].0, "subject");
        assert!(doc.main().payload().get("qty").is_none());

        // A clean batch applies every field.
        let mut ed = TypedWriter::new(&config, &mut doc);
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
        let mut ed = TypedWriter::new(&config, &mut doc);
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
    fn set_body_revises_main_body() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedWriter::new(&config, &mut doc);
        ed.set_body("**hi**").unwrap();
        assert_eq!(doc.main().body_markdown(), "**hi**\n");
    }

    #[test]
    fn add_card_fuses_new_commit_push() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedWriter::new(&config, &mut doc);
        ed.add_card("note", [("body", "**hi**")], Some("card body"), None)
            .unwrap();
        assert_eq!(doc.cards().len(), 1);
        assert_eq!(doc.cards()[0].kind(), Some("note"));
        assert_eq!(doc.cards()[0].field_markdown("body").unwrap().unwrap(), "**hi**\n");
        assert_eq!(doc.cards()[0].body_markdown(), "card body\n");
    }

    #[test]
    fn add_card_at_inserts_and_remove_card_returns() {
        let config = config();
        let mut doc = blank_doc();
        {
            let mut ed = TypedWriter::new(&config, &mut doc);
            ed.add_card("note", [("body", "a")], None, None).unwrap();
            ed.add_card("note", [("body", "c")], None, None).unwrap();
            // Positioned typed insert in one atomic call.
            ed.add_card("note", [("body", "b")], None, Some(1)).unwrap();
        }
        let bodies: Vec<String> = doc
            .cards()
            .iter()
            .map(|c| c.field_markdown("body").unwrap().unwrap())
            .collect();
        assert_eq!(bodies, ["a\n", "b\n", "c\n"]);

        // An out-of-range position is transactional — nothing is inserted.
        {
            let mut ed = TypedWriter::new(&config, &mut doc);
            let errs = ed
                .add_card("note", [("body", "x")], None, Some(9))
                .unwrap_err();
            assert_eq!(errs[0].0, "$kind");
        }
        assert_eq!(doc.cards().len(), 3);

        // remove_card returns the removed card; None out of range.
        {
            let mut ed = TypedWriter::new(&config, &mut doc);
            let removed = ed.remove_card(1).unwrap();
            assert_eq!(removed.field_markdown("body").unwrap().unwrap(), "b\n");
            assert!(ed.remove_card(5).is_none());
        }
        assert_eq!(doc.cards().len(), 2);
    }

    #[test]
    fn add_card_is_transactional_on_bad_field() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedWriter::new(&config, &mut doc);
        // An undeclared field aborts the commit; the card never joins the document.
        let errs = ed
            .add_card("note", [("stray", "x")], None, None)
            .unwrap_err();
        assert_eq!(errs[0].0, "stray");
        assert_eq!(errs[0].1.variant_name(), "UnknownField");
        assert_eq!(doc.cards().len(), 0);
    }

    #[test]
    fn add_card_reports_invalid_kind() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedWriter::new(&config, &mut doc);
        // A reserved kind is refused before any card is built.
        let errs = ed
            .add_card("$reserved", [] as [(&str, &str); 0], None, None)
            .unwrap_err();
        assert_eq!(errs[0].0, "$kind");
        assert_eq!(doc.cards().len(), 0);
    }

    #[test]
    fn card_writer_resolves_card_kind_schema() {
        let config = config();
        let mut doc = blank_doc();
        doc.push_card(Card::new("note").unwrap()).unwrap();

        let mut ed = TypedWriter::new(&config, &mut doc);
        let mut card_ed = ed.card(0).unwrap();
        card_ed.set("body", "**hi**").unwrap();
        // Unknown field on a known card → rejected as a typo.
        let err = card_ed.set("stray", "v").unwrap_err();
        assert_eq!(err.variant_name(), "UnknownField");

        assert_eq!(doc.cards()[0].field_markdown("body").unwrap().unwrap(), "**hi**\n");

        // Out-of-range card index errors.
        let mut ed = TypedWriter::new(&config, &mut doc);
        assert!(matches!(
            ed.card(9),
            Err(EditError::IndexOutOfRange { .. })
        ));
    }

    #[test]
    fn revise_field_is_typed_and_rejects_unknown_and_non_inline() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedWriter::new(&config, &mut doc);
        // Typed richtext write lands the content and returns a Delta receipt.
        let _delta = ed.revise_field("subject", "Hello").unwrap();
        assert_eq!(doc.main().field_markdown("subject").unwrap().unwrap(), "Hello\n");

        // Unknown name is a typo, not a fallback.
        let mut ed = TypedWriter::new(&config, &mut doc);
        assert_eq!(
            ed.revise_field("nope", "x").unwrap_err().variant_name(),
            "UnknownField"
        );
        // richtext(inline) rejects a multi-block result; the field is unchanged.
        let err = ed.revise_field("subject", "a\n\nb").unwrap_err();
        assert_eq!(err.variant_name(), "FieldRichtextNotInline");
        assert_eq!(doc.main().field_markdown("subject").unwrap().unwrap(), "Hello\n");
    }

    #[test]
    fn card_writer_revise_field_resolves_card_schema() {
        let config = config();
        let mut doc = blank_doc();
        doc.push_card(Card::new("note").unwrap()).unwrap();

        let mut ed = TypedWriter::new(&config, &mut doc);
        ed.card(0).unwrap().revise_field("body", "**hi**").unwrap();
        assert_eq!(doc.cards()[0].field_markdown("body").unwrap().unwrap(), "**hi**\n");

        let mut ed = TypedWriter::new(&config, &mut doc);
        assert_eq!(
            ed.card(0)
                .unwrap()
                .revise_field("stray", "x")
                .unwrap_err()
                .variant_name(),
            "UnknownField"
        );
    }
}
