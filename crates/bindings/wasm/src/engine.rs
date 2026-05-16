//! Quillmark WASM Engine - Simplified API

use crate::error::WasmError;
use crate::types::{Card, Diagnostic, RenderOptions, RenderResult};
use js_sys::{Array, Uint8Array};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

/// TypeScript declarations for the quill metadata surface and form view.
///
/// Emitted via `typescript_custom_section` so the types land in the generated
/// `.d.ts` as a single source of truth. Consumers can import these directly
/// rather than redeclaring the shape locally.
#[wasm_bindgen(typescript_custom_section)]
const METADATA_FORM_TS: &'static str = r#"
/** UI layout hints for a single field. */
export interface QuillFieldUi {
    group?: string;
    order?: number;
    compact?: boolean;
    multiline?: boolean;
}

/** UI layout hints for a card (main or named card kind). */
export interface QuillCardUi {
    title?: string;
}

/** Body namespace for a card (main or named card kind). */
export interface QuillCardBody {
    /** When false, consumers must not accept or store body content for this card kind. Defaults to true. */
    enabled?: boolean;
    /** Example body content embedded verbatim in the blueprint body region. Fallback is "Write <card> body here." */
    example?: string;
}

/** Schema entry for a single field declared in a quill's `Quill.yaml`. */
export interface QuillFieldSchema {
    type: "string" | "number" | "integer" | "boolean" | "array" | "object" | "date" | "datetime" | "markdown";
    description?: string;
    default?: unknown;
    example?: unknown;
    required?: boolean;
    enum?: string[];
    ui?: QuillFieldUi;
    properties?: Record<string, QuillFieldSchema>;
    items?: QuillFieldSchema;
}

/** Schema entry for the main card or a named card kind. */
export interface QuillCardSchema {
    description?: string;
    fields: Record<string, QuillFieldSchema>;
    ui?: QuillCardUi;
    body?: QuillCardBody;
}

/**
 * Document schema returned by `Quill.schema`. Includes optional `ui` keys.
 *
 * `main.fields.QUILL` and `card_kinds[name].fields.CARD` are required
 * sentinels with `const` values telling consumers what to write.
 */
export interface QuillSchema {
    main: QuillCardSchema;
    /** Present only when the quill declares at least one named card kind. */
    card_kinds?: Record<string, QuillCardSchema>;
}

/**
 * Identity snapshot mirroring the `quill:` section of `Quill.yaml`.
 * The schema lives on `Quill.schema`.
 * Extra `quill:` keys appear as `unknown`.
 */
export interface QuillMetadata {
    name: string;
    version: string;
    backend: string;
    author: string;
    description: string;
    supportedFormats: OutputFormat[];
    [key: string]: unknown;
}

/** Source of a field's effective value in a form view. */
export type FormFieldSource = "document" | "default" | "missing";

/**
 * A single field's view within a `FormCard`.
 *
 * - `value` — the document-supplied value (`null` when absent).
 * - `default` — the schema default (`null` when no default is declared).
 * - `source` — where the effective value comes from.
 */
export interface FormFieldValue {
    value: unknown;
    default: unknown;
    source: FormFieldSource;
}

/**
 * A card viewed through its schema, as returned by `Quill.form`,
 * `Quill.blankMain`, and `Quill.blankCard`.
 */
export interface FormCard {
    schema: QuillCardSchema;
    values: Record<string, FormFieldValue>;
}

/**
 * Schema-aware form view of a document, returned by `Quill.form`.
 *
 * - `main` — the main card viewed through the quill's main schema.
 * - `cards` — composable card blocks, in document order (unknown tags excluded).
 * - `diagnostics` — diagnostics from unknown card tags and validation.
 */
export interface Form {
    main: FormCard;
    cards: FormCard[];
    diagnostics: Diagnostic[];
}
"#;

/// TypeScript declaration for the `pushCard` / `insertCard` input shape.
///
/// `tag` is required; `fields` and `body` are optional (defaulted by serde).
/// Emitted via `typescript_custom_section` so it lands in the generated
/// `.d.ts` without forcing consumers to import a nominal type — the
/// `unchecked_param_type` attribute on each method references it by name.
#[wasm_bindgen(typescript_custom_section)]
const CARD_INPUT_TS: &'static str = r#"
/**
 * Input shape for `Document.pushCard` and `Document.insertCard`.
 *
 * Only `tag` is required. `fields` defaults to `{}`, `body` to `""`.
 */
export interface CardInput {
    tag: string;
    fields?: Record<string, unknown>;
    body?: string;
}
"#;

/// Backend identifier for the only canvas-capable backend today. Both
/// `Quill::supportsCanvas` and `RenderSession::supportsCanvas` route
/// through this so the two APIs can't drift; if a second canvas backend
/// ever ships, replace this with a richer check.
const CANVAS_BACKEND_ID: &str = "typst";

/// Maximum backing-store dimension the painter will produce, in device
/// pixels per side. Real browser limits vary (~32k on Chrome/Firefox,
/// 16k on Safari, lower on memory-constrained devices); 16384 is the
/// floor that works everywhere we ship to. When a requested
/// `layoutScale * densityScale` would exceed this, the painter clamps
/// `densityScale` proportionally and surfaces the actual backing
/// dimensions in the returned `PaintResult` so consumers can detect the
/// clamp.
const MAX_BACKING_DIMENSION: u32 = 16384;

fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dur = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        dur.as_millis() as f64
    }
}

/// Quillmark WASM Engine
#[wasm_bindgen]
pub struct Quillmark {
    inner: quillmark::Quillmark,
}

/// Opaque, shareable Quill handle.
#[wasm_bindgen]
pub struct Quill {
    inner: quillmark::Quill,
}

/// An iterative render handle backed by an immutable compiled snapshot.
///
/// Created via [`Quill::open`]. Holds the compiled output so that
/// [`RenderSession::render`], [`RenderSession::paint`], and
/// [`RenderSession::page_size`] can be called repeatedly without
/// recompiling.
///
/// **Empty documents.** A document that compiles to zero pages still
/// produces a valid session (`pageCount === 0`). Iterating
/// `0..pageCount` is then a no-op; calling `paint(ctx, 0)` or
/// `pageSize(0)` throws `"... page index 0 out of range
/// (pageCount=0)"`. Hosts that surface "no pages to preview" UI should
/// branch on `pageCount === 0` rather than on a thrown error.
#[wasm_bindgen]
pub struct RenderSession {
    inner: quillmark_core::RenderSession,
    backend_id: String,
}

