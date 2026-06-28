//! Backend trait for output backends.

use crate::error::RenderError;
use crate::quill::Quill;
use crate::{OutputFormat, RenderSession};

/// Backend trait for rendering different output formats.
pub trait Backend: Send + Sync + std::fmt::Debug {
    /// Get the backend identifier (e.g., "typst", "latex").
    fn id(&self) -> &'static str;

    /// Get supported output formats.
    fn supported_formats(&self) -> &'static [OutputFormat];

    /// Whether this backend can paint sessions to a canvas (iterative
    /// `pageSize` / `paint`). The honest capability the engine reports as
    /// `supports_canvas`, asked of the real backend rather than guessed from
    /// the id. Defaults to `false`; canvas-capable backends override it.
    ///
    /// This is the static, pre-session half of the raster-preview seam; the
    /// dynamic half is `SessionHandle::page_size_pt` / `render_rgba`, which
    /// default to `None`. The two are **expected to agree by convention** — a
    /// backend that returns `true` here should override both session methods,
    /// and one that leaves them `None` should return `false` here — but nothing
    /// in the type system enforces it: they are three separately-defaultable
    /// methods on two traits. The in-tree backends uphold the convention; an
    /// out-of-tree backend that returns `true` here while leaving `render_rgba`
    /// at its `None` default reproduces the paint-nothing failure the seam is
    /// meant to avoid. The painter dispatches generically through the session
    /// seam, so such a backend reports a canvas as available yet produces no
    /// raster (surfaced as a distinct "reported canvas but produced no raster"
    /// error, not a page-out-of-range one).
    fn supports_canvas(&self) -> bool {
        false
    }

    /// Open an iterative render session from plate + compiled JSON data.
    fn open(
        &self,
        plate_content: &str,
        source: &Quill,
        json_data: &serde_json::Value,
    ) -> Result<RenderSession, RenderError>;
}
