//! # Document Editor Surface
//!
//! Typed mutators for [`Document`] and [`Card`] with invariant enforcement.
//!
//! ## Invariants
//!
//! Every successful mutator call leaves the document in a state that:
//! - Contains no reserved key in any card's frontmatter (`BODY`, `CARDS`, `QUILL`, `KIND`).
//! - Has every composable `card.tag()` passing `sentinel::is_valid_tag_name`.
//! - Can be safely serialized via [`Document::to_plate_json`].
//!
//! **Mutators never modify `warnings`.**  Warnings are parse-time observations
//! and remain stable for the lifetime of the document.
//!
//! ## Surface
//!
//! Frontmatter and body mutators live on [`Card`]:
//! `doc.main_mut().set_field(…)`, `doc.main_mut().replace_body(…)`,
//! `doc.cards_mut()[i].set_field(…)`. [`Document`] keeps only document-level
//! operations (quill-ref, push/insert/remove/move card).

use unicode_normalization::UnicodeNormalization;

use crate::document::sentinel::is_valid_tag_name;
use crate::document::{Card, Document, Frontmatter, Sentinel};
use crate::value::QuillValue;
use crate::version::QuillReference;

// ── Reserved names ──────────────────────────────────────────────────────────

/// Reserved field names that may not appear in any `Card`'s frontmatter.
/// These are the sentinel keys whose presence in user-visible fields would
/// corrupt the plate wire format or the parser's structural invariants.
pub const RESERVED_NAMES: &[&str] = &["BODY", "CARDS", "QUILL", "KIND"];

/// Returns `true` if `name` is one of the four reserved sentinel names.
#[inline]
pub fn is_reserved_name(name: &str) -> bool {
    RESERVED_NAMES.contains(&name)
}

// ── Field name validation ───────────────────────────────────────────────────

/// Returns `true` if `name` is a valid frontmatter / card field name.
///
/// A valid field name matches `[a-z_][a-z0-9_]*` after NFC normalisation.
/// Upper-case identifiers are intentionally excluded; they are reserved for
/// sentinel keys (`QUILL`, `KIND`, `BODY`, `CARDS`).
pub fn is_valid_field_name(name: &str) -> bool {
    // NFC-normalize first so that, e.g., composed vs decomposed forms compare equal.
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

// ── EditError ────────────────────────────────────────────────────────────────

/// Errors returned by document and card mutators.
///
/// `EditError` is distinct from [`crate::error::ParseError`]: it carries no
/// source-location information because edits happen after parsing.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EditError {
    /// The supplied name is one of the four reserved sentinel keys
    /// (`BODY`, `CARDS`, `QUILL`, `KIND`).
    #[error("reserved name '{0}' cannot be used as a field name")]
    ReservedName(String),

    /// The supplied name does not match `[a-z_][a-z0-9_]*`.
    #[error("invalid field name '{0}': must match [a-z_][a-z0-9_]*")]
    InvalidFieldName(String),

    /// The supplied tag does not match `[a-z_][a-z0-9_]*`.
    #[error("invalid tag name '{0}': must match [a-z_][a-z0-9_]*")]
    InvalidTagName(String),

    /// A card index was out of the valid range.
    #[error("index {index} is out of range (len = {len})")]
    IndexOutOfRange { index: usize, len: usize },
}

// ── impl Document ────────────────────────────────────────────────────────────

impl Document {
    /// Replace the QUILL reference on the main card's sentinel.
    ///
    /// # Invariants enforced
    ///
    /// The `QuillReference` type guarantees structural validity; no further
    /// checks are needed here.
    ///
    /// # Warnings
    ///
    /// This method never modifies `warnings`.
    pub fn set_quill_ref(&mut self, reference: QuillReference) {
        self.main_mut().replace_sentinel(Sentinel::Main(reference));
    }

    // ── Card mutators ────────────────────────────────────────────────────────

