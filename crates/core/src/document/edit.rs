//! Typed mutators for [`Document`] and [`Card`] with invariant enforcement.
//!
//! Every successful mutator leaves the document with every user field name
//! matching `[A-Za-z_][A-Za-z0-9_]*` and every composable `$kind` passing
//! `meta::is_valid_kind_name`, so the result is safely serializable via
//! [`Document::to_plate_json`]. Mutators never modify `warnings` — those
//! are immutable parse-time observations.
//!
//! Payload/body mutators (field set/fill/remove, `$ext` and `$seed`
//! namespace writers, body replacement) live on [`Card`]; [`Document`] keeps
//! document-level ops (quill-ref, push/insert/remove/move card).
//!
//! The `$ext` mutators carry no field-name invariant ($ext is an opaque
//! mapping that never reaches the plate JSON backends consume), but they do
//! enforce the §8 value-depth bound: `$ext` flows through the recursive
//! emit and DTO paths like any other value.

use unicode_normalization::UnicodeNormalization;

use quillmark_richtext::delta::diff_import;
use quillmark_richtext::import::ImportError;
use quillmark_richtext::{ApplyError, Delta, LineOp, MarkOp, RichText};

use crate::document::meta::{validate_composable_kind, CardKindError};
use crate::document::{Card, Document, Payload};
use crate::quill::{CoercionError, FieldSchema, Leniency, QuillConfig};
use crate::value::QuillValue;
use crate::version::QuillReference;

/// `true` if `name` matches `[A-Za-z_][A-Za-z0-9_]*` after NFC normalisation.
///
/// Lowercase is the recommended (canonical) convention, but uppercase ASCII
/// letters are accepted and preserved verbatim. Collision-safety with system
/// metadata comes entirely from the `$`-prefix exclusion — `$`-prefixed keys
/// are reserved, so a user field can never shadow one regardless of case.
pub fn is_valid_field_name(name: &str) -> bool {
    let normalized: String = name.nfc().collect();
    if normalized.is_empty() {
        return false;
    }
    let mut chars = normalized.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    for ch in chars {
        if !ch.is_ascii_alphanumeric() && ch != '_' {
            return false;
        }
    }
    true
}

/// Errors returned by document and card mutators.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EditError {
    #[error("invalid field name '{0}': must match [A-Za-z_][A-Za-z0-9_]*")]
    InvalidFieldName(String),

    /// A typed write ([`TypedEditor::set`](crate::TypedEditor::set) /
    /// [`CardEditor::set`](crate::CardEditor::set)) addressed a well-formed name
    /// that the bound schema does not declare (or a card whose `$kind` carries
    /// no schema). The typed path resolves every name to a schema type, so an
    /// undeclared name is a typo, not a fallback — it fails here instead of
    /// landing silently in the opaque store. Reach for the raw
    /// [`Card::set_field`](Card::set_field) when opaque storage is the intent.
    #[error("field '{0}' is not declared in the schema")]
    UnknownField(String),

    #[error("invalid card kind '{0}': must match [a-z_][a-z0-9_]*")]
    InvalidKindName(String),

    #[error("card kind 'main' is reserved for the document root")]
    ReservedKind,

    #[error("index {index} is out of range (len = {len})")]
    IndexOutOfRange { index: usize, len: usize },

    #[error("value nests deeper than the maximum of {max} levels")]
    ValueTooDeep { max: usize },

    /// Markdown body import failed — the corpus codec rejected the input
    /// (e.g. container nesting past [`MAX_NESTING_DEPTH`](quillmark_richtext::MAX_NESTING_DEPTH)).
    /// Returned instead of silently degrading the body to empty on a rejected import.
    #[error("body import failed: {0}")]
    BodyImport(ImportError),

    /// A body value in the corpus-or-markdown encoding could not be decoded: a
    /// JSON object that is not a canonical richtext corpus, a markdown string
    /// that failed to import, or a shape that is neither object, string, nor
    /// null. Returned by [`Card::set_body_value`](Card::set_body_value).
    #[error("body decode failed: {0}")]
    BodyDecode(String),

    /// A richtext field value in the corpus-or-markdown encoding could not be
    /// decoded: a JSON object that is not a canonical richtext corpus, a
    /// markdown string that failed to import, or a shape that is neither
    /// object, string, nor null. The field-level twin of [`BodyDecode`](Self::BodyDecode);
    /// returned by [`Card::commit_field`](Card::commit_field) on a richtext field
    /// and by [`Card::apply_field_richtext_change`](Card::apply_field_richtext_change).
    #[error("richtext field '{field}' decode failed: {message}")]
    FieldRichtextDecode { field: String, message: String },

    /// A richtext field written under the `richtext(inline)` constraint decoded
    /// to a multi-block corpus (more than one line, a container, or an island).
    /// The write-time counterpart of the coercion/validation `richtext(inline)`
    /// check; returned by [`Card::commit_field`](Card::commit_field) when the
    /// field's schema is `richtext` with `inline: true`.
    #[error("richtext field '{0}' is not inline: richtext(inline) requires a single paragraph line with no list/quote container and no islands")]
    FieldRichtextNotInline(String),

    /// A typed write ([`Card::commit_field`](Card::commit_field)) could not
    /// conform the value to the field's schema type — the general write-commit
    /// failure for scalar/array/object types (a `"x"` for an `integer`, a
    /// non-object for an `object`, …). Richtext fields report through the
    /// dedicated [`FieldRichtextDecode`](Self::FieldRichtextDecode) /
    /// [`FieldRichtextNotInline`](Self::FieldRichtextNotInline) variants
    /// instead, so the richtext write surface is unchanged.
    #[error("field '{field}' does not conform to its schema type: {message}")]
    FieldConform { field: String, message: String },

    /// A corpus field-change bundle (text delta, line ops, mark ops) applied
    /// out of bounds or broke an invariant normalization could not repair.
    #[error("corpus apply failed: {0:?}")]
    CorpusApply(ApplyError),
}

