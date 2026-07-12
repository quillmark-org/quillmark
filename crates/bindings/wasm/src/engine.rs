//! Quillmark WASM Engine - Simplified API

use crate::error::WasmError;
use crate::types::Diagnostic;
#[cfg(any(feature = "typst", feature = "pdfform"))]
use crate::types::{ChangeSet, CorpusHit, FieldRegion, RenderOptions, RenderResult};
use js_sys::{Array, Uint8Array};
#[cfg(any(feature = "typst", feature = "pdfform"))]
use serde::Deserialize;
use serde::Serialize;
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
 * `!must_fill` marker; a marker left in the document yields the non-fatal
 * `validation::must_fill` warning from validate, and the render path
 * zero-fills the field). There is no separate `required` axis.
 */
export interface QuillFieldSchema {
    type: "string" | "number" | "integer" | "boolean" | "array" | "object" | "datetime" | "richtext";
    description?: string;
    default?: unknown;
    example?: unknown;
    enum?: string[];
    ui?: QuillFieldUi;
    properties?: Record<string, QuillFieldSchema>;
    items?: QuillFieldSchema;
    /** Present (and `true`) only on a `richtext` field declared `richtext(inline)`
     *  — the single-paragraph, container-free, island-free constraint. Core
     *  serializes `inline: true` into the schema JSON; absent otherwise. */
    inline?: boolean;
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
 * The schema lives on `Quill.schema`; the backend's output formats are a
 * resolved-backend capability read from the engine (`Quillmark.supportedFormats`),
 * not part of this pure-config snapshot.
 */
export interface QuillMetadata {
    name: string;
    version: string;
    backend: string;
    author: string;
    description: string;
}
"#;

/// TypeScript for the canonical `Card` wire shape (mirrors
/// `quillmark_core::CardWire`) and its write-input twin `CardInput`. `Card` is
/// the read shape *returned* by `Document.main` / `cards` / `removeCard` /
/// `quill.seedCard` / `Document.makeCard`; `CardInput` is the shape *accepted*
/// by `pushCard` / `insertCard` (referenced by name via `unchecked_param_type`).
/// They differ only in `body`: a read is always canonical `RichText`, a write
/// also takes a markdown `string`.
#[wasm_bindgen(typescript_custom_section)]
const CARD_TS: &'static str = r#"
/**
 * A path to a value nested inside a field `value`: `string` keys and
 * `number` array indices, e.g. `["addr", "street"]` or `["recipients", 0, "name"]`.
 */
export type PathStep = string | number;

/** A field or comment entry in a `Card.payloadItems` list. */
export type PayloadItem =
    | {
          type: "field";
          key: string;
          value: unknown;
          fill?: boolean;
          /**
           * Paths to `!must_fill` markers nested *inside* `value` (the `value`
           * projection itself is fill-free). Absent when the field has no nested
           * placeholders. Preserved across `pushCard` / `makeCard`.
           */
          nestedFills?: PathStep[][];
      }
    | { type: "comment"; text: string; inline?: boolean };

/**
 * A single card block, as read back from a document: returned by
 * `Document.main` / `Document.cards` / `Document.removeCard` / `Quill.seedCard`
 * / `Document.makeCard`. To feed a card *into* a document use `CardInput`
 * (which `pushCard` / `insertCard` accept); every `Card` is a valid `CardInput`,
 * so a card read from one document pushes straight into another.
 *
 * `$` system entries are hoisted to named fields: `kind` (the `$kind`, empty
 * string when none), optional `quill` (the `$quill` `name@version`, main card
 * only), optional `id` (`$id`), optional `ext` (`$ext`), and optional `seed`
 * (the `$seed` per-kind overlay map, main card only). `payloadItems` carries
 * user fields and comments in order.
 */
export interface Card {
    kind: string;
    quill?: string;
    id?: string;
    ext?: Record<string, unknown>;
    seed?: Record<string, unknown>;
    payloadItems: PayloadItem[];
    /**
     * The card body as canonical `RichText` — the source-of-truth content model.
     * Always this corpus shape on read, never a markdown string. For the markdown
     * projection call the codec `exportMarkdown(card.body)`. Write a body back
     * with `doc.install(addr, rt)` / `doc.revise(addr, md)`, or via `CardInput.body`.
     */
    body: RichText;
}

/**
 * A card written *into* a document — the input twin of `Card`, accepted by
 * `Document.pushCard` / `Document.insertCard`. Like `Card` but `body` also
 * takes a markdown `string` (imported to the corpus, so a markdown / LLM writer
 * needn't build the `RichText` shape), and every field but `kind` is optional —
 * an absent field defaults (no payload items, an empty body). Write one inline
 * (`{ kind, body }`) or build it with `Document.makeCard`.
 */
export interface CardInput {
    kind: string;
    quill?: string;
    id?: string;
    ext?: Record<string, unknown>;
    seed?: Record<string, unknown>;
    payloadItems?: PayloadItem[];
    body?: RichText | string;
}

/**
 * Canonical richtext corpus — the content model for a card body (and richtext
 * fields). One text sequence over a single coordinate space (Unicode scalar
 * values): `text` plus line attributes, anchored `marks`, and embedded
 * `islands`. Every edit is a splice; markdown is a projection, not the model.
 * Mirrors `quillmark_richtext::serial`'s canonical JSON encoding.
 */
export interface RichText {
    text: string;
    lines: RichTextLine[];
    marks: RichTextMark[];
    islands: RichTextIsland[];
}

/** One `\n`-separated segment of `RichText.text`, in order. */
export type RichTextLine = {
    containers: RichTextContainer[];
    /** A within-block hard line break rather than a new block. Omitted (false) in the common case. */
    continues?: boolean;
} & (
    | { kind: "para" }
    | { kind: "heading"; level: number }
    | { kind: "code"; lang?: string }
    | { kind: "island" }
    | { kind: "rule" }
);

/** An ancestor block a line nests inside, outermost first. */
export type RichTextContainer =
    | { container: "list_item"; ordered: boolean; start: number; ordinal: number }
    | { container: "quote" };

/** A mark over char range `[start, end)` into `RichText.text`. */
export type RichTextMark = { start: number; end: number } & (
    | { type: "strong" | "emph" | "underline" | "strike" | "code" }
    | { type: "link"; url: string }
    | { type: "anchor"; id: string }
    | { type: string; attrs: unknown }
);

/** A structured object (table, figure, …) occupying one island slot in `RichText.text`. */
export interface RichTextIsland {
    id: string;
    type: string;
    props: unknown;
    /** How faithfully the markdown projection can carry this island. */
    loss: "lossless" | "degraded" | "unrepresentable";
}

/**
 * A richtext write address. An absent `field` targets the card body; an absent
 * `card` targets the main card. `{}` is the main-card body; `{ card: 2 }` the
 * body of the composable card at index 2; `{ field: "intro" }` the main card's
 * `intro` richtext field; `{ card: 2, field: "intro" }` a card field.
 */
export interface Addr {
    card?: number;
    field?: string;
}

/**
 * A text-splice change set over the USV corpus (CodeMirror `ChangeSet`
 * semantics) — plain, structured-clone-able data. Returned by `revise` and by
 * the `rebase` codec; map a stored position through it with `mapPos`.
 */
export interface Delta {
    ops: ({ retain: number } | { insert: string } | { delete: number })[];
}

/** Which side of a same-position insertion `mapPos` lands a point on. */
export type Assoc = "before" | "after";

/**
 * A mark edit in post-text-delta coordinates. `add` / `remove` carry the
 * `RichTextMark` vocabulary (`{ type, … }`); `removeAnchor` drops one identity
 * anchor by id.
 */
export type MarkOp =
    | ({ op: "add" | "remove"; start: number; end: number } & (
          | { type: "strong" | "emph" | "underline" | "strike" | "code" }
          | { type: "link"; url: string }
          | { type: "anchor"; id: string }
          | { type: string; attrs: unknown }
      ))
    | { op: "removeAnchor"; id: string };

/** A line/block edit. `split`/`join` splice `\n`; `setKind`/`setContainers` touch metadata. */
export type LineOp =
    | { op: "split"; at: number }
    | { op: "join"; line: number }
    | ({ op: "setKind"; line: number } & (
          | { kind: "para" | "island" | "rule" }
          | { kind: "heading"; level: number }
          | { kind: "code"; lang?: string }
      ))
    | { op: "setContainers"; line: number; containers: RichTextContainer[] };

/**
 * A committed corpus edit bundle for `applyChange`: a text `delta` (default no
 * text change), then `lineOps`, then `markOps` (mark ranges are in post-delta
 * coordinates). Every field is optional.
 */
export interface ChangeBundle {
    delta?: Delta;
    lineOps?: LineOp[];
    markOps?: MarkOp[];
}
"#;

/// Maximum backing-store dimension the painter will produce, in device
/// pixels per side. Real browser limits vary (~32k on Chrome/Firefox,
/// 16k on Safari, lower on memory-constrained devices); 16384 is the
/// floor that works everywhere we ship to. When a requested
/// `layoutScale * densityScale` would exceed this, the painter clamps
/// `densityScale` proportionally and reports it on the returned
/// `PaintResult` (`clamped` / `effectiveDensityScale`, plus the actual
/// backing dimensions).
#[cfg(any(feature = "typst", feature = "pdfform"))]
const MAX_BACKING_DIMENSION: u32 = 16384;

#[cfg(any(feature = "typst", feature = "pdfform"))]
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

/// Render engine: a backend registry and render dispatcher. Render build only —
/// the core build constructs and validates quills without it.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[wasm_bindgen]
pub struct Quillmark {
    inner: quillmark::Quillmark,
}

#[wasm_bindgen]
pub struct Quill {
    inner: quillmark::Quill,
}

/// Live render session: reads (`render`, `paint`, `pageSize`, `regions`,
/// `fieldAt`, `positionAt`, `locate`) serve the current compile. `apply(doc)`
/// recompiles a whole document in place, transactionally (on throw every read
/// keeps serving the last-good compile). Geometry reads reflect the current
/// compile; anchoring a caret across edits is the editor's job — re-read
/// geometry after each committed `apply`.
///
/// **Empty documents.** A zero-page document yields a valid session
/// (`pageCount === 0`); `paint(ctx, 0)` or `pageSize(0)` throws with
/// `"page index 0 out of range (pageCount=0)"`. Branch on `pageCount === 0`
/// rather than catching the error.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[wasm_bindgen]
pub struct LiveSession {
    inner: quillmark_core::LiveSession,
    backend_id: String,
    /// Retained for `apply`: recompiles `doc` → data through the same schema
    /// pipeline as `open`. The config alone — schemas, not the quill's
    /// font/package bytes, which the backend session already owns.
    config: quillmark_core::quill::QuillConfig,
}

/// Typed in-memory Quillmark document.
#[wasm_bindgen]
pub struct Document {
    inner: quillmark_core::Document,
    /// Parse-time warnings (e.g. a `~~~` opener missing its blank line).
    parse_warnings: Vec<quillmark_core::Diagnostic>,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl Default for Quillmark {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
#[wasm_bindgen]
impl Quillmark {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Quillmark {
        Quillmark {
            inner: quillmark::Quillmark::new(),
        }
    }

    /// Open a live render session for `doc` against `quill`'s backend.
    #[wasm_bindgen(js_name = open)]
    pub fn open(&self, quill: &Quill, doc: &Document) -> Result<LiveSession, JsValue> {
        let session = self
            .inner
            .open(&quill.inner, &doc.inner)
            .map_err(|e| WasmError::from(e).to_js_value())?;
        Ok(LiveSession {
            inner: session,
            backend_id: quill.inner.backend_id().to_string(),
            config: quill.inner.config().clone(),
        })
    }

    /// Render `doc` against `quill` in one shot. Convenience over `open` +
    /// `LiveSession.render`: an unset `output_format` falls back to the
    /// backend's first supported format.
    #[wasm_bindgen(js_name = render)]
    pub fn render(
        &self,
        quill: &Quill,
        doc: &Document,
        opts: Option<RenderOptions>,
    ) -> Result<RenderResult, JsValue> {
        let start = now_ms();
        let rust_opts: quillmark_core::RenderOptions = opts.unwrap_or_default().into();
        let result = self
            .inner
            .render(&quill.inner, &doc.inner, &rust_opts)
            .map_err(|e| WasmError::from(e).to_js_value())?;
        let mut warnings: Vec<Diagnostic> =
            doc.parse_warnings.iter().cloned().map(Into::into).collect();
        warnings.extend(result.warnings.into_iter().map(Into::into));
        Ok(RenderResult {
            artifacts: result.artifacts.into_iter().map(Into::into).collect(),
            warnings,
            output_format: result.output_format.into(),
            render_time_ms: now_ms() - start,
            regions: result.regions.into_iter().map(Into::into).collect(),
        })
    }

    /// The output formats `quill`'s backend can emit. Static capability —
    /// resolves the backend but compiles nothing. Throws `engine::backend_not_found`
    /// if no registered backend matches the quill's declared backend.
    #[wasm_bindgen(js_name = supportedFormats, unchecked_return_type = "OutputFormat[]")]
    pub fn supported_formats(&self, quill: &Quill) -> Result<JsValue, JsValue> {
        let formats = self
            .inner
            .supported_formats(&quill.inner)
            .map_err(|e| WasmError::from(e).to_js_value())?;
        let out: Vec<crate::types::OutputFormat> = formats.iter().map(|f| (*f).into()).collect();
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        out.serialize(&serializer).map_err(|e| {
            WasmError::from(format!("supportedFormats: serialization failed: {e}")).to_js_value()
        })
    }

    /// Pre-session hint: `true` iff `quill`'s backend can paint sessions to a
    /// canvas, derived from the backend's output formats; `false` when the
    /// backend is unsupported. Use as a cheap precondition probe before mounting
    /// a canvas-based preview UI; the authoritative answer is the session's
    /// `supportsCanvas` getter once `open()` has been called.
    #[wasm_bindgen(js_name = supportsCanvas)]
    pub fn supports_canvas(&self, quill: &Quill) -> bool {
        self.inner.supports_canvas(&quill.inner)
    }
}

#[wasm_bindgen]
impl Quill {
    /// Build a quill from a file tree. Pure — no backend, no engine; the
    /// declared backend is resolved later, at render time.
    ///
    /// Accepts either a `Map<string, Uint8Array>` or a plain object
    /// (`Record<string, Uint8Array>`). Plain objects are walked via
    /// `Object.entries` at the boundary; the Rust side sees a single
    /// canonical shape.
    #[wasm_bindgen(js_name = fromTree)]
    pub fn from_tree(
        #[wasm_bindgen(unchecked_param_type = "Map<string, Uint8Array>")] tree: JsValue,
    ) -> Result<Quill, JsValue> {
        let root = file_tree_from_js_tree(&tree)?;
        let quill = quillmark::Quill::from_tree(root)
            .map_err(|diags| WasmError { diagnostics: diags }.to_js_value())?;
        Ok(Quill { inner: quill })
    }

    /// Flatten this quill back into its canonical file tree — the inverse of
    /// [`fromTree`](Self::from_tree). Round-trips: `Quill.fromTree(q.toTree())`
    /// reproduces an equivalent quill.
    ///
    /// This is how a quill crosses a WASM linear-memory boundary as data: a
    /// `Quill` built in one build (e.g. the Typst-less `@quillmark/wasm/core`)
    /// cannot be passed to an engine in another (separate linear memories), so
    /// `@quillmark/wasm/runtime` re-feeds this tree to the backend build's
    /// `Quill.fromTree` on demand. Keys are `"/"`-joined relative paths,
    /// matching what `fromTree` accepts.
    #[wasm_bindgen(js_name = toTree, unchecked_return_type = "Map<string, Uint8Array>")]
    pub fn to_tree(&self) -> JsValue {
        let map = js_sys::Map::new();
        for (path, contents) in self.inner.to_tree() {
            let bytes = Uint8Array::from(contents.as_slice());
            map.set(&JsValue::from_str(&path), &bytes);
        }
        map.into()
    }

    /// The *declared* backend identifier (`config.backend`, e.g. `"typst"`).
    /// Intent, not a resolved capability — capability (`supportedFormats` /
    /// `supportsCanvas`) is read from the engine.
    #[wasm_bindgen(getter, js_name = backendId)]
    pub fn backend_id(&self) -> String {
        self.inner.backend_id().to_string()
    }

    #[wasm_bindgen(getter, js_name = blueprint)]
    pub fn blueprint(&self) -> String {
        self.inner.config().blueprint()
    }

    /// Document schema for the quill: the user-fillable fields plus their
    /// `ui` hints (title / group / order / compact / multiline). The single
    /// field-metadata surface — drives form editors and LLM/MCP consumers
    /// alike. Returns the `QuillSchema` shape.
    #[wasm_bindgen(getter, js_name = schema, unchecked_return_type = "QuillSchema")]
    pub fn schema(&self) -> Result<JsValue, JsValue> {
        let value = self.inner.config().schema();
        serialize_or_throw(&value, "schema")
    }

    /// Identity snapshot of the `quill:` section of `Quill.yaml` plus any extra
    /// `quill:` keys. Pure config — the backend's output formats are a
    /// resolved-backend capability read from the engine
    /// (`Quillmark.supportedFormats`), not part of this snapshot.
    #[wasm_bindgen(getter, js_name = metadata, unchecked_return_type = "QuillMetadata")]
    pub fn metadata(&self) -> Result<JsValue, JsValue> {
        let source = &self.inner;
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

        // Unstructured keys declared under `quill:` (excluding fields already
        // surfaced above or now living under `schema`).
        for (key, value) in source.metadata() {
            if quillmark_core::STANDARD_METADATA_KEYS.contains(&key.as_str()) {
                continue;
            }
            if obj.contains_key(key) {
                continue;
            }
            obj.insert(key.clone(), value.as_json().clone());
        }

        let val = serde_json::Value::Object(obj);
        serialize_or_throw(&val, "metadata")
    }

    /// Validate `doc` against this quill's schema, returning every diagnostic
    /// (an empty array when the document is valid).
    ///
    /// Forwards the canonical `validation::*` diagnostics — same `code`,
    /// `path`, and `hint` the engine emits — including the non-fatal
    /// `validation::must_fill` warning for each `!must_fill` marker left in
    /// the document. Field values, defaults, and order are not part of this
    /// surface: read them from the `Document` payload and `Quill.schema`
    /// (fields carry `ui.order`).
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
    pub fn seed_main(&self) -> Result<JsValue, JsValue> {
        card_to_js(&self.inner.seed_main())
    }

    /// Seed a starter composable `Card` of the given kind (carries `$kind`),
    /// layering an optional per-kind seed `overlay` over the schema-example
    /// base (`overlay › example › absent`). Returns `undefined` if `cardKind`
    /// is not declared in this quill's schema, else a `Card` that feeds
    /// straight into `Document.pushCard` / `insertCard`.
    ///
    /// Pass `document.main.seed?.[cardKind]` as `overlay` so a card added to a
    /// template-derived document inherits its curated starting values; omit it
    /// (or pass `undefined` / `null`) for the bare schema seed. `overlay` is a
    /// plain object — this reads the document, it does not mutate it.
    #[wasm_bindgen(js_name = seedCard, unchecked_return_type = "Card | undefined")]
    pub fn seed_card(
        &self,
        card_kind: &str,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown> | undefined")]
        overlay: JsValue,
    ) -> Result<JsValue, JsValue> {
        let overlay = if overlay.is_undefined() || overlay.is_null() {
            None
        } else {
            let json = js_value_to_json(overlay, "seedCard")?;
            quillmark_core::SeedOverlay::from_json(&json)
        };
        match self.inner.seed_card(card_kind, overlay.as_ref()) {
            Some(core_card) => card_to_js(&core_card),
            None => Ok(JsValue::UNDEFINED),
        }
    }
}

#[wasm_bindgen]
impl Document {
    /// `new Document(quillRef)` — a blank document: a main card carrying only
    /// `$quill`, an empty body, and no composable cards. The programmatic
    /// blank canvas: absent fields resolve at render time (`default`, else
    /// type-empty zero), so nothing the caller did not set reaches the
    /// output. For an example-filled starter use `Quill.seedDocument()`.
    /// Throws on an invalid quill reference. Mirrors Python `Document(quill_ref)`.
    #[wasm_bindgen(constructor)]
    pub fn new(quill_ref: &str) -> Result<Document, JsValue> {
        let qr: quillmark_core::QuillReference = quill_ref.parse().map_err(|e| {
            WasmError::from(format!("invalid QuillReference '{quill_ref}': {e}")).to_js_value()
        })?;
        Ok(Document {
            inner: quillmark_core::Document::new(qr),
            parse_warnings: Vec::new(),
        })
    }

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
        quillmark_core::document::SCHEMA_V0_93_0.to_string()
    }

    /// Authoring-format rules for the card-yaml markdown surface. The canonical
    /// text is core's (`quillmark_core::document::FORMAT_RULES`), re-exposed
    /// here for JS consumers so it matches any other surface that draws from the
    /// same source. Read once at startup and cache; the value never changes
    /// between calls.
    #[wasm_bindgen(js_name = formatRules)]
    pub fn format_rules() -> String {
        quillmark_core::document::FORMAT_RULES.to_string()
    }

    /// Authoring-ergonomics header introducing a blueprint to an LLM/MCP
    /// consumer for the given `quillName`. Re-exposes core's canonical text for
    /// JS consumers; any surface that draws from the same core source stays
    /// uniform.
    #[wasm_bindgen(js_name = blueprintInstruction)]
    pub fn blueprint_instruction(quill_name: &str) -> String {
        quillmark_core::document::blueprint_instruction(quill_name)
    }

    /// The canonical `$quill` reference grammar as author-facing text. Core is
    /// the single source of truth: drive schema `describe` and validation
    /// messages from this instead of re-stating the rule — it matches the
    /// `hint` on `parse::invalid_quill_reference`. Cache it; the value never
    /// changes.
    #[wasm_bindgen(js_name = quillRefHint)]
    pub fn quill_ref_hint() -> String {
        quillmark_core::quill_ref_hint().to_string()
    }

    /// Render a Diagnostic as the canonical pretty-printed text (core's
    /// `Diagnostic::fmt_pretty`). Single source of truth so a Diagnostic looks
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

    /// Replace this document's contents **in place** from a versioned storage
    /// DTO string — the mutating twin of the static
    /// [`fromJson`](Document::from_json) constructor. Parse-time `warnings` are
    /// cleared. Throws (leaving the document unchanged) on an invalid DTO.
    ///
    /// The cross-WASM-memory `Document` bridge: mutate a document on a
    /// backend-memory clone, then write the mutated state back into the caller's
    /// canonical document with this — the one way to update a live handle across
    /// the linear-memory seam without the caller re-binding its variable.
    #[wasm_bindgen(js_name = loadJson)]
    pub fn load_json(&mut self, json: &str) -> Result<(), JsValue> {
        let inner: quillmark_core::Document = serde_json::from_str(json).map_err(|e| {
            WasmError::from(format!("loadJson: invalid storage DTO: {e}")).to_js_value()
        })?;
        self.inner = inner;
        self.parse_warnings.clear();
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = quillRef)]
    pub fn quill_ref(&self) -> String {
        self.inner.quill_reference().to_string()
    }

