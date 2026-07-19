//! Typed mutators for [`Document`] and [`Card`] with invariant enforcement.
//!
//! Every successful mutator leaves the document with every user field name
//! matching `[A-Za-z_][A-Za-z0-9_]*` and every composable `$kind` passing
//! `meta::is_valid_kind_name`, so the result is safely serializable via
//! [`Document::to_plate_json`]. Mutators never modify `warnings` — those
//! are immutable parse-time observations.
//!
//! Payload/body mutators (field store/fill/remove, `$ext` and `$seed`
//! namespace writers, body replacement) live on [`Card`]; [`Document`] keeps
//! document-level ops (quill-ref, push/insert/remove/move card).
//!
//! The `$ext` mutators carry no field-name invariant ($ext is an opaque
//! mapping that never reaches the plate JSON backends consume), but they do
//! enforce the §8 value-depth bound: `$ext` flows through the recursive
//! emit and DTO paths like any other value.

use unicode_normalization::UnicodeNormalization;

use quillmark_content::delta::diff_import;
use quillmark_content::import::ImportError;
use quillmark_content::{ApplyError, Delta, LineOp, MarkOp, Content};

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

    /// A typed write ([`TypedWriter::set`](crate::TypedWriter::set) /
    /// [`CardWriter::set`](crate::CardWriter::set)) addressed a well-formed name
    /// that the bound schema does not declare (or a card whose `$kind` carries
    /// no schema). The typed path resolves every name to a schema type, so an
    /// undeclared name is a typo, not a fallback — it fails here instead of
    /// landing silently in the opaque store. Reach for the raw
    /// [`Card::store_field`](Card::store_field) when opaque storage is the intent.
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

    /// Markdown import failed — the content codec rejected the input for a body
    /// *or* a field path (e.g. container nesting past
    /// [`MAX_NESTING_DEPTH`](quillmark_content::MAX_NESTING_DEPTH)). Returned
    /// instead of silently degrading the target to empty on a rejected import.
    #[error("markdown import failed: {0}")]
    Import(ImportError),

    /// A richtext field value in the content-or-markdown encoding could not be
    /// decoded: a JSON object that is not a canonical richtext content, a
    /// markdown string that failed to import, or a shape that is neither
    /// object, string, nor null. Returned by
    /// [`Card::commit_field`](Card::commit_field) on a richtext field, by
    /// [`Card::revise_field`](Card::revise_field) on a present non-content field,
    /// and by [`Card::apply_field_richtext_change`](Card::apply_field_richtext_change).
    #[error("richtext field '{field}' decode failed: {message}")]
    FieldRichtextDecode { field: String, message: String },

    /// A richtext field written under the `richtext(inline)` constraint decoded
    /// to a multi-block content (more than one line, a container, or an island).
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

    /// A content field-change bundle (text delta, line ops, mark ops) applied
    /// out of bounds or broke an invariant normalization could not repair.
    #[error("content apply failed: {0:?}")]
    ContentApply(ApplyError),
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
            EditError::Import(_) => "Import",
            EditError::FieldRichtextDecode { .. } => "FieldRichtextDecode",
            EditError::FieldRichtextNotInline(_) => "FieldRichtextNotInline",
            EditError::FieldConform { .. } => "FieldConform",
            EditError::ContentApply(_) => "ContentApply",
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

/// Map a [`FieldViolation`] to the mutator error surface — the single
/// translation the `Card` mutators and the validating
/// [`Payload::insert`](crate::document::Payload::insert) both route through.
pub(crate) fn edit_error_from_violation(name: &str, v: FieldViolation) -> EditError {
    match v {
        FieldViolation::InvalidName => EditError::InvalidFieldName(name.to_string()),
        FieldViolation::TooDeep => EditError::ValueTooDeep {
            max: crate::document::limits::MAX_YAML_DEPTH,
        },
    }
}

/// Validate a user field at the mutator boundary, mapping a violation to the
/// mutator error surface.
fn check_field(name: &str, value: &serde_json::Value) -> Result<(), EditError> {
    validate_field(name, value).map_err(|v| edit_error_from_violation(name, v))
}

