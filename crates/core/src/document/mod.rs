//! # Document Module
//!
//! Parsing functionality for markdown documents with YAML frontmatter.
//!
//! ## Overview
//!
//! The `document` module provides the [`Document::from_markdown`] function for parsing
//! markdown documents into a typed in-memory model.
//!
//! ## Key Types
//!
//! - [`Document`]: Typed in-memory Quillmark document — `main` card plus composable cards.
//! - [`Card`]: A single metadata fence block, main or composable, with a sentinel,
//!   typed frontmatter, and a body.
//! - [`Sentinel`]: Discriminates `QUILL:` main cards from `` ```card <kind> `` composable cards.
//! - [`Frontmatter`]: Ordered list of items (fields + comments) parsed from a YAML fence.
//!
//! ## Examples
//!
//! ### Basic Parsing
//!
//! ```
//! use quillmark_core::Document;
//!
//! let markdown = r#"---
//! QUILL: my_quill
//! title: My Document
//! author: John Doe
//! ---
//!
//! # Introduction
//!
//! Document content here.
//! "#;
//!
//! let doc = Document::from_markdown(markdown).unwrap();
//! let title = doc.main()
//!     .frontmatter()
//!     .get("title")
//!     .and_then(|v| v.as_str())
//!     .unwrap_or("Untitled");
//! assert_eq!(title, "My Document");
//! assert_eq!(doc.cards().len(), 0);
//! ```
//!
//! ### Document with cards
//!
//! ```
//! use quillmark_core::Document;
//!
//! let markdown = "---\nQUILL: my_quill\ntitle: Catalog\n---\n\nIntro.\n\n```card product\nname: Widget\n```\n";
//! let doc = Document::from_markdown(markdown).unwrap();
//! assert_eq!(doc.cards().len(), 1);
//! assert_eq!(doc.cards()[0].tag(), "product");
//! ```
//!
//! ### Accessing the main file wire format
//!
//! ```
//! use quillmark_core::Document;
//!
//! let doc = Document::from_markdown(
//!     "---\nQUILL: my_quill\ntitle: Hi\n---\n\nBody here.\n"
//! ).unwrap();
//! let json = doc.to_main_json();
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
//! - Unclosed frontmatter blocks
//! - Multiple global frontmatter blocks
//! - A card fence with a missing, invalid, or extra info-string kind token
//! - Reserved field name usage
//! - Name collisions
//!
//! See [PARSE.md](https://github.com/nibsbin/quillmark/blob/main/designs/PARSE.md) for
//! comprehensive documentation of the Extended YAML Metadata Standard.

use crate::error::ParseError;
use crate::version::QuillReference;
use crate::Diagnostic;

pub mod assemble;
pub mod edit;
pub mod emit;
pub mod fences;
pub mod frontmatter;
pub mod limits;
pub mod prescan;
pub mod sentinel;

pub use edit::EditError;
pub use frontmatter::{Frontmatter, FrontmatterItem};

// Re-export the sentinel type (defined below in this module file).
// `Sentinel` is exported at the crate root via `lib.rs`.

#[cfg(test)]
mod tests;

/// Parse result carrying both the parsed document and any non-fatal warnings
/// (e.g. near-miss sentinel lints emitted per spec §4.2).
#[derive(Debug)]
pub struct ParseOutput {
    /// The successfully parsed document.
    pub document: Document,
    /// Non-fatal warnings collected during parsing.
    pub warnings: Vec<Diagnostic>,
}

/// Discriminator for a [`Card`]'s metadata fence.
///
/// The first fence in a Quillmark document carries `QUILL: <ref>` and is the
/// document-level *main* card; every subsequent fence is a `` ```card <kind> ``
/// code block and is a composable card. `Sentinel` captures that distinction
/// in the typed model so every fence is one uniform shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Sentinel {
    /// `QUILL: <ref>` — the document entry card.
    Main(QuillReference),
    /// `` ```card <tag> `` — a composable card with the given kind tag.
    Inline(String),
}

impl Sentinel {
    /// String form of this sentinel's value: the quill reference for `Main`,
    /// the tag for `Card`.
    pub fn as_str(&self) -> String {
        match self {
            Sentinel::Main(r) => r.to_string(),
            Sentinel::Inline(t) => t.clone(),
        }
    }

    /// Returns `true` if this is a `Main` sentinel.
    pub fn is_main(&self) -> bool {
        matches!(self, Sentinel::Main(_))
    }
}