impl EditError {
    /// The bare variant name (e.g. `"InvalidFieldName"`). The wasm and Python
    /// bindings each surface it as the `[EditError::<Variant>]` message prefix;
    /// defined once here so a new variant cannot drift between the two
    /// binding error mappers.
    pub fn variant_name(&self) -> &'static str {
        match self {
            EditError::InvalidFieldName(_) => "InvalidFieldName",
            EditError::UnknownField(_) => "UnknownField",
            EditError::InvalidKindName(_) => "InvalidKindName",
            EditError::ReservedKind => "ReservedKind",
            EditError::IndexOutOfRange { .. } => "IndexOutOfRange",
            EditError::ValueTooDeep { .. } => "ValueTooDeep",
            EditError::BodyImport(_) => "BodyImport",
            EditError::BodyDecode(_) => "BodyDecode",
            EditError::FieldRichtextDecode { .. } => "FieldRichtextDecode",
            EditError::FieldRichtextNotInline(_) => "FieldRichtextNotInline",
            EditError::FieldConform { .. } => "FieldConform",
            EditError::CorpusApply(_) => "CorpusApply",
        }
    }
}

/// A field-level invariant violation, shared by every payload ingestion path.
///
/// Each boundary maps it to its own error type (`ParseError`,
/// `StorageError`, `WireError`, `EditError`), so the invariant — every user
/// field name matches `[A-Za-z_][A-Za-z0-9_]*` and no value nests past the §8
/// depth limit — is enforced once, here, and a constructed `Document` can
/// never violate it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldViolation {
    /// The field name does not match `[A-Za-z_][A-Za-z0-9_]*` (spec §3.4 / §10).
    InvalidName,
    /// The value nests deeper than [`MAX_YAML_DEPTH`](crate::document::limits::MAX_YAML_DEPTH)
    /// (spec §8).
    TooDeep,
}

/// Map a [`FieldViolation`] to the mutator error surface.
fn check_field(name: &str, value: &serde_json::Value) -> Result<(), EditError> {
    validate_field(name, value).map_err(|v| match v {
        FieldViolation::InvalidName => EditError::InvalidFieldName(name.to_string()),
        FieldViolation::TooDeep => EditError::ValueTooDeep {
            max: crate::document::limits::MAX_YAML_DEPTH,
        },
    })
}