    /// The document's main (entry) card. Allocates and serializes on each
    /// call — cache locally if read in a hot loop.
    #[wasm_bindgen(getter, js_name = main, unchecked_return_type = "Card")]
    pub fn main(&self) -> Result<JsValue, JsValue> {
        card_to_js(self.inner.main())
    }

    #[wasm_bindgen(getter, js_name = cards, unchecked_return_type = "Card[]")]
    pub fn cards(&self) -> Result<JsValue, JsValue> {
        let cards: Vec<quillmark_core::CardWire> =
            self.inner.cards().iter().map(Into::into).collect();
        serialize_or_throw(&cards, "cards")
    }

    /// Read a main-card field's stored value — the raw payload value (a corpus
    /// object for a richtext field, a scalar/array/object otherwise), or
    /// `undefined` when the field is absent. The quill-free read: reads need no
    /// schema, so they live on `Document`, not the typed editor. For the markdown
    /// projection of a richtext value use [`getMarkdown`](Self::get_markdown).
    #[wasm_bindgen(js_name = get)]
    pub fn get(&self, name: &str) -> Result<JsValue, JsValue> {
        match self.inner.main().payload().get(name) {
            Some(v) => serialize_or_throw(v.as_json(), "get"),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// The markdown projection of a main-card field (`name` given) or the main
    /// body (`name` omitted) — the on-demand, lossy export (corpus-only marks do
    /// not survive markdown), returning `""` for an absent field. Re-coins,
    /// lazily and by name, the projection the eager `fieldMarkdown` /
    /// `bodyMarkdown` getters dropped in #925; call it only when markdown is what
    /// you need out.
    #[wasm_bindgen(js_name = getMarkdown)]
    pub fn get_markdown(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "string")] name: Option<String>,
    ) -> String {
        match name {
            Some(n) => self.inner.main().field_markdown(&n).unwrap_or_default(),
            None => self.inner.main().body_markdown(),
        }
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
    pub fn warnings(&self) -> Result<JsValue, JsValue> {
        let diags: Vec<Diagnostic> = self
            .parse_warnings
            .iter()
            .cloned()
            .map(Into::into)
            .collect();
        serialize_or_throw(&diags, "warnings")
    }

    // ── Mutators ──────────────────────────────────────────────────────────────

    /// Update a payload field on the main card. Clears any existing `!must_fill` marker.
    ///
    /// Throws if `name` does not match `[A-Za-z_][A-Za-z0-9_]*`.
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

    /// Update a payload field on the main card and mark it as `!must_fill`.
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

    /// Set several main-card payload fields atomically from a plain object,
    /// clearing any `!must_fill` marker on each key. Nothing is applied on
    /// error; the thrown error's `diagnostics` array carries one entry per
    /// offending field (`path` = field name), so externally-sourced names
    /// (database columns, form keys) surface every violation in one pass.
    /// Mirrors Python `set_fields`.
    #[wasm_bindgen(js_name = setFields)]
    pub fn set_fields(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] fields: JsValue,
    ) -> Result<(), JsValue> {
        let batch = js_value_to_field_batch(&fields, "setFields")?;
        self.inner
            .main_mut()
            .set_fields(batch)
            .map_err(edit_errors_to_js)
    }

