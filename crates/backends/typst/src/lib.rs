//! Typst backend for Quillmark: converts CommonMark markdown + card-YAML data
//! to PDF, SVG, and PNG via the Typst typesetting system.
//!
//! [`TypstBackend`] implements the [`Backend`] trait from `quillmark-core`.
//! Callers typically reach it through the `quillmark` crate's `Quill` API.
//!
//! Richtext fields cross the seam as canonical corpus JSON and are lowered to
//! Typst markup by [`emit`] at codegen time — the only markup-producing path,
//! never a markdown re-parse. The plate accesses fields via the
//! `@local/quillmark-helper` virtual package. Unsigned AcroForm widgets (text,
//! checkbox, choice, signature) are embedded via the `form-field` helper in
//! `lib.typ`; only PDF output carries the interactive widget — SVG and PNG
//! render an invisible placeholder.
//!
//! The `compile` and `error_mapping` modules are internal and not part of the
//! public API. The public lowering surface is [`emit`].

mod compile;
/// The corpus → Typst-markup lowering + its per-segment source map (the codegen
/// tier of the richtext seam, Option A). The one place that both lowers a
/// [`RichText`](quillmark_richtext::RichText) and knows the resulting byte
/// layout, so the only place a source map can be produced. This is the sole
/// markup-producing path in the render engine — no code parses markdown.
pub mod emit;
mod error_mapping;

mod helper;
mod overlay;
mod world;

use std::borrow::Cow;

use quillmark_core::{
    quill::{build_transform_schema, RICHTEXT_MEDIA_TYPE},
    session::SessionHandle,
    Backend, ChangeSet, CorpusHit, Diagnostic, LiveSession, OutputFormat, Quill, RenderError,
    RenderOptions, RenderResult, RenderedRegion, Severity,
};

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
    /// The transform schema's address/date/content classification tables, built
    /// once at `open` from `build_transform_schema` and reused on every `apply`
    /// (the schema never changes for the session's lifetime). The schema itself
    /// is not retained — codegen and date validation read only these tables.
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