/// Typed in-memory Quillmark document.
///
/// Created via `Document.fromMarkdown(markdown)`. Exposes:
/// - `quillRef` (string)
/// - `frontmatter` (JS object/Record)
/// - `body` (string)
/// - `cards` (array of Card objects)
/// - `warnings` (array of Diagnostic objects)
///
/// `toMarkdown()` emits canonical Quillmark Markdown that round-trips back to
/// an equal `Document` by value and by type.
#[wasm_bindgen]
pub struct Document {
    inner: quillmark_core::Document,
    /// Parse-time warnings (e.g. near-miss sentinel lints).
    parse_warnings: Vec<quillmark_core::Diagnostic>,
}

impl Default for Quillmark {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Quillmark {
    /// JavaScript constructor: `new Quillmark()`
    #[wasm_bindgen(constructor)]
    pub fn new() -> Quillmark {
        Quillmark {
            inner: quillmark::Quillmark::new(),
        }
    }

    /// Load a quill from a file tree and attach the appropriate backend.
    ///
    /// Accepts either a `Map<string, Uint8Array>` or a plain object
    /// (`Record<string, Uint8Array>`). Plain objects are walked via
    /// `Object.entries` at the boundary; the Rust side sees a single
    /// canonical shape.
    #[wasm_bindgen(js_name = quill)]
    pub fn quill(
        &self,
        #[wasm_bindgen(unchecked_param_type = "Map<string, Uint8Array>")] tree: JsValue,
    ) -> Result<Quill, JsValue> {
        let root = file_tree_from_js_tree(&tree)?;
        let quill = self
            .inner
            .quill(root)
            .map_err(|e| WasmError::from(e).to_js_value())?;
        Ok(Quill { inner: quill })
    }
}

#[wasm_bindgen]
impl Quill {
    /// Render a document to final artifacts.
    #[wasm_bindgen(js_name = render)]
    pub fn render(
        &self,
        doc: &Document,
        opts: Option<RenderOptions>,
    ) -> Result<RenderResult, JsValue> {
        let start = now_ms();
        let rust_opts: quillmark_core::RenderOptions = opts.unwrap_or_default().into();
        let result = self
            .inner
            .render(&doc.inner, &rust_opts)
            .map_err(|e| WasmError::from(e).to_js_value())?;
        let mut warnings: Vec<Diagnostic> =
            doc.parse_warnings.iter().cloned().map(Into::into).collect();
        warnings.extend(result.warnings.into_iter().map(Into::into));
        Ok(RenderResult {
            artifacts: result.artifacts.into_iter().map(Into::into).collect(),
            warnings,
            output_format: result.output_format.into(),
            render_time_ms: now_ms() - start,
        })
    }

    /// Open an iterative render session for page-selective rendering.
    #[wasm_bindgen(js_name = open)]
    pub fn open(&self, doc: &Document) -> Result<RenderSession, JsValue> {
        let session = self
            .inner
            .open(&doc.inner)
            .map_err(|e| WasmError::from(e).to_js_value())?;
        Ok(RenderSession {
            inner: session,
            backend_id: self.inner.backend_id().to_string(),
        })
    }

    /// The resolved backend identifier (e.g. `"typst"`).
    #[wasm_bindgen(getter, js_name = backendId)]
    pub fn backend_id(&self) -> String {
        self.inner.backend_id().to_string()
    }

    /// Whether this quill's backend supports canvas preview.
    ///
    /// `true` iff `RenderSession.paint` and `RenderSession.pageSize` will
    /// succeed for sessions opened by this quill. Use this as a precondition
    /// probe before mounting a canvas-based preview UI; the throw on `paint`
    /// remains the enforcement contract.
    #[wasm_bindgen(getter, js_name = supportsCanvas)]
    pub fn supports_canvas(&self) -> bool {
        self.inner.backend_id() == CANVAS_BACKEND_ID
    }

    /// Auto-generated annotated Markdown blueprint for LLM consumers.
    #[wasm_bindgen(getter, js_name = blueprint)]
    pub fn blueprint(&self) -> String {
        self.inner.source().config().blueprint()
    }

    /// Document schema with `ui` hints stripped — for LLM/MCP consumers.
    #[wasm_bindgen(getter, js_name = schema, unchecked_return_type = "QuillSchema")]
    pub fn schema(&self) -> JsValue {
        let value = self.inner.source().config().schema();
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        value.serialize(&serializer).unwrap_or(JsValue::UNDEFINED)
    }

    /// Identity snapshot of the `quill:` section of `Quill.yaml`, plus
    /// `supportedFormats` and any custom `quill:` keys.
    ///
    /// Consumers that need validation run their own validator against
    /// `metadata.schema`.
    ///
    /// Equivalent by value for the lifetime of the handle; the quill is
    /// immutable once constructed.
    #[wasm_bindgen(getter, js_name = metadata, unchecked_return_type = "QuillMetadata")]
    pub fn metadata(&self) -> JsValue {
        let source = self.inner.source();
        let config = source.config();

        let mut obj = serde_json::Map::new();
        obj.insert(
            "name".to_string(),
            serde_json::Value::String(config.name.clone()),
        );
        obj.insert(
            "version".to_string(),
            serde_json::Value::String(config.version.clone()),
        );
        obj.insert(
            "backend".to_string(),
            serde_json::Value::String(config.backend.clone()),
        );
        obj.insert(
            "author".to_string(),
            serde_json::Value::String(config.author.clone()),
        );
        obj.insert(
            "description".to_string(),
            serde_json::Value::String(config.description.clone()),
        );

        let formats: Vec<serde_json::Value> = self
            .inner
            .supported_formats()
            .iter()
            .map(|f| {
                let wasm_format: crate::types::OutputFormat = (*f).into();
                serde_json::to_value(wasm_format).unwrap_or(serde_json::Value::Null)
            })
            .collect();
        obj.insert(
            "supportedFormats".to_string(),
            serde_json::Value::Array(formats),
        );

        // Unstructured keys declared under `quill:` (excluding fields already
        // surfaced above or now living under `schema`).
        for (key, value) in source.metadata() {
            if matches!(
                key.as_str(),
                "name" | "backend" | "description" | "version" | "author"
            ) {
                continue;
            }
            if obj.contains_key(key) {
                continue;
            }
            obj.insert(key.clone(), value.as_json().clone());
        }

        let val = serde_json::Value::Object(obj);
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        val.serialize(&serializer).unwrap_or(JsValue::UNDEFINED)
    }

