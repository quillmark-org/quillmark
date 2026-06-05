//! Quillmark WASM Engine - Simplified API

use crate::error::WasmError;
use crate::types::{Diagnostic, RenderOptions, RenderResult};
use js_sys::{Array, Uint8Array};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

/// TypeScript declarations for the quill metadata and schema surfaces.
/// Emitted via `typescript_custom_section` as the single source of truth.
#[wasm_bindgen(typescript_custom_section)]
const METADATA_TS: &'static str = r#"
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

/** Schema entry for a single field declared in a quill's `Quill.yaml`.
 *
 * A field's *cell* is determined by `default`: a field with a `default`
 * is **Endorsed** (the rendered value is shippable as-is), while a field
 * without a `default` is **Unendorsed** (the blueprint carries a
 * `<must-fill>` sentinel and validation reports
 * `validation::field_absent` if the field is absent at validate
 * time — a non-fatal signal, since the render path zero-fills an absent
 * field). There is no separate `required` axis.
 */
export interface QuillFieldSchema {
    type: "string" | "number" | "integer" | "boolean" | "array" | "object" | "datetime" | "markdown";
    description?: string;
    default?: unknown;
    example?: unknown;
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
 * Describes only the user-fillable fields. The quill reference
 * (constructed as `${metadata.name}@${metadata.version}`) and card-kind
 * discriminators are document-level metadata, not schema fields.
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
"#;

/// TypeScript declaration for `pushCard`/`insertCard` input shape.
/// Referenced by name via `unchecked_param_type` on those methods.
/// TypeScript for the canonical `Card` wire shape (mirrors
/// `quillmark_core::CardWire`). The single shape both *returned* by
/// `Document.main` / `cards` / `removeCard` / `quill.seedCard` and *accepted*
/// by `pushCard` / `insertCard` / `Document.makeCard`.
#[wasm_bindgen(typescript_custom_section)]
const CARD_TS: &'static str = r#"
/** A field or comment entry in a `Card.payloadItems` list. */
export type PayloadItem =
    | { type: "field"; key: string; value: unknown; fill?: boolean }
    | { type: "comment"; text: string; inline?: boolean };

/**
 * A single card block. The one shape exchanged in both directions: returned by
 * `Document.main` / `Document.cards` / `Document.removeCard` / `Quill.seedCard`,
 * and accepted by `Document.pushCard` / `Document.insertCard`. Build a fresh
 * one with `Document.makeCard`.
 *
 * `$` system entries are hoisted to named fields: `kind` (the `$kind`, empty
 * string when none), optional `quill` (the `$quill` `name@version`, main card
 * only), optional `id` (`$id`), and optional `ext` (`$ext`). `payloadItems`
 * carries user fields and comments in order.
 */
export interface Card {
    kind: string;
    quill?: string;
    id?: string;
    ext?: Record<string, unknown>;
    payloadItems: PayloadItem[];
    body: string;
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

#[wasm_bindgen]
pub struct Quillmark {
    inner: quillmark::Quillmark,
}

#[wasm_bindgen]
pub struct Quill {
    inner: quillmark::Quill,
}

/// Iterative render handle backed by an immutable compiled snapshot.
///
/// **Empty documents.** A zero-page document yields a valid session
/// (`pageCount === 0`); `paint(ctx, 0)` or `pageSize(0)` throws with
/// `"page index 0 out of range (pageCount=0)"`. Branch on `pageCount === 0`
/// rather than catching the error.
#[wasm_bindgen]
pub struct RenderSession {
    inner: quillmark_core::RenderSession,
    backend_id: String,
}

/// Typed in-memory Quillmark document.
#[wasm_bindgen]
pub struct Document {
    inner: quillmark_core::Document,
    /// Parse-time warnings (e.g. a `~~~` opener missing its blank line).
    parse_warnings: Vec<quillmark_core::Diagnostic>,
}

impl Default for Quillmark {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Quillmark {
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