/// Prepare raw document data for the helper codegen. The seam already carries
/// the render shape — richtext fields are canonical corpus JSON, not markdown
/// to re-parse (lowered to markup at codegen via [`emit::emit_richtext`]), dates
/// are strings (lowered to `datetime(..)` at codegen) — so there is no
/// per-field transform here. `meta` is the session's cached [`SchemaMeta`].
///
/// The one check kept is a defensive date validation: the coercion layer already
/// rejects malformed dates before render, but a direct `apply` can hand the
/// backend uncoerced data, and a bad date surfaces a real diagnostic here rather
/// than a silent `none` or a cryptic Typst error deep in the compile.
///
/// Borrows `json_data` unchanged for the object case (the render input on the
/// hot `apply`/`open` path); only a non-object input allocates, normalized to an
/// empty object.
fn transformed_data<'a>(
    meta: &SchemaMeta,
    json_data: &'a serde_json::Value,
) -> Result<Cow<'a, serde_json::Value>, RenderError> {
    match json_data.as_object() {
        Some(obj) => {
            validate_date_fields(meta, obj)?;
            Ok(Cow::Borrowed(json_data))
        }
        None => {
            // An empty object has no date fields to validate — return it directly.
            Ok(Cow::Owned(serde_json::Value::Object(serde_json::Map::new())))
        }
    }
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
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(str::to_string))
                        .collect()
                })
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
        let data = transformed_data(&self.schema_meta, json_data)?;
        let mut windows = self
            .world
            .inject_helper_package(data.as_ref(), &self.schema_meta)
            .map_err(|e| engine_err("typst::emit", e.to_string()))?;
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

    /// A point → corpus position in a content field — the fine-grained twin of
    /// [`field_at`](Self::field_at). Resolves the content glyph under `(x, y)`
    /// to a cluster-exact USV offset in its field's `RichText`, degrading to
    /// the containing segment's start on origin-less ink. `None` off all
    /// content ink or on scalar/widget ink (no corpus address). Widgets draw no
    /// spanned content ink, so — unlike `field_at` — they are not consulted.
    fn position_at(&self, page: usize, x: f32, y: f32) -> Option<CorpusHit> {
        overlay::position_at(
            &self.document,
            &self.world,
            &self.helper_source,
            &self.windows,
            page,
            x,
            y,
        )
    }

    /// A corpus position → caret rect — the reverse of
    /// [`position_at`](Self::position_at). Maps `pos` in `field`'s `RichText`
    /// to the box of the glyph the caret sits at, page-indexed.
    fn locate(&self, field: &str, pos: usize) -> Option<RenderedRegion> {
        overlay::locate(
            &self.document,
            &self.world,
            &self.helper_source,
            &self.windows,
            field,
            pos,
        )
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
        .map_err(|e| {
            engine_err(
                "typst::helper_source",
                format!("helper lib.typ unreadable: {e}"),
            )
        })
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
        let data = transformed_data(&schema_meta, json_data)?;
        let (world, mut windows) =
            world::QuillWorld::new_with_data(source, &plate_content, data.as_ref(), &schema_meta)
                .map_err(
                |e| {
                    RenderError::from_diag(
                        Diagnostic::new(
                            Severity::Error,
                            format!("Failed to create Typst compilation environment: {}", e),
                        )
                        .with_code("typst::world_creation".to_string())
                        .with_source(e.as_ref()),
                    )
                },
            )?;
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
                            segments: Vec::new(),
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

/// Check if a field schema indicates richtext content: `contentMediaType =
/// application/quillmark-richtext+json` (the value crossing the seam is a
/// canonical corpus object, lowered to markup at codegen).
fn is_richtext_field(field_schema: &serde_json::Value) -> bool {
    field_schema
        .get("contentMediaType")
        .and_then(|v| v.as_str())
        .map(|s| s == RICHTEXT_MEDIA_TYPE)
        .unwrap_or(false)
}

/// Check if a field schema indicates an array of richtext elements.
///
/// True when the field is `{type: array, items: {contentMediaType:
/// application/quillmark-richtext+json}}` — i.e. an `array<richtext>` field. Each
/// element is a corpus object lowered to a content block individually.
fn is_richtext_array_field(field_schema: &serde_json::Value) -> bool {
    field_schema
        .get("type")
        .and_then(|v| v.as_str())
        .map(|s| s == "array")
        .unwrap_or(false)
        && field_schema
            .get("items")
            .map(is_richtext_field)
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

/// Names of the richtext / `richtext[]` fields in a schema `properties` map —
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
        is_richtext_field(fs) || is_richtext_array_field(fs)
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
/// the content codegen only *produces* eval sites for `richtext[]` elements,
/// but a widget binding of a plain array element is a real, routable address.
fn array_field_names(properties: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    field_names_where(properties, |fs| {
        fs.get("type").and_then(|v| v.as_str()) == Some("array")
    })
}

/// Schema-derived tables backing `form-field` path validation and the helper's
/// content/date classification — a pure function of a transform schema.
/// `TypstSession` builds this once from `transform_schema` at `open` and reuses
/// it on every `apply`, since the schema never changes for the session's
/// lifetime; the recursive per-card codegen pass builds each card's field lists
/// from these tables.
///
/// A schema with no top-level `properties` yields the default (all tables
/// empty) — `build_transform_schema` always emits `properties`, so that case
/// only arises for hand-built schemas in tests.
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use quillmark_core::QuillValue;
    use serde_json::json;
    use std::collections::HashMap;

    /// A field's canonical corpus JSON, the shape the seam carries for a richtext
    /// field — `import(markdown)` then the canonical serializer.
    fn corpus(markdown: &str) -> serde_json::Value {
        let rt = quillmark_richtext::import::from_markdown(markdown).expect("import");
        quillmark_richtext::serial::to_canonical_value(&rt)
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
                "quill:\n  name: shift\n  version: 0.1.0\n  backend: typst\n  description: span shift probe\ntypst:\n  plate_file: plate.typ\nmain:\n  fields:\n    body:\n      type: richtext\n      description: body\n",
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

        // The body crosses the seam as canonical corpus JSON, not markdown.
        let json =
            serde_json::json!({ "body": corpus("A **markdown** body with real ink to lay out.") });
        let hashes_of = |quill: &Quill| {
            let plate_content = read_plate(quill).expect("plate");
            let transform_schema = build_transform_schema(quill.config());
            let schema_meta = SchemaMeta::from_schema_json(transform_schema.as_json());
            let data = transformed_data(&schema_meta, &json).expect("data");
            let (world, _windows) =
                world::QuillWorld::new_with_data(quill, &plate_content, data.as_ref(), &schema_meta)
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

    /// A paragraph whose text opens with a line-anchored Typst token (`= `, `- `,
    /// `+ `, `N. `, `/ `) must render as literal text, not a heading/list/term.
    /// The emitter prefixes a `\` at column 0; this compiles the corpus and asks
    /// Typst's introspector how many of each block it actually produced — the
    /// end-to-end teeth behind `emit::opens_line_anchor`, run against the real
    /// Typst grammar so a future Typst version that changes line-anchoring fails
    /// loud here.
    #[test]
    fn line_anchored_paragraph_text_stays_literal() {
        use quillmark_core::FileTreeNode;
        use typst::foundations::{NativeElement, Selector};
        use typst::introspection::Introspector;
        use typst::model::{EnumElem, HeadingElem, ListElem, TermsElem};

        const PLATE: &str = r#"#import "@local/quillmark-helper:0.1.0": data
#set page(width: 300pt, height: 400pt, margin: 20pt)
#set text(size: 11pt)
#data.body
"#;
        let quill = || {
            let yaml = "quill:\n  name: anchor\n  version: 0.1.0\n  backend: typst\n  description: line-anchor guard\ntypst:\n  plate_file: plate.typ\nmain:\n  fields:\n    body:\n      type: richtext\n      description: body\n";
            let mut files = HashMap::new();
            files.insert(
                "Quill.yaml".to_string(),
                FileTreeNode::File { contents: yaml.as_bytes().to_vec() },
            );
            files.insert(
                "plate.typ".to_string(),
                FileTreeNode::File { contents: PLATE.as_bytes().to_vec() },
            );
            Quill::from_tree(FileTreeNode::Directory { files }).expect("quill")
        };
        // Build the corpus as `Para` lines directly — an editor can place a
        // paragraph whose literal text opens with any of these tokens (markdown
        // import would instead parse `- `/`+ `/`N. ` as real lists, which is not
        // the bug). Each line is its own paragraph, so each starts at column 0.
        use quillmark_richtext::model::{Line, LineKind, RichText};
        let para = |_: usize| Line { kind: LineKind::Para, containers: vec![], continues: false };
        let mut rt = RichText {
            text: "= Heading\n- bullet\n+ numbered\n1. dotted\n/ term: desc".to_string(),
            lines: (0..5).map(para).collect(),
            marks: vec![],
            islands: vec![],
        };
        rt.normalize();
        assert_eq!(rt.validate(), Ok(()), "corpus invariants");
        let q = quill();
        let json =
            serde_json::json!({ "body": quillmark_richtext::serial::to_canonical_value(&rt) });
        let plate_content = read_plate(&q).expect("plate");
        let transform_schema = build_transform_schema(q.config());
        let schema_meta = SchemaMeta::from_schema_json(transform_schema.as_json());
        let data = transformed_data(&schema_meta, &json).expect("data");
        let (world, _w) =
            world::QuillWorld::new_with_data(&q, &plate_content, data.as_ref(), &schema_meta)
                .expect("world");
        let (document, _warn) = compile::compile_document(&world).expect("compile");

        let intro = document.introspector();
        let count = |e| intro.query(&Selector::Elem(e, None)).len();
        assert_eq!(count(<HeadingElem as NativeElement>::ELEM), 0, "no heading");
        assert_eq!(count(<ListElem as NativeElement>::ELEM), 0, "no bullet list");
        assert_eq!(count(<EnumElem as NativeElement>::ELEM), 0, "no enum");
        assert_eq!(count(<TermsElem as NativeElement>::ELEM), 0, "no term list");
    }

    #[test]
    fn test_is_richtext_field() {
        let richtext_schema = json!({
            "type": "object",
            "contentMediaType": RICHTEXT_MEDIA_TYPE
        });
        assert!(is_richtext_field(&richtext_schema));

        let string_schema = json!({ "type": "string" });
        assert!(!is_richtext_field(&string_schema));

        // `text/markdown` is not a richtext content type.
        let old_media_type = json!({ "type": "string", "contentMediaType": "text/markdown" });
        assert!(!is_richtext_field(&old_media_type));
    }

    #[test]
    fn test_is_richtext_array_field() {
        let rt_array = json!({
            "type": "array",
            "items": { "type": "object", "contentMediaType": RICHTEXT_MEDIA_TYPE }
        });
        assert!(is_richtext_array_field(&rt_array));

        let string_array = json!({
            "type": "array",
            "items": { "type": "string" }
        });
        assert!(!is_richtext_array_field(&string_array));

        // A plain richtext scalar is not a richtext array.
        let rt_scalar = json!({ "type": "object", "contentMediaType": RICHTEXT_MEDIA_TYPE });
        assert!(!is_richtext_array_field(&rt_scalar));
    }

    #[test]
    fn schema_meta_classifies_richtext_content_fields() {
        // The content-field selector keys on the richtext media type — a scalar
        // richtext field and an `array<richtext>` both count.
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "intro": { "type": "object", "contentMediaType": RICHTEXT_MEDIA_TYPE },
                "sections": {
                    "type": "array",
                    "items": { "type": "object", "contentMediaType": RICHTEXT_MEDIA_TYPE }
                }
            }
        }));
        let meta = SchemaMeta::from_schema_json(schema.as_json());
        assert!(meta.content_fields.contains(&"intro".to_string()));
        assert!(meta.content_fields.contains(&"sections".to_string()));
        assert!(!meta.content_fields.contains(&"title".to_string()));
    }

    #[test]
    fn schema_meta_array_fields_distinguish_scalar_from_array() {
        // Any array is element-addressable (`field.N`) — `array<richtext>` and
        // plain string arrays alike, matching the pdfform resolver's grammar.
        // Only scalars are excluded: no element exists for the address to
        // resolve to.
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {
                "subject": { "type": "object", "contentMediaType": RICHTEXT_MEDIA_TYPE },
                "sections": {
                    "type": "array",
                    "items": { "type": "object", "contentMediaType": RICHTEXT_MEDIA_TYPE }
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
                        "$body": { "type": "object", "contentMediaType": RICHTEXT_MEDIA_TYPE },
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
    fn schema_meta_collects_date_fields() {
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "issued": { "type": "string", "format": "date-time" },
                "created_at": { "type": "string", "format": "date-time" }
            },
            "$defs": {
                "indorsement_card": {
                    "type": "object",
                    "properties": {
                        "signed_on": { "type": "string", "format": "date-time" }
                    }
                }
            }
        }));
        let meta = SchemaMeta::from_schema_json(schema.as_json());
        assert!(meta.date_fields.contains(&"issued".to_string()));
        assert!(meta.date_fields.contains(&"created_at".to_string()));
        assert_eq!(meta.card_date_fields["indorsement"], json!(["signed_on"]));
    }

}