    /// Remove a payload field on the main card, returning the removed value or
    /// `undefined`. Throws if `name` does not match `[A-Za-z_][A-Za-z0-9_]*`.
    #[wasm_bindgen(js_name = removeField)]
    pub fn remove_field(&mut self, name: &str) -> Result<JsValue, JsValue> {
        let removed = self
            .inner
            .main_mut()
            .remove_field(name)
            .map_err(|e| edit_error_to_js(&e))?;
        Ok(match removed {
            Some(v) => serialize_or_throw(v.as_json(), "removeField")?,
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
        self.inner
            .main_mut()
            .set_ext(map)
            .map_err(|e| edit_error_to_js(&e))?;
        Ok(())
    }

    /// Remove the `$ext` map from the main card *entirely*, returning the
    /// previous map or `undefined`. This is a blunt escape hatch that discards
    /// every namespace at once — prefer `removeExtNamespace` to clear only your
    /// own slot while leaving sibling consumers' state intact.
    #[wasm_bindgen(js_name = removeExt, unchecked_return_type = "Record<string, unknown> | undefined")]
    pub fn remove_ext(&mut self) -> Result<JsValue, JsValue> {
        ext_map_to_js(self.inner.main_mut().remove_ext())
    }

    /// Merge `value` into the main card's `$ext` map under `namespace`, creating
    /// the map when absent and replacing any existing value at that key. Sibling
    /// namespaces are preserved, so independent consumers (`$ext.editor`,
    /// `$ext.agent`, …) don't clobber each other.
    #[wasm_bindgen(js_name = setExtNamespace)]
    pub fn set_ext_namespace(&mut self, namespace: &str, value: JsValue) -> Result<(), JsValue> {
        let json = js_value_to_json(value, "setExtNamespace")?;
        self.inner
            .main_mut()
            .set_ext_namespace(namespace, json)
            .map_err(|e| edit_error_to_js(&e))?;
        Ok(())
    }

    /// Remove `namespace` from the main card's `$ext` map, returning the value
    /// stored there or `undefined`. This is the recommended way to clear `$ext`
    /// state: sibling namespaces survive, and when the last namespace is removed
    /// the `$ext` entry is dropped entirely (not left as `$ext: {}`).
    #[wasm_bindgen(js_name = removeExtNamespace)]
    pub fn remove_ext_namespace(&mut self, namespace: &str) -> Result<JsValue, JsValue> {
        json_value_to_js(self.inner.main_mut().remove_ext_namespace(namespace))
    }

    /// Merge a card-kind's seed `overlay` into the main card's `$seed` map
    /// under `cardKind`, preserving sibling kinds. Sets the starting values
    /// new cards of that kind spawn with. Throws if `overlay` cannot be
    /// serialized or nests too deep.
    #[wasm_bindgen(js_name = setSeedNamespace)]
    pub fn set_seed_namespace(&mut self, card_kind: &str, overlay: JsValue) -> Result<(), JsValue> {
        let json = js_value_to_json(overlay, "setSeedNamespace")?;
        self.inner
            .main_mut()
            .set_seed_namespace(card_kind, json)
            .map_err(|e| edit_error_to_js(&e))?;
        Ok(())
    }

    /// Remove `cardKind` from the main card's `$seed` map, returning its
    /// overlay or `undefined`; drops `$seed` entirely once empty. Sibling
    /// kinds survive.
    #[wasm_bindgen(js_name = removeSeedNamespace)]
    pub fn remove_seed_namespace(&mut self, card_kind: &str) -> Result<JsValue, JsValue> {
        json_value_to_js(self.inner.main_mut().remove_seed_namespace(card_kind))
    }

    /// Replace the `$ext` map on the composable card at `index`. Throws if out
    /// of range or `value` is not a plain object. Named to mirror `setExt` on
    /// the main card; `setCardExtNamespace` is the sibling-safe alternative.
    #[wasm_bindgen(js_name = setCardExt)]
    pub fn set_card_ext(&mut self, index: usize, value: JsValue) -> Result<(), JsValue> {
        let map = js_value_to_object(&value, "setCardExt")?;
        self.card_mut_or_throw(index)?
            .set_ext(map)
            .map_err(|e| edit_error_to_js(&e))?;
        Ok(())
    }

    /// Remove the `$ext` map from the composable card at `index` *entirely*,
    /// returning the previous map or `undefined`. Throws if out of range.
    /// Prefer `removeCardExtNamespace` to clear only one consumer's slot.
    #[wasm_bindgen(js_name = removeCardExt, unchecked_return_type = "Record<string, unknown> | undefined")]
    pub fn remove_card_ext(&mut self, index: usize) -> Result<JsValue, JsValue> {
        ext_map_to_js(self.card_mut_or_throw(index)?.remove_ext())
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
            .set_ext_namespace(namespace, json)
            .map_err(|e| edit_error_to_js(&e))?;
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
        json_value_to_js(
            self.card_mut_or_throw(index)?
                .remove_ext_namespace(namespace),
        )
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

    /// **Deprecated** — alias for `revise({}, markdown)`, kept one release cycle.
    /// Revise the main card's body from a markdown string (edit semantics: a
    /// `diff_import` that rebases surviving anchors). Discards the text delta;
    /// call [`revise`](Document::revise) to receive it.
    #[wasm_bindgen(js_name = replaceBody)]
    pub fn replace_body(&mut self, body: &str) -> Result<(), JsValue> {
        self.inner
            .main_mut()
            .revise_body(body)
            .map(|_| ())
            .map_err(|e| edit_error_to_js(&e))
    }

    /// **Install** a richtext value at `addr` — **value semantics**, corpus only.
    /// Stores exactly `rt` (a canonical `RichText` corpus object); the identity
    /// anchors of any previous value are gone. An absent `addr.field` targets the
    /// body, an absent `addr.card` the main card. For "here's new markdown," use
    /// [`revise`](Document::revise); the cold-import path is spelled at the call
    /// site as `install(addr, importMarkdown(md))`, so anchor loss is visible in
    /// source.
    ///
    /// Throws on an out-of-range card, a malformed field name, or an `rt` that is
    /// not a canonical corpus object.
    #[wasm_bindgen(js_name = install)]
    pub fn install(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr")] addr: JsValue,
        #[wasm_bindgen(unchecked_param_type = "RichText")] rt: JsValue,
    ) -> Result<(), JsValue> {
        let addr = Addr::from_js(&addr)?;
        let corpus = js_to_corpus(rt, "install")?;
        let card = self.addr_card_mut(&addr)?;
        match &addr.field {
            None => {
                card.install_body(corpus);
                Ok(())
            }
            Some(field) => card.install_field(field, corpus).map_err(|e| edit_error_to_js(&e)),
        }
    }

    /// **Revise** the richtext value at `addr` from a markdown string — **edit
    /// semantics**, the default write path, returning the text [`Delta`]. Imports
    /// the markdown, diffs it against the current value, rebases surviving
    /// identity anchors, and returns the change an editor bridge maps its own
    /// positions through (`mapPos`). An absent `addr.field` targets the body, an
    /// absent `addr.card` the main card; an absent field cold-imports from empty.
    ///
    /// Throws on an out-of-range card, a malformed field name, a present
    /// non-corpus field value, or an over-nested markdown input.
    #[wasm_bindgen(js_name = revise, unchecked_return_type = "Delta")]
    pub fn revise(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr")] addr: JsValue,
        markdown: &str,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js(&addr)?;
        let card = self.addr_card_mut(&addr)?;
        let delta = match &addr.field {
            None => card.revise_body(markdown),
            Some(field) => card.revise_field(field, markdown),
        }
        .map_err(|e| edit_error_to_js(&e))?;
        serialize_or_throw(&delta, "revise")
    }

    /// **Apply** a committed corpus edit `bundle` (`{ delta?, lineOps?, markOps? }`)
    /// at `addr` — the editor splice: text delta first, then line ops, then mark
    /// ops (mark ranges in post-delta coordinates), each all-or-nothing. An absent
    /// `addr.field` targets the body, an absent `addr.card` the main card.
    ///
    /// Throws on an out-of-range card, a field that is not richtext, a malformed
    /// bundle, or an op that applies out of bounds (the value is unchanged on a
    /// failed apply).
    #[wasm_bindgen(js_name = applyChange)]
    pub fn apply_change(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr")] addr: JsValue,
        #[wasm_bindgen(unchecked_param_type = "ChangeBundle")] bundle: JsValue,
    ) -> Result<(), JsValue> {
        let addr = Addr::from_js(&addr)?;
        let (delta, line_ops, mark_ops) = parse_change_bundle(&bundle)?;
        let card = self.addr_card_mut(&addr)?;
        match &addr.field {
            None => card.apply_body_change(&delta, &line_ops, &mark_ops),
            Some(field) => {
                card.apply_field_richtext_change(field, &delta, &line_ops, &mark_ops)
            }
        }
        .map_err(|e| edit_error_to_js(&e))
    }

    /// Typed field write on the main card, resolving the field's schema `type`
    /// from `quill` — the one write verb for **every** field type (richtext,
    /// scalar, array, object). The schema carries the `inline` constraint, so no
    /// type token or flag is passed. A richtext-typed field stores the canonical
    /// corpus, so identity marks (anchors, island ids) and corpus-only marks
    /// (e.g. `underline`) live on it and survive compiles and the storage DTO.
    /// Values use the encoding the seam already speaks: a corpus object
    /// or markdown string for richtext, a scalar/array/object otherwise.
    ///
    /// A field declared in the schema is strict-committed — a mismatch throws
    /// now, not at render. A name the schema does not declare throws
    /// `[EditError::UnknownField]` rather than falling to the opaque store: on
    /// the typed path it is a typo. Use [`setField`](Document::set_field) when
    /// opaque storage is the intent. Also throws `[EditError::FieldConform]` /
    /// `[EditError::FieldRichtextDecode]` / `[EditError::FieldRichtextNotInline]`
    /// on a typed mismatch and `[EditError::InvalidFieldName]` on a malformed
    /// name.
    ///
    /// The `quill` handle is passed per call because a `Document` carries only a
    /// `$quill` reference, not the resolved schema.
    #[wasm_bindgen(js_name = commitField)]
    pub fn commit_field(
        &mut self,
        quill: &Quill,
        name: &str,
        value: JsValue,
    ) -> Result<(), JsValue> {
        let json = js_value_to_json(value, "commitField")?;
        let qv = quillmark_core::QuillValue::from_json(json);
        quill
            .inner
            .editor(&mut self.inner)
            .set(name, qv)
            .map_err(|e| edit_error_to_js(&e))
    }

    /// Batched twin of [`commitField`](Document::commit_field): typed-commit
    /// several main-card fields atomically, resolving each field's schema `type`
    /// from `quill`. All-or-nothing with the same per-field-diagnostic error
    /// contract as [`setFields`](Document::set_fields) — nothing is applied on
    /// error and the thrown error's `diagnostics` carry one entry per offending
    /// field, including an `[EditError::UnknownField]` for any name the schema
    /// does not declare, so a whole-form submit sees every typo in one pass.
    #[wasm_bindgen(js_name = commitFields)]
    pub fn commit_fields(
        &mut self,
        quill: &Quill,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] fields: JsValue,
    ) -> Result<(), JsValue> {
        let batch = js_value_to_field_batch(&fields, "commitFields")?;
        quill
            .inner
            .editor(&mut self.inner)
            .set_all(batch)
            .map_err(edit_errors_to_js)
    }

    /// Build a composable card of `kind`, typed-commit `fields` onto it, set its
    /// body from optional markdown, and append it — the ABI under
    /// `editor.addCard`. Fuses `makeCard` + typed commit + `pushCard`
    /// transactionally: the card is committed in full before it joins the
    /// document, so a rejected field (or an invalid kind or body) leaves the
    /// document untouched. Field errors throw the same per-field diagnostic
    /// bundle as [`commitFields`](Self::commit_fields), including an
    /// `[EditError::UnknownField]` per undeclared name; an invalid kind or body
    /// throws a single-entry bundle keyed `$kind` / `$body`.
    #[wasm_bindgen(js_name = addCard)]
    pub fn add_card(
        &mut self,
        quill: &Quill,
        kind: &str,
        #[wasm_bindgen(unchecked_optional_param_type = "Record<string, unknown>")] fields: Option<
            JsValue,
        >,
        #[wasm_bindgen(unchecked_optional_param_type = "string")] body: Option<String>,
    ) -> Result<(), JsValue> {
        let batch = match fields {
            Some(f) => js_value_to_field_batch(&f, "addCard")?,
            None => Vec::new(),
        };
        quill
            .inner
            .editor(&mut self.inner)
            .add_card(kind, batch, body.as_deref())
            .map_err(edit_errors_to_js)
    }

    /// Build a fresh `Card` from a kind and a flat field map — the ergonomic
    /// constructor for `pushCard` / `insertCard`. `fields` is an optional
    /// `Record<string, unknown>` (each entry becomes a card field, in
    /// insertion order); `body` defaults to `""`. Kind validity is checked by
    /// `pushCard` / `insertCard`, not here.
    #[wasm_bindgen(js_name = makeCard, unchecked_return_type = "Card")]
    pub fn make_card(
        kind: String,
        #[wasm_bindgen(unchecked_optional_param_type = "Record<string, unknown>")] fields: Option<
            JsValue,
        >,
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
                nested_fills: Vec::new(),
            })
            .collect();
        let string_wire = quillmark_core::CardWire {
            kind,
            quill: None,
            id: None,
            ext: None,
            seed: None,
            payload_items,
            // The `body` argument is markdown; `Card::try_from` imports it to the
            // corpus (and validates the fields).
            body: serde_json::Value::String(body.unwrap_or_default()),
        };
        // Round-trip through `Card` so the emitted card carries the corpus body
        // (the source-of-truth shape `cards()` returns), not the raw authored
        // string.
        let card = quillmark_core::Card::try_from(string_wire)
            .map_err(|e| WasmError::from(format!("makeCard: {e}")).to_js_value())?;
        let wire = quillmark_core::CardWire::from(&card);
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        wire.serialize(&serializer).map_err(|e| {
            WasmError::from(format!("makeCard: serialization failed: {e}")).to_js_value()
        })
    }

