use std::any::Any;

use crate::{Diagnostic, RenderError, RenderOptions, RenderResult};

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
    /// The `regions` sidecar on [`RenderResult`](crate::RenderResult) carries
    /// per-field geometry (and bound value) for *overlay* UIs regardless; it is
    /// never required to make the raster complete.
    ///
    /// A backend with no painter overrides neither this nor
    /// [`page_size_pt`](Self::page_size_pt); the defaults mark the session as
    /// non-canvas, which is exactly what [`RenderSession::supports_canvas`]
    /// reports. Capability is derived from the `page_size_pt` half of this seam,
    /// not declared as a separate flag — a canvas backend is contractually
    /// expected to pair this method with `page_size_pt` over the same page set.
    fn render_rgba(&self, _page: usize, _scale: f32) -> Option<(u32, u32, Vec<u8>)> {
        None
    }
}

/// Opaque, backend-backed iterative render session.
pub struct RenderSession {
    inner: Box<dyn SessionHandle>,
    warnings: Vec<Diagnostic>,
}

impl RenderSession {
    #[doc(hidden)]
    pub fn new(inner: Box<dyn SessionHandle>) -> Self {
        Self {
            inner,
            warnings: Vec::new(),
        }
    }

    /// Borrow the underlying [`SessionHandle`].
    ///
    /// The canonical canvas-preview path does **not** go through here: it
    /// dispatches generically through [`page_size_pt`](RenderSession::page_size_pt)
    /// / [`render_rgba`](RenderSession::render_rgba) on the session, with no
    /// downcast. This accessor exists only as a last-resort escape hatch for a
    /// backend that exposes a richer *typed* surface — reach it by downcasting
    /// via [`SessionHandle::as_any`]. (No in-tree caller currently does;
    /// `typst_session_of` is callerless and a candidate for removal.)
    /// Intentionally `#[doc(hidden)]` — the shape of this accessor is not part
    /// of the stable public API.
    #[doc(hidden)]
    pub fn handle(&self) -> &dyn SessionHandle {
        &*self.inner
    }

    /// Attach session-level warnings, surfaced by [`RenderSession::warnings`]
    /// and appended to [`RenderResult::warnings`] on every
    /// [`RenderSession::render`] call.
    ///
    /// A [`Backend`](crate::Backend) chains this onto the session it returns
    /// from `open` to carry non-fatal open-time diagnostics. The built-in Typst
    /// backend emits none, so the channel stays empty unless a backend opts in.
    pub fn with_warnings(mut self, warnings: Vec<Diagnostic>) -> Self {
        self.warnings = warnings;
        self
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

    /// Session-level warnings attached at `Backend::open` time, also appended
    /// to [`RenderResult::warnings`] on each [`RenderSession::render`] call.
    /// Exposed for consumers (e.g. canvas previews) that never call `render()`.
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }

    pub fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let mut result = self.inner.render(opts)?;
        result.warnings.extend(self.warnings.iter().cloned());
        Ok(result)
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

    #[test]
    fn supports_canvas_derives_from_seam() {
        // A session that exposes page geometry is canvas-capable…
        let canvas = RenderSession::new(Box::new(CanvasHandle { pages: 2 }));
        assert!(canvas.supports_canvas());
        // …one that leaves the seam at its defaults is not…
        let plain = RenderSession::new(Box::new(PlainHandle));
        assert!(!plain.supports_canvas());
        // …and a canvas backend with no pages has nothing to paint.
        let empty = RenderSession::new(Box::new(CanvasHandle { pages: 0 }));
        assert!(!empty.supports_canvas());
    }
}
