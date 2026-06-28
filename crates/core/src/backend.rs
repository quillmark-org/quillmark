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

    /// Open an iterative render session from a quill and compiled JSON data.
    ///
    /// The backend pulls whatever static inputs it needs straight from
    /// `source` ([`Quill::files`] for assets, [`Quill::config`] for
    /// backend-specific config). There is no universal "template" input: a
    /// template/plate is one backend's private notion, read by that backend
    /// from its own files, not a parameter every backend must accept.
    fn open(
        &self,
        source: &Quill,
        json_data: &serde_json::Value,
    ) -> Result<RenderSession, RenderError>;
}

/// Pre-session hint for whether a backend with these `formats` can paint pages
/// to a canvas, used before a session exists (e.g. a GUI deciding whether to
/// mount a canvas preview without first paying to open one).
///
/// Canvas paint needs a per-page *visual image* of the laid-out page, so the
/// predicate keys off the visual-page output formats — [`OutputFormat::Png`]
/// (raster) and [`OutputFormat::Svg`] (vector) — as opposed to
/// [`OutputFormat::Pdf`] (a document). A backend that can rasterize a page
/// advertises one of these in [`Backend::supported_formats`].
///
/// This is only a hint. The **authoritative** answer is
/// [`RenderSession::supports_canvas`](crate::RenderSession::supports_canvas),
/// which is derived from the session's actual canvas seam
/// ([`SessionHandle::page_size_pt`](crate::session::SessionHandle::page_size_pt))
/// — there is no separately maintained capability flag to drift from the
/// implementation (a canvas backend pairs `render_rgba` with `page_size_pt`).
pub fn formats_support_canvas(formats: &[OutputFormat]) -> bool {
    formats
        .iter()
        .any(|f| matches!(f, OutputFormat::Png | OutputFormat::Svg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_support_canvas_keys_off_visual_formats() {
        use OutputFormat::*;
        // A visual-page format present → canvas (Typst, pdfform-preview).
        assert!(formats_support_canvas(&[Pdf, Svg, Png]));
        assert!(formats_support_canvas(&[Pdf, Svg]));
        assert!(formats_support_canvas(&[Png]));
        // Document-only / text-only → no canvas (pdfform without preview).
        assert!(!formats_support_canvas(&[Pdf]));
        assert!(!formats_support_canvas(&[Txt]));
        assert!(!formats_support_canvas(&[]));
    }
}
