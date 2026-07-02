//! Typst backend for Quillmark: converts CommonMark markdown + card-YAML data
//! to PDF, SVG, and PNG via the Typst typesetting system.
//!
//! [`TypstBackend`] implements the [`Backend`] trait from `quillmark-core`.
//! Callers typically reach it through the `quillmark` crate's `Quill` API.
//!
//! Markdown fields are converted to Typst markup before compilation; the plate
//! accesses them via the `@local/quillmark-helper` virtual package. Unsigned
//! AcroForm widgets (text, checkbox, choice, signature) are embedded via the
//! `form-field` helper in `lib.typ`; only PDF output carries the interactive
//! widget — SVG and PNG render an invisible placeholder.
//!
//! The `compile` and `error_mapping` modules are internal and not part of the
//! public API. The public conversion surface is [`convert`].

mod compile;
pub mod convert;
mod error_mapping;

mod helper;
mod overlay;
mod world;

/// Utilities exposed for fuzzing tests.
/// Not intended for general use.
#[doc(hidden)]
pub mod fuzz_utils {
    pub use super::helper::inject_json;
}

use convert::mark_to_typst;
use quillmark_core::{
    quill::build_transform_schema, session::SessionHandle, Backend, ChangeSet, Diagnostic,
    LiveSession, OutputFormat, Quill, QuillValue, RenderError, RenderOptions, RenderResult,
    Severity,
};
use std::any::Any;
use std::collections::HashMap;

/// Typst backend implementation for Quillmark.
#[derive(Debug)]
pub struct TypstBackend;

const SUPPORTED_FORMATS: &[OutputFormat] =
    &[OutputFormat::Pdf, OutputFormat::Svg, OutputFormat::Png];

/// Typst-specific render session.
///
/// Holds the compiled `PagedDocument` *and* the `QuillWorld` it was compiled
/// through. Persisting the world keeps fonts, packages, and assets parsed once
/// per session rather than once per compile — the substrate for incremental
/// recompiles. Exposes Typst-only operations (page geometry, raster rendering)
/// used by the WASM canvas painter.
pub struct TypstSession {
    world: world::QuillWorld,
    document: typst_layout::PagedDocument,
    page_count: usize,
    /// Extracted from each committed compile. Converted to spine `FieldSpec`s
    /// on every render; PDF stamps them as AcroForm widgets, and every format
    /// carries the resulting regions.
    field_placements: Vec<overlay::FieldPlacement>,
    /// The quill schema's markdown/date transform, applied to the raw document
    /// data on `open` and every `apply`.
    transform_schema: QuillValue,
    /// Per-page content fingerprints of the live compile; diffed against the
    /// next compile's to produce `ChangeSet::dirty_pages`.
    page_hashes: Vec<u128>,
}

/// Per-page fingerprints of *visible* content, diffed across compiles for
/// `ChangeSet::dirty_pages`. Walks each page frame hashing text, shapes,
/// images, links, and group geometry — skipping introspection `Tag` items and
/// group parent locations, which carry element hashes spanning content on
/// *other* pages (a page-spanning paragraph's tag sits on its first page and
/// covers the whole text, so hashing it dirties page 0 on an end-of-document
/// edit the page never shows).
fn page_hashes(document: &typst_layout::PagedDocument) -> Vec<u128> {
    use std::hash::{Hash, Hasher};
    use typst::layout::FrameItem;

    fn walk<H: Hasher>(frame: &typst::layout::Frame, state: &mut H) {
        frame.size().hash(state);
        for (pos, item) in frame.items() {
            match item {
                FrameItem::Tag(_) => {}
                FrameItem::Group(g) => {
                    pos.hash(state);
                    g.transform.hash(state);
                    g.clip.hash(state);
                    walk(&g.frame, state);
                }
                other => {
                    pos.hash(state);
                    other.hash(state);
                }
            }
        }
    }

    struct VisiblePage<'a>(&'a typst_layout::Page);
    impl Hash for VisiblePage<'_> {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.0.fill.hash(state);
            self.0.numbering.hash(state);
            self.0.number.hash(state);
            walk(&self.0.frame, state);
        }
    }

    document
        .pages()
        .iter()
        .map(|page| typst::utils::hash128(&VisiblePage(page)))
        .collect()
}

