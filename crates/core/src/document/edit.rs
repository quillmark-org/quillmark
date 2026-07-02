//! Typed mutators for [`Document`] and [`Card`] with invariant enforcement.
//!
//! Every successful mutator leaves the document with every user field name
//! matching `[A-Za-z_][A-Za-z0-9_]*` and every composable `$kind` passing
//! `meta::is_valid_kind_name`, so the result is safely serializable via
//! [`Document::to_plate_json`]. Mutators never modify `warnings` — those
//! are immutable parse-time observations.
//!
//! Payload/body mutators live on [`Card`] (`set_field`, `set_fill`,
//! `remove_field`, `set_ext`, `remove_ext`, `set_ext_namespace`,
//! `remove_ext_namespace`, `replace_body`); [`Document`] keeps
//! document-level ops (quill-ref, push/insert/remove/move card).
//!
//! The `$ext` mutators carry no field-name invariant ($ext is an opaque
//! mapping that never reaches the plate JSON backends consume), but they do
//! enforce the §8 value-depth bound: `$ext` flows through the recursive
//! emit and DTO paths like any other value.

use unicode_normalization::UnicodeNormalization;

use crate::document::meta::{validate_composable_kind, CardKindError};
use crate::document::{Card, Document, Payload};
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

    #[error("invalid card kind '{0}': must match [a-z_][a-z0-9_]*")]
    InvalidKindName(String),

    #[error("card kind 'main' is reserved for the document root")]
    ReservedKind,

    #[error("index {index} is out of range (len = {len})")]
    IndexOutOfRange { index: usize, len: usize },

    #[error("value nests deeper than the maximum of {max} levels")]
    ValueTooDeep { max: usize },
}

impl EditError {
    /// The bare variant name (e.g. `"InvalidFieldName"`). Each binding surfaces
    /// it as the `[EditError::<Variant>]` message prefix; defined once here so a
    /// new variant cannot drift across the three binding error mappers.
    pub fn variant_name(&self) -> &'static str {
        match self {
            EditError::InvalidFieldName(_) => "InvalidFieldName",
            EditError::InvalidKindName(_) => "InvalidKindName",
            EditError::ReservedKind => "ReservedKind",
            EditError::IndexOutOfRange { .. } => "IndexOutOfRange",
            EditError::ValueTooDeep { .. } => "ValueTooDeep",
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
        Ok(Card::from_parts(payload, String::new()))
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

    pub fn replace_body(&mut self, body: impl Into<String>) {
        self.overwrite_body(body.into());
    }
}