    /// `true` iff `RenderSession.paint` and `RenderSession.pageSize` will
    /// succeed for sessions opened by this quill. Use as a precondition
    /// probe before mounting a canvas-based preview UI.
    #[wasm_bindgen(getter, js_name = supportsCanvas)]
    pub fn supports_canvas(&self) -> bool {
        self.inner.backend_id() == CANVAS_BACKEND_ID
    }

    #[wasm_bindgen(getter, js_name = blueprint)]
    pub fn blueprint(&self) -> String {
        self.inner.source().config().blueprint()
    }

    /// Document schema for the quill: the user-fillable fields plus their
    /// `ui` hints (group / order / showWhen). The single field-metadata
    /// surface — drives form editors and LLM/MCP consumers alike. Returns the
    /// `QuillSchema` shape.
    #[wasm_bindgen(getter, js_name = schema, unchecked_return_type = "QuillSchema")]
    pub fn schema(&self) -> JsValue {
        let value = self.inner.source().config().schema();
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        value.serialize(&serializer).unwrap_or(JsValue::UNDEFINED)
    }

    /// Identity snapshot of the `quill:` section of `Quill.yaml`, plus
    /// `supportedFormats` and any extra `quill:` keys.
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

    /// Validate `doc` against this quill's schema, returning every diagnostic
    /// (an empty array when the document is valid).
    ///
    /// Forwards the canonical `validation::*` diagnostics — same `code`,
    /// `path`, and `hint` the engine emits — including the non-fatal
    /// `validation::field_absent` completeness signal that `render` demotes.
    /// Field values, defaults, and order are not part of this surface: read
    /// them from the `Document` payload and `Quill.schema` (fields carry
    /// `ui.order`).
    #[wasm_bindgen(js_name = validate, unchecked_return_type = "Diagnostic[]")]
    pub fn validate(&self, doc: &Document) -> Result<JsValue, JsValue> {
        let diags = self.inner.validate(&doc.inner);
        let serializer = serde_wasm_bindgen::Serializer::new()
            .serialize_maps_as_objects(true)
            .serialize_missing_as_null(true);
        diags.serialize(&serializer).map_err(|e| {
            WasmError::from(format!("validate: serialization failed: {e}")).to_js_value()
        })
    }

    /// Seed a starter `Document` from the schema — the main card plus one
    /// instance of each composable card kind, each committing its fields'
    /// `example:` values and leaving every other field absent (interpolated at
    /// render: `default:`, else type-empty zero). Illustration-first: a field
    /// with both an `example` and a `default` renders its example. See
    /// `prose/canon/SCHEMAS.md` § "Document seeding".
    #[wasm_bindgen(js_name = seedDocument)]
    pub fn seed_document(&self) -> Document {
        Document {
            inner: self.inner.seed_document(),
            parse_warnings: Vec::new(),
        }
    }

    /// Seed a starter main `Card` (carries `$quill`) from the schema — the
    /// `$kind: main` card of [`seedDocument`](Self::seed_document) in
    /// isolation, committing each field's `example:` value. Returns the same
    /// `Card` shape as the `Document.main` getter.
    #[wasm_bindgen(js_name = seedMain, unchecked_return_type = "Card")]
    pub fn seed_main(&self) -> JsValue {
        card_to_js(&self.inner.seed_main())
    }

    /// Seed a starter composable `Card` of the given kind (carries `$kind`),
    /// committing its fields' `example:` values and leaving every other field
    /// absent. Returns `undefined` if `cardKind` is not declared in this
    /// quill's schema, else a `Card` that feeds straight into
    /// `Document.pushCard` / `insertCard`.
    #[wasm_bindgen(js_name = seedCard, unchecked_return_type = "Card | undefined")]
    pub fn seed_card(&self, card_kind: &str) -> JsValue {
        match self.inner.seed_card(card_kind) {
            Some(core_card) => card_to_js(&core_card),
            None => JsValue::UNDEFINED,
        }
    }
}

#[wasm_bindgen]
impl Document {
    /// Parse markdown into a typed Document. Throws on parse errors.
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

