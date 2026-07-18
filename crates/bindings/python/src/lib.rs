use pyo3::prelude::*;

mod enums;
mod errors;
mod types;

pub use enums::{PyOutputFormat, PySeverity};
pub use errors::{convert_edit_error, convert_render_error, QuillmarkError};
pub use types::{
    PyArtifact, PyCardWriter, PyDiagnostic, PyDocument, PyWriter, PyLocation, PyQuill, PyQuillmark,
    PyRenderResult,
};

#[pymodule]
fn _quillmark(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyQuillmark>()?;
    m.add_class::<PyQuill>()?;
    m.add_class::<PyDocument>()?;
    m.add_class::<PyWriter>()?;
    m.add_class::<PyCardWriter>()?;
    m.add_class::<PyRenderResult>()?;
    m.add_class::<PyArtifact>()?;
    m.add_class::<PyDiagnostic>()?;
    m.add_class::<PyLocation>()?;

    m.add_class::<PyOutputFormat>()?;
    m.add_class::<PySeverity>()?;

    // The document-free content codec — the on-demand markdown projection
    // (`export_markdown`) plus `import_markdown` / `rebase` / `map_pos`.
    m.add_function(wrap_pyfunction!(types::import_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(types::export_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(types::rebase, m)?)?;
    m.add_function(wrap_pyfunction!(types::map_pos, m)?)?;

    m.add("QuillmarkError", m.py().get_type::<QuillmarkError>())?;

    Ok(())
}