/// Depth-bound an out-of-band meta map (`$ext` / `$seed`). Both ride the same
/// recursive emit/DTO paths, so they carry the same §8 depth bound.
fn check_meta_depth(map: &serde_json::Map<String, serde_json::Value>) -> Result<(), EditError> {
    let as_value = serde_json::Value::Object(map.clone());
    if crate::value::json_depth_exceeds(&as_value, crate::document::limits::MAX_YAML_DEPTH) {
        return Err(EditError::ValueTooDeep {
            max: crate::document::limits::MAX_YAML_DEPTH,
        });
    }
    Ok(())
}

/// Validate a user field at the payload boundary: name conformance and
/// value-depth bound. See [`FieldViolation`] for the invariant.
pub fn validate_field(key: &str, value: &serde_json::Value) -> Result<(), FieldViolation> {
    if !is_valid_field_name(key) {
        return Err(FieldViolation::InvalidName);
    }
    if crate::value::json_depth_exceeds(value, crate::document::limits::MAX_YAML_DEPTH) {
        return Err(FieldViolation::TooDeep);
    }
    Ok(())
}

/// Map a strict-write [`CoercionError`] to the field-write [`EditError`] surface.
///
/// A failed richtext coercion routes to the dedicated `FieldRichtext*` variants
/// — the same surface [`Card::apply_field_richtext_change`] produces, and the
/// one the wasm/Python error mappers (and their tests) key on. This keys on the
/// coercion `target`, not the top-level field type, because the richtext
/// constraint can be **nested**: an `array` of `richtext(inline)` items fails
/// with `target == "richtext(inline)"` while the field's own type is `Array`.
/// The richtext coercion emits exactly `"richtext"` / `"richtext(inline)"`
/// (see `QuillConfig::conform_value`); every other target uses the general
/// [`EditError::FieldConform`].
fn conform_error_to_edit(name: &str, err: CoercionError) -> EditError {
    let CoercionError::Uncoercible { target, reason, .. } = err;
    match target.as_str() {
        "richtext(inline)" => EditError::FieldRichtextNotInline(name.to_string()),
        "richtext" => EditError::FieldRichtextDecode {
            field: name.to_string(),
            message: reason,
        },
        _ => EditError::FieldConform {
            field: name.to_string(),
            message: reason,
        },
    }
}

/// Compute the canonical stored form of a typed field write **without applying
/// it** — the dry-run shared by [`Card::commit_field`] and the batched,
/// all-or-nothing [`TypedEditor::set_all`](crate::TypedEditor::set_all).
///
/// Strict `Leniency::Write` conform against `schema`; the name and stored-value
/// depth are validated too, so a batch can collect every violation before any
/// mutation. The unknown-name case never reaches here — the editor rejects it
/// with [`EditError::UnknownField`] before there is a schema to conform against.
pub(crate) fn resolve_field_write(
    name: &str,
    value: QuillValue,
    schema: &FieldSchema,
) -> Result<QuillValue, EditError> {
    if !is_valid_field_name(name) {
        return Err(EditError::InvalidFieldName(name.to_string()));
    }
    let stored = QuillConfig::conform_value(&value, schema, name, Leniency::Write)
        .map_err(|e| conform_error_to_edit(name, e))?;
    // Depth-bound the stored form (name already validated above).
    check_field(name, stored.as_json())?;
    Ok(stored)
}

impl Document {
    pub fn set_quill_ref(&mut self, reference: QuillReference) {
        self.main_mut().payload_mut().set_quill(reference);
    }

    pub fn card_mut(&mut self, index: usize) -> Option<&mut Card> {
        self.cards_mut().get_mut(index)
    }

    /// Append a composable card. Its `$kind` must be a valid, non-reserved
    /// composable kind ([`EditError::InvalidKindName`] /
    /// [`EditError::ReservedKind`] otherwise) — the invariant for any card in
    /// the cards list, enforced here so every entry path shares it.
    pub fn push_card(&mut self, card: Card) -> Result<(), EditError> {
        Self::check_composable_kind(&card)?;
        self.cards_vec_mut().push(card);
        Ok(())
    }

