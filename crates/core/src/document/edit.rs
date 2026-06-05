//! Typed mutators for [`Document`] and [`Card`] with invariant enforcement.
//!
//! Every successful mutator leaves the document with every user field name
//! matching `[a-z_][a-z0-9_]*` and every composable `$kind` passing
//! `meta::is_valid_kind_name`, so the result is safely serializable via
//! [`Document::to_plate_json`]. Mutators never modify `warnings` — those
//! are immutable parse-time observations.
//!
//! Payload/body mutators live on [`Card`] (`set_field`, `set_fill`,
//! `remove_field`, `set_ext`, `remove_ext`, `set_ext_namespace`,
//! `remove_ext_namespace`, `replace_body`); [`Document`] keeps
//! document-level ops (quill-ref, push/insert/remove/move card).
//!
//! The `$ext` mutators are unguarded: `$ext` is an opaque mapping that
//! never reaches the plate JSON backends consume, so there is no field-name
//! or kind invariant to enforce — only that the value is a mapping, which
//! the types already guarantee.

use unicode_normalization::UnicodeNormalization;

use crate::document::meta::{validate_composable_kind, CardKindError};
use crate::document::{Card, Document, Payload};
use crate::value::QuillValue;
use crate::version::QuillReference;

/// `true` if `name` matches `[a-z_][a-z0-9_]*` after NFC normalisation.
pub fn is_valid_field_name(name: &str) -> bool {
    let normalized: String = name.nfc().collect();
    if normalized.is_empty() {
        return false;
    }
    let mut chars = normalized.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_lowercase() && first != '_' {
        return false;
    }
    for ch in chars {
        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && ch != '_' {
            return false;
        }
    }
    true
}

/// Errors returned by document and card mutators.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EditError {
    #[error("invalid field name '{0}': must match [a-z_][a-z0-9_]*")]
    InvalidFieldName(String),

    #[error("invalid card kind '{0}': must match [a-z_][a-z0-9_]*")]
    InvalidKindName(String),

    #[error("card kind 'main' is reserved for the document root")]
    ReservedKind,

    #[error("index {index} is out of range (len = {len})")]
    IndexOutOfRange { index: usize, len: usize },
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

    /// Set a payload field, clearing any `!fill` marker on that key.
    ///
    /// Returns [`EditError::InvalidFieldName`] when `name` does not match
    /// `[a-z_][a-z0-9_]*`.
    pub fn set_field(&mut self, name: &str, value: QuillValue) -> Result<(), EditError> {
        if !is_valid_field_name(name) {
            return Err(EditError::InvalidFieldName(name.to_string()));
        }
        self.payload_mut().insert(name.to_string(), value);
        Ok(())
    }

    /// Set a payload field and mark it as a `!fill` placeholder.
    /// `Null` emits as `key: !fill`; scalars/sequences as `key: !fill <value>`.
    /// Same validation as [`Card::set_field`].
    pub fn set_fill(&mut self, name: &str, value: QuillValue) -> Result<(), EditError> {
        if !is_valid_field_name(name) {
            return Err(EditError::InvalidFieldName(name.to_string()));
        }
        self.payload_mut().insert_fill(name.to_string(), value);
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
    pub fn set_ext(&mut self, value: serde_json::Map<String, serde_json::Value>) {
        self.payload_mut().set_ext(value);
    }

    /// Remove the card's `$ext` map *entirely*, returning the previous map if
    /// present. This is a blunt escape hatch — it discards every namespace
    /// (`$ext.presentation`, `$ext.agent`, …) at once. To clear consumer
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
    /// (`$ext.presentation`, `$ext.agent`, …) don't clobber each other.
    pub fn set_ext_namespace(&mut self, namespace: impl Into<String>, value: serde_json::Value) {
        let mut map = self.payload_mut().take_ext().unwrap_or_default();
        map.insert(namespace.into(), value);
        self.payload_mut().set_ext(map);
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

    pub fn replace_body(&mut self, body: impl Into<String>) {
        self.overwrite_body(body.into());
    }
}