    /// Reconstruct a `Document` from a versioned storage DTO string produced
    /// by [`toJson`](Document::to_json). Unknown `schema` tags are rejected.
    /// The result carries no parse-time warnings (`.warnings` is always empty).
    ///
    /// Throws if `json` is not a valid storage DTO (malformed JSON, unknown
    /// `schema`, missing fields, or unparseable quill reference).
    #[wasm_bindgen(js_name = fromJson)]
    pub fn from_json(json: &str) -> Result<Document, JsValue> {
        let inner: quillmark_core::Document = serde_json::from_str(json).map_err(|e| {
            WasmError::from(format!("fromJson: invalid storage DTO: {e}")).to_js_value()
        })?;
        Ok(Document {
            inner,
            parse_warnings: Vec::new(),
        })
    }

    /// Like [`fromJson`](Document::from_json) but returns `undefined` instead
    /// of throwing when `json` is not a valid storage DTO — use to
    /// discriminate format without exceptions as control flow.
    /// `undefined` means "not a storage DTO"; `fromMarkdown` still throws on
    /// genuinely malformed markdown.
    //
    // No `tryFromMarkdown` counterpart: a malformed-markdown failure is a
    // real input error the caller wants to see, not a format-discriminator
    // signal.
    #[wasm_bindgen(js_name = tryFromJson)]
    pub fn try_from_json(json: &str) -> Option<Document> {
        let inner: quillmark_core::Document = serde_json::from_str(json).ok()?;
        Some(Document {
            inner,
            parse_warnings: Vec::new(),
        })
    }

    /// Read the `schema` version tag from a raw storage DTO string without a
    /// full parse, or `undefined`. Returns unknown future versions as-is —
    /// useful to distinguish "build too old" from "payload corrupt" when
    /// `fromJson` throws.
    #[wasm_bindgen(js_name = schemaVersionOf)]
    pub fn schema_version_of(json: &str) -> Option<String> {
        quillmark_core::document::peek_schema_version(json)
    }

    /// Schema version this build writes via [`toJson`](Document::to_json).
    /// Tracks the `Document` model version (not the running crate version):
    /// the tag advances only when the wire format changes, not on every release.
    #[wasm_bindgen(js_name = currentSchemaVersion)]
    pub fn current_schema_version() -> String {
        quillmark_core::document::SCHEMA_V0_82_0.to_string()
    }

    /// Authoring-format rules for the card-yaml markdown surface — the same
    /// text every binding (CLI, Python, MCP) shows so callers reading errors
    /// from one binding can use the rules from any other. Read once at
    /// startup and cache; the value never changes between calls.
    #[wasm_bindgen(js_name = formatRules)]
    pub fn format_rules() -> String {
        quillmark_core::document::FORMAT_RULES.to_string()
    }

    /// Authoring-ergonomics header introducing a blueprint to an LLM/MCP
    /// consumer for the given `quillName`. Surfaced verbatim by every binding
    /// so the wording stays uniform across CLI / Python / MCP.
    #[wasm_bindgen(js_name = blueprintInstruction)]
    pub fn blueprint_instruction(quill_name: &str) -> String {
        quillmark_core::document::blueprint_instruction(quill_name)
    }

    /// The canonical `$quill` reference grammar as author-facing text. Single
    /// source of truth (CLI, Python, MCP): drive schema `describe` and
    /// validation messages from this instead of re-stating the rule — it
    /// matches the `hint` on `parse::invalid_quill_reference`. Cache it; the
    /// value never changes.
    #[wasm_bindgen(js_name = quillRefHint)]
    pub fn quill_ref_hint() -> String {
        quillmark_core::quill_ref_hint().to_string()
    }

    /// Render a Diagnostic as the canonical pretty-printed text every binding
    /// shows (CLI, Python, MCP). Single source of truth so a Diagnostic looks
    /// identical no matter which consumer surfaces it.
    #[wasm_bindgen(js_name = formatDiagnostic)]
    pub fn format_diagnostic(diag: Diagnostic) -> String {
        let core: quillmark_core::Diagnostic = diag.into();
        core.fmt_pretty()
    }