    /// Append a card to the end of the card list. Accepts a `CardInput` — a card
    /// read back (`cards` / `removeCard` / `quill.seedCard`), a
    /// [`makeCard`](Document::make_card) result, or a bare `{ kind, body }`
    /// (every returned `Card` is a valid `CardInput`). Throws if `card.kind` is
    /// not a valid kind name.
    #[wasm_bindgen(js_name = pushCard)]
    pub fn push_card(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "CardInput")] card: JsValue,
    ) -> Result<(), JsValue> {
        let core_card = js_to_card(&card)?;
        self.inner
            .push_card(core_card)
            .map_err(|e| edit_error_to_js(&e))
    }

    /// Insert a card at `index` (must be in `0..=cards.length`). Accepts a
    /// `CardInput` (see [`pushCard`](Self::push_card)).
    #[wasm_bindgen(js_name = insertCard)]
    pub fn insert_card(
        &mut self,
        index: usize,
        #[wasm_bindgen(unchecked_param_type = "CardInput")] card: JsValue,
    ) -> Result<(), JsValue> {
        let core_card = js_to_card(&card)?;
        self.inner
            .insert_card(index, core_card)
            .map_err(|e| edit_error_to_js(&e))
    }

    #[wasm_bindgen(js_name = removeCard, unchecked_return_type = "Card | undefined")]
    pub fn remove_card(&mut self, index: usize) -> Result<JsValue, JsValue> {
        match self.inner.remove_card(index) {
            Some(core_card) => card_to_js(&core_card),
            None => Ok(JsValue::UNDEFINED),
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

    /// Set a field on the card at `index` — the card-indexed twin of
    /// [`setField`](Document::set_field). Stores the value opaquely.
    /// Throws if `index` is out of range, `name` is reserved or invalid.
    #[wasm_bindgen(js_name = setCardField)]
    pub fn set_card_field(
        &mut self,
        index: usize,
        name: &str,
        value: JsValue,
    ) -> Result<(), JsValue> {
        let json = js_value_to_json(value, "setCardField")?;
        let qv = quillmark_core::QuillValue::from_json(json);
        self.card_mut_or_throw(index)?
            .set_field(name, qv)
            .map_err(|e| edit_error_to_js(&e))
    }

    /// Typed field write on the composable card at `index` — the card-indexed
    /// twin of [`commitField`](Document::commit_field). Resolves the field's
    /// type from the card's `$kind` schema in `quill` and strict-commits it.
    ///
    /// Throws `[EditError::IndexOutOfRange]` when `index` is out of range, and
    /// the same typed-mismatch / name errors as `commitField` — including
    /// `[EditError::UnknownField]` for a field the card-kind schema does not
    /// declare (an unknown `$kind` has no schema, so every field is undeclared).
    #[wasm_bindgen(js_name = commitCardField)]
    pub fn commit_card_field(
        &mut self,
        quill: &Quill,
        index: usize,
        name: &str,
        value: JsValue,
    ) -> Result<(), JsValue> {
        let json = js_value_to_json(value, "commitCardField")?;
        let qv = quillmark_core::QuillValue::from_json(json);
        let mut editor = quill.inner.editor(&mut self.inner);
        let mut card = editor.card(index).map_err(|e| edit_error_to_js(&e))?;
        card.set(name, qv).map_err(|e| edit_error_to_js(&e))
    }

    /// Batched twin of [`commitCardField`](Document::commit_card_field):
    /// typed-commit several fields on the card at `index` atomically, resolving
    /// each field's type from the card's `$kind` schema in `quill`. All-or-nothing
    /// with the same per-field-diagnostic contract as
    /// [`commitFields`](Document::commit_fields), including an
    /// `[EditError::UnknownField]` diagnostic per undeclared name. Throws
    /// `[EditError::IndexOutOfRange]` when `index` is out of range.
    #[wasm_bindgen(js_name = commitCardFields)]
    pub fn commit_card_fields(
        &mut self,
        quill: &Quill,
        index: usize,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] fields: JsValue,
    ) -> Result<(), JsValue> {
        let batch = js_value_to_field_batch(&fields, "commitCardFields")?;
        let mut editor = quill.inner.editor(&mut self.inner);
        editor
            .card(index)
            .map_err(|e| edit_error_to_js(&e))?
            .set_all(batch)
            .map_err(edit_errors_to_js)
    }

    /// Batched twin of [`setCardField`](Document::set_card_field): set
    /// several fields on the card at `index` atomically. Same all-or-nothing,
    /// one-diagnostic-per-field contract as [`setFields`](Document::set_fields).
    /// Throws if `index` is out of range.
    #[wasm_bindgen(js_name = setCardFields)]
    pub fn set_card_fields(
        &mut self,
        index: usize,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] fields: JsValue,
    ) -> Result<(), JsValue> {
        let batch = js_value_to_field_batch(&fields, "setCardFields")?;
        self.card_mut_or_throw(index)?
            .set_fields(batch)
            .map_err(edit_errors_to_js)
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
            Some(v) => serialize_or_throw(v.as_json(), "removeField")?,
            None => JsValue::UNDEFINED,
        })
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

    /// Resolve the card an [`Addr`] targets: the main card when `addr.card` is
    /// absent, else the composable card at that index (out-of-range throws). The
    /// static address axis the addressed content verbs share.
    fn addr_card_mut(&mut self, addr: &Addr) -> Result<&mut quillmark_core::Card, JsValue> {
        match addr.card {
            None => Ok(self.inner.main_mut()),
            Some(index) => self.card_mut_or_throw(index),
        }
    }
}

