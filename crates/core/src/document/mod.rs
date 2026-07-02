//! Parsing and typed in-memory model for Quillmark card-yaml documents.
//!
//! A [`Document`] holds a root [`Card`] plus ordered composable cards; each
//! card carries a [`Payload`] — source-ordered items ([`PayloadItem`]:
//! `$quill`/`$kind`/`$id` metadata, user fields, and comments, in the order
//! they appear in the block's YAML content) — and a Markdown body.
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
pub mod wire;
pub(crate) mod yaml_hints;

pub use dto::{peek_schema_version, StorageError, StoredDocument, SCHEMA_V0_92_0};
pub use edit::EditError;
pub use meta::{is_valid_kind_name, validate_composable_kind, CardKindError};
pub use payload::{MetaKey, Payload, PayloadItem};
pub use wire::{CardWire, PayloadItemWire, WireError};

/// Authoring-format rules for the `~~~` card-yaml markdown surface.
///
/// Surfaced verbatim to LLM/MCP consumers (and to CLI / Python bindings via
/// the same text) so error parity holds — every consumer reads the same
/// rules. This is the single source of truth; bindings should call into it
/// rather than re-stating the rules in their own glue.
pub const FORMAT_RULES: &str = "Document format rules:
\u{2022} Block opener and closer are EXACTLY `~~~` (three tildes, no info string). The `~~~card-yaml` opener is also accepted as a non-canonical alias.
\u{2022} A blank line must precede every `~~~` block opener (unless it is line 1), and the opener must be at column zero (no leading spaces). An indented `~~~` is an ordinary code block, not a card.
\u{2022} The first block is the root and MUST contain `$quill: <name>@<version>`. Its `$kind` is `main` by position \u{2014} an explicit `$kind: main` is accepted but not required. Additional blocks declare composable cards via `$kind: <card_kind>`.
\u{2022} Reserved `$`-keys: `$quill`, `$kind`, `$id`, `$ext`, `$seed`. User fields use lowercase snake_case.
\u{2022} Prose body is the text after a block's closing `~~~`, up to the next opener or EOF. To include a literal fenced code block in prose, use a backtick fence (```); any column-zero `~~~` block is parsed as card metadata.
\u{2022} A field that already shows a concrete value carries a default and is shippable as-is \u{2014} keep the line, override the value, or delete it to fall back to the default. A blank or null value (`field:`, `field: null`, `field: ~`) is treated the same as omitting the field: it falls back to the default, or to the type-empty zero value.
\u{2022} `field: !must_fill <value>` marks a placeholder awaiting your input \u{2014} replace it with a real value and drop the `!must_fill` tag before shipping. A bare `field: !must_fill` is an empty placeholder. A leftover marker never blocks rendering, but it is reported as a warning until you replace it.
\u{2022} Numbers and booleans MUST be unquoted (`year: 2025`, `pinned: true`); quoting turns them into strings and fails validation.
\u{2022} Plain-scalar values cannot start with `*` or `&` (YAML alias/anchor markers) and cannot contain `: ` (colon-space). For markdown emphasis, embedded colons, or other special prefixes, quote the value: `field: '**bold**'` or `field: \"Name: subtitle\"`. Multi-line values use `|-`, not multi-line quoted scalars.";

/// Authoring-ergonomics header that introduces a blueprint to an LLM/MCP
/// consumer. The `{quill}` placeholder is substituted with the quill name.
/// Designed to be shown above [`FORMAT_RULES`], which covers field-level
/// semantics like the `!must_fill` marker — keep the wording tight here so the
/// two strings do not duplicate guidance.
const BLUEPRINT_INSTRUCTION_TEMPLATE: &str =
    "Fill in the `{quill}` blueprint below: replace each `!must_fill` placeholder with a real \
value and edit the body prose. Submit the filled markdown as `content` to `create_document`.";

/// Render the blueprint-instruction header with `quill_name` substituted in.
/// Single source of truth for the prose so every binding shows identical text.
pub fn blueprint_instruction(quill_name: &str) -> String {
    BLUEPRINT_INSTRUCTION_TEMPLATE.replace("{quill}", quill_name)
}

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

