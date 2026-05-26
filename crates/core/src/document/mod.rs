//! Parsing and typed in-memory model for Quillmark card-yaml documents.
//!
//! ## Key types
//!
//! - [`Document`]: Root card plus ordered composable cards.
//! - [`Card`]: A single card block carrying a [`Payload`] and a Markdown body.
//! - [`Payload`]: Source-ordered items — `$quill`/`$kind`/`$id` metadata, user
//!   fields, and comments — from a block's YAML content.
//! - [`PayloadItem`]: The item variant — `Quill`/`Kind`/`Id`/`Field`/`Comment`.
//!
//! ## Errors
//!
//! [`Document::from_markdown`] returns errors for malformed YAML, unclosed
//! fences, a missing root `$quill`, or unknown `$`-prefixed system keys.
//!
//! See [markdown-spec.md](https://github.com/quillmark-org/quillmark/blob/main/prose/references/markdown-spec.md)
//! for the card-yaml format specification.

use serde::{Deserialize, Serialize};

use crate::error::ParseError;
use crate::version::QuillReference;
use crate::Diagnostic;

pub mod assemble;
pub mod dto;
pub mod edit;
pub mod emit;
pub mod fences;
pub mod limits;
pub mod meta;
pub mod payload;
pub mod prescan;
pub(crate) mod yaml_hints;

pub use dto::{peek_schema_version, StorageError, StoredDocument, SCHEMA_V0_81_0, SCHEMA_V0_82_0};
pub use edit::EditError;
pub use meta::{is_valid_kind_name, validate_composable_kind, CardKindError};
pub use payload::{Payload, PayloadItem};

/// Authoring-format rules for the card-yaml markdown surface.
///
/// Surfaced verbatim to LLM/MCP consumers (and to CLI / Python bindings via
/// the same text) so error parity holds — every consumer reads the same
/// rules. This is the single source of truth; bindings should call into it
/// rather than re-stating the rules in their own glue.
pub const FORMAT_RULES: &str = "Document format rules:
\u{2022} Metadata blocks use `~~~card-yaml` as the opener and `~~~` as the closer. Do NOT use `---` YAML frontmatter.
\u{2022} The closer is EXACTLY `~~~` (three tildes, no info string). Do NOT write `~~~card-yaml` as the closer.
\u{2022} A blank line is required before every `~~~card-yaml` opener (except when it is the first line of the document).
\u{2022} The first block is the root and MUST contain `$quill: <name>@<version>` and `$kind: main`.
\u{2022} Reserved `$`-keys: `$quill`, `$kind`, `$id`, `$ext`. User fields use lowercase snake_case.
\u{2022} Additional `~~~card-yaml` blocks declare composable cards via `$kind: <card_kind>`.
\u{2022} Prose body is the text after a block's closing `~~~`, before the next opener or EOF.
\u{2022} For optional fields with no value, OMIT the line entirely \u{2014} do not write `field: null`.
\u{2022} Respect field types: numbers unquoted (`word_count: 42`), booleans unquoted (`pinned: true`), strings as plain scalars or quoted. Quoting a number turns it into a string and will fail validation.
\u{2022} Plain-scalar values cannot start with `*` or `&` (YAML alias/anchor indicators) and cannot contain `:` followed by a space. For markdown emphasis, embedded colons, or other special prefixes, single-quote the value: `field: '**bold**'`, `field: \"Name: subtitle\"`.";

#[cfg(test)]
mod tests;

/// Parse result with the document and any non-fatal warnings.
#[derive(Debug)]
pub struct ParseOutput {
    pub document: Document,
    pub warnings: Vec<Diagnostic>,
}

/// A single card-yaml block (root or composable). `body` is `""` when no
/// content follows the closing fence; check `card.body().is_empty()`.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    payload: Payload,
    body: String,
}

impl Card {
    /// Create a `Card` from its parts without validation. For user-facing
    /// construction of composable cards use [`Card::new`].
    pub fn from_parts(payload: Payload, body: String) -> Self {
        Self { payload, body }
    }

    pub fn quill(&self) -> Option<&QuillReference> {
        self.payload.quill()
    }

    pub fn kind(&self) -> Option<&str> {
        self.payload.kind()
    }

    pub fn id(&self) -> Option<&str> {
        self.payload.id()
    }