    /// Emit canonical Quillmark Markdown. Round-trip safe: re-parsing the
    /// result produces a `Document` equal to `self` by value and by type.
    #[wasm_bindgen(js_name = toMarkdown)]
    pub fn to_markdown(&self) -> String {
        self.inner.to_markdown()
    }

    /// Serialize this document to a versioned storage DTO string.
    ///
    /// Prefer this over `toMarkdown` for persistence across restarts or crate
    /// upgrades — the wire format is frozen per `schema` version. Parse-time
    /// `warnings` are excluded from the DTO.
    ///
    /// Output is **byte-deterministic** within a `schema` version: equal
    /// documents produce byte-equal output, safe for content-hash use cases.
    #[wasm_bindgen(js_name = toJson)]
    pub fn to_json(&self) -> String {
        // Infallible: every field of `Document` and its DTO serializes via
        // standard derives into a `String` buffer — there is no `io::Write`
        // and no custom `Serialize` that can return an error.
        serde_json::to_string(&self.inner).expect("Document serialization is infallible")
    }

    #[wasm_bindgen(js_name = clone)]
    pub fn clone_doc(&self) -> Document {
        Document {
            inner: self.inner.clone(),
            parse_warnings: self.parse_warnings.clone(),
        }
    }

    #[wasm_bindgen(getter, js_name = quillRef)]
    pub fn quill_ref(&self) -> String {
        self.inner.quill_reference().to_string()
    }

    /// The document's main (entry) card. Allocates and serializes on each
    /// call — cache locally if read in a hot loop.
    #[wasm_bindgen(getter, js_name = main, unchecked_return_type = "Card")]
    pub fn main(&self) -> JsValue {
        card_to_js(self.inner.main())
    }

    #[wasm_bindgen(getter, js_name = cards, unchecked_return_type = "Card[]")]
    pub fn cards(&self) -> JsValue {
        let cards: Vec<quillmark_core::CardWire> =
            self.inner.cards().iter().map(Into::into).collect();
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        cards.serialize(&serializer).unwrap_or(JsValue::UNDEFINED)
    }

    /// Number of composable cards (excludes the main card). O(1).
    #[wasm_bindgen(getter, js_name = cardCount)]
    pub fn card_count(&self) -> usize {
        self.inner.cards().len()
    }

    /// Structural equality (parse-time `warnings` excluded). Use to debounce
    /// upstream prop updates instead of re-parsing on every keystroke.
    #[wasm_bindgen(js_name = equals)]
    pub fn equals(&self, other: &Document) -> bool {
        self.inner == other.inner
    }

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

    /// Update a payload field on the main card. Clears any existing `!fill` marker.
    ///
    /// Throws if `name` does not match `[a-z_][a-z0-9_]*`.
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

    /// Update a payload field on the main card and mark it as `!fill`.
    /// Throws on invalid name (see [`setField`](Document::set_field)).
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

    /// Remove a payload field on the main card, returning the removed value or
    /// `undefined`. Throws if `name` does not match `[a-z_][a-z0-9_]*`.
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

    /// Replace the opaque `$ext` map on the main card. `value` must be a plain
    /// object; throws otherwise. `$ext` carries out-of-band consumer state and
    /// never reaches the rendered output. Pass `{}` to record an explicit
    /// empty `$ext`.
    #[wasm_bindgen(js_name = setExt)]
    pub fn set_ext(&mut self, value: JsValue) -> Result<(), JsValue> {
        let map = js_value_to_object(&value, "setExt")?;
        self.inner.main_mut().set_ext(map);
        Ok(())
    }

    /// Remove the `$ext` map from the main card *entirely*, returning the
    /// previous map or `undefined`. This is a blunt escape hatch that discards
    /// every namespace at once — prefer `removeExtNamespace` to clear only your
    /// own slot while leaving sibling consumers' state intact.
    #[wasm_bindgen(js_name = removeExt, unchecked_return_type = "Record<string, unknown> | undefined")]
    pub fn remove_ext(&mut self) -> JsValue {
        ext_map_to_js(self.inner.main_mut().remove_ext())
    }

