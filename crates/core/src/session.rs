use std::any::Any;

use crate::{Diagnostic, RenderError, RenderOptions, RenderResult, RenderedRegion, Severity};

/// What a committed [`LiveSession::apply`] changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    /// Page count after the edit.
    pub page_count: usize,
    /// Pages whose rendered content differs from the previous compile,
    /// including pages the edit added. Pages the edit removed are implied by
    /// `page_count`. A preview repaints `dirty ∩ visible` and nothing else.
    pub dirty_pages: Vec<usize>,
}

/// Backend-specific session implementation.
///
/// Implementors must be `'static` (required by `Any`), `Send`, and `Sync`. The
/// `'static` bound prevents borrowing source data — own anything you need to
/// keep alive for the session's lifetime.
#[doc(hidden)]
pub trait SessionHandle: Any + Send + Sync {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError>;
    fn page_count(&self) -> usize;
    fn as_any(&self) -> &dyn Any;

    /// Recompile the session against new document data.
    ///
    /// Transactional: on `Err` the previous compile stays live — every read
    /// (`render`, `render_rgba`, `page_size_pt`, `regions`) keeps serving it.
    /// A backend with a persistent compilation environment recompiles
    /// incrementally; one whose compile is cheap recompiles fully. Either way
    /// the returned [`ChangeSet`] reports the pages the edit visibly changed.
    /// Default: apply is unsupported.
    fn apply(&mut self, _json_data: &serde_json::Value) -> Result<ChangeSet, RenderError> {
        Err(RenderError::from_diag(
            Diagnostic::new(
                Severity::Error,
                "this backend's session does not support apply".to_string(),
            )
            .with_code("backend::apply_unsupported".to_string()),
        ))
    }

    /// Page dimensions in points (1 pt = 1/72"), or `None` if `page` is out of
    /// range. The canvas-preview seam: a backend that can rasterize pages
    /// overrides this and [`render_rgba`](Self::render_rgba). Default `None`
    /// marks the session as having no canvas painter — the painter dispatches
    /// generically through these two methods rather than downcasting to a
    /// backend-specific session type.
    fn page_size_pt(&self, _page: usize) -> Option<(f32, f32)> {
        None
    }

    /// Render `page` to a non-premultiplied RGBA8 buffer at `scale`× the natural
    /// 72-ppi size, returning `(width_px, height_px, rgba)` (row-major, `w*h*4`
    /// bytes), or `None` if `page` is out of range or the backend has no canvas
    /// painter. The other half of the seam paired with
    /// [`page_size_pt`](Self::page_size_pt).
    ///
    /// # Per-backend contract
    ///
    /// A backend that returns `Some` here guarantees a **complete** raster of
    /// the page: every piece of page content is already visible in the returned
    /// pixels. The caller paints them straight to a canvas with **no
    /// compositing** of its own. Backends satisfy this differently:
    ///
    /// - **Typst** rasterizes its laid-out page natively.
    /// - **pdfform** pre-flattens the bound field values into the page content
    ///   streams at session-open, then rasterizes that flat PDF — so field
    ///   values appear in the raster without the caller drawing them.
    ///
    /// The [`regions`](Self::regions) accessor carries per-field geometry keyed
    /// on the quill schema field path, for *overlay* / cross-navigation UIs
    /// regardless; it is never required to make the raster complete.
    ///
    /// A backend with no painter overrides neither this nor
    /// [`page_size_pt`](Self::page_size_pt); the defaults mark the session as
    /// non-canvas, which is exactly what [`LiveSession::supports_canvas`]
    /// reports. Capability is derived from the `page_size_pt` half of this seam,
    /// not declared as a separate flag — a canvas backend is contractually
    /// expected to pair this method with `page_size_pt` over the same page set.
    fn render_rgba(&self, _page: usize, _scale: f32) -> Option<(u32, u32, Vec<u8>)> {
        None
    }

    /// Schema-field geometry for the compiled session — [`RenderedRegion`]s
    /// keyed on the quill schema address each field carries.
    ///
    /// A session-level query, not a render output: the geometry is a property of
    /// the current compile, computed from already-resolved field placements
    /// with no rasterization and no byte artifact. An interactive preview reads
    /// it to lay out overlays / field cross-navigation over a `paint`-ed canvas;
    /// a one-shot byte render carries it only on request
    /// ([`RenderOptions::regions`](crate::RenderOptions)). Default empty — a
    /// backend that places schema fields overrides this.
    ///
    /// Emit one region per **(placement, page fragment)**: a field may appear
    /// more than once — placed twice, breaking across pages, or arising from
    /// both tagged content and a bound widget — and every appearance surfaces
    /// independently ([`LiveSession::regions`] passes them through; consumers
    /// group by `field`). Order deterministically: widget regions first, then
    /// content regions in (page, field, placement) order.
    fn regions(&self) -> Vec<RenderedRegion> {
        Vec::new()
    }