    /// Insert a composable card at `index` (`index > len` →
    /// [`EditError::IndexOutOfRange`]; invalid `$kind` →
    /// [`EditError::InvalidKindName`] / [`EditError::ReservedKind`]).
    pub fn insert_card(&mut self, index: usize, card: Card) -> Result<(), EditError> {
        let len = self.cards().len();
        if index > len {
            return Err(EditError::IndexOutOfRange { index, len });
        }
        Self::check_composable_kind(&card)?;
        self.cards_vec_mut().insert(index, card);
        Ok(())
    }

    /// Validate that `card`'s `$kind` is a valid, non-reserved composable kind.
    /// A card with no `$kind` is rejected as an invalid (empty) name.
    fn check_composable_kind(card: &Card) -> Result<(), EditError> {
        let kind = card.kind().unwrap_or("");
        validate_composable_kind(kind).map_err(|e| match e {
            CardKindError::InvalidName => EditError::InvalidKindName(kind.to_string()),
            CardKindError::Reserved => EditError::ReservedKind,
        })
    }

    pub fn remove_card(&mut self, index: usize) -> Option<Card> {
        if index >= self.cards().len() {
            return None;
        }
        Some(self.cards_vec_mut().remove(index))
    }

    /// Replace the `$kind` of the composable card at `index`.
    ///
    /// Only the `$kind` metadata changes; the payload and body are untouched
    /// (field-bag semantics). Old-schema fields linger in the bag; new-schema
    /// fields are absent until set explicitly. Schema migration is the caller's
    /// responsibility — this is a structural primitive.
    ///
    /// Returns [`EditError::IndexOutOfRange`], [`EditError::InvalidKindName`],
    /// or [`EditError::ReservedKind`] on constraint violations.
    pub fn set_card_kind(
        &mut self,
        index: usize,
        new_kind: impl Into<String>,
    ) -> Result<(), EditError> {
        let new_kind = new_kind.into();
        validate_composable_kind(&new_kind).map_err(|e| match e {
            CardKindError::InvalidName => EditError::InvalidKindName(new_kind.clone()),
            CardKindError::Reserved => EditError::ReservedKind,
        })?;
        let len = self.cards().len();
        let card = self
            .card_mut(index)
            .ok_or(EditError::IndexOutOfRange { index, len })?;
        card.payload_mut().set_kind(new_kind);
        Ok(())
    }

    /// Move card at `from` to position `to`. No-op when `from == to`.
    /// Either index out of range → [`EditError::IndexOutOfRange`].
    pub fn move_card(&mut self, from: usize, to: usize) -> Result<(), EditError> {
        let len = self.cards().len();
        if from >= len {
            return Err(EditError::IndexOutOfRange { index: from, len });
        }
        if to >= len {
            return Err(EditError::IndexOutOfRange { index: to, len });
        }
        if from == to {
            return Ok(());
        }
        let card = self.cards_vec_mut().remove(from);
        self.cards_vec_mut().insert(to, card);
        Ok(())
    }
}

impl Card {
    /// Create a composable card with the given kind, no fields, and an empty body.
    pub fn new(kind: impl Into<String>) -> Result<Self, EditError> {
        let kind = kind.into();
        validate_composable_kind(&kind).map_err(|e| match e {
            CardKindError::InvalidName => EditError::InvalidKindName(kind.clone()),
            CardKindError::Reserved => EditError::ReservedKind,
        })?;
        let mut payload = Payload::new();
        payload.set_kind(kind);
        Ok(Card::from_parts(
            payload,
            quillmark_richtext::RichText::empty(),
        ))
    }

    /// Set a payload field, clearing any `!must_fill` marker on that key.
    /// Scalars convert in place (`set_field("qty", 3)`); see the `From`
    /// impls on [`QuillValue`].
    ///
    /// Returns [`EditError::InvalidFieldName`] when `name` does not match
    /// `[A-Za-z_][A-Za-z0-9_]*`.
    pub fn set_field(&mut self, name: &str, value: impl Into<QuillValue>) -> Result<(), EditError> {
        let value = value.into();
        check_field(name, value.as_json())?;
        self.payload_mut().insert(name.to_string(), value);
        Ok(())
    }

