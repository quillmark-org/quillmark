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

use convert::mark_to_typst;
use quillmark_core::{
    quill::build_transform_schema, session::SessionHandle, Backend, ChangeSet, Diagnostic,
    LiveSession, OutputFormat, Quill, QuillValue, RenderError, RenderOptions, RenderResult,
    Severity,
};
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
/// recompiles. Typst-only operations (page geometry, raster rendering) are
/// served generically through the `SessionHandle` trait; callers never name
/// this type.
struct TypstSession {
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
    /// `transform_schema`'s address/auto-eval tables, built once at `open`
    /// and reused on every `apply` rather than rebuilt from
    /// `transform_schema`'s `$defs` each time.
    schema_meta: SchemaMeta,
    /// The span scan's full classification table for the live compile:
    /// generated content-block windows in the helper `lib.typ` (regenerated
    /// with the helper on every committed `apply`) followed by the plate's
    /// scalar reference-site windows. Swapped transactionally with the document.
    windows: Vec<overlay::FieldWindow>,
    /// Byte windows of the plate's direct `data.<field>` scalar reference
    /// sites. The plate is static for the session's lifetime, so these are
    /// computed once at `open` and re-appended into `windows` per apply.
    scalar_windows: Vec<overlay::FieldWindow>,
    /// The helper `lib.typ` source the live compile was built from. Span
    /// resolution for regions/`field_at` goes through this snapshot, not the
    /// world: a failed `apply` leaves the *next* injection's text in the
    /// world while every read keeps serving this compile.
    helper_source: typst::syntax::Source,
    /// Per-page content fingerprints of the live compile; diffed against the
    /// next compile's to produce `ChangeSet::dirty_pages`.
    page_hashes: Vec<u128>,
    /// Typst's non-fatal warnings for the live compile, swapped with it on
    /// each committed `apply`.
    compile_warnings: Vec<Diagnostic>,
}