/// Depth-bound an out-of-band meta map (`$ext` / `$seed`). Both ride the same
/// recursive emit/DTO paths, so they carry the same §8 depth bound.
fn check_meta_depth(map: &serde_json::Map<String, serde_json::Value>) -> Result<(), EditError> {
    crate::value::depth_check_meta_map(map.clone(), |max| EditError::ValueTooDeep { max })?;
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
/// all-or-nothing [`TypedWriter::set_all`](crate::TypedWriter::set_all).
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
            quillmark_content::Content::empty(),
        ))
    }

    /// Store a payload field verbatim, clearing any `!must_fill` marker on that
    /// key — the opaque store (**store** = verbatim, coercion deferred to render;
    /// contrast the typed [`TypedWriter::set`](crate::TypedWriter::set)). Scalars
    /// convert in place (`store_field("qty", 3)`); see the `From` impls on
    /// [`QuillValue`].
    ///
    /// Returns [`EditError::InvalidFieldName`] when `name` does not match
    /// `[A-Za-z_][A-Za-z0-9_]*`.
    pub fn store_field(&mut self, name: &str, value: impl Into<QuillValue>) -> Result<(), EditError> {
        self.payload_mut()
            .insert(name.to_string(), value.into())
            .map_err(|v| edit_error_from_violation(name, v))?;
        Ok(())
    }

    /// Store a payload field verbatim and mark it as a `!must_fill` placeholder.
    /// `Null` emits as `key: !must_fill`; scalars/sequences as `key: !must_fill <value>`.
    /// The opaque store's fill variant (quill-free, verbatim); same validation as
    /// [`Card::store_field`].
    pub fn store_fill(&mut self, name: &str, value: impl Into<QuillValue>) -> Result<(), EditError> {
        self.payload_mut()
            .insert_fill(name.to_string(), value.into())
            .map_err(|v| edit_error_from_violation(name, v))?;
        Ok(())
    }

    /// Store several payload fields verbatim and atomically, clearing any
    /// `!must_fill` marker on each key — the opaque store's batch (contrast the
    /// typed [`TypedWriter::set_all`](crate::TypedWriter::set_all)). The whole
    /// batch is validated first — on any violation nothing is applied and every
    /// offending field is reported as a `(name, error)` pair, so a caller feeding
    /// externally-sourced names (database columns, form keys) sees all violations
    /// in one pass instead of fix-rerun-repeat. Per-field rules are those of
    /// [`Card::store_field`]; insertion order follows the iterator, and a
    /// repeated name behaves like repeated `store_field` calls (last value
    /// wins, first position kept).
    pub fn store_fields<K, V, I>(&mut self, fields: I) -> Result<(), Vec<(String, EditError)>>
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
        // Batch validated above; apply through the unchecked insert so the
        // whole-batch check is not re-run per field.
        for (name, value) in fields {
            self.payload_mut().insert_unchecked(name, value);
        }
        Ok(())
    }

    /// Remove a payload field; returns `Ok(None)` if the name is absent.
    /// Removal has no lane — the one verb serves every write path. Same
    /// validation as [`Card::store_field`].
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
    ///
    /// Quill-free and never coerced — an opaque `store_*` verb by the vocabulary
    /// rule, not a typed `set`.
    pub fn store_ext(
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
    /// the §8 depth limit (see [`Card::store_ext`]); the card's `$ext` is
    /// unchanged on error. Quill-free and never coerced — an opaque `store_*`
    /// verb.
    pub fn store_ext_namespace(
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
    /// namespace-scoped inverse of [`Card::store_ext_namespace`] and preserves
    /// sibling namespaces, where [`Card::remove_ext`] would wipe them all.
    /// When removing the last namespace empties the map, the `$ext` entry is
    /// dropped entirely (not left as `$ext: {}`), so
    /// `store_ext_namespace(ns, v)` followed by `remove_ext_namespace(ns)`
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
    /// [`Card::store_ext_namespace`]. `card_kind` must be a valid, non-reserved
    /// composable kind ([`EditError::InvalidKindName`] / [`EditError::ReservedKind`]
    /// otherwise) — `$seed` is keyed by composable card-kind, unlike the
    /// free-form namespaces of `$ext`. Returns [`EditError::ValueTooDeep`] when
    /// the merged map nests past the §8 depth limit; the card is unchanged on
    /// error. Quill-free and never coerced — an opaque `store_*` verb.
    pub fn store_seed_namespace(
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

    /// Install the body content directly from a pre-built [`Content`] — **value
    /// semantics**, the native richtext writer. A content is valid by
    /// construction, so this is infallible: no markdown import, no diff, no
    /// schema check; the identity anchors of the previous body are *gone*
    /// (install-this-exact-value, so a `to_markdown → install` round-trip cannot
    /// resurrect them). Use it when the caller already holds a content (a decoded
    /// canonical-JSON body, another field's value, an editor's serialized state).
    /// For "here's new authored markdown," use [`revise_body`](Self::revise_body),
    /// which rebases surviving anchors; the cold-import path is spelled at the
    /// call site as `install_body(import_body(md)?)`.
    pub fn install_body(&mut self, content: Content) {
        self.overwrite_body(content);
    }

    /// Install a richtext field's content directly from a pre-built [`Content`]
    /// — the field-level twin of [`install_body`](Self::install_body). Value
    /// semantics: stores the canonical content JSON verbatim (identity marks and
    /// content-only marks such as `underline` intact), no diff, no schema check
    /// (schema-blind, like [`apply_field_richtext_change`](Self::apply_field_richtext_change)
    /// — [`commit_field`](Self::commit_field) is the typed door). Returns
    /// [`EditError::InvalidFieldName`] for a malformed name.
    pub fn install_field(&mut self, name: &str, content: Content) -> Result<(), EditError> {
        if !is_valid_field_name(name) {
            return Err(EditError::InvalidFieldName(name.to_string()));
        }
        self.store_field_content(name, &content);
        Ok(())
    }

    /// Store `content` as the canonical content-JSON value of field `name` — the
    /// one place a richtext field's content is committed to the payload, shared by
    /// [`install_field`](Self::install_field), [`revise_field`](Self::revise_field),
    /// and [`apply_field_richtext_change`](Self::apply_field_richtext_change).
    /// Assumes `name` is already validated (all three callers check it or resolve
    /// an existing field first).
    fn store_field_content(&mut self, name: &str, content: &Content) {
        let canonical = quillmark_content::serial::to_canonical_value(content);
        self.payload_mut()
            .insert_unchecked(name.to_string(), QuillValue::from_json(canonical));
    }

    /// Write-time commit: validate and normalize `value` per the field's schema
    /// `type` and store the canonical form. The typed sibling of the opaque
    /// [`store_field`](Self::store_field) — the one write verb for *every* field
    /// type (richtext today, any future content model tomorrow), dispatching on
    /// the [`FieldSchema`] rather than growing a per-type method.
    ///
    /// The two write disciplines: [`store_field`](Self::store_field) stores the
    /// value opaquely and defers coercion to render (keystroke-level state,
    /// data-in-flight); `commit_field` canonicalizes now and fails now (an
    /// editor blur/save, an agent write). Neither is forced on the other.
    ///
    /// Behavior by `type`:
    /// - **richtext** — imports a markdown string / adopts a content object and
    ///   stores canonical content JSON, so identity marks (anchors, island ids)
    ///   live on the stored value from the write; a `richtext(inline)` schema
    ///   rejects a multi-block value with [`EditError::FieldRichtextNotInline`].
    /// - **scalars** (`string`/`integer`/`number`/`boolean`/`datetime`) — stores
    ///   the coerced canonical (`"3"` → `3`), applying only value-parsing
    ///   normalizations; a cross-type value that the render floor would coerce
    ///   (e.g. `1` → `true`) or a shape mismatch fails here instead.
    /// - **array** / **object** — coerces each element/property against the
    ///   element/property schema.
    /// - **null** — passes through unchanged (the null ≡ absent rule); nothing
    ///   is coerced (a richtext `null` reads back as the empty content via
    ///   [`field_richtext`](Self::field_richtext)).
    ///
    /// The caller supplies the `schema` because a [`Document`] holds only a
    /// `$quill` *reference*, not the resolved schema; an editor holds it (see
    /// [`crate::TypedWriter`], which resolves the schema per field and calls
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
        // `resolve_field_write` already validated name + stored-value depth.
        self.payload_mut().insert_unchecked(name.to_string(), stored);
        Ok(())
    }

    /// Revise the body from an authored markdown string — **edit semantics**,
    /// the whole-document (stale-text / LLM / MCP) writer, and the receipt-
    /// returning default write path. Imports the markdown, diffs it against the
    /// current body, and rebases surviving identity anchors onto the new text
    /// (cold import + [`diff_import`]), then returns the text [`Delta`] from the
    /// old body to the new one — the change an editor bridge maps its own
    /// positions through across a whole-document replace ([`Delta::map_pos`]).
    /// Surviving identity anchors rebase; formatting marks are re-derived by the
    /// fresh import. A pathologically over-nested input (`> MAX_NESTING_DEPTH`)
    /// returns [`EditError::Import`] rather than silently degrading to the
    /// empty content. Discard the receipt with `let _ = card.revise_body(md)?;`
    /// when caret stability is not needed.
    pub fn revise_body(&mut self, body: impl Into<String>) -> Result<Delta, EditError> {
        let (content, delta) =
            diff_import(self.body(), &body.into()).map_err(EditError::Import)?;
        self.overwrite_body(content);
        Ok(delta)
    }

    /// Decode the field's current content (an absent field imports from empty),
    /// diff `body` against it so surviving anchors rebase, and return the new
    /// content with its text [`Delta`] — the shared preamble of
    /// [`revise_field`](Self::revise_field) and
    /// [`revise_field_checked`](Self::revise_field_checked). Neither stores; the
    /// caller lands the diffed content (raw, or schema-checked).
    fn diff_field(
        &self,
        name: &str,
        body: impl Into<String>,
    ) -> Result<(Content, Delta), EditError> {
        if !is_valid_field_name(name) {
            return Err(EditError::InvalidFieldName(name.to_string()));
        }
        let base = match self.field_richtext(name) {
            Some(Ok(rt)) => rt,
            Some(Err(e)) => {
                return Err(EditError::FieldRichtextDecode {
                    field: name.to_string(),
                    message: e.into_message(),
                })
            }
            None => Content::empty(),
        };
        diff_import(&base, &body.into()).map_err(EditError::Import)
    }

    /// Revise a richtext field from an authored markdown string — the
    /// field-level twin of [`revise_body`](Self::revise_body), and the
    /// field-level `diff_import` the write surface previously lacked (the only
    /// field-content writers were the cold [`commit_field`](Self::commit_field)
    /// and the splice [`apply_field_richtext_change`](Self::apply_field_richtext_change),
    /// so an LLM rewriting a richtext field's markdown had no anchor-preserving
    /// path). Decodes the field's current content as the diff base (an **absent**
    /// field cold-imports from empty), rebases surviving anchors onto the new
    /// text, re-stores the canonical content, and returns the text [`Delta`].
    ///
    /// Schema-blind by design — the content-writer stratum splices without the
    /// quill (like [`apply_field_richtext_change`](Self::apply_field_richtext_change));
    /// [`commit_field`](Self::commit_field) is the typed door that enforces
    /// `richtext(inline)`, and a violation otherwise surfaces at validate/render.
    ///
    /// Returns [`EditError::InvalidFieldName`] for a malformed name,
    /// [`EditError::FieldRichtextDecode`] when the field is present but is not a
    /// richtext content (a scalar a `store_field` wrote), and
    /// [`EditError::Import`] on an over-nested markdown input.
    pub fn revise_field(&mut self, name: &str, body: impl Into<String>) -> Result<Delta, EditError> {
        let (content, delta) = self.diff_field(name, body)?;
        self.store_field_content(name, &content);
        Ok(delta)
    }

    /// Revise a richtext field from markdown **with schema enforcement** — the
    /// typed *and* anchor-preserving field write that neither
    /// [`revise_field`](Self::revise_field) nor [`commit_field`](Self::commit_field)
    /// provides alone. [`revise_field`](Self::revise_field) rebases anchors but is
    /// schema-blind; [`commit_field`](Self::commit_field) enforces the schema but
    /// cold-imports (the previous value's anchors are gone). This does both: diff
    /// the markdown against the field's current content so surviving anchors rebase
    /// (as [`revise_field`](Self::revise_field)), then enforce `schema` on the
    /// *diffed result* through the same typed-conform path
    /// [`commit_field`](Self::commit_field) runs — so a `richtext(inline)` schema
    /// rejects a multi-block result with [`EditError::FieldRichtextNotInline`],
    /// the error surface unchanged, while the anchors survive. Returns the text
    /// [`Delta`] receipt.
    ///
    /// The primitive that [`TypedWriter::revise_field`](crate::TypedWriter::revise_field)
    /// and [`CardWriter::revise_field`](crate::CardWriter::revise_field) wrap: they
    /// resolve `schema` from the bound quill and call here. The schema runs on the
    /// content the diff produced, so a non-richtext `schema` (nothing to preserve)
    /// fails with the same [`EditError::FieldConform`]
    /// [`commit_field`](Self::commit_field) would raise.
    ///
    /// Errors: [`EditError::InvalidFieldName`], [`EditError::FieldRichtextDecode`]
    /// when the field is present but not a richtext content, [`EditError::Import`]
    /// on an over-nested markdown input, and the conform errors of
    /// [`commit_field`](Self::commit_field) on the diffed result. On any error the
    /// field is unchanged.
    pub fn revise_field_checked(
        &mut self,
        name: &str,
        body: impl Into<String>,
        schema: &FieldSchema,
    ) -> Result<Delta, EditError> {
        let (content, delta) = self.diff_field(name, body)?;
        // Enforce `schema` on the diffed (anchor-rebased) content through the same
        // typed path `commit_field` uses: re-canonicalizing a content object keeps
        // its identity marks (`decode_richtext_value`), so the inline check fires
        // on the value anchors survived onto and the error surface is identical.
        let canonical = quillmark_content::serial::to_canonical_value(&content);
        let stored = resolve_field_write(name, QuillValue::from_json(canonical), schema)?;
        self.payload_mut().insert_unchecked(name.to_string(), stored);
        Ok(delta)
    }

    /// Apply a committed field-change bundle to the body content — the native
    /// form-editor writer. Order is text delta → line ops → mark ops, then one
    /// terminal normalization ([`Content::apply_field_change`]); mark ranges are
    /// in final-text coordinates. Returns
    /// [`EditError::ContentApply`] when an op is out of bounds; the apply is
    /// all-or-nothing ([`Content::apply_field_change`]), so the body is
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
            .map_err(EditError::ContentApply)
    }

    /// Splice a content field-change bundle into a **richtext-valued field**'s
    /// stored content — the field-path twin of [`apply_body_change`](Self::apply_body_change),
    /// and what lets identity marks (anchors, island ids) persist on field
    /// content across incremental edits. Decodes the field's canonical content,
    /// applies the text delta plus any line/mark ops in the same all-or-nothing
    /// bundle, and re-stores the canonical result.
    ///
    /// Returns [`EditError::FieldRichtextDecode`] when the field is absent or its
    /// stored value is not a richtext content (the caller addresses a field it
    /// knows is richtext, exactly as when writing it), and
    /// [`EditError::ContentApply`] when the bundle applies out of bounds.
    pub fn apply_field_richtext_change(
        &mut self,
        name: &str,
        text_delta: &Delta,
        line_ops: &[LineOp],
        mark_ops: &[MarkOp],
    ) -> Result<(), EditError> {
        let mut content = match self.field_richtext(name) {
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
        content
            .apply_field_change(text_delta, line_ops, mark_ops)
            .map_err(EditError::ContentApply)?;
        self.store_field_content(name, &content);
        Ok(())
    }
}