    /// Merge `value` into the main card's `$ext` map under `namespace`, creating
    /// the map when absent and replacing any existing value at that key. Sibling
    /// namespaces are preserved, so independent consumers (`$ext.presentation`,
    /// `$ext.agent`, …) don't clobber each other.
    #[wasm_bindgen(js_name = setExtNamespace)]
    pub fn set_ext_namespace(&mut self, namespace: &str, value: JsValue) -> Result<(), JsValue> {
        let json = js_value_to_json(value, "setExtNamespace")?;
        self.inner.main_mut().set_ext_namespace(namespace, json);
        Ok(())
    }

    /// Remove `namespace` from the main card's `$ext` map, returning the value
    /// stored there or `undefined`. This is the recommended way to clear `$ext`
    /// state: sibling namespaces survive, and when the last namespace is removed
    /// the `$ext` entry is dropped entirely (not left as `$ext: {}`).
    #[wasm_bindgen(js_name = removeExtNamespace)]
    pub fn remove_ext_namespace(&mut self, namespace: &str) -> JsValue {
        json_value_to_js(self.inner.main_mut().remove_ext_namespace(namespace))
    }

    /// Replace the `$ext` map on the composable card at `index`. Throws if out
    /// of range or `value` is not a plain object. Named to mirror `setExt` on
    /// the main card; `setCardExtNamespace` is the sibling-safe alternative.
    #[wasm_bindgen(js_name = setCardExt)]
    pub fn set_card_ext(&mut self, index: usize, value: JsValue) -> Result<(), JsValue> {
        let map = js_value_to_object(&value, "setCardExt")?;
        self.card_mut_or_throw(index)?.set_ext(map);
        Ok(())
    }

    /// Remove the `$ext` map from the composable card at `index` *entirely*,
    /// returning the previous map or `undefined`. Throws if out of range.
    /// Prefer `removeCardExtNamespace` to clear only one consumer's slot.
    #[wasm_bindgen(js_name = removeCardExt, unchecked_return_type = "Record<string, unknown> | undefined")]
    pub fn remove_card_ext(&mut self, index: usize) -> Result<JsValue, JsValue> {
        Ok(ext_map_to_js(self.card_mut_or_throw(index)?.remove_ext()))
    }

    /// Merge `value` into the composable card's `$ext` map under `namespace`,
    /// preserving sibling namespaces. The card-indexed twin of `setExtNamespace`.
    /// Throws if out of range or `value` cannot be serialized.
    #[wasm_bindgen(js_name = setCardExtNamespace)]
    pub fn set_card_ext_namespace(
        &mut self,
        index: usize,
        namespace: &str,
        value: JsValue,
    ) -> Result<(), JsValue> {
        let json = js_value_to_json(value, "setCardExtNamespace")?;
        self.card_mut_or_throw(index)?
            .set_ext_namespace(namespace, json);
        Ok(())
    }

    /// Remove `namespace` from the composable card's `$ext` map, returning the
    /// value stored there or `undefined`; clears `$ext` entirely once empty.
    /// The card-indexed twin of `removeExtNamespace`. Throws if out of range.
    #[wasm_bindgen(js_name = removeCardExtNamespace)]
    pub fn remove_card_ext_namespace(
        &mut self,
        index: usize,
        namespace: &str,
    ) -> Result<JsValue, JsValue> {
        Ok(json_value_to_js(
            self.card_mut_or_throw(index)?
                .remove_ext_namespace(namespace),
        ))
    }

    /// Replace the QUILL reference string. Throws if `ref_str` is invalid.
    #[wasm_bindgen(js_name = setQuillRef)]
    pub fn set_quill_ref(&mut self, ref_str: &str) -> Result<(), JsValue> {
        let qr: quillmark_core::QuillReference = ref_str.parse().map_err(|e| {
            // Same shape document parsing emits, so mutator and parser don't drift.
            let diag = quillmark_core::Diagnostic::new(
                quillmark_core::Severity::Error,
                format!("setQuillRef: invalid reference '{}': {}", ref_str, e),
            )
            .with_code("parse::invalid_quill_reference".to_string())
            .with_hint(quillmark_core::quill_ref_hint().to_string());
            WasmError {
                diagnostics: vec![diag],
            }
            .to_js_value()
        })?;
        self.inner.set_quill_ref(qr);
        Ok(())
    }