    /// Non-fatal diagnostics of the **current compile**. A backend whose
    /// compile emits warnings (Typst: font fallback, overfull pages, …)
    /// overrides this to expose them; they swap with the compile on each
    /// committed [`apply`](Self::apply), so a failed apply keeps the last-good
    /// compile's warnings alongside its document. Default empty — a backend
    /// whose compile cannot warn leaves it.
    fn warnings(&self) -> &[Diagnostic] {
        &[]
    }
}

/// Opaque, backend-backed live render session: a persistent compiler that
/// serves reads (`render`, `paint` seams, `regions`) from its current compile
/// and takes edits via [`apply`](LiveSession::apply). Reads between edits see
/// a stable document — `apply` is transactional, swapping the compile only on
/// success — so immutability is an invariant between commits, not a type.
pub struct LiveSession {
    inner: Box<dyn SessionHandle>,
}

impl LiveSession {
    #[doc(hidden)]
    pub fn new(inner: Box<dyn SessionHandle>) -> Self {
        Self { inner }
    }

    /// Borrow the underlying [`SessionHandle`].
    ///
    /// The canonical canvas-preview path does **not** go through here: it
    /// dispatches generically through [`page_size_pt`](LiveSession::page_size_pt)
    /// / [`render_rgba`](LiveSession::render_rgba) on the session, with no
    /// downcast. This accessor exists only as a last-resort escape hatch for a
    /// backend that exposes a richer *typed* surface — reach it by downcasting
    /// via [`SessionHandle::as_any`]. No in-tree caller does. Intentionally
    /// `#[doc(hidden)]` — the shape of this accessor is not part of the stable
    /// public API.
    #[doc(hidden)]
    pub fn handle(&self) -> &dyn SessionHandle {
        &*self.inner
    }

    pub fn page_count(&self) -> usize {
        self.inner.page_count()
    }

    /// Whether this session can paint pages to a canvas — the authoritative,
    /// session-level capability. Derived directly from the canvas seam (a
    /// painter exposes [`page_size_pt`](SessionHandle::page_size_pt) for its
    /// pages), so there is no separate capability flag to keep in sync: a
    /// canvas backend pairs [`render_rgba`](Self::render_rgba) with
    /// `page_size_pt`, so this reflects what `paint` will do. A canvas-capable
    /// backend with zero pages reports `false` (nothing to paint).
    ///
    /// For a pre-session estimate (no open session yet), see
    /// [`formats_support_canvas`](crate::formats_support_canvas).
    pub fn supports_canvas(&self) -> bool {
        self.inner.page_count() > 0 && self.inner.page_size_pt(0).is_some()
    }

    /// Page dimensions in points, or `None` if `page` is out of range or the
    /// backend has no canvas painter. Generalized canvas-preview seam — see
    /// [`SessionHandle::page_size_pt`].
    pub fn page_size_pt(&self, page: usize) -> Option<(f32, f32)> {
        self.inner.page_size_pt(page)
    }

    /// Rasterize `page` to non-premultiplied RGBA8 at `scale`× 72 ppi, or `None`
    /// if `page` is out of range or the backend has no canvas painter. A `Some`
    /// result is a **complete** raster of the page — all content visible, no
    /// caller-side compositing — per the per-backend contract on
    /// [`SessionHandle::render_rgba`].
    pub fn render_rgba(&self, page: usize, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
        self.inner.render_rgba(page, scale)
    }

    /// Schema-field geometry for the compiled session — **one
    /// [`RenderedRegion`] per (placement, page fragment)**, keyed on the quill
    /// schema field path. A session-level query computed without rendering
    /// bytes; an interactive preview reads it to place overlays / field
    /// cross-navigation over a `paint`-ed canvas. Empty for backends that
    /// place no schema fields.
    ///
    /// A field may appear more than once: placed at several sites, breaking
    /// across pages (one fragment per page it touches, so a highlight covers
    /// continuation pages), or arising from both tagged content and a
    /// `field:`-bound widget (overlapping rects that route to the same field).
    /// Group by `field`; every entry routes to that field in the editor.
    pub fn regions(&self) -> Vec<RenderedRegion> {
        self.inner.regions()
    }

