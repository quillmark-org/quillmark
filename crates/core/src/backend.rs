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
