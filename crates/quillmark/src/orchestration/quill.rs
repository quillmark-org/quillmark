//! Renderable `Quill` — the engine-constructed composition of a
//! [`QuillSource`] with a resolved backend.

use indexmap::IndexMap;
use std::str::FromStr;
use std::sync::Arc;

use quillmark_core::{
    normalize::normalize_document, quill::CardSchema, zero_value, Backend, Card, Diagnostic,
    Document, OutputFormat, Payload, QuillSource, QuillValue, RenderError, RenderOptions,
    RenderResult, RenderSession, Severity, Version,
};

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
        self.check_quill_reference(doc)?;
        let json_data = self.compile_data(doc)?;
        let plate_content = self
            .source
            .plate()
            .filter(|s| !s.is_empty())
            .unwrap_or("")
            .to_string();
        self.backend.open(&plate_content, &self.source, &json_data)
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
        self.check_quill_reference(doc)?;
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

    /// Enforce the document's `$quill` reference (`name@selector`) against the
    /// loaded quill, failing with [`RenderError::QuillMismatch`] if either
    /// component diverges. The document is well-formed; it was paired with the
    /// wrong quill — a different format, or an incompatible version of one —
    /// which yields undefined output, so it errors rather than warns.
    ///
    /// Name is the prerequisite (a selector belongs to a *named* quill): a name
    /// mismatch (`quill::name_mismatch`) short-circuits and the version is left
    /// unevaluated; otherwise the selector is checked (`quill::version_mismatch`).
    /// The version parses infallibly in practice (validated at load); if it
    /// somehow doesn't, the version check is skipped.
    fn check_quill_reference(&self, doc: &Document) -> Result<(), RenderError> {
        let doc_ref = doc.quill_reference();

        if doc_ref.name.as_str() != self.source.name() {
            return Err(quill_mismatch(
                format!(
                    "document declares $quill '{}' but was rendered with '{}'",
                    doc_ref,
                    self.source.name()
                ),
                "quill::name_mismatch",
                "render with the quill named by $quill, or update the $quill name",
            ));
        }

        let Ok(quill_version) = Version::from_str(&self.source.config().version) else {
            return Ok(());
        };
        if !doc_ref.selector.matches(quill_version) {
            return Err(quill_mismatch(
                format!(
                    "document declares $quill '{}' but the loaded quill is version '{}'",
                    doc_ref, quill_version
                ),
                "quill::version_mismatch",
                "render with a quill whose version satisfies the selector, or update the $quill selector",
            ));
        }

        Ok(())
    }

    /// Validate `doc` against this quill's schema, returning every diagnostic
    /// (an empty `Vec` when the document is valid).
    ///
    /// This is the editor-facing validation surface. It forwards the canonical
    /// `validation::*` diagnostics verbatim — same code, `path`, and `hint` the
    /// engine emits — so consumers can route on the code and navigate by path
    /// without parsing message text. It covers type mismatches, unknown card
    /// kinds (`validation::unknown_card`), body-on-disabled-body, and the
    /// surviving-`<must-fill>`-sentinel error.
    ///
    /// Unlike [`Quill::render`], it *includes* the non-fatal
    /// `validation::field_absent` signal that render demotes (an absent
    /// Unendorsed field zero-fills rather than failing). Treat
    /// `field_absent` as a per-field completeness hint and the remaining
    /// `error`-severity diagnostics as blockers.
    ///
    /// Field values, defaults, and presentation order are not part of this
    /// surface: a consumer reads them directly from the [`Document`] payload
    /// and the quill schema (`quill.source().config().schema()`), where fields
    /// carry their `ui.order` as the presentation-ordering signal.
    pub fn validate(&self, doc: &Document) -> Vec<Diagnostic> {
        match self.source.config().validate_document(doc) {
            Ok(()) => Vec::new(),
            Err(errors) => errors.iter().map(|e| e.to_diagnostic()).collect(),
        }
    }

    /// Seed a starter [`Document`]: the main card plus one instance of each
    /// declared composable card kind, each committing its fields' `example`
    /// values and leaving all other fields absent (interpolated at render:
    /// `default` → type-empty zero). The committed, structured "filled-out"
    /// twin of the [`blueprint`](quillmark_core::quill::QuillConfig::blueprint).
    /// See [`crate::seed`].
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
                // Zero-filled render: a merely *incomplete* document (Unendorsed
                // fields absent) renders fine — each absent field is zero-filled
                // in `resolve_fields`. Only *malformed* input is fatal: a
                // surviving `<must-fill>` sentinel, or a value that won't
                // coerce/validate. So `validation::field_absent` is demoted
                // here (the editor-facing `Quill::validate` keeps it as the
                // per-field doneness signal).
                //
                // Each surviving ValidationError gets its own Diagnostic so
                // consumers can use `path` for UI navigation via
                // `RenderError::diagnostics()`.
                let diags: Vec<Diagnostic> = errors
                    .iter()
                    .filter(|e| e.code() != "validation::field_absent")
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

/// A single-diagnostic [`RenderError::QuillMismatch`]. `path` is unset — the
/// mismatch is the root `$quill` line, not a field.
fn quill_mismatch(message: String, code: &str, hint: &str) -> RenderError {
    RenderError::QuillMismatch {
        diags: vec![Diagnostic::new(Severity::Error, message)
            .with_code(code.to_string())
            .with_hint(hint.to_string())],
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