/// Per-page fingerprints of *visible* content, diffed across compiles for
/// `ChangeSet::dirty_pages`. Walks each page frame hashing text, shapes,
/// images, links, and group geometry — skipping introspection `Tag` items and
/// group parent locations, which carry element hashes spanning content on
/// *other* pages (a page-spanning paragraph's tag sits on its first page and
/// covers the whole text, so hashing it dirties page 0 on an end-of-document
/// edit the page never shows).
///
/// PIXELS, NOT SPANS. Every hashed item drops its source-location `Span` — the
/// glyph `span` on a `Text` run, the trailing `Span` on `Shape`/`Image`. A
/// `Span` is a `FileId` plus a parse-numbering index; it locates source, it does
/// not paint. This fingerprint's whole contract is *visible content*, so folding
/// in a span is a category error: two compiles whose pages rasterize
/// pixel-for-pixel identically must hash identically, and only render-affecting
/// data (font, size, paint, glyph geometry, position) may enter the hash.
///
/// This is the #801 dirty-every-reapply bug, and it is real, not theoretical.
/// The span rework (#795) routes content-field glyphs' spans into the helper
/// `lib.typ`, which is regenerated per `apply` with a `data` literal whose keys
/// serialize in field order (`serde_json` is built with `preserve_order`). An
/// editor's mutate path can hand `apply` the SAME content in a different field
/// order than `open` saw; that shifts the helper's byte layout, hence every
/// content block's glyph spans below it — with no change to a single rendered
/// pixel. Folding those spans into the hash reported the content page dirty on
/// every such reapply (see `reapply_with_reordered_fields_same_content_is_clean`
/// in `tests/live_apply.rs`). Excluding spans makes the invariant structural: a
/// page cannot be reported dirty for a source-location shift that moved no ink.
fn page_hashes(document: &typst_layout::PagedDocument) -> Vec<u128> {
    use std::hash::{Hash, Hasher};
    use typst::layout::FrameItem;

    // A text run's rendered content: font, size, paint, and per-glyph geometry —
    // but not each glyph's `span` (see the fn doc). Everything that moves a pixel
    // is retained, so a genuine content change still re-hashes.
    fn hash_text<H: Hasher>(text: &typst::text::TextItem, state: &mut H) {
        text.font.hash(state);
        text.size.hash(state);
        text.fill.hash(state);
        text.stroke.hash(state);
        text.lang.hash(state);
        text.region.hash(state);
        text.text.hash(state);
        for g in &text.glyphs {
            g.id.hash(state);
            g.x_advance.hash(state);
            g.x_offset.hash(state);
            g.y_advance.hash(state);
            g.y_offset.hash(state);
            g.range.hash(state);
            // g.span deliberately omitted — source location, not pixels.
        }
    }

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
                FrameItem::Text(text) => {
                    pos.hash(state);
                    hash_text(text, state);
                }
                // Shape/Image carry a trailing `Span` their derived `Hash` would
                // fold in; destructure to hash the visible parts and drop it, same
                // reason as glyph spans above.
                FrameItem::Shape(shape, _span) => {
                    pos.hash(state);
                    shape.hash(state);
                }
                FrameItem::Image(image, size, _span) => {
                    pos.hash(state);
                    image.hash(state);
                    size.hash(state);
                }
                FrameItem::Link(dest, size) => {
                    pos.hash(state);
                    dest.hash(state);
                    size.hash(state);
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

/// Run the schema's markdown/date transform over raw document data, returning
/// the transformed data object the helper codegen turns into a Typst literal
/// (markdown fields already converted to markup; `__meta__` sentinel stripped,
/// since the codegen reads classification from `meta` directly). `meta` is the
/// session's cached [`SchemaMeta`].
fn transformed_data(
    schema: &QuillValue,
    meta: &SchemaMeta,
    json_data: &serde_json::Value,
) -> Result<serde_json::Value, RenderError> {
    let fields = json_data.as_object().map_or_else(HashMap::new, |obj| {
        obj.iter()
            .map(|(key, value)| (key.clone(), QuillValue::from_json(value.clone())))
            .collect::<HashMap<_, _>>()
    });

    let transformed_fields = transform_markdown_fields(&fields, schema, meta);
    let mut transformed_json: serde_json::Map<String, serde_json::Value> = transformed_fields
        .into_iter()
        .map(|(key, value)| (key, value.into_json()))
        .collect();
    // The codegen reads content/date classification and address tables from
    // `meta`; the `__meta__` sentinel the transform injects for its own use is
    // never emitted into `data`.
    transformed_json.remove("__meta__");

    // A date field the codegen emits as `datetime(..)` must parse. The
    // coercion layer already rejects malformed dates before render, so this is
    // a defensive backend invariant — but when data reaches the backend
    // uncoerced (a direct `apply`), a bad date produces a real diagnostic here
    // rather than a silent `none` or a cryptic Typst error deep in the compile.
    validate_date_fields(meta, &transformed_json)?;

    Ok(serde_json::Value::Object(transformed_json))
}

/// Reject any date field whose value is a non-empty string the shared
/// [`parse_date_ymd`](quillmark_core::quill::parse_date_ymd) parser — the same
/// one the coercion layer validates with — cannot parse. Walks the top-level
/// date fields and each card kind's date fields.
fn validate_date_fields(
    meta: &SchemaMeta,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), RenderError> {
    fn bad_date(field: &str, value: &str) -> RenderError {
        RenderError::from_diag(
            Diagnostic::new(
                Severity::Error,
                format!("invalid date in field {field:?}: {value:?} is not a recognized date"),
            )
            .with_code("backend::invalid_date".to_string()),
        )
    }
    fn check(
        names: &[String],
        dict: &serde_json::Map<String, serde_json::Value>,
        prefix: &str,
    ) -> Result<(), RenderError> {
        for key in names {
            if let Some(serde_json::Value::String(s)) = dict.get(key) {
                if !s.is_empty() && quillmark_core::quill::parse_date_ymd(s).is_none() {
                    return Err(bad_date(&format!("{prefix}{key}"), s));
                }
            }
        }
        Ok(())
    }

    check(&meta.date_fields, obj, "")?;
    if let Some(cards) = obj.get("$cards").and_then(|v| v.as_array()) {
        for card in cards {
            let Some(card_obj) = card.as_object() else {
                continue;
            };
            let Some(kind) = card_obj.get("$kind").and_then(|v| v.as_str()) else {
                continue;
            };
            let names: Vec<String> = meta
                .card_date_fields
                .get(kind)
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            check(&names, card_obj, &format!("$cards.{kind}."))?;
        }
    }
    Ok(())
}

impl SessionHandle for TypstSession {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let format = opts.output_format.unwrap_or(OutputFormat::Pdf);

        if !SUPPORTED_FORMATS.contains(&format) {
            return Err(RenderError::from_diag(
                Diagnostic::new(
                    Severity::Error,
                    format!("{:?} not supported by typst backend", format),
                )
                .with_code("backend::format_not_supported".to_string())
                .with_hint(format!("Supported formats: {:?}", SUPPORTED_FORMATS)),
            ));
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

    /// Incremental recompile against new document data. The persistent world
    /// keeps fonts/packages/assets parsed; the helper `lib.typ` is swapped via
    /// `Source::replace` (incremental reparse), and `comemo` reuses every
    /// memoized result the edit did not reach. Transactional: the live
    /// document, placements, hashes, and compile warnings swap together only
    /// after the compile *and* placement extraction succeed — on `Err` every
    /// read keeps serving the last-good compile and its warnings (the world
    /// may hold the failed source; the next `apply` overwrites it).
    fn apply(&mut self, json_data: &serde_json::Value) -> Result<ChangeSet, RenderError> {
        let data = transformed_data(&self.transform_schema, &self.schema_meta, json_data)?;
        let mut windows = self.world.inject_helper_package(&data, &self.schema_meta);
        windows.extend(self.scalar_windows.iter().cloned());

        let (document, compile_warnings) = compile::compile_document(&self.world)?;
        let helper_source = helper_source(&self.world)?;
        let field_placements = overlay::extract(&document)?;
        let new_hashes = page_hashes(&document);

        let dirty_pages = (0..new_hashes.len())
            .filter(|&i| self.page_hashes.get(i) != Some(&new_hashes[i]))
            .collect();

        self.document = document;
        self.field_placements = field_placements;
        self.windows = windows;
        self.helper_source = helper_source;
        self.page_count = new_hashes.len();
        self.page_hashes = new_hashes;
        self.compile_warnings = compile_warnings;

        Ok(ChangeSet {
            page_count: self.page_count,
            dirty_pages,
        })
    }

    /// Typst's non-fatal warnings for the current compile.
    fn warnings(&self) -> &[Diagnostic] {
        &self.compile_warnings
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
    /// rects keyed on the schema-field path. Two sources, deterministically
    /// ordered: form-field widgets (one fixed-size box each) first, then
    /// span-tracked content in (page, field, site) order — each content
    /// field's / scalar reference site's **first placement**, one region per
    /// page it touches, geometry read from the laid-out frames' glyph spans.
    /// Entries pass through
    /// [`LiveSession::regions`](quillmark_core::LiveSession::regions) as-is —
    /// `field` is still not unique (page fragments, several scalar reference
    /// sites, or tracked content plus a bound widget); consumers group by
    /// field. Geometry math over the frames, no rasterization. Widget regions
    /// are empty if the placements fail to resolve (a render would surface
    /// the same error).
    fn regions(&self) -> Vec<quillmark_core::RenderedRegion> {
        let mut regions = self.widget_regions();
        regions.extend(overlay::scan_content_regions(
            &self.document,
            &self.world,
            &self.helper_source,
            &self.windows,
        ));
        regions
    }

    /// The schema field under a point on `page` (PDF points, bottom-left
    /// origin) — the forward click→field direction. `field:`-bound widget
    /// boxes answer first: a widget is a deliberate click target that draws
    /// no spanned ink of its own, so content ink beneath it must not swallow
    /// the click. Among two spatially-overlapping widgets the later-painted one
    /// wins (`rev()` over the paint-ordered placements), matching the
    /// content-field rule in `span_scan::field_at`. Otherwise the span data
    /// answers, over every placement, not just the first: one concrete point
    /// identifies one frame item, whose span is unambiguous however many times
    /// its field is placed. Overrides the regions-hit-testing default.
    fn field_at(&self, page: usize, x: f32, y: f32) -> Option<String> {
        self.widget_regions()
            .into_iter()
            .rev()
            .find(|r| r.contains(page, x, y))
            .map(|r| r.field)
            .or_else(|| {
                overlay::field_at(
                    &self.document,
                    &self.world,
                    &self.helper_source,
                    &self.windows,
                    page,
                    x,
                    y,
                )
            })
    }
}

impl TypstSession {
    /// Regions for the `field:`-bound form-field widgets of the live compile.
    /// The single derivation `regions` and `field_at` both read, so widget
    /// geometry cannot drift between the two queries.
    fn widget_regions(&self) -> Vec<quillmark_core::RenderedRegion> {
        overlay::build_field_specs(&self.document, &self.field_placements)
            .map(|specs| quillmark_pdf::regions_of(&specs))
            .unwrap_or_default()
    }
}

/// The world's current helper `lib.typ` [`Source`](typst::syntax::Source),
/// snapshotted right after a successful compile — the text the served
/// document's spans resolve against.
fn helper_source(world: &world::QuillWorld) -> Result<typst::syntax::Source, RenderError> {
    use typst::World as _;
    world
        .source(world::QuillWorld::helper_fid("lib.typ"))
        .map_err(|e| engine_err("typst::helper_source", format!("helper lib.typ unreadable: {e}")))
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
        let schema_meta = SchemaMeta::from_schema_json(transform_schema.as_json());
        let data = transformed_data(&transform_schema, &schema_meta, json_data)?;
        let (world, mut windows) =
            world::QuillWorld::new_with_data(source, &plate_content, &data, &schema_meta)
                .map_err(|e| {
                    RenderError::from_diag(
                        Diagnostic::new(
                            Severity::Error,
                            format!("Failed to create Typst compilation environment: {}", e),
                        )
                        .with_code("typst::world_creation".to_string())
                        .with_source(e.as_ref()),
                    )
                })?;
        // The plate is static for the session, so its direct scalar
        // reference sites are windowed once here.
        let scalar_windows: Vec<overlay::FieldWindow> = {
            use typst::World as _;
            let main_id = world.main();
            world
                .source(main_id)
                .ok()
                .map(|src| {
                    overlay::scalar_windows(&src, &schema_meta.fields)
                        .into_iter()
                        .map(|(path, range)| overlay::FieldWindow {
                            path,
                            file: main_id,
                            range,
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        windows.extend(scalar_windows.iter().cloned());
        let (document, compile_warnings) = compile::compile_document(&world)?;
        let helper_src = helper_source(&world)?;
        let page_count = document.pages().len();
        let field_placements = overlay::extract(&document)?;
        let hashes = page_hashes(&document);
        let session = TypstSession {
            world,
            document,
            page_count,
            field_placements,
            transform_schema,
            schema_meta,
            windows,
            scalar_windows,
            helper_source: helper_src,
            page_hashes: hashes,
            compile_warnings,
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

/// A single-diagnostic [`RenderError`] carrying `code`.
fn engine_err(code: &str, message: impl Into<String>) -> RenderError {
    RenderError::from_diag(
        Diagnostic::new(Severity::Error, message.into()).with_code(code.to_string()),
    )
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
/// Names of the schema `properties` whose field schema satisfies `predicate`,
/// in map order. Shared spine of the field-class selectors below.
fn field_names_where(
    properties: &serde_json::Map<String, serde_json::Value>,
    predicate: impl Fn(&serde_json::Value) -> bool,
) -> Vec<String> {
    properties
        .iter()
        .filter(|(_, fs)| predicate(fs))
        .map(|(name, _)| name.clone())
        .collect()
}

fn content_field_names(properties: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    field_names_where(properties, |fs| {
        is_markdown_field(fs) || is_markdown_array_field(fs)
    })
}

/// Names of the date fields in a schema `properties` map.
fn date_field_names(properties: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    field_names_where(properties, is_date_field)
}

/// Names of the array-typed fields in a schema `properties` map — the fields
/// whose elements are addressable by index suffix (`field.0`, `field.1`, ...).
/// `form-field`'s path validator uses this to reject an index suffix on a
/// scalar field, where no element exists for the address to resolve to. Any
/// array qualifies, matching the pdfform resolver's shallow-path grammar:
/// the content codegen only *produces* eval sites for `markdown[]` elements,
/// but a widget binding of a plain array element is a real, routable address.
fn array_field_names(properties: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    field_names_where(properties, |fs| {
        fs.get("type").and_then(|v| v.as_str()) == Some("array")
    })
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

/// Schema-derived tables backing `form-field` path validation and
/// the helper's content/date auto-eval — a pure function of a transform
/// schema. `TypstSession` builds this once from `transform_schema` at `open`
/// and reuses it on every `apply`, since the schema never changes for the
/// session's lifetime; the recursive per-card pass in
/// [`transform_cards_array`] still builds one fresh per call (each card's own
/// schema is a different, and already cheap, computation).
///
/// A schema with no top-level `properties` yields the default (all tables
/// empty) — `build_transform_schema` always emits `properties`, so that case
/// only arises for hand-built schemas in tests. The template treats an empty
/// `__meta__` the same as an absent one.
#[derive(Default)]
pub(crate) struct SchemaMeta {
    pub(crate) content_fields: Vec<String>,
    pub(crate) date_fields: Vec<String>,
    pub(crate) array_fields: Vec<String>,
    pub(crate) card_content_fields: serde_json::Map<String, serde_json::Value>,
    pub(crate) card_date_fields: serde_json::Map<String, serde_json::Value>,
    pub(crate) card_field_names: serde_json::Map<String, serde_json::Value>,
    pub(crate) card_array_fields: serde_json::Map<String, serde_json::Value>,
    pub(crate) fields: Vec<String>,
}

impl SchemaMeta {
    pub(crate) fn from_schema_json(schema_json: &serde_json::Value) -> Self {
        let Some(properties_obj) = schema_json.get("properties").and_then(|v| v.as_object()) else {
            return Self::default();
        };

        let content_fields = content_field_names(properties_obj);
        let date_fields = date_field_names(properties_obj);
        let array_fields = array_field_names(properties_obj);
        let fields = properties_obj.keys().cloned().collect();

        // Collect per-card-kind content/date/array field names from schema
        // $defs, plus the full per-kind property-name lists that back
        // `form-field` path validation.
        let mut card_content_fields = serde_json::Map::new();
        let mut card_date_fields = serde_json::Map::new();
        let mut card_field_names = serde_json::Map::new();
        let mut card_array_fields = serde_json::Map::new();
        fn insert_names(
            table: &mut serde_json::Map<String, serde_json::Value>,
            kind: &str,
            names: Vec<String>,
        ) {
            if !names.is_empty() {
                table.insert(kind.to_string(), names.into());
            }
        }
        if let Some(defs) = schema_json.get("$defs").and_then(|v| v.as_object()) {
            for (def_name, def_schema) in defs {
                if let Some(card_kind) = def_name.strip_suffix("_card") {
                    let card_props = def_schema.get("properties").and_then(|v| v.as_object());
                    if let Some(props) = card_props {
                        card_field_names.insert(
                            card_kind.to_string(),
                            props.keys().cloned().collect::<Vec<String>>().into(),
                        );
                    }
                    insert_names(
                        &mut card_content_fields,
                        card_kind,
                        card_props.map(content_field_names).unwrap_or_default(),
                    );
                    insert_names(
                        &mut card_date_fields,
                        card_kind,
                        card_props.map(date_field_names).unwrap_or_default(),
                    );
                    insert_names(
                        &mut card_array_fields,
                        card_kind,
                        card_props.map(array_field_names).unwrap_or_default(),
                    );
                }
            }
        }

        Self {
            content_fields,
            date_fields,
            array_fields,
            card_content_fields,
            card_date_fields,
            card_field_names,
            card_array_fields,
            fields,
        }
    }

    /// The `__meta__` object injected into document data for the helper
    /// package: content/date auto-eval field lists, plus the schema address
    /// tables (`fields` / `card_fields` / `array_fields` / `card_array_fields`)
    /// that `form-field` validates explicit `field:` paths against.
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "content_fields": self.content_fields,
            "card_content_fields": self.card_content_fields,
            "date_fields": self.date_fields,
            "card_date_fields": self.card_date_fields,
            "fields": self.fields,
            "card_fields": self.card_field_names,
            "array_fields": self.array_fields,
            "card_array_fields": self.card_array_fields,
        })
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
/// markup strings into Typst content objects. `meta` is `schema`'s
/// [`SchemaMeta`] — the session passes its per-open cache; the recursive
/// per-card pass builds one fresh per card.
fn transform_markdown_fields(
    fields: &HashMap<String, QuillValue>,
    schema: &QuillValue,
    meta: &SchemaMeta,
) -> HashMap<String, QuillValue> {
    let mut result = fields.clone();

    // Convert every markdown / markdown[] field the schema declares; the
    // helper package maps `eval(.., mode: "markup")` over these names.
    for field_name in &meta.content_fields {
        if let Some(value) = fields.get(field_name) {
            if let Some(converted) = convert_content_value(value) {
                result.insert(field_name.clone(), converted);
            }
        }
    }

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

    result.insert(
        "__meta__".to_string(),
        QuillValue::from_json(meta.to_json()),
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
                    let card_schema = QuillValue::from_json(card_schema_json.clone());
                    let card_meta = SchemaMeta::from_schema_json(card_schema.as_json());
                    let mut transformed_card_fields =
                        transform_markdown_fields(&card_fields, &card_schema, &card_meta);
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

    /// [`transform_markdown_fields`] with `schema`'s meta built inline — the
    /// session's cache is irrelevant to these unit cases.
    fn transform(
        fields: &HashMap<String, QuillValue>,
        schema: &QuillValue,
    ) -> HashMap<String, QuillValue> {
        transform_markdown_fields(
            fields,
            schema,
            &SchemaMeta::from_schema_json(schema.as_json()),
        )
    }

    #[test]
    fn test_backend_info() {
        let backend = TypstBackend;
        assert_eq!(backend.id(), "typst");
        assert!(backend.supported_formats().contains(&OutputFormat::Pdf));
        assert!(backend.supported_formats().contains(&OutputFormat::Svg));
    }

    /// Direct teeth for the pixels-not-spans contract (#801): two compiles
    /// whose pages ink identically must fingerprint identically even when every
    /// glyph's `Span` differs. The quills below are identical except one schema
    /// declares an extra unused field, which lengthens the generated `_qm-meta`
    /// literal ahead of the content blocks in `lib.typ` — shifting every block's
    /// byte position, hence every content glyph's span, while the rendered
    /// pages stay pixel-identical. Folding spans into the hash fails this.
    #[test]
    fn page_hashes_ignore_span_shift_when_ink_is_identical() {
        use quillmark_core::FileTreeNode;

        const PLATE: &str = r#"#import "@local/quillmark-helper:0.1.0": data
#set page(width: 300pt, height: 200pt, margin: 20pt)
#set text(size: 11pt)
#data.body
"#;
        let quill_with = |extra_field: bool| {
            let mut yaml = String::from(
                "quill:\n  name: shift\n  version: 0.1.0\n  backend: typst\n  description: span shift probe\ntypst:\n  plate_file: plate.typ\nmain:\n  fields:\n    body:\n      type: markdown\n      description: body\n",
            );
            if extra_field {
                yaml.push_str(
                    "    zz_unused:\n      type: string\n      description: never placed\n",
                );
            }
            let mut files = HashMap::new();
            files.insert(
                "Quill.yaml".to_string(),
                FileTreeNode::File {
                    contents: yaml.into_bytes(),
                },
            );
            files.insert(
                "plate.typ".to_string(),
                FileTreeNode::File {
                    contents: PLATE.as_bytes().to_vec(),
                },
            );
            Quill::from_tree(FileTreeNode::Directory { files }).expect("quill")
        };

        let json = serde_json::json!({ "body": "A **markdown** body with real ink to lay out." });
        let hashes_of = |quill: &Quill| {
            let plate_content = read_plate(quill).expect("plate");
            let transform_schema = build_transform_schema(quill.config());
            let schema_meta = SchemaMeta::from_schema_json(transform_schema.as_json());
            let data = transformed_data(&transform_schema, &schema_meta, &json).expect("data");
            let (world, _windows) =
                world::QuillWorld::new_with_data(quill, &plate_content, &data, &schema_meta)
                    .expect("world");
            let (document, _warnings) = compile::compile_document(&world).expect("compile");
            page_hashes(&document)
        };

        assert_eq!(
            hashes_of(&quill_with(false)),
            hashes_of(&quill_with(true)),
            "identical ink must fingerprint identically across a whole-file span shift"
        );
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

        let result = transform(&fields, &schema);

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
    fn schema_meta_array_fields_distinguish_scalar_from_array() {
        // Any array is element-addressable (`field.N`) — markdown[] and plain
        // string arrays alike, matching the pdfform resolver's grammar. Only
        // scalars are excluded: no element exists for the address to resolve to.
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {
                "subject": { "type": "string", "contentMediaType": "text/markdown" },
                "sections": {
                    "type": "array",
                    "items": { "type": "string", "contentMediaType": "text/markdown" }
                },
                "signature_block": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            },
            "$defs": {
                "indorsement_card": {
                    "type": "object",
                    "properties": {
                        "$body": { "type": "string", "contentMediaType": "text/markdown" },
                        "refs": {
                            "type": "array",
                            "items": { "type": "string" }
                        }
                    }
                }
            }
        }));

        let meta = SchemaMeta::from_schema_json(schema.as_json());

        assert!(meta.array_fields.contains(&"sections".to_string()));
        assert!(meta.array_fields.contains(&"signature_block".to_string()));
        assert!(!meta.array_fields.contains(&"subject".to_string()));

        let card_arrays = meta.card_array_fields.get("indorsement").unwrap();
        assert_eq!(card_arrays, &serde_json::json!(["refs"]));
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

        let result = transform(&fields, &schema);

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

        let result = transform(&fields, &schema);

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

        let result = transform(&fields, &schema);

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

        let result = transform(&fields, &schema);
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
        let result = transform(&fields, &schema);
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