    /// Opaque `$ext` map for out-of-band extension data (UI editor state,
    /// agent annotations, …). Carried through Markdown and storage DTO
    /// round-trips; never emitted into the plate JSON consumed by
    /// backends.
    pub fn ext(&self) -> Option<&serde_json::Map<String, serde_json::Value>> {
        self.payload.ext()
    }

    pub fn payload(&self) -> &Payload {
        &self.payload
    }

    pub fn payload_mut(&mut self) -> &mut Payload {
        &mut self.payload
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub(crate) fn overwrite_body(&mut self, body: String) {
        self.body = body;
    }
}

/// A fully-parsed Quillmark document. Serde routes through [`StoredDocument`];
/// for the plate wire shape see [`Document::to_plate_json`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(into = "StoredDocument", try_from = "StoredDocument")]
pub struct Document {
    main: Card,
    cards: Vec<Card>,
    warnings: Vec<Diagnostic>,
}

// `warnings` are parse-time observations and vary on round-trips; equality
// covers only structural content (`main` and `cards`).
impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        self.main == other.main && self.cards == other.cards
    }
}

impl Document {
    /// Create a `Document` from a pre-built main card and composable cards.
    /// `main` must carry `$quill`; composable cards must not.
    pub fn from_main_and_cards(main: Card, cards: Vec<Card>, warnings: Vec<Diagnostic>) -> Self {
        debug_assert!(main.quill().is_some(), "main card must carry `$quill`");
        debug_assert!(
            cards.iter().all(|c| c.quill().is_none()),
            "composable cards must not carry `$quill`"
        );
        Self {
            main,
            cards,
            warnings,
        }
    }

    pub fn from_markdown(markdown: &str) -> Result<Self, ParseError> {
        assemble::decompose(markdown)
    }

    pub fn from_markdown_with_warnings(markdown: &str) -> Result<ParseOutput, ParseError> {
        assemble::decompose_with_warnings(markdown)
            .map(|(document, warnings)| ParseOutput { document, warnings })
    }

    pub fn main(&self) -> &Card {
        &self.main
    }

    pub fn main_mut(&mut self) -> &mut Card {
        &mut self.main
    }

    /// The `$quill` reference from the root block. Always present on parsed documents.
    pub fn quill_reference(&self) -> QuillReference {
        self.main
            .quill()
            .cloned()
            .expect("root block's $quill is validated at parse time")
    }

    pub fn cards(&self) -> &[Card] {
        &self.cards
    }

    pub fn cards_mut(&mut self) -> &mut [Card] {
        &mut self.cards
    }

    /// Non-fatal warnings from the parse; empty for programmatically built documents.
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }

    pub(crate) fn cards_vec_mut(&mut self) -> &mut Vec<Card> {
        &mut self.cards
    }

    /// Serialize to the JSON wire shape consumed by backend plates. This is
    /// the **only** place in `quillmark-core` that produces this shape:
    ///
    /// ```json
    /// {
    ///   "$quill": "<ref>",
    ///   "$body": "<global-body>",
    ///   "$cards": [{ "$kind": "<tag>", "$body": "<card-body>", "<field>": <value>, ... }],
    ///   "<field>": <value>, ...
    /// }
    /// ```
    ///
    /// `$`-prefixed keys carry document-level metadata (quill ref, body
    /// text, card list, card kind). User payload fields stay flat at the
    /// root — they cannot collide with `$` keys because field names must
    /// match `[a-z_][a-z0-9_]*`.
    pub fn to_plate_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();

        map.insert(
            "$quill".to_string(),
            serde_json::Value::String(self.quill_reference().to_string()),
        );

        map.insert(
            "$body".to_string(),
            serde_json::Value::String(self.main.body.clone()),
        );

        let cards_array: Vec<serde_json::Value> = self
            .cards
            .iter()
            .map(|card| {
                let mut card_map = serde_json::Map::new();
                card_map.insert(
                    "$kind".to_string(),
                    serde_json::Value::String(card.kind().unwrap_or("").to_string()),
                );
                card_map.insert(
                    "$body".to_string(),
                    serde_json::Value::String(card.body.clone()),
                );
                for (key, value) in card.payload.iter() {
                    card_map.insert(key.clone(), value.as_json().clone());
                }
                serde_json::Value::Object(card_map)
            })
            .collect();

        map.insert("$cards".to_string(), serde_json::Value::Array(cards_array));

        for (key, value) in self.main.payload.iter() {
            map.insert(key.clone(), value.as_json().clone());
        }

        serde_json::Value::Object(map)
    }
}
