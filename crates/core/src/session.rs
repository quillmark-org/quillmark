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

    /// Borrow the underlying [`SessionHandle`] for typed-side-channel access.
    ///
    /// Bindings call this and downcast via [`SessionHandle::as_any`] to reach
    /// backend-specific surfaces. Intentionally `#[doc(hidden)]` — the shape
    /// of this accessor is not part of the stable public API.
    #[doc(hidden)]
    pub fn handle(&self) -> &dyn SessionHandle {
        &*self.inner
    }

    /// Attach session-level warnings, surfaced verbatim by
    /// [`RenderSession::warnings`] and appended to [`RenderResult::warnings`]
    /// on every [`RenderSession::render`] call.
    ///
    /// This is the populator for the open-time warnings channel that bindings
    /// expose as `session.warnings` (read by canvas previews, which never call
    /// `render()`). A [`Backend`](crate::Backend) chains it onto the session it
    /// returns from `open` to carry non-fatal diagnostics. The built-in Typst
    /// backend emits none, so the channel is empty unless a backend opts in.
    pub fn with_warnings(mut self, warnings: Vec<Diagnostic>) -> Self {
        self.warnings = warnings;
        self
    }

    pub fn page_count(&self) -> usize {
        self.inner.page_count()
    }

    /// Snapshot of session-level warnings attached at `Backend::open` time.
    ///
    /// Empty when the backend produced none. These are also appended to
    /// [`RenderResult::warnings`] on each [`RenderSession::render`] call;
    /// this accessor surfaces them to consumers (e.g. canvas previews) that
    /// don't go through `render()`.
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }

    pub fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let mut result = self.inner.render(opts)?;
        result.warnings.extend(self.warnings.iter().cloned());
        Ok(result)
    }
}
