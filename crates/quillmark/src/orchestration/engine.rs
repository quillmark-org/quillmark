use quillmark_core::{
    Backend, Diagnostic, Document, OutputFormat, Quill, RenderError, RenderOptions, RenderResult,
    RenderSession, Severity,
};
use std::collections::HashMap;
use std::sync::Arc;

/// High-level engine: a backend registry and render dispatcher.
///
/// The engine resolves a [`Quill`]'s *declared* backend at render time and is
/// the sole home of backend-dependent surface — capability
/// ([`supported_formats`](Self::supported_formats) /
/// [`supports_canvas`](Self::supports_canvas)) and rendering
/// ([`open`](Self::open) / [`render`](Self::render)). Quill loading lives
/// elsewhere: construct a [`Quill`] with [`Quill::from_tree`] or
/// [`quill_from_path`](crate::quill_from_path).
pub struct Quillmark {
    backends: HashMap<String, Arc<dyn Backend>>,
}

impl Quillmark {
    /// Create a new Quillmark with auto-registered backends based on enabled features.
    pub fn new() -> Self {
        // `mut` is unused when no backend features are enabled (e.g. a
        // Typst-less core build), so allow it rather than cfg-juggle.
        #[allow(unused_mut)]
        let mut engine = Self {
            backends: HashMap::new(),
        };

        #[cfg(feature = "typst")]
        {
            engine.register_backend(Box::new(quillmark_typst::TypstBackend));
        }

        #[cfg(feature = "pdfform")]
        {
            engine.register_backend(Box::new(quillmark_pdfform::PdfformBackend));
        }

        engine
    }

    /// Register a backend with the engine.
    pub fn register_backend(&mut self, backend: Box<dyn Backend>) {
        let id = backend.id().to_string();
        self.backends.insert(id, Arc::from(backend));
    }

    /// Get a list of registered backend IDs.
    pub fn registered_backends(&self) -> Vec<&str> {
        self.backends.keys().map(|s| s.as_str()).collect()
    }

    /// Resolve a quill's declared backend, erroring with `UnsupportedBackend`
    /// when none is registered. The backend-existence check lives here — at
    /// render time, not load time — so a backend-less core can still load and
    /// validate quills.
    fn resolve_backend(&self, quill: &Quill) -> Result<&Arc<dyn Backend>, RenderError> {
        let backend_id = quill.backend_id();
        self.backends
            .get(backend_id)
            .ok_or_else(|| RenderError::UnsupportedBackend {
                diags: vec![Diagnostic::new(
                    Severity::Error,
                    format!("Backend '{}' not registered or not enabled", backend_id),
                )
                .with_code("engine::backend_not_found".to_string())
                .with_hint(format!(
                    "Available backends: {}",
                    self.backends.keys().cloned().collect::<Vec<_>>().join(", ")
                ))],
            })
    }

    /// Open an iterative render session for `doc` against `quill`'s backend.
    pub fn open(&self, quill: &Quill, doc: &Document) -> Result<RenderSession, RenderError> {
        let backend = self.resolve_backend(quill)?;
        quill.check_quill_reference(doc)?;
        let json_data = quill.compile_data(doc)?;
        backend.open(quill, &json_data)
    }

    /// Render `doc` against `quill` in one shot. Convenience over
    /// [`open`](Self::open) + [`RenderSession::render`]: an unset
    /// `output_format` falls back to the backend's first supported format.
    pub fn render(
        &self,
        quill: &Quill,
        doc: &Document,
        opts: &RenderOptions,
    ) -> Result<RenderResult, RenderError> {
        let default_format = self.supported_formats(quill)?.first().copied();
        let session = self.open(quill, doc)?;
        let resolved = RenderOptions {
            output_format: opts.output_format.or(default_format),
            ppi: opts.ppi,
            pages: opts.pages.clone(),
            producer: opts.producer.clone(),
        };
        session.render(&resolved)
    }

    /// The output formats `quill`'s backend can emit. Static capability —
    /// resolves the backend but compiles nothing.
    pub fn supported_formats(&self, quill: &Quill) -> Result<&'static [OutputFormat], RenderError> {
        Ok(self.resolve_backend(quill)?.supported_formats())
    }

    /// Pre-session hint for whether `quill`'s backend can paint sessions to a
    /// canvas, derived from the backend's output formats (see
    /// [`quillmark_core::formats_support_canvas`]); `false` when the backend is
    /// unsupported. Resolves the backend but compiles nothing — use it to decide
    /// whether to offer a canvas preview before opening a session. The
    /// authoritative answer is
    /// [`RenderSession::supports_canvas`](quillmark_core::RenderSession::supports_canvas)
    /// once a session exists.
    pub fn supports_canvas(&self, quill: &Quill) -> bool {
        self.resolve_backend(quill)
            .map(|b| quillmark_core::formats_support_canvas(b.supported_formats()))
            .unwrap_or(false)
    }
}

impl Default for Quillmark {
    fn default() -> Self {
        Self::new()
    }
}