    /// The schema-aware form view of `doc`.
    ///
    /// Returns a plain JS object (not a class) that is immediately
    /// `JSON.stringify`-able. The shape mirrors [`Form`]:
    ///
    /// ```json
    /// {
    ///   "main":  { "schema": {...}, "values": { "field": {...} } },
    ///   "cards": [ ... ],
    ///   "diagnostics": [ ... ]
    /// }
    /// ```
    ///
    /// **Snapshot semantics.** This is a read-only snapshot of the document
    /// at call time. Subsequent edits to `doc` require calling `form` again.
    ///
    /// [`Form`]: quillmark::form::Form
    #[wasm_bindgen(js_name = form, unchecked_return_type = "Form")]
    pub fn form(&self, doc: &Document) -> Result<JsValue, JsValue> {
        let form = self.inner.form(&doc.inner);
        let serializer = serde_wasm_bindgen::Serializer::new()
            .serialize_maps_as_objects(true)
            .serialize_missing_as_null(true);
        form.serialize(&serializer)
            .map_err(|e| WasmError::from(format!("form: serialization failed: {e}")).to_js_value())
    }

    /// A blank form for the main card — no document values supplied.
    ///
    /// Returns a plain JS object with the same shape as one entry in
    /// [`Form::main`]. Every declared field's `source` is `"default"` (when
    /// the schema declares a default) or `"missing"`.
    ///
    /// [`Form::main`]: quillmark::form::Form::main
    #[wasm_bindgen(js_name = blankMain, unchecked_return_type = "FormCard")]
    pub fn blank_main(&self) -> Result<JsValue, JsValue> {
        let card = self.inner.blank_main();
        let serializer = serde_wasm_bindgen::Serializer::new()
            .serialize_maps_as_objects(true)
            .serialize_missing_as_null(true);
        card.serialize(&serializer).map_err(|e| {
            WasmError::from(format!("blankMain: serialization failed: {e}")).to_js_value()
        })
    }

    /// A blank form for a card of the given type — no document values supplied.
    ///
    /// Returns `null` if `cardKind` is not declared in this quill's schema.
    /// Otherwise returns a plain JS object shaped like a single entry in
    /// [`Form::cards`].
    ///
    /// [`Form::cards`]: quillmark::form::Form::cards
    #[wasm_bindgen(js_name = blankCard, unchecked_return_type = "FormCard | null")]
    pub fn blank_card(&self, card_kind: &str) -> Result<JsValue, JsValue> {
        match self.inner.blank_card(card_kind) {
            Some(card) => {
                let serializer = serde_wasm_bindgen::Serializer::new()
                    .serialize_maps_as_objects(true)
                    .serialize_missing_as_null(true);
                card.serialize(&serializer).map_err(|e| {
                    WasmError::from(format!("blankCard: serialization failed: {e}")).to_js_value()
                })
            }
            None => Ok(JsValue::NULL),
        }
    }
}

#[wasm_bindgen]
impl Document {
    /// Parse markdown into a typed Document.
    ///
    /// Returns the document with any parse-time warnings accessible via `.warnings`.
    /// Throws on parse errors.
    #[wasm_bindgen(js_name = fromMarkdown)]
    pub fn from_markdown(markdown: &str) -> Result<Document, JsValue> {
        let output = quillmark_core::Document::from_markdown_with_warnings(markdown)
            .map_err(WasmError::from)
            .map_err(|e| e.to_js_value())?;

        Ok(Document {
            inner: output.document,
            parse_warnings: output.warnings,
        })
    }

    /// Emit canonical Quillmark Markdown.
    ///
    /// Returns the document serialised as a Quillmark Markdown string.
    /// The output is type-fidelity round-trip safe: re-parsing the result
    /// produces a `Document` equal to `self` by value and by type.
    #[wasm_bindgen(js_name = toMarkdown)]
    pub fn to_markdown(&self) -> String {
        self.inner.to_markdown()
    }

    /// Return a fresh `Document` handle with the same parse state.
    ///
    /// Mutations on the returned handle do not affect the original and
    /// vice versa. Parse-time warnings are snapshotted alongside the
    /// document — they describe the original parse, not the edit
    /// history of either handle.
    #[wasm_bindgen(js_name = clone)]
    pub fn clone_doc(&self) -> Document {
        Document {
            inner: self.inner.clone(),
            parse_warnings: self.parse_warnings.clone(),
        }
    }

    /// The QUILL reference string (e.g. `"usaf_memo@0.1"`).
    #[wasm_bindgen(getter, js_name = quillRef)]
    pub fn quill_ref(&self) -> String {
        self.inner.quill_reference().to_string()
    }

    /// The document's main (entry) card.
    ///
    /// Carries the QUILL sentinel, the document-level frontmatter, and the
    /// global body. Frontmatter/body reads and mutations go through this
    /// handle — there are no document-level shortcuts after the rework.
    ///
    /// Allocates and serializes on each call — cache locally if read in a hot loop.
    #[wasm_bindgen(getter, js_name = main, unchecked_return_type = "Card")]
    pub fn main(&self) -> JsValue {
        let card = Card::from(self.inner.main());
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        card.serialize(&serializer).unwrap_or(JsValue::UNDEFINED)
    }

    /// Ordered list of composable card blocks as typed `Card` objects.
    #[wasm_bindgen(getter, js_name = cards, unchecked_return_type = "Card[]")]
    pub fn cards(&self) -> JsValue {
        let cards: Vec<Card> = self.inner.cards().iter().map(Card::from).collect();
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        cards.serialize(&serializer).unwrap_or(JsValue::UNDEFINED)
    }