/// A single metadata fence parsed from a Quillmark Markdown document.
///
/// A `Card` is the uniform shape for both the document entry (main) fence and
/// composable card fences. `sentinel` distinguishes the two.
///
/// Every card has:
/// - `sentinel` — the `QUILL` reference (for main) or `KIND` tag (for composable).
/// - `frontmatter` — ordered items parsed from the YAML fence body (with the
///   sentinel key already removed).
/// - `body` — the Markdown text that follows the closing fence, up to the next
///   fence (or EOF).
///
/// ## Card body absence
///
/// If a card block has no trailing Markdown content (e.g. the next block or
/// EOF immediately follows the closing fence), `body` is the empty string `""`.
/// It is never `None`; callers that need to distinguish "absent" from "empty"
/// should check `card.body().is_empty()`.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    sentinel: Sentinel,
    frontmatter: Frontmatter,
    body: String,
}

impl Card {
    /// Create a `Card` directly from a sentinel, a typed frontmatter, and a
    /// body. Does **not** validate the sentinel tag or any field names —
    /// callers are responsible for providing already-valid data. For
    /// user-facing construction of composable cards use [`Card::new`]
    /// (defined in `edit.rs`).
    pub fn new_with_sentinel(sentinel: Sentinel, frontmatter: Frontmatter, body: String) -> Self {
        Self {
            sentinel,
            frontmatter,
            body,
        }
    }

    /// The sentinel discriminating this card as main or composable.
    pub fn sentinel(&self) -> &Sentinel {
        &self.sentinel
    }

    /// The card tag — the kind for composable cards, or the string form of
    /// the quill reference for main cards.
    pub fn tag(&self) -> String {
        self.sentinel.as_str()
    }

    /// Typed frontmatter (map-keyed view and ordered item list).
    pub fn frontmatter(&self) -> &Frontmatter {
        &self.frontmatter
    }

    /// Mutable access to the frontmatter.
    pub fn frontmatter_mut(&mut self) -> &mut Frontmatter {
        &mut self.frontmatter
    }

    /// Markdown body that follows this card's closing fence.
    ///
    /// Empty string when no trailing content is present.
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Returns `true` if this is the document entry (main) card.
    pub fn is_main(&self) -> bool {
        self.sentinel.is_main()
    }

    /// Replace this card's sentinel. Internal helper; public mutators
    /// ([`Document::set_quill_ref`], the parser) call this.
    pub(crate) fn replace_sentinel(&mut self, sentinel: Sentinel) {
        self.sentinel = sentinel;
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
/// - `main` — the entry `Card` (sentinel is `Sentinel::Main(reference)`).
/// - `cards` — ordered composable cards (each with `Sentinel::Inline(tag)`).
///
/// Backend main files consume the flat JSON wire shape produced by
/// [`Document::to_main_json`]. That method is the **only** place in core
/// that reconstructs `{"QUILL": ..., "CARDS": [...], "BODY": "..."}`.
#[derive(Debug, Clone)]
pub struct Document {
    main: Card,
    cards: Vec<Card>,
    warnings: Vec<Diagnostic>,
}

// Equality is defined over the structural content only — `warnings` are
// parse-time observations that depend on what the source text happened to
// contain (near-miss sentinels, unsupported tag drops, etc.) and so differ
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
    /// The caller must guarantee that `main.sentinel` is `Sentinel::Main(_)`
    /// and every card in `cards` has `sentinel` = `Sentinel::Inline(_)`.
    pub fn from_main_and_cards(main: Card, cards: Vec<Card>, warnings: Vec<Diagnostic>) -> Self {
        debug_assert!(main.sentinel.is_main(), "main card must be Sentinel::Main");
        debug_assert!(
            cards.iter().all(|c| !c.sentinel.is_main()),
            "composable cards must be Sentinel::Inline"
        );
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

    /// The quill reference (`name@version-selector`) carried by the main card's
    /// sentinel. Convenience reader over `doc.main().sentinel()`.
    pub fn quill_reference(&self) -> &QuillReference {
        match &self.main.sentinel {
            Sentinel::Main(r) => r,
            Sentinel::Inline(_) => {
                unreachable!("main card must carry Sentinel::Main by construction")
            }
        }
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

    /// Serialize this document to the JSON shape expected by backend main files.
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
    ///     { "KIND": "<tag>", "<field>": <value>, ..., "BODY": "<card-body>" },
    ///     ...
    ///   ]
    /// }
    /// ```
    ///
    /// This is the **only** place in `quillmark-core` that knows about the
    /// main file wire format. All internal consumers (Quill, backends) call
    /// this instead of constructing the shape by hand.
    pub fn to_main_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();

        // QUILL first — main file authors expect this at the top.
        map.insert(
            "QUILL".to_string(),
            serde_json::Value::String(self.quill_reference().to_string()),
        );

        // Frontmatter fields in insertion order.
        for (key, value) in self.main.frontmatter.iter() {
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
                card_map.insert("KIND".to_string(), serde_json::Value::String(card.tag()));
                for (key, value) in card.frontmatter.iter() {
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
