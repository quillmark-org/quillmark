use pyo3::prelude::*;

mod enums;
mod errors;
mod types;

pub use enums::{PyOutputFormat, PySeverity};
pub use errors::{convert_edit_error, convert_render_error, QuillmarkError};
pub use types::{
    PyArtifact, PyCardView, PyCardWriter, PyDiagnostic, PyDocument, PyLocation, PyQuill,
    PyQuillmark, PyRenderResult, PyView, PyWriter,
};

// Python is Tier 1 + storage + render: field I/O flows through `quill.writer(doc)`
// / `quill.view(doc)`. The opaque store and the anchor-preserving content lane
// (`install` / `revise` / `apply_change` + the `importMarkdown` / `exportMarkdown`
// / `rebase` / `mapPos` codec) are WASM-only by scope. See prose/canon/BINDINGS.md.

#[pymodule]
fn _quillmark(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyQuillmark>()?;
    m.add_class::<PyQuill>()?;
    m.add_class::<PyDocument>()?;
    m.add_class::<PyWriter>()?;
    m.add_class::<PyCardWriter>()?;
    m.add_class::<PyView>()?;
    m.add_class::<PyCardView>()?;
    m.add_class::<PyRenderResult>()?;
    m.add_class::<PyArtifact>()?;
    m.add_class::<PyDiagnostic>()?;
    m.add_class::<PyLocation>()?;

    m.add_class::<PyOutputFormat>()?;
    m.add_class::<PySeverity>()?;

    m.add("QuillmarkError", m.py().get_type::<QuillmarkError>())?;

    Ok(())
}