    /// Number of composable cards (excludes the main card).
    ///
    /// O(1). Use this to validate indices before calling card mutators
    /// instead of allocating the full `cards` array.
    #[wasm_bindgen(getter, js_name = cardCount)]
    pub fn card_count(&self) -> usize {
        self.inner.cards().len()
    }

    /// Structural equality against another `Document`.
    ///
    /// Compares `main` and `cards` by value (matching core's [`PartialEq`]).
    /// Parse-time `warnings` are intentionally excluded — they describe the
    /// source text, not the document's content.
    ///
    /// Use this to debounce upstream prop updates: keep the last parsed
    /// `Document` and compare instead of re-parsing on every keystroke.
    #[wasm_bindgen(js_name = equals)]
    pub fn equals(&self, other: &Document) -> bool {
        self.inner == other.inner
    }

    /// Non-fatal parse-time warnings as an array of typed `Diagnostic` objects.
    #[wasm_bindgen(getter, js_name = warnings, unchecked_return_type = "Diagnostic[]")]
    pub fn warnings(&self) -> JsValue {
        let diags: Vec<Diagnostic> = self
            .parse_warnings
            .iter()
            .cloned()
            .map(Into::into)
            .collect();
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        diags.serialize(&serializer).unwrap_or(JsValue::UNDEFINED)
    }

    // ── Mutators ──────────────────────────────────────────────────────────────

    /// Update a frontmatter field on the main card.
    ///
    /// Convenience method: equivalent to `doc.mainMut().setField(name, value)`.
    /// Clears any existing `!fill` marker on the field.
    ///
    /// Throws an `Error` whose message includes the `EditError` variant name and
    /// details if `name` is reserved (`BODY`, `CARDS`, `QUILL`, `CARD`) or does
    /// not match `[a-z_][a-z0-9_]*`.
    ///
    /// Mutators never modify `warnings`.
    #[wasm_bindgen(js_name = setField)]
    pub fn set_field(&mut self, name: &str, value: JsValue) -> Result<(), JsValue> {
        let json: serde_json::Value = serde_wasm_bindgen::from_value(value).map_err(|e| {
            WasmError::from(format!("setField: invalid value: {}", e)).to_js_value()
        })?;
        let qv = quillmark_core::QuillValue::from_json(json);
        self.inner
            .main_mut()
            .set_field(name, qv)
            .map_err(|e| edit_error_to_js(&e))
    }

    /// Update a frontmatter field on the main card AND mark it as `!fill`.
    ///
    /// Convenience method: equivalent to `doc.mainMut().setFill(name, value)`.
    ///
    /// Throws on invalid name (see [`setField`](Document::set_field)).
    ///
    /// Mutators never modify `warnings`.
    #[wasm_bindgen(js_name = setFill)]
    pub fn set_fill(&mut self, name: &str, value: JsValue) -> Result<(), JsValue> {
        let json: serde_json::Value = serde_wasm_bindgen::from_value(value)
            .map_err(|e| WasmError::from(format!("setFill: invalid value: {}", e)).to_js_value())?;
        let qv = quillmark_core::QuillValue::from_json(json);
        self.inner
            .main_mut()
            .set_fill(name, qv)
            .map_err(|e| edit_error_to_js(&e))
    }

    /// Remove a frontmatter field on the main card, returning the removed value or `undefined`.
    ///
    /// Throws an `Error` whose message includes the `EditError` variant name
    /// and details if `name` is reserved (`BODY`, `CARDS`, `QUILL`, `CARD`)
    /// or does not match `[a-z_][a-z0-9_]*`. Absence of an otherwise-valid
    /// name returns `undefined`.
    ///
    /// Mutators never modify `warnings`.
    #[wasm_bindgen(js_name = removeField)]
    pub fn remove_field(&mut self, name: &str) -> Result<JsValue, JsValue> {
        let removed = self
            .inner
            .main_mut()
            .remove_field(name)
            .map_err(|e| edit_error_to_js(&e))?;
        Ok(match removed {
            Some(v) => {
                let serializer =
                    serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
                v.as_json()
                    .serialize(&serializer)
                    .unwrap_or(JsValue::UNDEFINED)
            }
            None => JsValue::UNDEFINED,
        })
    }

    /// Replace the QUILL reference string.
    ///
    /// Throws if `ref_str` is not a valid `QuillReference`.
    ///
    /// Mutators never modify `warnings`.
    #[wasm_bindgen(js_name = setQuillRef)]
    pub fn set_quill_ref(&mut self, ref_str: &str) -> Result<(), JsValue> {
        let qr: quillmark_core::QuillReference = ref_str.parse().map_err(|e| {
            WasmError::from(format!(
                "setQuillRef: invalid reference '{}': {}",
                ref_str, e
            ))
            .to_js_value()
        })?;
        self.inner.set_quill_ref(qr);
        Ok(())
    }

    /// Replace the main card's body (the global Markdown body).
    ///
    /// Mutators never modify `warnings`.
    #[wasm_bindgen(js_name = replaceBody)]
    pub fn replace_body(&mut self, body: &str) {
        self.inner.main_mut().replace_body(body);
    }