/// A parsed, per-kind **seed overlay**: the sparse fields (and optional body)
/// a newly-added card of a given kind starts with. Built from a `$seed[<kind>]`
/// entry of the main card's [`Card::seed`] map via [`SeedOverlay::from_json`],
/// and layered over the quill's schema-example seed by
/// [`crate::Quill::seed_card`] (overlay › example › absent). The reserved inner
/// key `$body` carries the body override; every other user field becomes an
/// entry, while any other `$`-prefixed key is reserved and dropped.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SeedOverlay {
    /// Field-value overrides, keyed by field name.
    pub fields: indexmap::IndexMap<String, crate::value::QuillValue>,
    /// Body override, when the overlay declares a `$body` string.
    pub body: Option<String>,
}

impl SeedOverlay {
    /// Parse an overlay from a `$seed[<kind>]` JSON value, or `None` when it is
    /// not a mapping. Use this to turn the raw overlay object a consumer reads
    /// from the main card's `$seed` map ([`Card::seed`]) into a typed overlay to
    /// hand to [`crate::Quill::seed_card`] — e.g.
    /// `doc.main().seed().and_then(|m| m.get(kind)).and_then(SeedOverlay::from_json)`.
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        value.as_object().map(Self::from_json_map)
    }

    /// Build an overlay from a single `$seed[<kind>]` JSON map: the reserved
    /// `$body` string becomes [`body`](Self::body); every other user-field entry
    /// becomes a field. A non-string `$body` is ignored (no body override). Any
    /// other `$`-prefixed key is reserved and dropped — never stored as a user
    /// field — since an overlay only ever carries user fields plus `$body`.
    fn from_json_map(map: &serde_json::Map<String, serde_json::Value>) -> Self {
        let mut fields = indexmap::IndexMap::new();
        let mut body = None;
        for (key, value) in map {
            if key == "$body" {
                if let Some(s) = value.as_str() {
                    body = Some(s.to_string());
                }
            } else if key.starts_with('$') {
                // Reserved key other than `$body`: not a user field. Drop it
                // rather than smuggle a `$`-key into the field set.
                continue;
            } else {
                fields.insert(
                    key.clone(),
                    crate::value::QuillValue::from_json(value.clone()),
                );
            }
        }
        SeedOverlay { fields, body }
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
    /// Create a blank document: a main card carrying only `$quill`, an empty
    /// body, and no composable cards. The programmatic blank canvas — every
    /// schema field is absent and resolves at render time (`default`, else
    /// type-empty zero), so nothing the caller did not set reaches the
    /// output. For an example-filled starter shaped like the blueprint, use
    /// `Quill::seed_document`.
    pub fn new(quill: QuillReference) -> Self {
        let mut payload = Payload::new();
        payload.set_quill(quill);
        // Parsed main cards always carry `$kind: main` (the parser normalizes
        // it in); match that shape so a blank document round-trips equal.
        payload.set_kind("main");
        Self {
            main: Card::from_parts(payload, String::new()),
            cards: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Create a `Document` from a pre-built main card and composable cards.
    /// `main` must carry `$quill`; composable cards must not.
    pub fn from_main_and_cards(main: Card, cards: Vec<Card>, warnings: Vec<Diagnostic>) -> Self {
        debug_assert!(main.quill().is_some(), "main card must carry `$quill`");
        debug_assert!(
            cards.iter().all(|c| c.quill().is_none()),
            "composable cards must not carry `$quill`"
        );
        debug_assert!(
            cards.iter().all(|c| c.seed().is_none()),
            "composable cards must not carry `$seed`"
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

    /// Non-fatal warnings from the parse; empty for a [`Document::new`] blank
    /// canvas. [`Document::from_main_and_cards`] carries whatever `warnings`
    /// the caller passes.
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
    /// root — they cannot collide with `$` keys because user field names are
    /// never `$`-prefixed (they match `[A-Za-z_][A-Za-z0-9_]*`).
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