// ── Addressed write surface ────────────────────────────────────────────────────

/// A richtext write address (`{ card?, field? }`). Absent `field` = body, absent
/// `card` = main. Mirrors the `Addr` TS interface.
#[derive(serde::Deserialize, Default)]
struct Addr {
    #[serde(default)]
    card: Option<usize>,
    #[serde(default)]
    field: Option<String>,
}

impl Addr {
    /// Deserialize an `Addr` from its JS object (`undefined`/`null`/`{}` all mean
    /// the main-card body).
    fn from_js(value: &JsValue) -> Result<Addr, JsValue> {
        if value.is_undefined() || value.is_null() {
            return Ok(Addr::default());
        }
        serde_wasm_bindgen::from_value(value.clone())
            .map_err(|e| WasmError::from(format!("addr must be an Addr object: {e}")).to_js_value())
    }
}

/// Decode a JS value as a canonical `RichText` corpus object — the `install`
/// input (value semantics, corpus only). Rejects a markdown string: the cold
/// path is spelled `install(addr, importMarkdown(md))`.
fn js_to_corpus(value: JsValue, ctx: &str) -> Result<quillmark_core::RichText, JsValue> {
    let json = js_value_to_json(value, ctx)?;
    if !json.is_object() {
        return Err(WasmError::from(format!(
            "{ctx}: expected a RichText corpus object; for markdown use importMarkdown(md) first"
        ))
        .to_js_value());
    }
    quillmark_richtext::serial::from_canonical_value(&json)
        .map_err(|e| WasmError::from(format!("{ctx}: not a canonical RichText corpus: {e}")).to_js_value())
}

