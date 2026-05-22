//! Typed mutators for [`Document`] and [`Card`] with invariant enforcement.
//!
//! Every successful mutator leaves the document with every user field name
//! matching `[a-z_][a-z0-9_]*` and every composable `$kind` passing
//! `meta::is_valid_kind_name`, so the result is safely serializable via
//! [`Document::to_plate_json`]. Mutators never modify `warnings` — those
//! are immutable parse-time observations.
//!
//! Payload/body mutators live on [`Card`] (`set_field`, `set_fill`,
//! `remove_field`, `replace_body`); [`Document`] keeps document-level ops
//! (quill-ref, push/insert/remove/move card).

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

    pub fn push_card(&mut self, card: Card) {
        self.cards_vec_mut().push(card);
    }

    /// Insert a composable card at `index` (`index > len` → [`EditError::IndexOutOfRange`]).
    pub fn insert_card(&mut self, index: usize, card: Card) -> Result<(), EditError> {
        let len = self.cards().len();
        if index > len {
            return Err(EditError::IndexOutOfRange { index, len });
        }
        self.cards_vec_mut().insert(index, card);
        Ok(())
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

    pub fn replace_body(&mut self, body: impl Into<String>) {
        self.overwrite_body(body.into());
    }
}