    /// Append a card to the end of the card list.
    ///
    /// `card` must be a JS object with a `tag` string field and optional
    /// `fields` (object) and `body` (string).
    ///
    /// Throws an `Error` if `card.tag` is not a valid tag name.
    ///
    /// Mutators never modify `warnings`.
    #[wasm_bindgen(js_name = pushCard)]
    pub fn push_card(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "CardInput")] card: JsValue,
    ) -> Result<(), JsValue> {
        let core_card = js_value_to_card(&card)?;
        self.inner.push_card(core_card);
        Ok(())
    }

    /// Insert a card at the given index.
    ///
    /// `index` must be in `0..=cards.length`. Out-of-range throws an `Error`.
    ///
    /// Mutators never modify `warnings`.
    #[wasm_bindgen(js_name = insertCard)]
    pub fn insert_card(
        &mut self,
        index: usize,
        #[wasm_bindgen(unchecked_param_type = "CardInput")] card: JsValue,
    ) -> Result<(), JsValue> {
        let core_card = js_value_to_card(&card)?;
        self.inner
            .insert_card(index, core_card)
            .map_err(|e| edit_error_to_js(&e))
    }

    /// Remove the card at `index` and return it, or `undefined` if out of range.
    ///
    /// Mutators never modify `warnings`.
    #[wasm_bindgen(js_name = removeCard, unchecked_return_type = "Card | undefined")]
    pub fn remove_card(&mut self, index: usize) -> JsValue {
        match self.inner.remove_card(index) {
            Some(core_card) => {
                let card = Card::from(&core_card);
                let serializer =
                    serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
                card.serialize(&serializer).unwrap_or(JsValue::UNDEFINED)
            }
            None => JsValue::UNDEFINED,
        }
    }

    /// Move the card at `from` to position `to`.
    ///
    /// `from == to` is a no-op. Both indices must be in `0..cards.length`.
    /// Out-of-range throws an `Error`.
    ///
    /// Mutators never modify `warnings`.
    #[wasm_bindgen(js_name = moveCard)]
    pub fn move_card(&mut self, from: usize, to: usize) -> Result<(), JsValue> {
        self.inner
            .move_card(from, to)
            .map_err(|e| edit_error_to_js(&e))
    }

    /// Replace the tag of the composable card at `index`.
    ///
    /// Mutates only the sentinel — the card's frontmatter and body are
    /// untouched. Schema-aware migration (clearing orphan fields, applying
    /// new defaults) is the caller's responsibility; `setCardTag` is a
    /// structural primitive.
    ///
    /// Throws if `index` is out of range or if `newTag` does not match
    /// `[a-z_][a-z0-9_]*`.
    ///
    /// Mutators never modify `warnings`.
    #[wasm_bindgen(js_name = setCardTag)]
    pub fn set_card_tag(&mut self, index: usize, new_tag: &str) -> Result<(), JsValue> {
        self.inner
            .set_card_tag(index, new_tag)
            .map_err(|e| edit_error_to_js(&e))
    }

    /// Update a field on the card at `index`.
    ///
    /// Convenience method: equivalent to `doc.card_mut(index)?.set_field(name, value)`.
    ///
    /// Throws if `index` is out of range, `name` is reserved or invalid, or
    /// `value` cannot be serialized.
    ///
    /// Mutators never modify `warnings`.
    #[wasm_bindgen(js_name = updateCardField)]
    pub fn update_card_field(
        &mut self,
        index: usize,
        name: &str,
        value: JsValue,
    ) -> Result<(), JsValue> {
        let len = self.inner.cards().len();
        let card = self.inner.card_mut(index).ok_or_else(|| {
            edit_error_to_js(&quillmark_core::EditError::IndexOutOfRange { index, len })
        })?;
        let json: serde_json::Value = serde_wasm_bindgen::from_value(value).map_err(|e| {
            WasmError::from(format!("updateCardField: invalid value: {}", e)).to_js_value()
        })?;
        let qv = quillmark_core::QuillValue::from_json(json);
        card.set_field(name, qv).map_err(|e| edit_error_to_js(&e))
    }

    /// Remove a frontmatter field on the card at `index`, returning the
    /// removed value or `undefined` if the field was absent.
    ///
    /// Throws if `index` is out of range, `name` is reserved, or `name` does
    /// not match `[a-z_][a-z0-9_]*`.
    ///
    /// Mutators never modify `warnings`.
    #[wasm_bindgen(js_name = removeCardField)]
    pub fn remove_card_field(&mut self, index: usize, name: &str) -> Result<JsValue, JsValue> {
        let len = self.inner.cards().len();
        let card = self.inner.card_mut(index).ok_or_else(|| {
            edit_error_to_js(&quillmark_core::EditError::IndexOutOfRange { index, len })
        })?;
        let removed = card.remove_field(name).map_err(|e| edit_error_to_js(&e))?;
        Ok(match removed {
            Some(v) => {
                let serializer =
                    serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
                v.as_json()
                    .serialize(&serializer)
                    .unwrap_or(JsValue::UNDEFINED)
            }
            None => JsValue::UNDEFINED,
        })
    }

    /// Replace the body of the card at `index`.
    ///
    /// Throws if `index` is out of range.
    ///
    /// Mutators never modify `warnings`.
    #[wasm_bindgen(js_name = updateCardBody)]
    pub fn update_card_body(&mut self, index: usize, body: &str) -> Result<(), JsValue> {
        let len = self.inner.cards().len();
        let card = self.inner.card_mut(index).ok_or_else(|| {
            edit_error_to_js(&quillmark_core::EditError::IndexOutOfRange { index, len })
        })?;
        card.replace_body(body);
        Ok(())
    }
}

// ── Edit helpers ──────────────────────────────────────────────────────────────

/// Convert an [`quillmark_core::EditError`] into a JS `Error` value whose
/// message includes the variant name and details.
fn edit_error_to_js(err: &quillmark_core::EditError) -> JsValue {
    let variant = match err {
        quillmark_core::EditError::ReservedName(_) => "ReservedName",
        quillmark_core::EditError::InvalidFieldName(_) => "InvalidFieldName",
        quillmark_core::EditError::InvalidTagName(_) => "InvalidTagName",
        quillmark_core::EditError::IndexOutOfRange { .. } => "IndexOutOfRange",
    };
    WasmError::from(format!("[EditError::{}] {}", variant, err)).to_js_value()
}

/// Deserialise a JS object `{ tag: string, fields?: object, body?: string }`
/// into a [`quillmark_core::Card`].  Throws on invalid tag.
fn js_value_to_card(value: &JsValue) -> Result<quillmark_core::Card, JsValue> {
    #[derive(Deserialize)]
    struct CardInput {
        tag: String,
        #[serde(default)]
        fields: serde_json::Map<String, serde_json::Value>,
        #[serde(default)]
        body: String,
    }

    let input: CardInput = serde_wasm_bindgen::from_value(value.clone()).map_err(|e| {
        WasmError::from(format!("card must be {{ tag, fields?, body? }}: {}", e)).to_js_value()
    })?;

    // Validate tag via Card::new, then upgrade with fields and body.
    let mut card = quillmark_core::Card::new(input.tag).map_err(|e| edit_error_to_js(&e))?;

    for (k, v) in input.fields {
        let qv = quillmark_core::QuillValue::from_json(v);
        card.set_field(&k, qv).map_err(|e| edit_error_to_js(&e))?;
    }
    card.replace_body(input.body);
    Ok(card)
}

