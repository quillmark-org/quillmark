//! Renderable `Quill` — the engine-constructed composition of a
//! [`QuillSource`] with a resolved backend.

use indexmap::IndexMap;
use std::sync::Arc;

use quillmark_core::{
    normalize::normalize_document, quill::CardSchema, zero_value, Backend, Card, Diagnostic,
    Document, OutputFormat, Payload, QuillSource, QuillValue, RenderError, RenderOptions,
    RenderResult, RenderSession, Severity,
};

use crate::form::{self, Form, FormCard};
use crate::seed;

/// Renderable quill: an [`Arc<QuillSource>`] paired with a resolved [`Backend`].
/// Constructed by the engine; immutable once created.
#[derive(Clone)]
pub struct Quill {
    source: Arc<QuillSource>,
    backend: Arc<dyn Backend>,
}

impl Quill {
    /// Engine-internal; external callers use [`crate::Quillmark::quill`] or
    /// [`crate::Quillmark::quill_from_path`].
    pub(crate) fn new(source: Arc<QuillSource>, backend: Arc<dyn Backend>) -> Self {
        Self { source, backend }
    }

    pub fn source(&self) -> &QuillSource {
        &self.source
    }

    /// The resolved backend identifier (e.g. `"typst"`).
    pub fn backend_id(&self) -> &str {
        self.backend.id()
    }

    pub fn supported_formats(&self) -> &'static [OutputFormat] {
        self.backend.supported_formats()
    }

    pub fn name(&self) -> &str {
        self.source.name()
    }

    /// Render a document. Pass `&RenderOptions::default()` for backend defaults.
    pub fn render(
        &self,
        doc: &Document,
        opts: &RenderOptions,
    ) -> Result<RenderResult, RenderError> {
        let session = self.open(doc)?;
        let resolved = RenderOptions {
            output_format: opts
                .output_format
                .or_else(|| self.backend.supported_formats().first().copied()),
            ppi: opts.ppi,
            pages: opts.pages.clone(),
            producer: opts.producer.clone(),
        };
        session.render(&resolved)
    }

    pub fn open(&self, doc: &Document) -> Result<RenderSession, RenderError> {
        let json_data = self.compile_data(doc)?;
        let plate_content = self
            .source
            .plate()
            .filter(|s| !s.is_empty())
            .unwrap_or("")
            .to_string();
        let warnings: Vec<_> = self.ref_mismatch_warning(doc).into_iter().collect();
        let session = self
            .backend
            .open(&plate_content, &self.source, &json_data)?;
        Ok(session.with_warnings(warnings))
    }

    /// Compile a document to JSON wire format for the backend.
    ///
    /// Applies coercion, validation, normalization, and **zero-filled render**:
    /// every absent schema field is resolved to its authored value, else its
    /// schema default, else its type-empty zero value — in this plate-JSON
    /// projection only, never in the persisted document. A merely *incomplete*
    /// document renders fine; only a *malformed* one (a surviving `<must-fill>`
    /// sentinel or a value that won't coerce/validate) errors. See
    /// `prose/canon/SCHEMAS.md`.
    pub fn compile_data(&self, doc: &Document) -> Result<serde_json::Value, RenderError> {
        let coerced = self.coerce_and_validate(doc)?;
        let normalized = normalize_document(coerced)?;
        let config = self.source.config();

        let main_resolved =
            resolve_fields(&normalized.main().payload().to_index_map(), &config.main);
        let cards_resolved: Vec<Card> = normalized
            .cards()
            .iter()
            .map(|card| {
                let fields = match config.card_kind(card.kind().unwrap_or("")) {
                    Some(schema) => resolve_fields(&card.payload().to_index_map(), schema),
                    None => card.payload().to_index_map(),
                };
                Card::from_parts(
                    rebuild_payload_with_meta(card, fields),
                    card.body().to_string(),
                )
            })
            .collect();

        let final_main = Card::from_parts(
            rebuild_payload_with_meta(normalized.main(), main_resolved),
            normalized.main().body().to_string(),
        );
        let final_doc = Document::from_main_and_cards(final_main, cards_resolved, Vec::new());

        Ok(final_doc.to_plate_json())
    }

    /// Validate without backend compilation.
    pub fn dry_run(&self, doc: &Document) -> Result<(), RenderError> {
        self.coerce_and_validate(doc).map(|_| ())
    }

    fn coerce_and_validate(&self, doc: &Document) -> Result<Document, RenderError> {
        let config = self.source.config();

        let coerced_payload = config
            .coerce_payload(&doc.main().payload().to_index_map())
            .map_err(coercion_error)?;

        let mut coerced_cards: Vec<Card> = Vec::with_capacity(doc.cards().len());
        for card in doc.cards() {
            let coerced_fields = config
                .coerce_card(card.kind().unwrap_or(""), &card.payload().to_index_map())
                .map_err(coercion_error)?;
            coerced_cards.push(Card::from_parts(
                rebuild_payload_with_meta(card, coerced_fields),
                card.body().to_string(),
            ));
        }

        let coerced_main = Card::from_parts(
            rebuild_payload_with_meta(doc.main(), coerced_payload),
            doc.main().body().to_string(),
        );
        let coerced_doc = Document::from_main_and_cards(coerced_main, coerced_cards, Vec::new());

        self.validate_document(&coerced_doc)?;

        Ok(coerced_doc)
    }

    fn ref_mismatch_warning(&self, doc: &Document) -> Option<Diagnostic> {
        let doc_ref = doc.quill_reference();
        if doc_ref.name.as_str() != self.source.name() {
            Some(
                Diagnostic::new(
                    Severity::Warning,
                    format!(
                        "document declares $quill '{}' but was rendered with '{}'",
                        doc_ref,
                        self.source.name()
                    ),
                )
                .with_code("quill::ref_mismatch".to_string())
                .with_hint(
                    "the $quill reference is informational; ensure you are rendering with the intended quill"
                        .to_string(),
                ),
            )
        } else {
            None
        }
    }

    /// Schema-aware form view of `doc`. Read-only snapshot — re-call after edits.
    /// Unknown card kinds surface as `form::unknown_card_kind` diagnostics;
    /// validation errors forward through with their canonical
    /// `validation::*` codes, paths, and hints (same as
    /// [`Quill::render`]'s `RenderError::diagnostics`).
    pub fn form(&self, doc: &Document) -> Form {
        form::build_form(self, doc)
    }

    /// Blank form for the main card — fields are [`form::FormFieldSource::Default`]
    /// or [`form::FormFieldSource::Missing`]. Use as a starting state for a fresh document.
    pub fn blank_main(&self) -> FormCard {
        FormCard::blank(&self.source.config().main)
    }

    /// Blank form for a card of the given kind; `None` if the kind is not in
    /// the schema. Use to render a new-card form before committing to the document.
    pub fn blank_card(&self, card_kind: &str) -> Option<FormCard> {
        form::blank_card_for_kind(self, card_kind)
    }

    /// Seed a starter [`Document`]: the main card plus one instance of each
    /// declared composable card kind, each committing its fields' `example`
    /// values and leaving all other fields absent (interpolated at render:
    /// `default` → type-empty zero). The committed, structured counterpart of
    /// [`QuillConfig::example`](quillmark_core::quill::QuillConfig::example)'s
    /// illustrative string. See [`crate::seed`].
    pub fn seed_document(&self) -> Document {
        seed::seed_document(self)
    }

    /// Seed a starter main [`Card`] (carries `$quill`). Use as the main card
    /// of a fresh document. See [`Quill::seed_document`].
    pub fn seed_main(&self) -> Card {
        seed::seed_main(self)
    }

    /// Seed a starter composable [`Card`] of the given kind (carries `$kind`);
    /// `None` if the kind is not declared. Use to add a new card to a document.
    pub fn seed_card(&self, card_kind: &str) -> Option<Card> {
        seed::seed_card_for_kind(self, card_kind)
    }

    fn validate_document(&self, doc: &Document) -> Result<(), RenderError> {
        match self.source.config().validate_document(doc) {
            Ok(_) => Ok(()),
            Err(errors) => {
                // Zero-filled render: a merely *incomplete* document (Must Fill
                // fields absent) renders fine — each absent field is zero-filled
                // in `resolve_fields`. Only *malformed* input is fatal: a
                // surviving `<must-fill>` sentinel, or a value that won't
                // coerce/validate. So `validation::must_fill_absent` is demoted
                // (absence warnings are a deferred follow-up; today the form's
                // `FormFieldSource::Missing` carries the doneness signal).
                //
                // Each surviving ValidationError gets its own Diagnostic so
                // consumers can use `path` for UI navigation via
                // `RenderError::diagnostics()`.
                let diags: Vec<Diagnostic> = errors
                    .iter()
                    .filter(|e| e.code() != "validation::must_fill_absent")
                    .map(|e| e.to_diagnostic())
                    .collect();
                if diags.is_empty() {
                    Ok(())
                } else {
                    Err(RenderError::ValidationFailed { diags })
                }
            }
        }
    }
}