    /// Return a mutable reference to the composable card at `index`, or `None`
    /// if out of range.
    ///
    /// # Warnings
    ///
    /// This method never modifies `warnings`.
    pub fn card_mut(&mut self, index: usize) -> Option<&mut Card> {
        self.cards_mut().get_mut(index)
    }

    /// Append a composable card to the end of the card list.
    ///
    /// # Invariants
    ///
    /// `card.sentinel()` must be [`Sentinel::Inline`]; a main card cannot be
    /// appended as a composable card. Debug assert.
    ///
    /// # Warnings
    ///
    /// This method never modifies `warnings`.
    pub fn push_card(&mut self, card: Card) {
        debug_assert!(
            !card.sentinel().is_main(),
            "cannot push a Main-sentinel card as a composable card"
        );
        self.cards_vec_mut().push(card);
    }

    /// Insert a composable card at `index`.
    ///
    /// # Invariants enforced
    ///
    /// `index` must be in `0..=len`.  An `index > len` returns
    /// [`EditError::IndexOutOfRange`].
    ///
    /// # Warnings
    ///
    /// This method never modifies `warnings`.
    pub fn insert_card(&mut self, index: usize, card: Card) -> Result<(), EditError> {
        debug_assert!(
            !card.sentinel().is_main(),
            "cannot insert a Main-sentinel card as a composable card"
        );
        let len = self.cards().len();
        if index > len {
            return Err(EditError::IndexOutOfRange { index, len });
        }
        self.cards_vec_mut().insert(index, card);
        Ok(())
    }

    /// Remove and return the composable card at `index`, or `None` if out of range.
    ///
    /// # Warnings
    ///
    /// This method never modifies `warnings`.
    pub fn remove_card(&mut self, index: usize) -> Option<Card> {
        if index >= self.cards().len() {
            return None;
        }
        Some(self.cards_vec_mut().remove(index))
    }

    /// Replace the tag (sentinel) of the composable card at `index`.
    ///
    /// **Field-bag semantics.** This mutates only the sentinel; the card's
    /// frontmatter and body are untouched. After the call:
    ///
    /// - Fields valid under both old and new schemas round-trip unchanged.
    /// - Fields only in the old schema linger in the bag (silently ignored
    ///   by `Quill::form` and `validate_document`, but still emitted by
    ///   `to_markdown()`).
    /// - Fields only in the new schema are absent — surfaced as `Default`
    ///   or `Missing` by `Quill::form`, and `MissingRequired` by
    ///   `validate_document`.
    ///
    /// Schema-aware migration (clearing orphans, applying defaults, etc.) is
    /// the caller's responsibility — `set_card_tag` is a structural primitive.
    ///
    /// # Invariants enforced
    ///
    /// - `index` must be in `0..len`. Out of range returns
    ///   [`EditError::IndexOutOfRange`].
    /// - `new_tag` must match `[a-z_][a-z0-9_]*`. Invalid tags return
    ///   [`EditError::InvalidTagName`].
    ///
    /// # Warnings
    ///
    /// This method never modifies `warnings`.
    pub fn set_card_tag(
        &mut self,
        index: usize,
        new_tag: impl Into<String>,
    ) -> Result<(), EditError> {
        let new_tag = new_tag.into();
        if !is_valid_tag_name(&new_tag) {
            return Err(EditError::InvalidTagName(new_tag));
        }
        let len = self.cards().len();
        let card = self
            .card_mut(index)
            .ok_or(EditError::IndexOutOfRange { index, len })?;
        card.replace_sentinel(Sentinel::Inline(new_tag));
        Ok(())
    }