/// Run the schema's markdown/date transform over raw document data and
/// serialize it for helper-package injection.
fn transformed_json_str(
    schema: &QuillValue,
    json_data: &serde_json::Value,
) -> Result<String, RenderError> {
    let fields = json_data.as_object().map_or_else(HashMap::new, |obj| {
        obj.iter()
            .map(|(key, value)| (key.clone(), QuillValue::from_json(value.clone())))
            .collect::<HashMap<_, _>>()
    });

    let transformed_fields = transform_markdown_fields(&fields, schema);
    let transformed_json = serde_json::Value::Object(
        transformed_fields
            .into_iter()
            .map(|(key, value)| (key, value.into_json()))
            .collect(),
    );

    serde_json::to_string(&transformed_json).map_err(|e| RenderError::EngineCreation {
        diags: vec![Diagnostic::new(
            Severity::Error,
            format!(
                "failed to serialize document data for the typst backend: {}",
                e
            ),
        )
        .with_code("backend::data_serialization_failed".to_string())],
    })
}

impl SessionHandle for TypstSession {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let format = opts.output_format.unwrap_or(OutputFormat::Pdf);

        if !SUPPORTED_FORMATS.contains(&format) {
            return Err(RenderError::FormatNotSupported {
                diags: vec![Diagnostic::new(
                    Severity::Error,
                    format!("{:?} not supported by typst backend", format),
                )
                .with_code("backend::format_not_supported".to_string())
                .with_hint(format!("Supported formats: {:?}", SUPPORTED_FORMATS))],
            });
        }

        compile::render_document_pages(
            &self.document,
            opts.pages.as_deref(),
            format,
            opts.ppi,
            &self.field_placements,
            opts.producer.as_deref(),
        )
    }

    fn page_count(&self) -> usize {
        self.page_count
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// Incremental recompile against new document data. The persistent world
    /// keeps fonts/packages/assets parsed; the helper `lib.typ` is swapped via
    /// `Source::replace` (incremental reparse), and `comemo` reuses every
    /// memoized result the edit did not reach. Transactional: the live
    /// document, placements, and hashes swap together only after the compile
    /// *and* placement extraction succeed — on `Err` every read keeps serving
    /// the last-good compile (the world may hold the failed source; the next
    /// `apply` overwrites it).
    fn apply(&mut self, json_data: &serde_json::Value) -> Result<ChangeSet, RenderError> {
        let json_str = transformed_json_str(&self.transform_schema, json_data)?;
        self.world.inject_helper_package(&json_str);

        let document = compile::compile_document(&self.world)?;
        let field_placements = overlay::extract(&document)?;
        let new_hashes = page_hashes(&document);

        let dirty_pages = (0..new_hashes.len())
            .filter(|&i| self.page_hashes.get(i) != Some(&new_hashes[i]))
            .collect();

        self.document = document;
        self.field_placements = field_placements;
        self.page_count = new_hashes.len();
        self.page_hashes = new_hashes;

        Ok(ChangeSet {
            page_count: self.page_count,
            dirty_pages,
        })
    }

    /// Page dimensions in Typst points (1 pt = 1/72 inch). `None` if `page` is
    /// out of range. Overrides the default-`None` canvas seam.
    fn page_size_pt(&self, page: usize) -> Option<(f32, f32)> {
        let frame = &self.document.pages().get(page)?.frame;
        let size = frame.size();
        Some((size.x.to_pt() as f32, size.y.to_pt() as f32))
    }

    /// Render `page` to a non-premultiplied RGBA8 buffer at `scale`× the
    /// natural 72 ppi (`scale = 1` → 1 device pixel per Typst pt). Returns
    /// `(width_px, height_px, rgba)` (`w * h * 4` bytes, row-major), or `None`
    /// if `page` is out of range. Overrides the default-`None` canvas seam.
    fn render_rgba(&self, page: usize, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
        let p = self.document.pages().get(page)?;
        let pixmap = typst_render::render(
            p,
            &typst_render::RenderOptions {
                pixel_per_pt: typst::utils::Scalar::new(scale as f64),
                ..Default::default()
            },
        );
        let width = pixmap.width();
        let height = pixmap.height();
        let mut rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for px in pixmap.pixels() {
            let c = px.demultiply();
            rgba.push(c.red());
            rgba.push(c.green());
            rgba.push(c.blue());
            rgba.push(c.alpha());
        }
        Some((width, height, rgba))
    }

    /// Schema-field geometry for the compiled document — bottom-left PDF-point
    /// rects keyed on the schema-field path. Two sources, ordered: form-field
    /// widgets (one fixed-size box each) first, then auto-tagged content fields
    /// (markdown bodies, geometry read from the laid-out frames). The
    /// widgets-first order sets the precedence for the one-region-per-field
    /// dedup that [`LiveSession::regions`](quillmark_core::LiveSession::regions)
    /// applies: an explicit `field:`-bound widget wins over a content auto-tag of
    /// the same path, and a page-spanning body keeps its first page-fragment
    /// (content arrives sorted by `(page, field)`, so the lowest page leads).
    /// Geometry math over the frames, no rasterization. Widget regions are empty
    /// if the placements fail to resolve (a render would surface the same error).
    fn regions(&self) -> Vec<quillmark_core::RenderedRegion> {
        let mut regions = overlay::build_field_specs(&self.document, &self.field_placements)
            .map(|specs| quillmark_pdf::regions_of(&specs))
            .unwrap_or_default();
        regions.extend(overlay::scan_content_regions(&self.document));
        regions
    }
}

