//! # Document Module
//!
//! Parsing functionality for markdown documents with card-yaml blocks.
//!
//! ## Overview
//!
//! The `document` module provides the [`Document::from_markdown`] function for parsing
//! markdown documents into a typed in-memory model.
//!
//! ## Key Types
//!
//! - [`Document`]: Typed in-memory Quillmark document — `main` card plus composable cards.
//! - [`Card`]: A single card block, root or composable, with a `#@` metadata
//!   header, a typed payload, and a body.
//! - [`CardMetadata`]: A block's typed `#@` system metadata (`#@quill`, `#@kind`, `#@id`).
//! - [`Payload`]: Ordered list of items (fields + comments) parsed from a
//!   block's YAML payload.
//!
//! ## Examples
//!
//! ### Basic Parsing
//!
//! ```
//! use quillmark_core::Document;
//!
//! let markdown = r#"~~~card-yaml
//! #@quill: my_quill
//! title: My Document
//! author: John Doe
//! ~~~
//!
//! # Introduction
//!
//! Document content here.
//! "#;
//!
//! let doc = Document::from_markdown(markdown).unwrap();
//! let title = doc.main()
//!     .payload()
//!     .get("title")
//!     .and_then(|v| v.as_str())
//!     .unwrap_or("Untitled");
//! assert_eq!(title, "My Document");
//! assert_eq!(doc.cards().len(), 0);
//! ```
//!
//! ### Accessing the plate wire format
//!
//! ```
//! use quillmark_core::Document;
//!
//! let doc = Document::from_markdown(
//!     "~~~card-yaml\n#@quill: my_quill\ntitle: Hi\n~~~\n\nBody here.\n"
//! ).unwrap();
//! let json = doc.to_plate_json();
//! assert_eq!(json["QUILL"], "my_quill");
//! assert_eq!(json["title"], "Hi");
//! assert_eq!(json["BODY"], "\nBody here.\n");
//! assert!(json["CARDS"].is_array());
//! ```
//!
//! ## Error Handling
//!
//! [`Document::from_markdown`] returns errors for:
//! - Malformed YAML syntax
//! - Unclosed `~~~card-yaml` blocks
//! - A root block missing its required `#@quill` system metadata
//! - A malformed or duplicated `#@` metadata line
//! - Reserved field name usage
//!
//! See [MARKDOWN.md](https://github.com/nibsbin/quillmark/blob/main/prose/designs/MARKDOWN.md)
//! for comprehensive documentation of the card-yaml format.

use crate::error::ParseError;
use crate::version::QuillReference;
use crate::Diagnostic;

pub mod assemble;
pub mod edit;
pub mod emit;
pub mod fences;
pub mod limits;
pub mod meta;
pub mod payload;
pub mod prescan;

pub use edit::EditError;
pub use meta::CardMetadata;
pub use payload::{Payload, PayloadItem};

#[cfg(test)]
mod tests;

/// Parse result carrying both the parsed document and any non-fatal warnings
/// (e.g. a `~~~card-yaml` opener missing its blank line, unsupported YAML tags).
#[derive(Debug)]
pub struct ParseOutput {
    /// The successfully parsed document.
    pub document: Document,
    /// Non-fatal warnings collected during parsing.
    pub warnings: Vec<Diagnostic>,
}

/// A single card-yaml block parsed from a Quillmark Markdown document.
///
/// A `Card` is the uniform shape for both the document's root block and
/// composable card blocks. Root vs. composable is purely positional — the
/// root is the document's first block, held in [`Document::main`]; composable
/// cards live in [`Document::cards`]. A `Card` carries no flag of its own
/// recording which collection it belongs to.
///
/// Every card has:
/// - `meta` — the block's typed `#@` system metadata (`#@quill`, `#@kind`, `#@id`).
/// - `payload` — ordered items parsed from the block's YAML payload.
/// - `body` — the Markdown text that follows the closing `~~~` fence, up to
///   the next block (or EOF).
///
/// ## Card body absence
///
/// If a card block has no trailing Markdown content (e.g. the next block or
/// EOF immediately follows the closing fence), `body` is the empty string `""`.
/// It is never `None`; callers that need to distinguish "absent" from "empty"
/// should check `card.body().is_empty()`.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    meta: CardMetadata,
    payload: Payload,
    body: String,
}

impl Card {
    /// Create a `Card` from its parts. Does **not** validate metadata or
    /// field names — callers are responsible for providing already-valid
    /// data. For user-facing construction of composable cards use
    /// [`Card::new`] (defined in `edit.rs`).
    pub fn from_parts(meta: CardMetadata, payload: Payload, body: String) -> Self {
        Self {
            meta,
            payload,
            body,
        }
    }

    /// The block's typed `#@` system metadata.
    pub fn meta(&self) -> &CardMetadata {
        &self.meta
    }

    /// Mutable access to the system metadata.
    pub fn meta_mut(&mut self) -> &mut CardMetadata {
        &mut self.meta
    }

    /// The `#@kind` card kind, if the block declares one.
    pub fn kind(&self) -> Option<&str> {
        self.meta.kind.as_deref()
    }

    /// The `#@id` opaque identifier, if the block declares one.
    pub fn id(&self) -> Option<&str> {
        self.meta.id.as_deref()
    }

    /// Typed payload (map-keyed view and ordered item list).
    pub fn payload(&self) -> &Payload {
        &self.payload
    }

    /// Mutable access to the payload.
    pub fn payload_mut(&mut self) -> &mut Payload {
        &mut self.payload
    }