    /// Move the composable card at `from` to position `to`.
    ///
    /// If `from == to`, this is a no-op and returns `Ok(())`.
    ///
    /// # Invariants enforced
    ///
    /// Both `from` and `to` must be in `0..len`.  Either being out of range
    /// returns [`EditError::IndexOutOfRange`] with the offending index.
    ///
    /// # Warnings
    ///
    /// This method never modifies `warnings`.
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

// ── impl Card ────────────────────────────────────────────────────────────────

impl Card {
    /// Create a new, empty composable card with the given tag.
    ///
    /// # Invariants enforced
    ///
    /// `tag` must match `[a-z_][a-z0-9_]*`.  An invalid tag returns
    /// [`EditError::InvalidTagName`].
    ///
    /// The new card has no fields and an empty body.
    pub fn new(tag: impl Into<String>) -> Result<Self, EditError> {
        let tag = tag.into();
        if !is_valid_tag_name(&tag) {
            return Err(EditError::InvalidTagName(tag));
        }
        Ok(Card::new_with_sentinel(
            Sentinel::Inline(tag),
            Frontmatter::new(),
            String::new(),
        ))
    }

    /// Set a frontmatter field by name. Always clears the `!fill` marker for
    /// that key — the "user filled in" path.
    ///
    /// # Invariants enforced
    ///
    /// - `name` must not be one of the reserved sentinel names.
    ///   Returns [`EditError::ReservedName`].
    /// - `name` must match `[a-z_][a-z0-9_]*` after NFC normalisation.
    ///   Returns [`EditError::InvalidFieldName`].
    ///
    /// # Validity
    ///
    /// After a successful call the card remains valid: `frontmatter`
    /// contains no reserved key and the value is stored at the correct key.
    ///
    /// # Warnings
    ///
    /// Card mutators never modify the parent document's `warnings`.
    pub fn set_field(&mut self, name: &str, value: QuillValue) -> Result<(), EditError> {
        if is_reserved_name(name) {
            return Err(EditError::ReservedName(name.to_string()));
        }
        if !is_valid_field_name(name) {
            return Err(EditError::InvalidFieldName(name.to_string()));
        }
        self.frontmatter_mut().insert(name.to_string(), value);
        Ok(())
    }

    /// Set a frontmatter field AND mark it as a `!fill` placeholder — the
    /// "reset to placeholder" path. A `Null` value emits as `key: !fill`;
    /// a scalar or sequence value emits as `key: !fill <value>`.
    ///
    /// # Invariants enforced
    ///
    /// Same as [`Card::set_field`].
    ///
    /// # Warnings
    ///
    /// Card mutators never modify the parent document's `warnings`.
    pub fn set_fill(&mut self, name: &str, value: QuillValue) -> Result<(), EditError> {
        if is_reserved_name(name) {
            return Err(EditError::ReservedName(name.to_string()));
        }
        if !is_valid_field_name(name) {
            return Err(EditError::InvalidFieldName(name.to_string()));
        }
        self.frontmatter_mut().insert_fill(name.to_string(), value);
        Ok(())
    }

    /// Remove a frontmatter field by name, returning the value if it existed.
    ///
    /// # Invariants enforced
    ///
    /// - `name` must not be one of the reserved sentinel names.
    ///   Returns [`EditError::ReservedName`].
    /// - `name` must match `[a-z_][a-z0-9_]*` after NFC normalisation.
    ///   Returns [`EditError::InvalidFieldName`].
    ///
    /// Absence of an otherwise-valid name returns `Ok(None)`.
    ///
    /// # Warnings
    ///
    /// Card mutators never modify the parent document's `warnings`.
    pub fn remove_field(&mut self, name: &str) -> Result<Option<QuillValue>, EditError> {
        if is_reserved_name(name) {
            return Err(EditError::ReservedName(name.to_string()));
        }
        if !is_valid_field_name(name) {
            return Err(EditError::InvalidFieldName(name.to_string()));
        }
        Ok(self.frontmatter_mut().remove(name))
    }

    /// Replace the card's Markdown body.
    ///
    /// # Warnings
    ///
    /// Card mutators never modify the parent document's `warnings`.
    pub fn replace_body(&mut self, body: impl Into<String>) {
        self.overwrite_body(body.into());
    }
}