    /// Set a payload field and mark it as a `!must_fill` placeholder.
    /// `Null` emits as `key: !must_fill`; scalars/sequences as `key: !must_fill <value>`.
    /// Same validation as [`Card::set_field`].
    pub fn set_fill(&mut self, name: &str, value: impl Into<QuillValue>) -> Result<(), EditError> {
        let value = value.into();
        check_field(name, value.as_json())?;
        self.payload_mut().insert_fill(name.to_string(), value);
        Ok(())
    }

    /// Set several payload fields atomically, clearing any `!must_fill`
    /// marker on each key. The whole batch is validated first — on any
    /// violation nothing is applied and every offending field is reported
    /// as a `(name, error)` pair, so a caller feeding externally-sourced
    /// names (database columns, form keys) sees all violations in one
    /// pass instead of fix-rerun-repeat. Per-field rules are those of
    /// [`Card::set_field`]; insertion order follows the iterator, and a
    /// repeated name behaves like repeated `set_field` calls (last value
    /// wins, first position kept).
    pub fn set_fields<K, V, I>(&mut self, fields: I) -> Result<(), Vec<(String, EditError)>>
    where
        K: Into<String>,
        V: Into<QuillValue>,
        I: IntoIterator<Item = (K, V)>,
    {
        let fields: Vec<(String, QuillValue)> = fields
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        let errors: Vec<(String, EditError)> = fields
            .iter()
            .filter_map(|(name, value)| {
                check_field(name, value.as_json())
                    .err()
                    .map(|e| (name.clone(), e))
            })
            .collect();
        if !errors.is_empty() {
            return Err(errors);
        }
        for (name, value) in fields {
            self.payload_mut().insert(name, value);
        }
        Ok(())
    }

    /// Remove a payload field; returns `Ok(None)` if the name is absent.
    /// Same validation as [`Card::set_field`].
    pub fn remove_field(&mut self, name: &str) -> Result<Option<QuillValue>, EditError> {
        if !is_valid_field_name(name) {
            return Err(EditError::InvalidFieldName(name.to_string()));
        }
        Ok(self.payload_mut().remove(name))
    }

    /// Replace the card's opaque `$ext` map wholesale, inserting it at the
    /// canonical position (after `$quill`/`$kind`/`$id`, before user fields)
    /// when none existed. Passing an empty map records an explicit `$ext: {}`.
    ///
    /// `$ext` carries out-of-band consumer state (editor renames, agent
    /// annotations, …) and is stripped from [`Document::to_plate_json`], so a
    /// write here can never affect a render. Any nested comments attached to a
    /// replaced `$ext` are dropped.
    /// Returns [`EditError::ValueTooDeep`] when the map nests past the §8
    /// depth limit — `$ext` never reaches the plate JSON, but it does flow
    /// through the recursive emit and DTO paths, so it carries the same
    /// depth bound as user fields.
    pub fn set_ext(
        &mut self,
        value: serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), EditError> {
        check_meta_depth(&value)?;
        self.payload_mut().set_ext(value);
        Ok(())
    }

    /// Remove the card's `$ext` map *entirely*, returning the previous map if
    /// present. This is a blunt escape hatch — it discards every namespace
    /// (`$ext.editor`, `$ext.agent`, …) at once. To clear consumer
    /// state, prefer [`Card::remove_ext_namespace`], which drops only your
    /// own slot and leaves sibling consumers' state intact.
    pub fn remove_ext(&mut self) -> Option<serde_json::Map<String, serde_json::Value>> {
        self.payload_mut().take_ext()
    }

    /// Merge `value` into the card's `$ext` map under `namespace`, creating
    /// the map when absent and replacing any existing value at that key.
    ///
    /// This is the recommended way to write `$ext`: it preserves sibling
    /// namespaces, so independent consumers keying on their own slot
    /// (`$ext.editor`, `$ext.agent`, …) don't clobber each other.
    /// Returns [`EditError::ValueTooDeep`] when the merged map nests past
    /// the §8 depth limit (see [`Card::set_ext`]); the card's `$ext` is
    /// unchanged on error.
    pub fn set_ext_namespace(
        &mut self,
        namespace: impl Into<String>,
        value: serde_json::Value,
    ) -> Result<(), EditError> {
        let mut map = self.payload_mut().ext().cloned().unwrap_or_default();
        map.insert(namespace.into(), value);
        check_meta_depth(&map)?;
        self.payload_mut().take_ext();
        self.payload_mut().set_ext(map);
        Ok(())
    }