/// Wrap a coercion error into `RenderError::ValidationFailed`.
/// `Diagnostic::path` is unset — coercion runs before structured validation.
fn coercion_error(e: impl std::fmt::Display) -> RenderError {
    RenderError::ValidationFailed {
        diags: vec![Diagnostic::new(Severity::Error, e.to_string())
            .with_code("validation::coercion_failed".to_string())
            .with_hint("Ensure all fields can be coerced to their declared types".to_string())],
    }
}

/// Resolve every schema field absent from `fields`, by precedence: an
/// authored value wins; else the schema `default:`; else the type-empty
/// [`zero_value`]. This is the zero-filled render projection — the fill lives
/// only here and is never persisted (see
/// `prose/canon/SCHEMAS.md`). Non-schema fields already present
/// are preserved untouched.
fn resolve_fields(
    fields: &IndexMap<String, QuillValue>,
    schema: &CardSchema,
) -> IndexMap<String, QuillValue> {
    let mut result = fields.clone();
    for (name, field) in &schema.fields {
        if result.contains_key(name) {
            continue;
        }
        let value = field.default.clone().unwrap_or_else(|| zero_value(field));
        result.insert(name.clone(), value);
    }
    result
}

/// Build a [`Payload`] from a coerced/defaulted field map, re-attaching
/// `$quill` / `$kind` / `$id` from `source`. Comments are dropped —
/// this payload feeds backend rendering, not round-trip storage.
fn rebuild_payload_with_meta(source: &Card, fields: IndexMap<String, QuillValue>) -> Payload {
    let mut payload = Payload::from_index_map(fields);
    if let Some(q) = source.quill() {
        payload.set_quill(q.clone());
    }
    if let Some(k) = source.kind() {
        payload.set_kind(k.to_string());
    }
    if let Some(id) = source.id() {
        payload.set_id(id.to_string());
    }
    payload
}

impl std::fmt::Debug for Quill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Quill")
            .field("name", &self.source.name())
            .field("backend", &self.backend.id())
            .finish()
    }
}