    /// Non-fatal diagnostics of the session's **current compile** — set at
    /// `Backend::open` and refreshed by each committed [`apply`](Self::apply);
    /// a failed apply keeps the last-good compile *and* its warnings. Also
    /// appended to [`RenderResult::warnings`] on each
    /// [`render`](Self::render) call. Exposed for consumers (e.g. canvas
    /// previews) that never call `render()`.
    pub fn warnings(&self) -> &[Diagnostic] {
        self.inner.warnings()
    }

    pub fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let mut result = self.inner.render(opts)?;
        result.warnings.extend(self.inner.warnings().iter().cloned());
        // The regions sidecar is attached here, at the wrapper, so every
        // backend's one-shot render carries it without implementing anything
        // beyond the `regions` accessor it already has.
        if opts.regions {
            result.regions = self.inner.regions();
        }
        Ok(result)
    }

    /// Recompile the session against new document data — the edit verb of a
    /// live preview. Transactional: on `Err` the previous compile stays live,
    /// so every read keeps serving the last-good document and its
    /// [`warnings`](Self::warnings); on `Ok` the session serves the new
    /// compile — warnings included — and the [`ChangeSet`] reports what
    /// changed. Pass data compiled by the same schema pipeline as
    /// `Backend::open`'s `json_data` (`Quill::compile_data`) — and from the
    /// *same quill*: the `$quill` reference check lives at the layer that
    /// still holds a `Document` (`Quillmark::open`, the WASM `apply`);
    /// compiled data does not carry the reference, so this seam cannot
    /// re-check it.
    pub fn apply(&mut self, json_data: &serde_json::Value) -> Result<ChangeSet, RenderError> {
        self.inner.apply(json_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A canvas-capable session: overrides the seam for `pages` pages.
    struct CanvasHandle {
        pages: usize,
    }
    impl SessionHandle for CanvasHandle {
        fn render(&self, _: &RenderOptions) -> Result<RenderResult, RenderError> {
            unimplemented!("render is not exercised by capability tests")
        }
        fn page_count(&self) -> usize {
            self.pages
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn page_size_pt(&self, page: usize) -> Option<(f32, f32)> {
            (page < self.pages).then_some((612.0, 792.0))
        }
    }

    /// A non-canvas session: leaves the seam at its `None` defaults.
    struct PlainHandle;
    impl SessionHandle for PlainHandle {
        fn render(&self, _: &RenderOptions) -> Result<RenderResult, RenderError> {
            unimplemented!("render is not exercised by capability tests")
        }
        fn page_count(&self) -> usize {
            1
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// A warning-emitting session: `warnings` reflects the current compile
    /// (one warning per committed apply), and `render` succeeds empty.
    struct WarningHandle {
        current: Vec<Diagnostic>,
        applies: usize,
    }
    impl SessionHandle for WarningHandle {
        fn render(&self, _: &RenderOptions) -> Result<RenderResult, RenderError> {
            Ok(RenderResult::new(Vec::new(), crate::OutputFormat::Pdf))
        }
        fn page_count(&self) -> usize {
            1
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn apply(&mut self, _: &serde_json::Value) -> Result<ChangeSet, RenderError> {
            self.applies += 1;
            self.current = vec![Diagnostic::new(
                Severity::Warning,
                format!("warning of compile {}", self.applies),
            )];
            Ok(ChangeSet {
                page_count: 1,
                dirty_pages: vec![],
            })
        }
        fn warnings(&self) -> &[Diagnostic] {
            &self.current
        }
    }

    /// `LiveSession::warnings` reflects the handle's current compile —
    /// refreshed by a committed apply — and `render` appends the same set to
    /// `RenderResult::warnings`.
    #[test]
    fn warnings_track_current_compile() {
        let open_warning = vec![Diagnostic::new(Severity::Warning, "open-time".to_string())];
        let mut session = LiveSession::new(Box::new(WarningHandle {
            current: open_warning,
            applies: 0,
        }));
        assert_eq!(session.warnings()[0].message, "open-time");

        session.apply(&serde_json::Value::Null).unwrap();
        assert_eq!(session.warnings()[0].message, "warning of compile 1");

        let result = session.render(&RenderOptions::default()).unwrap();
        assert_eq!(result.warnings[0].message, "warning of compile 1");
    }

    #[test]
    fn supports_canvas_derives_from_seam() {
        // A session that exposes page geometry is canvas-capable…
        let canvas = LiveSession::new(Box::new(CanvasHandle { pages: 2 }));
        assert!(canvas.supports_canvas());
        // …one that leaves the seam at its defaults is not…
        let plain = LiveSession::new(Box::new(PlainHandle));
        assert!(!plain.supports_canvas());
        // …and a canvas backend with no pages has nothing to paint.
        let empty = LiveSession::new(Box::new(CanvasHandle { pages: 0 }));
        assert!(!empty.supports_canvas());
    }
}