    /// Remove `namespace` from the card's `$ext` map, returning the value
    /// that was stored there (or `None` when the map or the key was absent).
    ///
    /// This is the recommended way to clear `$ext` state: it is the
    /// namespace-scoped inverse of [`Card::set_ext_namespace`] and preserves
    /// sibling namespaces, where [`Card::remove_ext`] would wipe them all.
    /// When removing the last namespace empties the map, the `$ext` entry is
    /// dropped entirely (not left as `$ext: {}`), so
    /// `set_ext_namespace(ns, v)` followed by `remove_ext_namespace(ns)`
    /// restores a card that had no `$ext` to its original state.
    pub fn remove_ext_namespace(&mut self, namespace: &str) -> Option<serde_json::Value> {
        let mut map = self.payload_mut().take_ext()?;
        let removed = map.remove(namespace);
        if !map.is_empty() {
            self.payload_mut().set_ext(map);
        }
        removed
    }

    /// The raw `$seed` map (keyed by card-kind), or `None`. For a parsed,
    /// per-kind overlay, index this map by kind and pass the entry to
    /// [`crate::SeedOverlay::from_json`]. Only the main card carries `$seed`.
    pub fn seed(&self) -> Option<&serde_json::Map<String, serde_json::Value>> {
        self.payload().seed()
    }

    /// Merge a card-kind's seed overlay `value` into the card's `$seed` map
    /// under `card_kind`, creating the map when absent and replacing any
    /// existing overlay for that kind. Sibling kinds are preserved — this is
    /// the per-kind-safe writer, the seed analogue of
    /// [`Card::set_ext_namespace`]. `card_kind` must be a valid, non-reserved
    /// composable kind ([`EditError::InvalidKindName`] / [`EditError::ReservedKind`]
    /// otherwise) — `$seed` is keyed by composable card-kind, unlike the
    /// free-form namespaces of `$ext`. Returns [`EditError::ValueTooDeep`] when
    /// the merged map nests past the §8 depth limit; the card is unchanged on
    /// error.
    pub fn set_seed_namespace(
        &mut self,
        card_kind: impl Into<String>,
        value: serde_json::Value,
    ) -> Result<(), EditError> {
        let card_kind = card_kind.into();
        validate_composable_kind(&card_kind).map_err(|e| match e {
            CardKindError::InvalidName => EditError::InvalidKindName(card_kind.clone()),
            CardKindError::Reserved => EditError::ReservedKind,
        })?;
        let mut map = self.payload_mut().seed().cloned().unwrap_or_default();
        map.insert(card_kind, value);
        check_meta_depth(&map)?;
        self.payload_mut().take_seed();
        self.payload_mut().set_seed(map);
        Ok(())
    }

    /// Remove `card_kind` from the card's `$seed` map, returning the overlay
    /// stored there (or `None`). When removing the last kind empties the map,
    /// the `$seed` entry is dropped entirely (not left as `$seed: {}`).
    /// The seed analogue of [`Card::remove_ext_namespace`].
    pub fn remove_seed_namespace(&mut self, card_kind: &str) -> Option<serde_json::Value> {
        let mut map = self.payload_mut().take_seed()?;
        let removed = map.remove(card_kind);
        if !map.is_empty() {
            self.payload_mut().set_seed(map);
        }
        removed
    }

    /// Set the body corpus directly from a pre-built [`RichText`]. The native
    /// richtext writer: a corpus is valid by construction, so this is
    /// infallible — no markdown import, no schema check. Use it when the caller
    /// already holds a corpus (a decoded canonical-JSON body, another field's
    /// value, an editor's serialized state); use [`replace_body`](Self::replace_body)
    /// to import from an authored markdown string instead.
    pub fn set_body_corpus(&mut self, corpus: RichText) {
        self.overwrite_body(corpus);
    }