fn file_tree_from_js_tree(tree: &JsValue) -> Result<quillmark_core::FileTreeNode, JsValue> {
    let entries = js_tree_entries(tree)?;
    let mut root = quillmark_core::FileTreeNode::Directory {
        files: HashMap::new(),
    };

    for (path, value) in entries {
        let bytes = js_bytes_for_tree_entry(&path, value)?;
        root.insert(
            path.as_str(),
            quillmark_core::FileTreeNode::File { contents: bytes },
        )
        .map_err(|e| {
            WasmError::from(format!("Invalid tree path '{}': {}", path, e)).to_js_value()
        })?;
    }

    Ok(root)
}

fn js_tree_entries(tree: &JsValue) -> Result<Vec<(String, JsValue)>, JsValue> {
    if tree.is_instance_of::<js_sys::Map>() {
        let map = tree.clone().unchecked_into::<js_sys::Map>();
        let iter = js_sys::try_iter(&map.entries())
            .map_err(|e| {
                WasmError::from(format!("Failed to iterate Map entries: {:?}", e)).to_js_value()
            })?
            .ok_or_else(|| WasmError::from("Map entries are not iterable").to_js_value())?;

        let mut entries: Vec<(String, JsValue)> = Vec::new();
        for entry in iter {
            let pair = entry.map_err(|e| {
                WasmError::from(format!("Failed to read Map entry: {:?}", e)).to_js_value()
            })?;
            let pair = Array::from(&pair);
            let path = pair
                .get(0)
                .as_string()
                .ok_or_else(|| WasmError::from("quill Map key must be a string").to_js_value())?;
            let value = pair.get(1);
            entries.push((path, value));
        }
        return Ok(entries);
    }

    // Plain object: walk via `Object.entries`.
    if tree.is_object() && !tree.is_null() {
        let obj = tree.clone().unchecked_into::<js_sys::Object>();
        let pairs = js_sys::Object::entries(&obj);
        let mut entries: Vec<(String, JsValue)> = Vec::with_capacity(pairs.length() as usize);
        for i in 0..pairs.length() {
            let pair = Array::from(&pairs.get(i));
            let path = pair.get(0).as_string().ok_or_else(|| {
                WasmError::from("quill object key must be a string").to_js_value()
            })?;
            entries.push((path, pair.get(1)));
        }
        return Ok(entries);
    }

    Err(
        WasmError::from("quill requires a Map<string, Uint8Array> or Record<string, Uint8Array>")
            .to_js_value(),
    )
}

fn js_bytes_for_tree_entry(path: &str, value: JsValue) -> Result<Vec<u8>, JsValue> {
    if !value.is_instance_of::<Uint8Array>() {
        return Err(WasmError::from(format!(
            "Invalid tree entry '{}': expected Uint8Array value",
            path
        ))
        .to_js_value());
    }

    let bytes = value.unchecked_into::<Uint8Array>();
    Ok(bytes.to_vec())
}

/// TypeScript declarations for the canvas-preview surface.
///
/// `paint` is the single source of truth for canvas backing-store sizing —
/// consumers do not multiply by `devicePixelRatio` themselves and do not
/// write to `canvas.width` / `canvas.height` directly. They supply layout
/// (`layoutScale`) and density (`densityScale`) inputs separately; the
/// painter folds them into the rasterization scale, sizes the backing
/// store, and reports what it picked.
#[wasm_bindgen(typescript_custom_section)]
const CANVAS_PREVIEW_TS: &'static str = r#"
/**
 * Page dimensions in Typst points (1 pt = 1/72 inch).
 *
 * Report-only: the painter sizes the canvas itself based on
 * `PaintOptions`. `pageSize` is exposed for callers that need page
 * geometry up-front (e.g. to lay out a scrollable list of canvases
 * before any pixels are rendered).
 */
export interface PageSize {
    widthPt: number;
    heightPt: number;
}

/**
 * Inputs to `RenderSession.paint`. Both fields are optional and default
 * to `1`.
 *
 * - `layoutScale` — layout-space pixels per Typst point. For on-screen
 *   canvases this is CSS pixels per pt; the page's layout-pixel size is
 *   `widthPt * layoutScale × heightPt * layoutScale`. The painter
 *   surfaces these dimensions as `layoutWidth` / `layoutHeight` so
 *   consumers can drive `canvas.style.*` (or any layout system).
 * - `densityScale` — backing-store density multiplier. Fold
 *   `window.devicePixelRatio`, in-app zoom, and `visualViewport.scale`
 *   (pinch-zoom) into a single value here. Defaults to `1`, which
 *   produces a non-retina backing store — pass `window.devicePixelRatio`
 *   for crisp output on high-DPI displays.
 *
 * The effective rasterization scale is `layoutScale * densityScale`.
 * Both must be finite and `> 0`. For `OffscreenCanvasRenderingContext2D`
 * the two collapse to a single scalar; folding everything into
 * `densityScale` is the simplest convention.
 */
export interface PaintOptions {
    layoutScale?: number;
    densityScale?: number;
}

/**
 * Returned by `RenderSession.paint`.
 *
 * - `layoutWidth` / `layoutHeight` — layout-pixel dimensions of the
 *   canvas's display box. For on-screen canvases this is CSS pixels:
 *   set `canvas.style.width = layoutWidth + "px"` and
 *   `canvas.style.height = layoutHeight + "px"` (or feed these into
 *   your layout system). Independent of `densityScale`.
 * - `pixelWidth` / `pixelHeight` — integer backing-store pixel
 *   dimensions the painter wrote to `canvas.width` / `canvas.height`.
 *   Equal to `round(layoutWidth * densityScale)` ×
 *   `round(layoutHeight * densityScale)` *unless* the requested backing
 *   exceeded the painter's safe maximum (16384 px per side), in which
 *   case `densityScale` was clamped to fit. Detect clamping via
 *   `pixelWidth < round(layoutWidth * densityScale)`.
 *
 * The painter owns `canvas.width` / `canvas.height`; consumers must not
 * write to them. The painter does **not** touch `canvas.style.*`;
 * consumers own layout.
 *
 * For `OffscreenCanvasRenderingContext2D` (Worker rasterization, no
 * DOM), `layoutWidth` / `layoutHeight` are informational — there's no
 * CSS layout box to apply them to.
 */