    #[wasm_bindgen(js_name = replaceBody)]
    pub fn replace_body(&mut self, body: &str) {
        self.inner.main_mut().replace_body(body);
    }

    /// Build a fresh `Card` from a kind and a flat field map — the ergonomic
    /// constructor for `pushCard` / `insertCard`. `fields` is an optional
    /// `Record<string, unknown>` (each entry becomes a card field, in
    /// insertion order); `body` defaults to `""`. Kind validity is checked by
    /// `pushCard` / `insertCard`, not here.
    #[wasm_bindgen(js_name = makeCard, unchecked_return_type = "Card")]
    pub fn make_card(
        kind: String,
        #[wasm_bindgen(unchecked_optional_param_type = "Record<string, unknown>")]
        fields: Option<JsValue>,
        #[wasm_bindgen(unchecked_optional_param_type = "string")] body: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let field_map: serde_json::Map<String, serde_json::Value> = match fields {
            Some(fields) if !fields.is_undefined() && !fields.is_null() => {
                serde_wasm_bindgen::from_value(fields).map_err(|e| {
                    WasmError::from(format!("makeCard: `fields` must be an object: {e}"))
                        .to_js_value()
                })?
            }
            _ => serde_json::Map::new(),
        };
        let payload_items = field_map
            .into_iter()
            .map(|(key, value)| quillmark_core::PayloadItemWire::Field {
                key,
                value,
                fill: false,
            })
            .collect();
        let wire = quillmark_core::CardWire {
            kind,
            quill: None,
            id: None,
            ext: None,
            payload_items,
            body: body.unwrap_or_default(),
        };
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        wire.serialize(&serializer)
            .map_err(|e| WasmError::from(format!("makeCard: serialization failed: {e}")).to_js_value())
    }

    /// Append a card to the end of the card list. Accepts a `Card` (the shape
    /// returned by `cards` / `removeCard` / `quill.seedCard`); build a fresh
    /// one with [`Document.makeCard`](Document::make_card). Throws if
    /// `card.kind` is not a valid kind name.
    #[wasm_bindgen(js_name = pushCard)]
    pub fn push_card(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Card")] card: JsValue,
    ) -> Result<(), JsValue> {
        let core_card = js_to_card(&card)?;
        self.inner
            .push_card(core_card)
            .map_err(|e| edit_error_to_js(&e))
    }

    /// Insert a card at `index` (must be in `0..=cards.length`). Accepts a
    /// `Card` (see [`pushCard`](Self::push_card)).
    #[wasm_bindgen(js_name = insertCard)]
    pub fn insert_card(
        &mut self,
        index: usize,
        #[wasm_bindgen(unchecked_param_type = "Card")] card: JsValue,
    ) -> Result<(), JsValue> {
        let core_card = js_to_card(&card)?;
        self.inner
            .insert_card(index, core_card)
            .map_err(|e| edit_error_to_js(&e))
    }

    #[wasm_bindgen(js_name = removeCard, unchecked_return_type = "Card | undefined")]
    pub fn remove_card(&mut self, index: usize) -> JsValue {
        match self.inner.remove_card(index) {
            Some(core_card) => card_to_js(&core_card),
            None => JsValue::UNDEFINED,
        }
    }

    /// Move the card at `from` to position `to`. `from == to` is a no-op.
    #[wasm_bindgen(js_name = moveCard)]
    pub fn move_card(&mut self, from: usize, to: usize) -> Result<(), JsValue> {
        self.inner
            .move_card(from, to)
            .map_err(|e| edit_error_to_js(&e))
    }