    /// Set the body from either accepted richtext encoding — a canonical corpus
    /// **object** (decoded and validated) or an authored markdown **string**
    /// (imported) — the write twin of the `Card.body` read shape
    /// (`RichText | string`). `null` installs the empty corpus. Routes through
    /// the one object-vs-string dispatch (`decode_richtext_value`), so it
    /// stays lossless for corpus-only marks a markdown projection cannot carry
    /// (e.g. `underline`), unlike serializing to markdown and calling
    /// [`replace_body`](Self::replace_body). Prefer the typed
    /// [`set_body_corpus`](Self::set_body_corpus) when the caller already holds a
    /// [`RichText`]; this is for a JSON-valued body crossing a binding boundary.
    pub fn set_body_value(&mut self, value: &serde_json::Value) -> Result<(), EditError> {
        match crate::document::decode_richtext_value(value) {
            Some(result) => {
                let corpus = result.map_err(|e| EditError::BodyDecode(e.into_message()))?;
                self.overwrite_body(corpus);
                Ok(())
            }
            // Neither object nor string: `null` is the empty corpus; anything
            // else is an invalid body value.
            None => match value {
                serde_json::Value::Null => {
                    self.overwrite_body(RichText::empty());
                    Ok(())
                }
                other => Err(EditError::BodyDecode(format!(
                    "expected a richtext corpus object or a markdown string, got {}",
                    match other {
                        serde_json::Value::Bool(_) => "a boolean",
                        serde_json::Value::Number(_) => "a number",
                        serde_json::Value::Array(_) => "an array",
                        _ => "an unsupported value",
                    }
                ))),
            },
        }
    }

    /// Write-time commit: validate and normalize `value` per the field's schema
    /// `type` and store the canonical form. The typed sibling of the opaque
    /// [`set_field`](Self::set_field) — the one write verb for *every* field
    /// type (richtext today, any future corpus model tomorrow), dispatching on
    /// the [`FieldSchema`] rather than growing a per-type method.
    ///
    /// The two write disciplines: [`set_field`](Self::set_field) stores the
    /// value opaquely and defers coercion to render (keystroke-level state,
    /// data-in-flight); `commit_field` canonicalizes now and fails now (an
    /// editor blur/save, an agent write). Neither is forced on the other.
    ///
    /// Behavior by `type`:
    /// - **richtext** — imports a markdown string / adopts a corpus object and
    ///   stores canonical corpus JSON, so identity marks (anchors, island ids)
    ///   live on the stored value from the write; a `richtext(inline)` schema
    ///   rejects a multi-block value with [`EditError::FieldRichtextNotInline`].
    /// - **scalars** (`string`/`integer`/`number`/`boolean`/`datetime`) — stores
    ///   the coerced canonical (`"3"` → `3`), applying only value-parsing
    ///   normalizations; a cross-type value that the render floor would coerce
    ///   (e.g. `1` → `true`) or a shape mismatch fails here instead.
    /// - **array** / **object** — coerces each element/property against the
    ///   element/property schema.
    /// - **null** — passes through unchanged (the null ≡ absent rule); nothing
    ///   is coerced (a richtext `null` reads back as the empty corpus via
    ///   [`field_richtext`](Self::field_richtext)).
    ///
    /// The caller supplies the `schema` because a [`Document`] holds only a
    /// `$quill` *reference*, not the resolved schema; an editor holds it (see
    /// [`crate::TypedEditor`], which resolves the schema per field and calls
    /// this).
    ///
    /// Returns [`EditError::InvalidFieldName`] for a malformed name,
    /// [`EditError::FieldRichtextDecode`] / [`EditError::FieldRichtextNotInline`]
    /// for a richtext field, [`EditError::FieldConform`] for any other type
    /// mismatch, and [`EditError::ValueTooDeep`] when the stored value nests
    /// past the §8 depth limit.
    pub fn commit_field(
        &mut self,
        name: &str,
        value: impl Into<QuillValue>,
        schema: &FieldSchema,
    ) -> Result<(), EditError> {
        let stored = resolve_field_write(name, value.into(), schema)?;
        self.payload_mut().insert(name.to_string(), stored);
        Ok(())
    }