export interface PaintResult {
    layoutWidth: number;
    layoutHeight: number;
    pixelWidth: number;
    pixelHeight: number;
}
"#;

#[wasm_bindgen]
impl RenderSession {
    /// Number of pages in this render session.
    ///
    /// Stable for the lifetime of the session — the underlying compiled
    /// document is an immutable snapshot.
    #[wasm_bindgen(getter, js_name = pageCount)]
    pub fn page_count(&self) -> usize {
        self.inner.page_count()
    }

    /// The backend that produced this session (e.g. `"typst"`).
    ///
    /// Equal to the `backendId` of the [`Quill`] that opened this session
    /// (sessions inherit their quill's backend), so checking either is fine.
    #[wasm_bindgen(getter, js_name = backendId)]
    pub fn backend_id(&self) -> String {
        self.backend_id.clone()
    }

    /// Whether this session's backend supports canvas preview.
    ///
    /// `true` iff [`paint`](Self::paint) and [`page_size`](Self::page_size)
    /// will succeed. Equal to `Quill.supportsCanvas` for the quill that
    /// opened this session.
    #[wasm_bindgen(getter, js_name = supportsCanvas)]
    pub fn supports_canvas(&self) -> bool {
        self.backend_id == CANVAS_BACKEND_ID
    }

    /// Session-level warnings attached at `quill.open(...)` time.
    ///
    /// Snapshot of any non-fatal diagnostics emitted while opening the
    /// session (e.g. version compatibility shims). Stable across the
    /// session's lifetime. These are also appended to
    /// [`RenderResult.warnings`] on every `render()` call; the accessor
    /// surfaces them to canvas-preview consumers that don't go through
    /// `render()`.
    #[wasm_bindgen(getter, js_name = warnings, unchecked_return_type = "Diagnostic[]")]
    pub fn warnings(&self) -> JsValue {
        let diags: Vec<Diagnostic> = self
            .inner
            .warnings()
            .iter()
            .cloned()
            .map(Into::into)
            .collect();
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        diags.serialize(&serializer).unwrap_or(JsValue::UNDEFINED)
    }

    /// Render all or selected pages from this session.
    #[wasm_bindgen(js_name = render)]
    pub fn render(&self, opts: Option<RenderOptions>) -> Result<RenderResult, JsValue> {
        let start = now_ms();
        let rust_opts: quillmark_core::RenderOptions = opts.unwrap_or_default().into();

        let result = self
            .inner
            .render(&rust_opts)
            .map_err(|e| WasmError::from(e).to_js_value())?;

        Ok(RenderResult {
            artifacts: result.artifacts.into_iter().map(Into::into).collect(),
            warnings: result.warnings.into_iter().map(Into::into).collect(),
            output_format: result.output_format.into(),
            render_time_ms: now_ms() - start,
        })
    }

    /// Page dimensions in Typst points (1 pt = 1/72 inch).
    ///
    /// Report-only: the painter sizes the canvas itself based on
    /// `PaintOptions`. Exposed for consumers that need page geometry
    /// up-front (e.g. to lay out a scrollable list of canvases before
    /// any pixels are rendered).
    ///
    /// Stable for a given `page` across the session's lifetime — the
    /// compiled document is an immutable snapshot, so callers can cache
    /// results.
    ///
    /// Throws if the underlying backend has no canvas painter (i.e. is not
    /// the Typst backend) or if `page` is out of range.
    #[wasm_bindgen(js_name = pageSize, unchecked_return_type = "PageSize")]
    pub fn page_size(&self, page: usize) -> Result<JsValue, JsValue> {
        let typst = self.typst_session("pageSize")?;
        let (width_pt, height_pt) = typst
            .page_size_pt(page)
            .ok_or_else(|| self.page_oob_error("pageSize", page))?;
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        PageSize {
            width_pt,
            height_pt,
        }
        .serialize(&serializer)
        .map_err(|e| WasmError::from(format!("pageSize: serialization failed: {e}")).to_js_value())
    }