/// Lower a `ChangeBundle` (`{ delta?, lineOps?, markOps? }`) to core ops via the
/// shared richtext reader, mapping its message to a `WasmError`.
fn parse_change_bundle(
    value: &JsValue,
) -> Result<
    (
        quillmark_core::Delta,
        Vec<quillmark_core::LineOp>,
        Vec<quillmark_core::MarkOp>,
    ),
    JsValue,
> {
    let json = js_value_to_json(value.clone(), "applyChange")?;
    quillmark_richtext::change_bundle_from_value(&json)
        .map_err(|e| WasmError::from(format!("applyChange: {e}")).to_js_value())
}

// ── Corpus codec (document-free) ────────────────────────────────────────────────

/// Import a markdown string to a canonical `RichText` corpus — the pure,
/// document-free codec. Pair with `install(addr, importMarkdown(md))` to spell
/// the cold (anchor-losing) write at the call site; prefer `revise` for edit
/// semantics. Throws on an over-nested input.
#[wasm_bindgen(js_name = importMarkdown, unchecked_return_type = "RichText")]
pub fn import_markdown(markdown: &str) -> Result<JsValue, JsValue> {
    let corpus = quillmark_richtext::from_markdown(markdown)
        .map_err(|e| WasmError::from(format!("importMarkdown: {e}")).to_js_value())?;
    serialize_or_throw(
        &quillmark_richtext::serial::to_canonical_value(&corpus),
        "importMarkdown",
    )
}

/// Export a canonical `RichText` corpus to its markdown projection — the pure
/// codec that replaces the eager `bodyMarkdown` / `fieldMarkdown` precomputes
/// (`exportMarkdown(card.body)`). Throws if `rt` is not a canonical corpus.
#[wasm_bindgen(js_name = exportMarkdown)]
pub fn export_markdown(
    #[wasm_bindgen(unchecked_param_type = "RichText")] rt: JsValue,
) -> Result<String, JsValue> {
    let corpus = js_to_corpus(rt, "exportMarkdown")?;
    Ok(quillmark_richtext::to_markdown(&corpus))
}

/// Rebase `markdown` onto a `base` corpus — the pure, document-free twin of
/// `revise`: cold-import + `diff_import`, returning the new `corpus` and the
/// text `delta` (surviving anchors rebased). Use it to compute a revise without
/// a document in hand; `revise(addr, md)` fuses this with the store for
/// atomicity. Throws on an over-nested markdown input or a non-corpus `base`.
#[wasm_bindgen(js_name = rebase, unchecked_return_type = "{ corpus: RichText; delta: Delta }")]
pub fn rebase(
    #[wasm_bindgen(unchecked_param_type = "RichText")] base: JsValue,
    markdown: &str,
) -> Result<JsValue, JsValue> {
    let base = js_to_corpus(base, "rebase")?;
    let (corpus, delta) = quillmark_richtext::diff_import(&base, markdown)
        .map_err(|e| WasmError::from(format!("rebase: {e}")).to_js_value())?;
    let out = serde_json::json!({
        "corpus": quillmark_richtext::serial::to_canonical_value(&corpus),
        "delta": serde_json::to_value(&delta).unwrap_or(serde_json::Value::Null),
    });
    serialize_or_throw(&out, "rebase")
}

/// Map a base corpus position through a `delta` to its new position — the pure
/// position-mapping codec an editor bridge composes to hold a caret stable
/// across a `revise`. `assoc` decides the side of a same-position insertion
/// (`"after"` moves past it). Throws on a malformed `delta`.
#[wasm_bindgen(js_name = mapPos)]
pub fn map_pos(
    #[wasm_bindgen(unchecked_param_type = "Delta")] delta: JsValue,
    pos: usize,
    #[wasm_bindgen(unchecked_param_type = "Assoc")] assoc: JsValue,
) -> Result<usize, JsValue> {
    let delta: quillmark_core::Delta = serde_wasm_bindgen::from_value(delta)
        .map_err(|e| WasmError::from(format!("mapPos: invalid delta: {e}")).to_js_value())?;
    let assoc = match assoc.as_string().as_deref() {
        Some("before") => quillmark_core::Assoc::Before,
        Some("after") => quillmark_core::Assoc::After,
        _ => {
            return Err(
                WasmError::from("mapPos: assoc must be \"before\" or \"after\"").to_js_value(),
            )
        }
    };
    Ok(delta.map_pos(pos, assoc))
}

// ── Edit helpers ──────────────────────────────────────────────────────────────