    /// Markdown body that follows this card's closing fence.
    ///
    /// Empty string when no trailing content is present.
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Overwrite the body string. Internal helper used by [`Card::replace_body`].
    pub(crate) fn overwrite_body(&mut self, body: String) {
        self.body = body;
    }
}

/// A fully-parsed, typed in-memory Quillmark document.
///
/// `Document` is the canonical representation of a Quillmark Markdown file.
/// Markdown is both the import and export format; the structured data here
/// is primary.
///
/// ## Structure
///
/// - `main` — the document's root `Card`.
/// - `cards` — ordered composable cards.
///
/// Backend plates consume the flat JSON wire shape produced by
/// [`Document::to_plate_json`]. That method is the **only** place in core
/// that reconstructs `{"QUILL": ..., "CARDS": [...], "BODY": "..."}`.
#[derive(Debug, Clone)]
pub struct Document {
    main: Card,
    cards: Vec<Card>,
    warnings: Vec<Diagnostic>,
}

// Equality is defined over the structural content only — `warnings` are
// parse-time observations that depend on what the source text happened to
// contain (unsupported tag drops, missing-blank-line lints, etc.) and so differ
// between a source document and its round-tripped emission. Two documents
// are equal when their `main` and `cards` match.
impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        self.main == other.main && self.cards == other.cards
    }
}

impl Document {
    /// Create a `Document` from a pre-built main `Card` and composable cards.
    ///
    /// The caller must guarantee that `main`'s `#@quill` metadata is present
    /// and valid.
    pub fn from_main_and_cards(main: Card, cards: Vec<Card>, warnings: Vec<Diagnostic>) -> Self {
        Self {
            main,
            cards,
            warnings,
        }
    }

    /// Parse a Quillmark Markdown document, discarding any non-fatal warnings.
    pub fn from_markdown(markdown: &str) -> Result<Self, ParseError> {
        assemble::decompose(markdown)
    }

    /// Parse a Quillmark Markdown document, returning warnings alongside the document.
    pub fn from_markdown_with_warnings(markdown: &str) -> Result<ParseOutput, ParseError> {
        assemble::decompose_with_warnings(markdown)
            .map(|(document, warnings)| ParseOutput { document, warnings })
    }

    // ── Accessors ──────────────────────────────────────────────────────────────

    /// The document's main (entry) card.
    pub fn main(&self) -> &Card {
        &self.main
    }

    /// Mutable access to the main card.
    pub fn main_mut(&mut self) -> &mut Card {
        &mut self.main
    }

    /// The quill reference (`name@version-selector`) the document binds to,
    /// parsed from the root block's `#@quill` system metadata.
    ///
    /// The root `#@quill` is validated when the document is parsed, so this
    /// never fails for a `Document` produced by [`Document::from_markdown`].
    pub fn quill_reference(&self) -> QuillReference {
        self.main
            .meta
            .quill
            .clone()
            .expect("root block's #@quill is validated at parse time")
    }

    /// Ordered list of composable card blocks.
    pub fn cards(&self) -> &[Card] {
        &self.cards
    }

    /// Mutable access to the composable cards slice.
    pub fn cards_mut(&mut self) -> &mut [Card] {
        &mut self.cards
    }

    /// Internal mutable access to the backing `Vec<Card>`. Used by edit
    /// operations ([`Document::push_card`], etc.) that need to insert or
    /// remove elements.
    pub(crate) fn cards_vec_mut(&mut self) -> &mut Vec<Card> {
        &mut self.cards
    }

    /// Non-fatal warnings collected during parsing.
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }

    // ── Wire format ────────────────────────────────────────────────────────────

    /// Serialize this document to the JSON shape expected by backend plates.
    ///
    /// The output has the following top-level keys, which match what
    /// `lib.typ.template` reads at Typst runtime:
    ///
    /// ```json
    /// {
    ///   "QUILL": "<ref>",
    ///   "<field>": <value>,
    ///   ...
    ///   "BODY": "<global-body>",
    ///   "CARDS": [
    ///     { "CARD": "<tag>", "<field>": <value>, ..., "BODY": "<card-body>" },
    ///     ...
    ///   ]
    /// }
    /// ```
    ///
    /// This is the **only** place in `quillmark-core` that knows about the plate
    /// wire format. All internal consumers (Quill, backends) call this instead
    /// of constructing the shape by hand.
    pub fn to_plate_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();

        // QUILL first — plate authors expect this at the top.
        map.insert(
            "QUILL".to_string(),
            serde_json::Value::String(self.quill_reference().to_string()),
        );

        // Payload fields in insertion order.
        for (key, value) in self.main.payload.iter() {
            map.insert(key.clone(), value.as_json().clone());
        }

        // Global body.
        map.insert(
            "BODY".to_string(),
            serde_json::Value::String(self.main.body.clone()),
        );

        // Cards array.
        let cards_array: Vec<serde_json::Value> = self
            .cards
            .iter()
            .map(|card| {
                let mut card_map = serde_json::Map::new();
                card_map.insert(
                    "CARD".to_string(),
                    serde_json::Value::String(card.meta.kind.as_deref().unwrap_or("").to_string()),
                );
                for (key, value) in card.payload.iter() {
                    card_map.insert(key.clone(), value.as_json().clone());
                }
                card_map.insert(
                    "BODY".to_string(),
                    serde_json::Value::String(card.body.clone()),
                );
                serde_json::Value::Object(card_map)
            })
            .collect();

        map.insert("CARDS".to_string(), serde_json::Value::Array(cards_array));

        serde_json::Value::Object(map)
    }
}