impl Backend for TypstBackend {
    fn id(&self) -> &'static str {
        "typst"
    }

    fn supported_formats(&self) -> &'static [OutputFormat] {
        SUPPORTED_FORMATS
    }

    fn open(
        &self,
        source: &Quill,
        json_data: &serde_json::Value,
    ) -> Result<LiveSession, RenderError> {
        let plate_content = read_plate(source)?;

        let transform_schema = build_transform_schema(source.config());
        let json_str = transformed_json_str(&transform_schema, json_data)?;
        let world =
            world::QuillWorld::new_with_data(source, &plate_content, &json_str).map_err(|e| {
                RenderError::EngineCreation {
                    diags: vec![Diagnostic::new(
                        Severity::Error,
                        format!("Failed to create Typst compilation environment: {}", e),
                    )
                    .with_code("typst::world_creation".to_string())
                    .with_source(e.as_ref())],
                }
            })?;
        let document = compile::compile_document(&world)?;
        let page_count = document.pages().len();
        let field_placements = overlay::extract(&document)?;
        let hashes = page_hashes(&document);
        let session = TypstSession {
            world,
            document,
            page_count,
            field_placements,
            transform_schema,
            page_hashes: hashes,
        };
        Ok(LiveSession::new(Box::new(session)))
    }
}

impl Default for TypstBackend {
    /// Creates a new [`TypstBackend`] instance.
    fn default() -> Self {
        Self
    }
}

/// Read the Typst plate (template) this quill renders through.
///
/// The plate is a Typst-only notion, not a universal backend input: its
/// filename is declared under the `typst:` backend-config section as
/// `plate_file`, and the source lives in the quill's file bundle. The backend
/// resolves it here, the same way `pdfform` resolves its own `form.pdf` /
/// `form.json`. A quill that declares no `plate_file` renders through an empty
/// plate (`""`).
fn read_plate(source: &Quill) -> Result<String, RenderError> {
    let plate_file = source
        .config()
        .backend_config
        .get("plate_file")
        .and_then(|v| v.as_str());

    let Some(plate_file) = plate_file else {
        return Ok(String::new());
    };

    let bytes = source.files().get_file(plate_file).ok_or_else(|| {
        engine_err(
            "typst::plate_missing",
            format!("plate file '{plate_file}' not found in the quill's file tree"),
        )
    })?;

    String::from_utf8(bytes.to_vec()).map_err(|e| {
        engine_err(
            "typst::invalid_utf8",
            format!("plate file '{plate_file}' is not valid UTF-8: {e}"),
        )
    })
}

/// A single-diagnostic [`RenderError::EngineCreation`] carrying `code`.
fn engine_err(code: &str, message: impl Into<String>) -> RenderError {
    RenderError::EngineCreation {
        diags: vec![Diagnostic::new(Severity::Error, message.into()).with_code(code.to_string())],
    }
}

/// Check if a field schema indicates markdown content: `contentMediaType =
/// "text/markdown"`.
fn is_markdown_field(field_schema: &serde_json::Value) -> bool {
    field_schema
        .get("contentMediaType")
        .and_then(|v| v.as_str())
        .map(|s| s == "text/markdown")
        .unwrap_or(false)
}

/// Check if a field schema indicates an array of markdown elements.
///
/// True when the field is `{type: array, items: {contentMediaType:
/// text/markdown}}` — i.e. a `markdown[]` field. Each element is markdown
/// text that must be converted to backend markup individually.
fn is_markdown_array_field(field_schema: &serde_json::Value) -> bool {
    field_schema
        .get("type")
        .and_then(|v| v.as_str())
        .map(|s| s == "array")
        .unwrap_or(false)
        && field_schema
            .get("items")
            .map(is_markdown_field)
            .unwrap_or(false)
}