/// Maps `EditError` to a JS `Error` with the variant name and details in the message.
fn edit_error_to_js(err: &quillmark_core::EditError) -> JsValue {
    WasmError::from(format!("[EditError::{}] {}", err.variant_name(), err)).to_js_value()
}

/// Batched-mutator twin of [`edit_error_to_js`]: one diagnostic per offending
/// field, each with `path` set to the field name.
fn edit_errors_to_js(errors: Vec<(String, quillmark_core::EditError)>) -> JsValue {
    let diagnostics: Vec<quillmark_core::Diagnostic> = errors
        .into_iter()
        .map(|(name, err)| {
            quillmark_core::Diagnostic::new(
                quillmark_core::Severity::Error,
                format!("[EditError::{}] {}", err.variant_name(), err),
            )
            .with_path(name)
        })
        .collect();
    WasmError { diagnostics }.to_js_value()
}

/// Deserialize a plain JS object into the `(name, value)` batch
/// `Card::set_fields` consumes. Key order is preserved (`preserve_order`
/// is on workspace-wide) so field insertion order follows the object.
fn js_value_to_field_batch(
    value: &JsValue,
    ctx: &str,
) -> Result<Vec<(String, quillmark_core::QuillValue)>, JsValue> {
    match js_value_to_json(value.clone(), ctx)? {
        serde_json::Value::Object(map) => Ok(map
            .into_iter()
            .map(|(name, v)| (name, quillmark_core::QuillValue::from_json(v)))
            .collect()),
        _ => Err(WasmError::from(format!("{}: fields must be a plain object", ctx)).to_js_value()),
    }
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

/// Serialize `value` to its JS shape (maps as objects), throwing a
/// `WasmError` naming `what` when serialization fails. The throwing
/// counterpart of a silent `undefined` fallback: `undefined` from a getter
/// reads as "property absent" and crashes callers far from the cause, where
/// a thrown error names it.
fn serialize_or_throw<T: serde::Serialize + ?Sized>(
    value: &T,
    what: &str,
) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    value
        .serialize(&serializer)
        .map_err(|e| WasmError::from(format!("{what}: serialization failed: {e}")).to_js_value())
}

/// Serialize an optional JSON value to JS, or `undefined` when `None`. Backs
/// both the namespaced reads (any value) and the whole-map reads (via
/// `ext_map_to_js`).
fn json_value_to_js(value: Option<serde_json::Value>) -> Result<JsValue, JsValue> {
    match value {
        Some(v) => serialize_or_throw(&v, "ext value"),
        None => Ok(JsValue::UNDEFINED),
    }
}

/// Serialize an optional `$ext` map to a JS object, or `undefined` when `None`.
fn ext_map_to_js(
    map: Option<serde_json::Map<String, serde_json::Value>>,
) -> Result<JsValue, JsValue> {
    json_value_to_js(map.map(serde_json::Value::Object))
}

/// Serialize a core [`Card`](quillmark_core::Card) to its `Card` JS shape via
/// the canonical [`CardWire`](quillmark_core::CardWire). The single place WASM
/// turns a core card into JS — used by `Document.main`, `cards`, `removeCard`,
/// and the seed getters.
fn card_to_js(card: &quillmark_core::Card) -> Result<JsValue, JsValue> {
    serialize_or_throw(&quillmark_core::CardWire::from(card), "card")
}