    /// Paint `page` into a 2D canvas context.
    ///
    /// Accepts either a `CanvasRenderingContext2D` (main thread) or an
    /// `OffscreenCanvasRenderingContext2D` (Worker / off-DOM rasterization).
    /// Both dispatch to the same Rust rasterizer; the dispatch happens at
    /// the JS boundary so neither context type is privileged.
    ///
    /// The painter owns `canvas.width` / `canvas.height` and writes them
    /// itself; consumers must not. The painter does not touch
    /// `canvas.style.*` — that's layout, owned by the consumer (see
    /// `PaintResult.layoutWidth` / `layoutHeight`).
    ///
    /// `opts.layoutScale` (default 1.0) is layout-space pixels per Typst
    /// point and determines the canvas's display-box size. `opts.densityScale`
    /// (default 1.0) is the rasterization density multiplier the consumer
    /// folds `window.devicePixelRatio`, in-app zoom, and
    /// `visualViewport.scale` (pinch-zoom) into. The effective
    /// rasterization scale is `layoutScale * densityScale`.
    ///
    /// If `layoutScale * densityScale` would exceed the safe backing-store
    /// maximum (16384 px per side), `densityScale` is clamped
    /// proportionally so the largest dimension fits. The actual
    /// backing-store dimensions are reported in the returned
    /// `PaintResult` — compare against
    /// `round(layoutWidth * densityScale)` to detect clamping.
    ///
    /// Each call resets the backing store (`paint` is always a full
    /// repaint). Consumers do not need to call `clearRect`.
    ///
    /// Throws when:
    /// - the backend does not support canvas preview (message includes the
    ///   resolved `backendId`),
    /// - `page` is out of range,
    /// - `ctx` is neither `CanvasRenderingContext2D` nor
    ///   `OffscreenCanvasRenderingContext2D`,
    /// - `opts.layoutScale` or `opts.densityScale` is non-finite or `<= 0`.
    #[wasm_bindgen(js_name = paint, unchecked_return_type = "PaintResult")]
    pub fn paint(
        &self,
        #[wasm_bindgen(
            unchecked_param_type = "CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D"
        )]
        ctx: JsValue,
        page: usize,
        #[wasm_bindgen(unchecked_param_type = "PaintOptions | undefined")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        let typst = self.typst_session("paint")?;
        let canvas_ctx = CanvasCtx::from_js(&ctx)?;

        let (width_pt, height_pt) = typst
            .page_size_pt(page)
            .ok_or_else(|| self.page_oob_error("paint", page))?;

        let opts: PaintOptions = if opts.is_undefined() || opts.is_null() {
            PaintOptions::default()
        } else {
            serde_wasm_bindgen::from_value(opts).map_err(|e| {
                WasmError::from(format!("paint: invalid options: {e}")).to_js_value()
            })?
        };

        let layout_scale = opts.layout_scale.unwrap_or(1.0);
        let requested_density = opts.density_scale.unwrap_or(1.0);

        if !layout_scale.is_finite() || layout_scale <= 0.0 {
            return Err(WasmError::from(
                "paint: layoutScale must be a finite number greater than 0",
            )
            .to_js_value());
        }
        if !requested_density.is_finite() || requested_density <= 0.0 {
            return Err(WasmError::from(
                "paint: densityScale must be a finite number greater than 0",
            )
            .to_js_value());
        }

        let layout_width = (width_pt as f64) * (layout_scale as f64);
        let layout_height = (height_pt as f64) * (layout_scale as f64);

        let desired_w = (layout_width * requested_density as f64).round();
        let desired_h = (layout_height * requested_density as f64).round();
        let max_dim = desired_w.max(desired_h);

        let effective_density = if max_dim > MAX_BACKING_DIMENSION as f64 {
            (requested_density as f64) * (MAX_BACKING_DIMENSION as f64 / max_dim)
        } else {
            requested_density as f64
        };

        let render_scale = (layout_scale as f64) * effective_density;

        let (pixel_w, pixel_h, mut rgba) = typst
            .render_rgba(page, render_scale as f32)
            .ok_or_else(|| self.page_oob_error("paint", page))?;

        // The painter owns canvas.width/height. Setting these clears the
        // backing store automatically — no clearRect needed.
        canvas_ctx.set_canvas_dims(pixel_w, pixel_h)?;

        let img = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
            wasm_bindgen::Clamped(rgba.as_mut_slice()),
            pixel_w,
            pixel_h,
        )
        .map_err(|e| {
            WasmError::from(format!("paint: ImageData construction failed: {:?}", e)).to_js_value()
        })?;
        canvas_ctx.put_image_data(&img)?;

        let result = PaintResult {
            layout_width,
            layout_height,
            pixel_width: pixel_w,
            pixel_height: pixel_h,
        };
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        result
            .serialize(&serializer)
            .map_err(|e| WasmError::from(format!("paint: serialization failed: {e}")).to_js_value())
    }
}

impl RenderSession {
    /// Borrow the Typst backend's typed session, or build a JS error citing
    /// `op` (the public method name) and the resolved `backendId`.
    fn typst_session(&self, op: &str) -> Result<&quillmark_typst::TypstSession, JsValue> {
        quillmark_typst::typst_session_of(&self.inner).ok_or_else(|| {
            WasmError::from(format!(
                "{op}: backend '{}' has no canvas painter",
                self.backend_id
            ))
            .to_js_value()
        })
    }

    fn page_oob_error(&self, op: &str, page: usize) -> JsValue {
        WasmError::from(format!(
            "{op}: page index {page} out of range (pageCount={})",
            self.inner.page_count()
        ))
        .to_js_value()
    }
}

/// Adapter unifying `CanvasRenderingContext2D` and
/// `OffscreenCanvasRenderingContext2D` behind one Rust shape so `paint`
/// can size the backing store and emit pixels without repeating the
/// downcast.
enum CanvasCtx<'a> {
    OnScreen(&'a web_sys::CanvasRenderingContext2d),
    OffScreen(&'a web_sys::OffscreenCanvasRenderingContext2d),
}

impl<'a> CanvasCtx<'a> {
    fn from_js(ctx: &'a JsValue) -> Result<Self, JsValue> {
        if let Some(c) = ctx.dyn_ref::<web_sys::CanvasRenderingContext2d>() {
            return Ok(Self::OnScreen(c));
        }
        if let Some(c) = ctx.dyn_ref::<web_sys::OffscreenCanvasRenderingContext2d>() {
            return Ok(Self::OffScreen(c));
        }
        Err(WasmError::from(
            "paint: ctx must be CanvasRenderingContext2D or OffscreenCanvasRenderingContext2D",
        )
        .to_js_value())
    }

    /// Set `canvas.width` and `canvas.height`. Writing to either is
    /// specified to clear the backing store, which is exactly the
    /// contract `paint` wants on each call (full repaint, no stale
    /// pixels in transparent regions).
    fn set_canvas_dims(&self, width: u32, height: u32) -> Result<(), JsValue> {
        match self {
            Self::OnScreen(c) => {
                let canvas = c.canvas().ok_or_else(|| {
                    WasmError::from("paint: rendering context has no associated <canvas> element")
                        .to_js_value()
                })?;
                canvas.set_width(width);
                canvas.set_height(height);
            }
            Self::OffScreen(c) => {
                let canvas = c.canvas();
                canvas.set_width(width);
                canvas.set_height(height);
            }
        }
        Ok(())
    }

    fn put_image_data(&self, img: &web_sys::ImageData) -> Result<(), JsValue> {
        match self {
            Self::OnScreen(c) => c.put_image_data(img, 0.0, 0.0),
            Self::OffScreen(c) => c.put_image_data(img, 0.0, 0.0),
        }
    }
}

#[derive(Serialize)]
struct PageSize {
    #[serde(rename = "widthPt")]
    width_pt: f32,
    #[serde(rename = "heightPt")]
    height_pt: f32,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaintOptions {
    #[serde(default)]
    layout_scale: Option<f32>,
    #[serde(default)]
    density_scale: Option<f32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PaintResult {
    layout_width: f64,
    layout_height: f64,
    pixel_width: u32,
    pixel_height: u32,
}