/// Check if a field schema indicates a datetime field (`format = "date-time"`).
fn is_date_field(field_schema: &serde_json::Value) -> bool {
    field_schema
        .get("format")
        .and_then(|v| v.as_str())
        .map(|s| s == "date-time")
        .unwrap_or(false)
}

/// Names of the markdown / `markdown[]` fields in a schema `properties` map —
/// the fields whose values carry backend markup for the helper to `eval`.
fn content_field_names(properties: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    properties
        .iter()
        .filter(|(_, fs)| is_markdown_field(fs) || is_markdown_array_field(fs))
        .map(|(name, _)| name.clone())
        .collect()
}

/// Names of the date fields in a schema `properties` map.
fn date_field_names(properties: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    properties
        .iter()
        .filter(|(_, fs)| is_date_field(fs))
        .map(|(name, _)| name.clone())
        .collect()
}

/// Convert a content field's value to backend markup: a markdown string is
/// converted in place; a `markdown[]` array converts each string element.
/// Returns `None` when the value is neither (e.g. a string that fails to
/// convert), leaving it untouched.
fn convert_content_value(value: &QuillValue) -> Option<QuillValue> {
    match value.as_json() {
        serde_json::Value::String(s) => mark_to_typst(s)
            .ok()
            .map(|markup| QuillValue::from_json(serde_json::json!(markup))),
        serde_json::Value::Array(arr) => {
            let converted = arr
                .iter()
                .map(|elem| match elem.as_str() {
                    Some(s) => match mark_to_typst(s) {
                        Ok(markup) => serde_json::json!(markup),
                        Err(_) => elem.clone(),
                    },
                    None => elem.clone(),
                })
                .collect();
            Some(QuillValue::from_json(serde_json::Value::Array(converted)))
        }
        _ => None,
    }
}

/// Transform markdown fields to Typst markup based on schema.
///
/// Identifies fields with `contentMediaType = "text/markdown"` and converts
/// their content using `mark_to_typst()`. This includes recursive handling
/// of the `$cards` array.
///
/// Also injects a `__meta__` key into the result containing the names of
/// converted fields, which the quillmark-helper package uses to auto-evaluate
/// markup strings into Typst content objects.
fn transform_markdown_fields(
    fields: &HashMap<String, QuillValue>,
    schema: &QuillValue,
) -> HashMap<String, QuillValue> {
    let mut result = fields.clone();
    let schema_json = schema.as_json();

    // Get the properties object from the schema
    let properties_obj = match schema_json.get("properties").and_then(|v| v.as_object()) {
        Some(obj) => obj,
        None => return result,
    };

    // Convert every markdown / markdown[] field the schema declares; the
    // helper package maps `eval(.., mode: "markup")` over these names.
    let content_fields = content_field_names(properties_obj);
    for field_name in &content_fields {
        if let Some(value) = fields.get(field_name) {
            if let Some(converted) = convert_content_value(value) {
                result.insert(field_name.clone(), converted);
            }
        }
    }

    let date_fields = date_field_names(properties_obj);

    // Handle `$cards` array recursively
    if let Some(cards_value) = result.get("$cards") {
        if let Some(cards_array) = cards_value.as_array() {
            let transformed_cards = transform_cards_array(schema, cards_array);
            result.insert(
                "$cards".to_string(),
                QuillValue::from_json(serde_json::Value::Array(transformed_cards)),
            );
        }
    }

    // Collect per-card-kind content field names from schema $defs
    let mut card_content_fields = serde_json::Map::new();
    let mut card_date_fields = serde_json::Map::new();
    if let Some(defs) = schema_json.get("$defs").and_then(|v| v.as_object()) {
        for (def_name, def_schema) in defs {
            if let Some(card_kind) = def_name.strip_suffix("_card") {
                let card_props = def_schema.get("properties").and_then(|v| v.as_object());
                let card_fields = card_props.map(content_field_names).unwrap_or_default();
                if !card_fields.is_empty() {
                    card_content_fields.insert(
                        card_kind.to_string(),
                        serde_json::Value::Array(
                            card_fields
                                .into_iter()
                                .map(serde_json::Value::String)
                                .collect(),
                        ),
                    );
                }

                let date_fields = card_props.map(date_field_names).unwrap_or_default();
                if !date_fields.is_empty() {
                    card_date_fields.insert(
                        card_kind.to_string(),
                        serde_json::Value::Array(
                            date_fields
                                .into_iter()
                                .map(serde_json::Value::String)
                                .collect(),
                        ),
                    );
                }
            }
        }
    }

    // Inject __meta__ so the helper package can auto-eval content fields
    result.insert(
        "__meta__".to_string(),
        QuillValue::from_json(serde_json::json!({
            "content_fields": content_fields,
            "card_content_fields": card_content_fields,
            "date_fields": date_fields,
            "card_date_fields": card_date_fields,
        })),
    );

    result
}