/// Deserialize a `CardInput`-shaped JS value into a core
/// [`Card`](quillmark_core::Card) via [`CardWire`](quillmark_core::CardWire).
/// The single place WASM turns JS into a core card — used by `pushCard` /
/// `insertCard`.
fn js_to_card(value: &JsValue) -> Result<quillmark_core::Card, JsValue> {
    // `serde_wasm_bindgen` does not honor the core type's
    // `#[serde(deny_unknown_fields)]` (it looks up known fields rather than
    // visiting every key), so enforce it here to match the Python binding:
    // a flat `{ kind, fields }` object fails loudly instead of yielding a
    // silently-empty card.
    if let Some(obj) = value.dyn_ref::<js_sys::Object>() {
        const ALLOWED: &[&str] = &[
            "kind",
            "quill",
            "id",
            "ext",
            "seed",
            "payloadItems",
            "body",
        ];
        for key in js_sys::Object::keys(obj).iter() {
            if let Some(k) = key.as_string() {
                if !ALLOWED.contains(&k.as_str()) {
                    return Err(WasmError::from(format!(
                        "card has unknown field `{k}`; expected a CardInput \
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
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[wasm_bindgen(typescript_custom_section)]
const CANVAS_PREVIEW_TS: &'static str = r#"
/**
 * Page dimensions in points (1 pt = 1/72 inch). Typst measures in Typst
 * points; pdfform measures in PDF points — the same unit.
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
 * Inputs to `LiveSession.paint`. Both fields are optional and default
 * to `1`.
 *
 * - `layoutScale` — layout-space pixels per point (Typst point / PDF
 *   point — the same 1/72″ unit). For on-screen
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
 * Returned by `LiveSession.paint`.
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
 *   case `densityScale` was clamped to fit.
 * - `clamped` — `true` when that 16384-px clamp fired, so the page is
 *   painted at fewer device pixels than requested and renders soft at the
 *   same `canvas.style` size. Reads the clamp off the return value instead
 *   of the `pixelWidth < round(layoutWidth * densityScale)` derivation.
 * - `effectiveDensityScale` — the `densityScale` actually applied: the
 *   requested value unless `clamped`, then reduced proportionally.
 *   `layoutScale * effectiveDensityScale` is the scale the backing store
 *   was rasterized at.
 *
 * The painter owns `canvas.width` / `canvas.height`; consumers must not
 * write to them. The painter does **not** touch `canvas.style.*`;
 * consumers own layout. The write is a whole-backing-store `putImageData`,
 * which bypasses the 2D context transform, `globalAlpha`, and clip: give
 * each visible page its own `` — you cannot composite two pages, a
 * sub-rect, or a context transform through `paint`.
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
    clamped: boolean;
    effectiveDensityScale: number;
}
"#;

#[cfg(any(feature = "typst", feature = "pdfform"))]
#[wasm_bindgen]
impl LiveSession {
    #[wasm_bindgen(getter, js_name = pageCount)]
    pub fn page_count(&self) -> usize {
        self.inner.page_count()
    }

    /// The backend that produced this session (e.g. `"typst"`).
    #[wasm_bindgen(getter, js_name = backendId)]
    pub fn backend_id(&self) -> String {
        self.backend_id.clone()
    }

    /// `true` iff `paint` and `pageSize` will succeed for this session. Derived
    /// from the session's canvas seam, so it reflects exactly what `paint` will
    /// do — no separately captured flag.
    #[wasm_bindgen(getter, js_name = supportsCanvas)]
    pub fn supports_canvas(&self) -> bool {
        self.inner.supports_canvas()
    }

    /// Non-fatal diagnostics of the session's **current compile** (e.g. Typst
    /// font fallback) — set at open and refreshed by each committed `apply`;
    /// a failed apply keeps the last-good compile's warnings. Also appended
    /// to `RenderResult.warnings` on each `render()` call.
    #[wasm_bindgen(getter, js_name = warnings, unchecked_return_type = "Diagnostic[]")]
    pub fn warnings(&self) -> Result<JsValue, JsValue> {
        let diags: Vec<Diagnostic> = self
            .inner
            .warnings()
            .iter()
            .cloned()
            .map(Into::into)
            .collect();
        serialize_or_throw(&diags, "warnings")
    }

    /// Recompile the session against `doc` — the edit verb of a live preview.
    /// The document is compiled through the same schema pipeline as `open`
    /// (same quill), then applied transactionally: on throw every read
    /// (`render`, `paint`, `pageSize`, `regions`, `fieldAt`) keeps serving the last-good
    /// compile, and the session recovers on the next successful `apply`. On
    /// success reads serve the new compile; repaint `dirtyPages ∩ visible`.
    #[wasm_bindgen(js_name = apply)]
    pub fn apply(&mut self, doc: &Document) -> Result<ChangeSet, JsValue> {
        let json_data = self.compile_checked(&doc.inner)?;
        let cs = self
            .inner
            .apply(&json_data)
            .map_err(|e| WasmError::from(e).to_js_value())?;
        Ok(ChangeSet {
            page_count: cs.page_count,
            dirty_pages: cs.dirty_pages,
        })
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
            regions: result.regions.into_iter().map(Into::into).collect(),
        })
    }

    /// Schema-field geometry for this compiled session — each content field's
    /// **first placement** (one region per page it touches) plus widget and
    /// scalar-reference-site regions, keyed on the quill schema field path; a
    /// field may still appear more than once (group by `field`, see
    /// `FieldRegion`). A session-level query: no render, no byte artifact. An
    /// interactive preview reads it to scroll to / highlight the focused
    /// field over a `paint`-ed canvas; the click direction is `fieldAt`.
    /// Empty for backends that place no schema fields.
    #[wasm_bindgen(js_name = regions, unchecked_return_type = "FieldRegion[]")]
    pub fn regions(&self) -> Result<JsValue, JsValue> {
        let regions: Vec<FieldRegion> = self.inner.regions().into_iter().map(Into::into).collect();
        serialize_or_throw(&regions, "regions")
    }

    /// The whole-field highlight boxes for `field` — one union rect per page,
    /// over the field's `span`-bearing content segments. The convenience that
    /// owns the union `regions()` leaves derived: it keeps `regions()` the
    /// low-level disjoint truth (#829) and folds the span-filter + per-page
    /// union here, so a "highlight the focused field" consumer stops
    /// reimplementing it. **Content only** — a field placed solely as a scalar
    /// reference or a bound widget carries no `span` and returns `[]`; its box
    /// is a single `regions()` rect. Reflects the current compile, like
    /// `regions()`.
    #[wasm_bindgen(js_name = fieldBoxes, unchecked_return_type = "FieldRegion[]")]
    pub fn field_boxes(&self, field: &str) -> Result<JsValue, JsValue> {
        let boxes: Vec<FieldRegion> = self
            .inner
            .field_boxes(field)
            .into_iter()
            .map(Into::into)
            .collect();
        serialize_or_throw(&boxes, "fieldBoxes")
    }

    /// The schema field whose content is under a point on `page` — the
    /// forward (click → field) direction: hit-test a click against the
    /// compiled document and get back the field address to focus in the
    /// editor, or `undefined` off any field's ink. `x`/`y` are PDF points
    /// with a **bottom-left** origin, the same space as `FieldRegion.rect` —
    /// from a canvas click, invert the overlay transform documented on
    /// `FieldRegion`: `x = clickPx.x / renderScale`,
    /// `y = pageHeightPt - clickPx.y / renderScale`. Unlike `regions()`,
    /// *every* placement answers, not just the first.
    #[wasm_bindgen(js_name = fieldAt)]
    pub fn field_at(&self, page: usize, x: f32, y: f32) -> Option<String> {
        self.inner.field_at(page, x, y)
    }

    /// A point → **corpus position** — the fine-grained click direction:
    /// hit-test a point and get back the field *and* a USV offset into its
    /// `RichText` (for placing a caret or mapping a selection into the content
    /// model), or `undefined` off all content ink. `x`/`y` are PDF points,
    /// bottom-left origin — the same space as `fieldAt`. The offset is
    /// cluster-exact and degrades to the containing segment's start on
    /// origin-less ink (list markers, a code fence's interior). See
    /// `CorpusHit`.
    #[wasm_bindgen(js_name = positionAt)]
    pub fn position_at(&self, page: usize, x: f32, y: f32) -> Option<CorpusHit> {
        self.inner.position_at(page, x, y).map(Into::into)
    }

    /// A corpus position → **caret rect** — the reverse of `positionAt`: given
    /// a field and a USV offset into its `RichText`, return the box (in the
    /// same bottom-left PDF-point space as `FieldRegion.rect`) to draw a caret
    /// at, its `span` collapsed to `[pos, pos]`; `undefined` when the field
    /// places no tracked content or the offset maps to no drawn glyph.
    #[wasm_bindgen(js_name = locate)]
    pub fn locate(&self, field: &str, pos: usize) -> Option<FieldRegion> {
        self.inner.locate(field, pos).map(Into::into)
    }

    /// Page dimensions in points (1 pt = 1/72 inch).
    /// Throws if the backend has no canvas painter or `page` is out of range.
    #[wasm_bindgen(js_name = pageSize, unchecked_return_type = "PageSize")]
    pub fn page_size(&self, page: usize) -> Result<JsValue, JsValue> {
        self.ensure_canvas("pageSize")?;
        let (width_pt, height_pt) = self
            .inner
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
    /// per side, `densityScale` is clamped — `PaintResult.clamped` reports it and
    /// `PaintResult.effectiveDensityScale` carries the density actually applied.
    ///
    /// `put_image_data` writes the whole backing store, bypassing the 2D
    /// context's transform, `globalAlpha`, and clip: the painter owns the entire
    /// canvas, so each visible page needs its own `` — you cannot composite
    /// two pages, a sub-rect, or a context transform through this call.
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
        self.ensure_canvas("paint")?;
        let canvas_ctx = CanvasCtx::from_js(&ctx)?;

        let (width_pt, height_pt) = self
            .inner
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

        let clamped = max_dim > MAX_BACKING_DIMENSION as f64;
        let effective_density = if clamped {
            (requested_density as f64) * (MAX_BACKING_DIMENSION as f64 / max_dim)
        } else {
            requested_density as f64
        };

        let render_scale = (layout_scale as f64) * effective_density;
        // `layout_scale` and `density` are each validated finite/positive, but
        // their product (or the f64->f32 cast) can still overflow to infinity
        // for extreme inputs — e.g. a zero-dimension page bypasses the
        // MAX_BACKING_DIMENSION clamp. Guard before handing it to the renderer.
        if !render_scale.is_finite() || render_scale <= 0.0 || render_scale > f32::MAX as f64 {
            return Err(WasmError::from(
                "paint: computed render scale is non-finite or out of range",
            )
            .to_js_value());
        }

        // `page_size_pt(page)` already succeeded above, so `page` is in range;
        // a `None` here therefore means the backend reported a canvas
        // (`ensure_canvas` passed) but produced no raster — a capability/impl
        // disagreement, not a bad page index. Label it as such instead of
        // mislabelling it page-out-of-range.
        let (pixel_w, pixel_h, mut rgba) = self
            .inner
            .render_rgba(page, render_scale as f32)
            .ok_or_else(|| {
                WasmError::from(format!(
                    "paint: backend '{}' reported a canvas painter but produced no raster \
                     for page {page} (render_rgba returned None on an in-range page)",
                    self.backend_id
                ))
                .to_js_value()
            })?;

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
            clamped,
            effective_density_scale: effective_density,
        };
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        result
            .serialize(&serializer)
            .map_err(|e| WasmError::from(format!("paint: serialization failed: {e}")).to_js_value())
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl LiveSession {
    /// Gate a canvas operation on the session's canvas capability, derived from
    /// the core `SessionHandle` seam (`page_size_pt` / `render_rgba`) — the same
    /// seam the painter dispatches through, so the gate cannot disagree with the
    /// paint.
    fn ensure_canvas(&self, op: &str) -> Result<(), JsValue> {
        if self.inner.supports_canvas() {
            Ok(())
        } else {
            Err(WasmError::from(format!(
                "{op}: backend '{}' has no canvas painter",
                self.backend_id
            ))
            .to_js_value())
        }
    }

    fn page_oob_error(&self, op: &str, page: usize) -> JsValue {
        WasmError::from(format!(
            "{op}: page index {page} out of range (pageCount={})",
            self.inner.page_count()
        ))
        .to_js_value()
    }

    /// The compile preamble used by `apply`: verify
    /// `doc` still references this session's quill, then compile it to plate
    /// data through the same schema pipeline as `open`. Errors map to JS via
    /// `WasmError`, as the render path does.
    fn compile_checked(&self, doc: &quillmark_core::Document) -> Result<serde_json::Value, JsValue> {
        self.config
            .check_quill_reference(doc)
            .map_err(|e| WasmError::from(e).to_js_value())?;
        self.config
            .compile_data(doc)
            .map_err(|e| WasmError::from(e).to_js_value())
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
enum CanvasCtx<'a> {
    OnScreen(&'a web_sys::CanvasRenderingContext2d),
    OffScreen(&'a web_sys::OffscreenCanvasRenderingContext2d),
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
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

#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Serialize)]
struct PageSize {
    #[serde(rename = "widthPt")]
    width_pt: f32,
    #[serde(rename = "heightPt")]
    height_pt: f32,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaintOptions {
    #[serde(default)]
    layout_scale: Option<f32>,
    #[serde(default)]
    density_scale: Option<f32>,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PaintResult {
    layout_width: f64,
    layout_height: f64,
    pixel_width: u32,
    pixel_height: u32,
    /// True when `MAX_BACKING_DIMENSION` forced `densityScale` down: the page is
    /// painted at fewer device pixels than requested, so it renders soft at the
    /// same `canvas.style` size. The honest form of the pixel-dim derivation
    /// consumers would otherwise reinvent.
    clamped: bool,
    /// The `densityScale` actually applied — equal to the requested value unless
    /// `clamped`, then reduced proportionally. `layoutScale × effectiveDensityScale`
    /// is the scale the backing store was rasterized at.
    effective_density_scale: f64,
}