    /// Replace the body from an authored markdown string — the whole-document
    /// (stale-text / LLM / MCP) writer. Imports the markdown, diffs it against
    /// the current body, and rebases surviving identity anchors onto the new
    /// text (cold import + [`diff_import`]). A pathologically over-nested input
    /// (`> MAX_NESTING_DEPTH`) returns [`EditError::BodyImport`] rather than
    /// silently degrading to the empty corpus. To also obtain the text
    /// [`Delta`] for recording into a session change log, call
    /// [`import_body_delta`](Self::import_body_delta).
    pub fn replace_body(&mut self, body: impl Into<String>) -> Result<(), EditError> {
        self.import_body_delta(body).map(|_| ())
    }

    /// Import an authored markdown string into the body and return the text
    /// [`Delta`] from the old body to the new one — the observable form of
    /// [`replace_body`](Self::replace_body). The returned delta is the text
    /// change an editor bridge maps its own positions through across a
    /// whole-document replace. Surviving identity anchors rebase onto the new
    /// text; formatting marks are re-derived by the fresh import. Returns
    /// [`EditError::BodyImport`] on an over-nested input.
    pub fn import_body_delta(&mut self, body: impl Into<String>) -> Result<Delta, EditError> {
        let (corpus, delta) =
            diff_import(self.body(), &body.into()).map_err(EditError::BodyImport)?;
        self.overwrite_body(corpus);
        Ok(delta)
    }

    /// Apply a committed field-change bundle to the body corpus — the native
    /// form-editor writer. Order is text delta → line ops → mark ops, each
    /// followed by normalization ([`RichText::apply_field_change`]); mark
    /// ranges are in post-text-delta coordinates. Returns
    /// [`EditError::CorpusApply`] when an op is out of bounds; the apply is
    /// all-or-nothing ([`RichText::apply_field_change`]), so the body is
    /// unchanged on error — apply the bundle against the body the delta was
    /// computed from.
    pub fn apply_body_change(
        &mut self,
        text_delta: &Delta,
        line_ops: &[LineOp],
        mark_ops: &[MarkOp],
    ) -> Result<(), EditError> {
        self.body_mut()
            .apply_field_change(text_delta, line_ops, mark_ops)
            .map_err(EditError::CorpusApply)
    }

    /// Splice a corpus field-change bundle into a **richtext-valued field**'s
    /// stored corpus — the field-path twin of [`apply_body_change`](Self::apply_body_change),
    /// and what lets identity marks (anchors, island ids) persist on field
    /// content across incremental edits. Decodes the field's canonical corpus,
    /// applies the text delta plus any line/mark ops in the same all-or-nothing
    /// bundle, and re-stores the canonical result.
    ///
    /// Returns [`EditError::FieldRichtextDecode`] when the field is absent or its
    /// stored value is not a richtext corpus (the caller addresses a field it
    /// knows is richtext, exactly as when writing it), and
    /// [`EditError::CorpusApply`] when the bundle applies out of bounds.
    pub fn apply_field_richtext_change(
        &mut self,
        name: &str,
        text_delta: &Delta,
        line_ops: &[LineOp],
        mark_ops: &[MarkOp],
    ) -> Result<(), EditError> {
        let mut corpus = match self.field_richtext(name) {
            Some(Ok(rt)) => rt,
            Some(Err(e)) => {
                return Err(EditError::FieldRichtextDecode {
                    field: name.to_string(),
                    message: e.into_message(),
                })
            }
            None => {
                return Err(EditError::FieldRichtextDecode {
                    field: name.to_string(),
                    message: "field is absent".to_string(),
                })
            }
        };
        corpus
            .apply_field_change(text_delta, line_ops, mark_ops)
            .map_err(EditError::CorpusApply)?;
        let canonical = quillmark_richtext::serial::to_canonical_value(&corpus);
        self.payload_mut()
            .insert(name.to_string(), QuillValue::from_json(canonical));
        Ok(())
    }
}