    /// Replace the kind of the card at `index`. Payload and body are untouched;
    /// schema-aware migration is the caller's responsibility.
    /// Throws if `index` is out of range or `newKind` is invalid.
    #[wasm_bindgen(js_name = setCardKind)]
    pub fn set_card_kind(&mut self, index: usize, new_kind: &str) -> Result<(), JsValue> {
        self.inner
            .set_card_kind(index, new_kind)
            .map_err(|e| edit_error_to_js(&e))
    }

    /// Update a field on the card at `index`.
    /// Throws if `index` is out of range, `name` is reserved or invalid.
    #[wasm_bindgen(js_name = updateCardField)]
    pub fn update_card_field(
        &mut self,
        index: usize,
        name: &str,
        value: JsValue,
    ) -> Result<(), JsValue> {
        let json = js_value_to_json(value, "updateCardField")?;
        let qv = quillmark_core::QuillValue::from_json(json);
        self.card_mut_or_throw(index)?
            .set_field(name, qv)
            .map_err(|e| edit_error_to_js(&e))
    }

    /// Remove a field on the card at `index`. Returns the removed value or
    /// `undefined`. Throws if `index` is out of range or `name` is invalid.
    #[wasm_bindgen(js_name = removeCardField)]
    pub fn remove_card_field(&mut self, index: usize, name: &str) -> Result<JsValue, JsValue> {
        let removed = self
            .card_mut_or_throw(index)?
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

    /// Replace the body of the card at `index`. Throws if out of range.
    #[wasm_bindgen(js_name = updateCardBody)]
    pub fn update_card_body(&mut self, index: usize, body: &str) -> Result<(), JsValue> {
        self.card_mut_or_throw(index)?.replace_body(body);
        Ok(())
    }
}

impl Document {
    /// Resolve a mutable composable card by index, mapping out-of-range to the
    /// same `IndexOutOfRange` JS error the other card mutators throw. Shared by
    /// the card-indexed `$ext` mutators so they don't each re-spell the bounds
    /// check.
    fn card_mut_or_throw(&mut self, index: usize) -> Result<&mut quillmark_core::Card, JsValue> {
        let len = self.inner.cards().len();
        self.inner.card_mut(index).ok_or_else(|| {
            edit_error_to_js(&quillmark_core::EditError::IndexOutOfRange { index, len })
        })
    }
}

// ── Edit helpers ──────────────────────────────────────────────────────────────

/// Maps `EditError` to a JS `Error` with the variant name and details in the message.
fn edit_error_to_js(err: &quillmark_core::EditError) -> JsValue {
    let variant = match err {
        quillmark_core::EditError::InvalidFieldName(_) => "InvalidFieldName",
        quillmark_core::EditError::InvalidKindName(_) => "InvalidKindName",
        quillmark_core::EditError::ReservedKind => "ReservedKind",
        quillmark_core::EditError::IndexOutOfRange { .. } => "IndexOutOfRange",
    };
    WasmError::from(format!("[EditError::{}] {}", variant, err)).to_js_value()
}

/// Deserialize a JS value into an arbitrary JSON value. The namespaced `$ext`
/// mutators take any shape (the consumer's slot may hold an array, scalar, or
/// map); `js_value_to_object` adds the object constraint on top.
fn js_value_to_json(value: JsValue, ctx: &str) -> Result<serde_json::Value, JsValue> {
    serde_wasm_bindgen::from_value(value)
        .map_err(|e| WasmError::from(format!("{}: invalid value: {}", ctx, e)).to_js_value())
}

/// Deserialize a JS value into a JSON object map, rejecting non-objects. Used by
/// the whole-map `$ext` mutators, whose value must be a plain object.
fn js_value_to_object(
    value: &JsValue,
    ctx: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, JsValue> {
    match js_value_to_json(value.clone(), ctx)? {
        serde_json::Value::Object(map) => Ok(map),
        _ => Err(WasmError::from(format!("{}: $ext must be a plain object", ctx)).to_js_value()),
    }
}

/// Serialize an optional JSON value to JS, or `undefined` when `None`. Backs
/// both the namespaced reads (any value) and the whole-map reads (via
/// `ext_map_to_js`).
fn json_value_to_js(value: Option<serde_json::Value>) -> JsValue {
    match value {
        Some(v) => {
            let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
            v.serialize(&serializer).unwrap_or(JsValue::UNDEFINED)
        }
        None => JsValue::UNDEFINED,
    }
}

/// Serialize an optional `$ext` map to a JS object, or `undefined` when `None`.
fn ext_map_to_js(map: Option<serde_json::Map<String, serde_json::Value>>) -> JsValue {
    json_value_to_js(map.map(serde_json::Value::Object))
}

/// Serialize a core [`Card`](quillmark_core::Card) to its `Card` JS shape via
/// the canonical [`CardWire`](quillmark_core::CardWire). The single place WASM
/// turns a core card into JS — used by `Document.main`, `cards`, `removeCard`,
/// and the seed getters.
fn card_to_js(card: &quillmark_core::Card) -> JsValue {
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    quillmark_core::CardWire::from(card)
        .serialize(&serializer)
        .unwrap_or(JsValue::UNDEFINED)
}

/// Deserialize a `Card`-shaped JS value into a core
/// [`Card`](quillmark_core::Card) via [`CardWire`](quillmark_core::CardWire).
/// The single place WASM turns JS into a core card — used by `pushCard` /
/// `insertCard`.
fn js_to_card(value: &JsValue) -> Result<quillmark_core::Card, JsValue> {
    // `serde_wasm_bindgen` does not honor the core type's
    // `#[serde(deny_unknown_fields)]` (it looks up known fields rather than
    // visiting every key), so enforce it here to match the Python binding and
    // fail loudly on the retired flat `{ kind, fields }` shape instead of
    // yielding a silently-empty card.
    if let Some(obj) = value.dyn_ref::<js_sys::Object>() {
        const ALLOWED: &[&str] = &["kind", "quill", "id", "ext", "payloadItems", "body"];
        for key in js_sys::Object::keys(obj).iter() {
            if let Some(k) = key.as_string() {
                if !ALLOWED.contains(&k.as_str()) {
                    return Err(WasmError::from(format!(
                        "card has unknown field `{k}`; expected a Card \
                         {{ kind, payloadItems, body, … }} — build one with \
                         Document.makeCard(kind, fields, body)"
                    ))
                    .to_js_value());
                }
            }
        }
    }
    let wire: quillmark_core::CardWire = serde_wasm_bindgen::from_value(value.clone())
        .map_err(|e| WasmError::from(format!("card must be a Card object: {e}")).to_js_value())?;
    quillmark_core::Card::try_from(wire).map_err(|e| WasmError::from(e.to_string()).to_js_value())
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
    #[wasm_bindgen(getter, js_name = pageCount)]
    pub fn page_count(&self) -> usize {
        self.inner.page_count()
    }

    /// The backend that produced this session (e.g. `"typst"`).
    #[wasm_bindgen(getter, js_name = backendId)]
    pub fn backend_id(&self) -> String {
        self.backend_id.clone()
    }

    /// `true` iff `paint` and `pageSize` will succeed for this session.
    #[wasm_bindgen(getter, js_name = supportsCanvas)]
    pub fn supports_canvas(&self) -> bool {
        self.backend_id == CANVAS_BACKEND_ID
    }

    /// Non-fatal diagnostics emitted when opening the session. Also appended
    /// to `RenderResult.warnings` on each `render()` call.
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
    /// Throws if the backend has no canvas painter or `page` is out of range.
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

    /// Paint `page` into a `CanvasRenderingContext2D` or
    /// `OffscreenCanvasRenderingContext2D`. The painter owns
    /// `canvas.width`/`height` (no `clearRect` needed); consumers own
    /// `canvas.style.*`. If `layoutScale * densityScale` exceeds 16384 px
    /// per side, `densityScale` is clamped — detect via `PaintResult.pixelWidth`.
    ///
    /// Throws if the backend has no canvas painter, `page` is out of range,
    /// `ctx` is the wrong type, or either scale is non-finite or `<= 0`.
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