/// Transform markdown fields in `$cards` array items.
fn transform_cards_array(
    document_schema: &QuillValue,
    cards_array: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    let mut transformed_cards = Vec::new();

    // Get definitions for card schemas
    let defs = document_schema
        .as_json()
        .get("$defs")
        .and_then(|v| v.as_object());

    for card in cards_array {
        if let Some(card_obj) = card.as_object() {
            if let Some(card_kind) = card_obj.get("$kind").and_then(|v| v.as_str()) {
                // Construct the definition name: {kind}_card
                let def_name = format!("{}_card", card_kind);

                // Look up the schema for this card kind
                if let Some(card_schema_json) = defs.and_then(|d| d.get(&def_name)) {
                    // Convert the card object to HashMap<String, QuillValue>
                    let mut card_fields: HashMap<String, QuillValue> = HashMap::new();
                    for (k, v) in card_obj {
                        card_fields.insert(k.clone(), QuillValue::from_json(v.clone()));
                    }

                    // Recursively transform this card's fields. `transform_markdown_fields`
                    // appends a `__meta__` entry for the top-level eval pass; the template
                    // drives card processing from the top-level `meta.card_*` maps and
                    // iterates each card directly, so strip the per-card `__meta__` rather
                    // than leak the sentinel into every card object plate authors see.
                    let mut transformed_card_fields = transform_markdown_fields(
                        &card_fields,
                        &QuillValue::from_json(card_schema_json.clone()),
                    );
                    transformed_card_fields.remove("__meta__");

                    // Convert back to JSON Value
                    let mut transformed_card_obj = serde_json::Map::new();
                    for (k, v) in transformed_card_fields {
                        transformed_card_obj.insert(k, v.into_json());
                    }

                    transformed_cards.push(serde_json::Value::Object(transformed_card_obj));
                    continue;
                }
            }
        }

        // If not an object, no `$kind`, or no matching schema, keep as-is
        transformed_cards.push(card.clone());
    }

    transformed_cards
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_backend_info() {
        let backend = TypstBackend;
        assert_eq!(backend.id(), "typst");
        assert!(backend.supported_formats().contains(&OutputFormat::Pdf));
        assert!(backend.supported_formats().contains(&OutputFormat::Svg));
    }

    #[test]
    fn test_is_markdown_field() {
        let markdown_schema = json!({
            "type": "string",
            "contentMediaType": "text/markdown"
        });
        assert!(is_markdown_field(&markdown_schema));

        let string_schema = json!({
            "type": "string"
        });
        assert!(!is_markdown_field(&string_schema));

        let other_media_type = json!({
            "type": "string",
            "contentMediaType": "text/plain"
        });
        assert!(!is_markdown_field(&other_media_type));
    }

    #[test]
    fn test_is_markdown_array_field() {
        let md_array = json!({
            "type": "array",
            "items": { "type": "string", "contentMediaType": "text/markdown" }
        });
        assert!(is_markdown_array_field(&md_array));

        let string_array = json!({
            "type": "array",
            "items": { "type": "string" }
        });
        assert!(!is_markdown_array_field(&string_array));

        // A plain markdown scalar is not a markdown array.
        let md_scalar = json!({ "type": "string", "contentMediaType": "text/markdown" });
        assert!(!is_markdown_array_field(&md_scalar));
    }

    #[test]
    fn test_transform_markdown_array_field() {
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {
                "sections": {
                    "type": "array",
                    "items": { "type": "string", "contentMediaType": "text/markdown" }
                }
            }
        }));

        let mut fields = HashMap::new();
        fields.insert(
            "sections".to_string(),
            QuillValue::from_json(json!(["This is **bold** text.", "Plain line."])),
        );

        let result = transform_markdown_fields(&fields, &schema);

        // Each element is converted to Typst markup.
        let sections = result.get("sections").unwrap().as_array().unwrap();
        assert!(sections[0].as_str().unwrap().contains("#strong[bold]"));
        assert!(sections[1].as_str().unwrap().contains("Plain line."));

        // The field is registered for auto-eval in __meta__.
        let meta = result.get("__meta__").unwrap().as_json();
        let content_fields = meta["content_fields"].as_array().unwrap();
        assert!(content_fields.iter().any(|v| v == "sections"));
    }

    #[test]
    fn test_is_date_field() {
        let datetime_schema = json!({
            "type": "string",
            "format": "date-time"
        });
        assert!(is_date_field(&datetime_schema));

        let no_format_schema = json!({ "type": "string" });
        assert!(!is_date_field(&no_format_schema));
    }

    #[test]
    fn test_transform_markdown_fields_basic() {
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "$body": { "type": "string", "contentMediaType": "text/markdown" }
            }
        }));

        let mut fields = HashMap::new();
        fields.insert(
            "title".to_string(),
            QuillValue::from_json(json!("My Title")),
        );
        fields.insert(
            "$body".to_string(),
            QuillValue::from_json(json!("This is **bold** text.")),
        );

        let result = transform_markdown_fields(&fields, &schema);

        // title should be unchanged
        assert_eq!(result.get("title").unwrap().as_str(), Some("My Title"));

        // `$body` should be converted to Typst markup
        let body = result.get("$body").unwrap().as_str().unwrap();
        assert!(body.contains("#strong[bold]"));
    }

    #[test]
    fn test_transform_markdown_fields_no_markdown() {
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "count": { "type": "number" }
            }
        }));

        let mut fields = HashMap::new();
        fields.insert(
            "title".to_string(),
            QuillValue::from_json(json!("My Title")),
        );
        fields.insert("count".to_string(), QuillValue::from_json(json!(42)));

        let result = transform_markdown_fields(&fields, &schema);

        // All fields should be unchanged
        assert_eq!(result.get("title").unwrap().as_str(), Some("My Title"));
        assert_eq!(result.get("count").unwrap().as_i64(), Some(42));
    }

    #[test]
    fn test_transform_markdown_fields_wrapper() {
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {
                "$body": { "type": "string", "contentMediaType": "text/markdown" }
            }
        }));

        let mut fields = HashMap::new();
        fields.insert(
            "$body".to_string(),
            QuillValue::from_json(json!("_italic_ text")),
        );

        let result = transform_markdown_fields(&fields, &schema);

        let body = result.get("$body").unwrap().as_str().unwrap();
        assert!(body.contains("#emph[italic]"));
    }

    #[test]
    fn test_transform_markdown_fields_collects_top_level_date_metadata() {
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "issued": { "type": "string", "format": "date-time" },
                "created_at": { "type": "string", "format": "date-time" }
            }
        }));

        let mut fields = HashMap::new();
        fields.insert(
            "title".to_string(),
            QuillValue::from_json(json!("My Title")),
        );

        let result = transform_markdown_fields(&fields, &schema);
        let meta = result.get("__meta__").expect("missing __meta__").as_json();

        let date_fields = meta["date_fields"].as_array().unwrap();
        assert_eq!(date_fields.len(), 2);
        assert!(date_fields.iter().any(|v| v == "issued"));
        assert!(date_fields.iter().any(|v| v == "created_at"));
    }

    #[test]
    fn test_transform_markdown_fields_collects_card_date_metadata() {
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {},
            "$defs": {
                "indorsement_card": {
                    "type": "object",
                    "properties": {
                        "signed_on": { "type": "string", "format": "date-time" },
                        "$body": { "type": "string", "contentMediaType": "text/markdown" }
                    }
                }
            }
        }));

        let fields = HashMap::new();
        let result = transform_markdown_fields(&fields, &schema);
        let meta = result.get("__meta__").expect("missing __meta__").as_json();

        assert_eq!(
            meta["card_date_fields"]["indorsement"],
            json!(["signed_on"])
        );
    }

    #[test]
    fn test_transform_cards_array_strips_per_card_meta() {
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {},
            "$defs": {
                "indorsement_card": {
                    "type": "object",
                    "properties": {
                        "$body": { "type": "string", "contentMediaType": "text/markdown" }
                    }
                }
            }
        }));

        let cards = vec![json!({ "$kind": "indorsement", "$body": "**hi**" })];
        let transformed = transform_cards_array(&schema, &cards);

        // The per-card `__meta__` sentinel must not leak into card objects;
        // card eval is driven by the top-level `meta.card_*` maps.
        let card = transformed[0].as_object().unwrap();
        assert!(
            !card.contains_key("__meta__"),
            "card object leaked a __meta__ key: {:?}",
            card.keys().collect::<Vec<_>>()
        );
    }
}
