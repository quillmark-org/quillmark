use pyo3::conversion::IntoPyObjectExt;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*; // PyResult, Python, etc.
use pyo3::pycell::PyRef; // PyRef
use pyo3::types::PyDict; // PyDict
use pyo3::Bound; // Bound

use quillmark::{
    Diagnostic, Document, Location, OutputFormat, Quill, Quillmark, RenderResult, RenderSession,
};
use std::path::PathBuf;

use crate::enums::{PyOutputFormat, PySeverity};
use crate::errors::{convert_edit_error, convert_render_error};

// Quillmark Engine wrapper
#[pyclass(name = "Quillmark")]
pub struct PyQuillmark {
    inner: Quillmark,
}

#[pymethods]
impl PyQuillmark {
    #[new]
    fn new() -> Self {
        Self {
            inner: Quillmark::new(),
        }
    }

    fn quill_from_path(&self, path: PathBuf) -> PyResult<PyQuill> {
        let quill = self
            .inner
            .quill_from_path(&path)
            .map_err(convert_render_error)?;
        Ok(PyQuill { inner: quill })
    }

    fn registered_backends(&self) -> Vec<String> {
        self.inner
            .registered_backends()
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}

// Quill wrapper
#[pyclass(name = "Quill")]
#[derive(Clone)]
pub struct PyQuill {
    pub(crate) inner: Quill,
}

#[pymethods]
impl PyQuill {
    #[getter]
    fn print_tree(&self) -> String {
        self.inner.source().files().print_tree().clone()
    }

    #[getter]
    fn name(&self) -> String {
        self.inner.source().name().to_string()
    }

    #[getter]
    fn backend(&self) -> String {
        self.inner.backend_id().to_string()
    }

    #[getter]
    fn plate(&self) -> Option<String> {
        self.inner.source().plate().map(str::to_string)
    }

    #[getter]
    fn example(&self) -> Option<String> {
        self.inner.source().example().map(str::to_string)
    }

    #[getter]
    fn quill_ref(&self) -> String {
        let source = self.inner.source();
        let version = source
            .metadata()
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0");
        format!("{}@{}", source.name(), version)
    }

    #[getter]
    fn metadata<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in self.inner.source().metadata() {
            dict.set_item(key, quillvalue_to_py(py, value)?)?;
        }
        Ok(dict)
    }

    /// Document schema as YAML, including `ui` hints.
    #[getter]
    fn schema<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let yaml = self
            .inner
            .source()
            .config()
            .schema_yaml()
            .map_err(|e| PyValueError::new_err(format!("schema: {}", e)))?;
        Ok(yaml.into_pyobject(py)?.into_any())
    }

    #[getter]
    fn defaults<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in self.inner.source().config().main.defaults() {
            dict.set_item(key, quillvalue_to_py(py, &value)?)?;
        }
        Ok(dict)
    }

    #[getter]
    fn blueprint(&self) -> String {
        self.inner.source().config().blueprint()
    }

    #[getter]
    fn supported_formats(&self) -> Vec<PyOutputFormat> {
        self.inner
            .supported_formats()
            .iter()
            .map(|f| (*f).into())
            .collect()
    }

    #[pyo3(signature = (doc, format=None))]
    fn render(
        &self,
        doc: PyRef<'_, PyDocument>,
        format: Option<PyOutputFormat>,
    ) -> PyResult<PyRenderResult> {
        let opts = quillmark_core::RenderOptions {
            output_format: format.map(OutputFormat::from),
            ..Default::default()
        };
        let mut result = self
            .inner
            .render(&doc.inner, &opts)
            .map_err(convert_render_error)?;
        result
            .warnings
            .splice(0..0, doc.parse_warnings.iter().cloned());
        Ok(PyRenderResult { inner: result })
    }

    fn open(&self, doc: PyRef<'_, PyDocument>) -> PyResult<PyRenderSession> {
        let session = self.inner.open(&doc.inner).map_err(convert_render_error)?;
        Ok(PyRenderSession { inner: session })
    }

    /// Perform a dry run validation without backend compilation.
    ///
    /// Raises QuillmarkError with diagnostic payload on validation failure.
    fn dry_run(&self, doc: PyRef<'_, PyDocument>) -> PyResult<()> {
        self.inner.dry_run(&doc.inner).map_err(convert_render_error)
    }

    /// The schema-aware form view of `doc`.
    ///
    /// Returns a dict with keys `main`, `leaves`, and `diagnostics`:
    ///
    /// - `main`: dict with `schema` (dict) and `values` (dict of field → value info)
    /// - `leaves`: list of dicts in the same shape as `main`
    /// - `diagnostics`: list of dicts with `severity`, `code`, `message`, etc.
    ///
    /// Each `values` entry is a dict with:
    /// - `value`: the current document value, or `None` if absent
    /// - `default`: the schema default value, or `None` if none declared
    /// - `source`: one of `"document"`, `"default"`, or `"missing"`
    ///
    /// This is a **read-only snapshot**. Call `form` again after any edits
    /// to the document to obtain an updated view.
    ///
    /// Leaves with unknown tags are excluded from `leaves`; each produces a
    /// diagnostic with code `"form::unknown_leaf_kind"`.
    fn form<'py>(
        &self,
        py: Python<'py>,
        doc: PyRef<'_, PyDocument>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let form = self.inner.form(&doc.inner);

        // Serialise through serde_json → Python dict to avoid writing bespoke
        // conversion for every nested type (LeafSchema, FormFieldValue, etc.).
        let json_value = serde_json::to_value(&form).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyValueError, _>(format!(
                "form: serialization failed: {e}"
            ))
        })?;
        let py_obj = json_to_py(py, &json_value)?;
        let dict = py_obj.downcast::<PyDict>().map_err(|_| {
            PyErr::new::<pyo3::exceptions::PyValueError, _>("form: expected object at top level")
        })?;
        Ok(dict.clone())
    }

    /// A blank form for the main leaf — no document values supplied.
    ///
    /// Returns a dict shaped like one entry in `form()['main']`. Every
    /// declared field's `source` is `"default"` (when the schema declares a
    /// default) or `"missing"`.
    fn blank_main<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let leaf = self.inner.blank_main();
        let json_value = serde_json::to_value(&leaf).map_err(|e| {
            PyErr::new::<PyValueError, _>(format!("blank_main: serialization failed: {e}"))
        })?;
        let py_obj = json_to_py(py, &json_value)?;
        let dict = py_obj.downcast::<PyDict>().map_err(|_| {
            PyErr::new::<PyValueError, _>("blank_main: expected object at top level")
        })?;
        Ok(dict.clone())
    }

    /// A blank form for a leaf of the given type — no document values supplied.
    ///
    /// Returns `None` if `leaf_kind` is not declared in this quill's schema.
    /// Otherwise returns a dict shaped like a single entry in `form()['leaves']`.
    fn blank_leaf<'py>(
        &self,
        py: Python<'py>,
        leaf_kind: &str,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        let Some(leaf) = self.inner.blank_leaf(leaf_kind) else {
            return Ok(None);
        };
        let json_value = serde_json::to_value(&leaf).map_err(|e| {
            PyErr::new::<PyValueError, _>(format!("blank_leaf: serialization failed: {e}"))
        })?;
        let py_obj = json_to_py(py, &json_value)?;
        let dict = py_obj.downcast::<PyDict>().map_err(|_| {
            PyErr::new::<PyValueError, _>("blank_leaf: expected object at top level")
        })?;
        Ok(Some(dict.clone()))
    }
}

/// Python wrapper for the typed Quillmark `Document`.
///
/// Exposes:
/// - `from_markdown(markdown)` — static constructor
/// - `to_markdown()` — emit canonical Quillmark Markdown
/// - `quill_ref()` — quill reference string
/// - `frontmatter` — dict of typed YAML fields (no QUILL/BODY/LEAVES)
/// - `body` — global Markdown body (str, never None)
/// - `leaves` — list of `Leaf` dicts
/// - `warnings` — list of `Diagnostic` objects
#[pyclass(name = "Document")]
pub struct PyDocument {
    pub(crate) inner: Document,
    pub(crate) parse_warnings: Vec<quillmark_core::Diagnostic>,
}

#[pymethods]
impl PyDocument {
    #[staticmethod]
    fn from_markdown(markdown: &str) -> PyResult<Self> {
        let output = Document::from_markdown_with_warnings(markdown).map_err(|e| {
            let py_err = PyErr::new::<crate::errors::ParseError, _>(e.to_string());
            Python::attach(|py| {
                if let Ok(exc) = py_err.value(py).downcast::<pyo3::types::PyAny>() {
                    let py_diag = crate::types::PyDiagnostic {
                        inner: e.to_diagnostic(),
                    };
                    let _ = exc.setattr("diagnostic", py_diag);
                }
            });
            py_err
        })?;
        Ok(PyDocument {
            inner: output.document,
            parse_warnings: output.warnings,
        })
    }

    /// Emit canonical Quillmark Markdown.
    ///
    /// Returns the document serialised as a Quillmark Markdown string.
    /// The output is type-fidelity round-trip safe: re-parsing the result
    /// produces a `Document` equal to `self` by value and by type.
    fn to_markdown(&self) -> String {
        self.inner.to_markdown()
    }

    /// The QUILL reference string (e.g. `"usaf_memo@0.1"`).
    fn quill_ref(&self) -> String {
        self.inner.quill_reference().to_string()
    }

    /// Non-fatal parse-time warnings.
    #[getter]
    fn warnings(&self) -> Vec<PyDiagnostic> {
        self.parse_warnings
            .iter()
            .map(|d| PyDiagnostic { inner: d.clone() })
            .collect()
    }

    /// Main leaf's global Markdown body (str, never None).
    ///
    /// Convenience accessor equivalent to `doc.main['body']`.
    #[getter]
    fn body(&self) -> &str {
        self.inner.main().body()
    }

    /// Typed YAML frontmatter fields on the main leaf (no QUILL, BODY, or LEAVES keys).
    ///
    /// Convenience accessor equivalent to `doc.main['frontmatter']`.
    #[getter]
    fn frontmatter<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in self.inner.main().frontmatter().iter() {
            dict.set_item(key, quillvalue_to_py(py, value)?)?;
        }
        Ok(dict)
    }

    /// The document's main (entry) leaf as a dict.
    ///
    /// Keys: `tag` (str), `frontmatter` (dict), `frontmatter_items` (list),
    /// `fields` (dict — alias of `frontmatter`), `body` (str).
    #[getter]
    fn main<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        leaf_to_pydict(py, self.inner.main())
    }

    /// Ordered list of composable leaf blocks.
    ///
    /// Each leaf is a dict with keys: `tag` (str), `frontmatter` (dict),
    /// `frontmatter_items` (list), `fields` (dict), `body` (str).
    #[getter]
    fn leaves<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
        let mut result = Vec::new();
        for leaf in self.inner.leaves() {
            result.push(leaf_to_pydict(py, leaf)?);
        }
        Ok(result)
    }

    // ── Mutators ──────────────────────────────────────────────────────────────

    /// Set a frontmatter field by name on the main leaf.
    ///
    /// Convenience method equivalent to `doc.main_mut().set_field(name, value)`.
    /// Clears any `!fill` marker on the field.
    ///
    /// Raises `quillmark.EditError` if `name` is a reserved sentinel
    /// (`BODY`, `LEAVES`, `QUILL`, `KIND`) or does not match `[a-z_][a-z0-9_]*`.
    ///
    /// This method never modifies `warnings`.
    fn set_field(&mut self, name: &str, value: Bound<'_, PyAny>) -> PyResult<()> {
        let qv = py_to_quillvalue(&value)?;
        self.inner
            .main_mut()
            .set_field(name, qv)
            .map_err(convert_edit_error)
    }

    /// Set a frontmatter field on the main leaf AND mark it as `!fill`.
    ///
    /// Convenience method equivalent to `doc.main_mut().set_fill(name, value)`.
    fn set_fill(&mut self, name: &str, value: Bound<'_, PyAny>) -> PyResult<()> {
        let qv = py_to_quillvalue(&value)?;
        self.inner
            .main_mut()
            .set_fill(name, qv)
            .map_err(convert_edit_error)
    }

    /// Remove a frontmatter field from the main leaf, returning the value or `None`.
    ///
    /// Raises `quillmark.EditError` if `name` is reserved (`BODY`, `LEAVES`,
    /// `QUILL`, `KIND`) or does not match `[a-z_][a-z0-9_]*`. Absence of an
    /// otherwise-valid name returns `None`.
    ///
    /// This method never modifies `warnings`.
    fn remove_field<'py>(&mut self, py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyAny>> {
        match self
            .inner
            .main_mut()
            .remove_field(name)
            .map_err(convert_edit_error)?
        {
            Some(v) => quillvalue_to_py(py, &v),
            None => py.None().into_bound_py_any(py),
        }
    }

    /// Replace the QUILL reference string.
    ///
    /// Raises `ValueError` if `ref_str` is not a valid `QuillReference`.
    ///
    /// This method never modifies `warnings`.
    fn set_quill_ref(&mut self, ref_str: &str) -> PyResult<()> {
        let qr: quillmark_core::QuillReference = ref_str.parse().map_err(|e| {
            PyValueError::new_err(format!("invalid QuillReference '{}': {}", ref_str, e))
        })?;
        self.inner.set_quill_ref(qr);
        Ok(())
    }

    /// Replace the main leaf's body (the global Markdown body).
    ///
    /// This method never modifies `warnings`.
    fn replace_body(&mut self, body: &str) {
        self.inner.main_mut().replace_body(body);
    }

    /// Append a leaf to the leaf list.
    ///
    /// `leaf` must be a dict with a `tag` key (str) and optional `fields` (dict)
    /// and `body` (str).
    ///
    /// Raises `quillmark.EditError` if `leaf["tag"]` is not a valid tag name or
    /// if any field name is invalid.
    ///
    /// This method never modifies `warnings`.
    fn push_leaf(&mut self, leaf: Bound<'_, PyAny>) -> PyResult<()> {
        let core_leaf = py_dict_to_leaf(&leaf)?;
        self.inner.push_leaf(core_leaf);
        Ok(())
    }

    /// Insert a leaf at the given index.
    ///
    /// `index` must be in `0..=len`. Out-of-range raises `quillmark.EditError`.
    ///
    /// This method never modifies `warnings`.
    fn insert_leaf(&mut self, index: usize, leaf: Bound<'_, PyAny>) -> PyResult<()> {
        let core_leaf = py_dict_to_leaf(&leaf)?;
        self.inner
            .insert_leaf(index, core_leaf)
            .map_err(convert_edit_error)
    }

    /// Remove and return the leaf at `index`, or `None` if out of range.
    ///
    /// This method never modifies `warnings`.
    fn remove_leaf<'py>(
        &mut self,
        py: Python<'py>,
        index: usize,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        match self.inner.remove_leaf(index) {
            Some(leaf) => Ok(Some(leaf_to_pydict(py, &leaf)?)),
            None => Ok(None),
        }
    }

    /// Move the leaf at `from_idx` to position `to_idx`.
    ///
    /// `from_idx == to_idx` is a no-op. Both indices must be in `0..len`.
    /// Out-of-range raises `quillmark.EditError`.
    ///
    /// This method never modifies `warnings`.
    fn move_leaf(&mut self, from_idx: usize, to_idx: usize) -> PyResult<()> {
        self.inner
            .move_leaf(from_idx, to_idx)
            .map_err(convert_edit_error)
    }

    /// Replace the tag of the composable leaf at `index`.
    ///
    /// Mutates only the sentinel — the leaf's frontmatter and body are
    /// untouched. Schema-aware migration (clearing orphan fields, applying
    /// new defaults) is the caller's responsibility; `set_leaf_tag` is a
    /// structural primitive.
    ///
    /// Raises `quillmark.EditError` if `index` is out of range or `new_tag`
    /// does not match `[a-z_][a-z0-9_]*`.
    ///
    /// This method never modifies `warnings`.
    fn set_leaf_tag(&mut self, index: usize, new_tag: &str) -> PyResult<()> {
        self.inner
            .set_leaf_tag(index, new_tag)
            .map_err(convert_edit_error)
    }

    /// Update a field on the leaf at `index`.
    ///
    /// Raises `quillmark.EditError` if `index` is out of range, `name` is
    /// reserved or invalid, or `value` cannot be converted.
    ///
    /// This method never modifies `warnings`.
    fn update_leaf_field(
        &mut self,
        index: usize,
        name: &str,
        value: Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let qv = py_to_quillvalue(&value)?;
        let len = self.inner.leaves().len();
        let leaf = self.inner.leaf_mut(index).ok_or_else(|| {
            convert_edit_error(quillmark_core::EditError::IndexOutOfRange { index, len })
        })?;
        leaf.set_field(name, qv).map_err(convert_edit_error)
    }

    /// Replace the body of the leaf at `index`.
    ///
    /// Raises `quillmark.EditError` if `index` is out of range.
    ///
    /// This method never modifies `warnings`.
    fn update_leaf_body(&mut self, index: usize, body: &str) -> PyResult<()> {
        let len = self.inner.leaves().len();
        let leaf = self.inner.leaf_mut(index).ok_or_else(|| {
            convert_edit_error(quillmark_core::EditError::IndexOutOfRange { index, len })
        })?;
        leaf.replace_body(body);
        Ok(())
    }
}

// RenderResult wrapper
#[pyclass(name = "RenderResult")]
pub struct PyRenderResult {
    pub(crate) inner: RenderResult,
}

#[pyclass(name = "RenderSession")]
pub struct PyRenderSession {
    pub(crate) inner: RenderSession,
}

#[pymethods]
impl PyRenderSession {
    #[getter]
    fn page_count(&self) -> usize {
        self.inner.page_count()
    }

    #[pyo3(signature = (format=None, pages=None))]
    fn render(
        &self,
        format: Option<PyOutputFormat>,
        pages: Option<Vec<usize>>,
    ) -> PyResult<PyRenderResult> {
        let opts = quillmark::RenderOptions {
            output_format: format.map(OutputFormat::from),
            ppi: None,
            pages,
        };
        let result = self.inner.render(&opts).map_err(convert_render_error)?;
        Ok(PyRenderResult { inner: result })
    }
}

#[pymethods]
impl PyRenderResult {
    #[getter]
    fn artifacts(&self) -> Vec<PyArtifact> {
        self.inner
            .artifacts
            .iter()
            .map(|a| PyArtifact {
                inner: a.bytes.clone(),
                output_format: a.output_format,
            })
            .collect()
    }

    #[getter]
    fn warnings(&self) -> Vec<PyDiagnostic> {
        self.inner
            .warnings
            .iter()
            .map(|d| PyDiagnostic { inner: d.clone() })
            .collect()
    }

    #[getter]
    fn output_format(&self) -> PyOutputFormat {
        self.inner.output_format.into()
    }
}

// Artifact wrapper
#[pyclass(name = "Artifact")]
#[derive(Clone)]
pub struct PyArtifact {
    pub(crate) inner: Vec<u8>,
    pub(crate) output_format: OutputFormat,
}

#[pymethods]
impl PyArtifact {
    #[getter]
    fn bytes(&self) -> Vec<u8> {
        self.inner.clone()
    }

    #[getter]
    fn output_format(&self) -> PyOutputFormat {
        self.output_format.into()
    }

    fn save(&self, path: String) -> PyResult<()> {
        std::fs::write(&path, &self.inner).map_err(|e| {
            PyErr::new::<crate::errors::QuillmarkError, _>(format!(
                "Failed to save artifact to {}: {}",
                path, e
            ))
        })
    }

    #[getter]
    fn mime_type(&self) -> &'static str {
        match self.output_format {
            OutputFormat::Pdf => "application/pdf",
            OutputFormat::Svg => "image/svg+xml",
            OutputFormat::Txt => "text/plain",
            OutputFormat::Png => "image/png",
        }
    }
}

// Diagnostic wrapper
#[pyclass(name = "Diagnostic")]
#[derive(Clone)]
pub struct PyDiagnostic {
    pub(crate) inner: Diagnostic,
}

#[pymethods]
impl PyDiagnostic {
    #[getter]
    fn severity(&self) -> PySeverity {
        self.inner.severity.into()
    }

    #[getter]
    fn message(&self) -> &str {
        &self.inner.message
    }

    #[getter]
    fn code(&self) -> Option<&str> {
        self.inner.code.as_deref()
    }

    #[getter]
    fn location(&self) -> Option<PyLocation> {
        self.inner
            .location
            .as_ref()
            .map(|l| PyLocation { inner: l.clone() })
    }

    #[getter]
    fn hint(&self) -> Option<&str> {
        self.inner.hint.as_deref()
    }

    /// Document-model path anchor (e.g. `"leaves.indorsement[0].signature_block"`).
    ///
    /// Set on schema validation diagnostics; `None` otherwise. See the Rust
    /// `quillmark_core::error` module docs for the path grammar.
    #[getter]
    fn path(&self) -> Option<&str> {
        self.inner.path.as_deref()
    }

    #[getter]
    fn source_chain(&self) -> Vec<String> {
        self.inner.source_chain.clone()
    }
}

// Location wrapper
#[pyclass(name = "Location")]
#[derive(Clone)]
pub struct PyLocation {
    pub(crate) inner: Location,
}

#[pymethods]
impl PyLocation {
    #[getter]
    fn file(&self) -> &str {
        &self.inner.file
    }

    #[getter]
    fn line(&self) -> usize {
        self.inner.line as usize
    }

    #[getter]
    fn column(&self) -> usize {
        self.inner.column as usize
    }
}

// Helper function to convert QuillValue (backed by JSON) to Python objects
fn quillvalue_to_py<'py>(
    py: Python<'py>,
    value: &quillmark_core::QuillValue,
) -> PyResult<Bound<'py, PyAny>> {
    json_to_py(py, value.as_json())
}

// Helper: convert a typed Leaf into the Python dict shape exposed to callers.
fn leaf_to_pydict<'py>(
    py: Python<'py>,
    leaf: &quillmark_core::Leaf,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("sentinel", if leaf.is_main() { "main" } else { "leaf" })?;
    d.set_item("tag", leaf.tag())?;

    // Map-keyed frontmatter view (values only, no comments).
    let fm = PyDict::new(py);
    for (k, v) in leaf.frontmatter().iter() {
        fm.set_item(k, quillvalue_to_py(py, v)?)?;
    }
    d.set_item("frontmatter", fm.clone())?;
    d.set_item("fields", fm)?;

    // Ordered item list with comments and fill flags.
    let items = pyo3::types::PyList::empty(py);
    for item in leaf.frontmatter().items() {
        let entry = PyDict::new(py);
        match item {
            quillmark_core::FrontmatterItem::Field { key, value, fill } => {
                entry.set_item("kind", "field")?;
                entry.set_item("key", key)?;
                entry.set_item("value", quillvalue_to_py(py, value)?)?;
                entry.set_item("fill", *fill)?;
            }
            quillmark_core::FrontmatterItem::Comment { text, inline } => {
                entry.set_item("kind", "comment")?;
                entry.set_item("text", text)?;
                entry.set_item("inline", *inline)?;
            }
        }
        items.append(entry)?;
    }
    d.set_item("frontmatter_items", items)?;

    d.set_item("body", leaf.body())?;
    Ok(d)
}

// Helper function to convert JSON values to Python objects
fn json_to_py<'py>(py: Python<'py>, value: &serde_json::Value) -> PyResult<Bound<'py, PyAny>> {
    match value {
        serde_json::Value::Null => py.None().into_bound_py_any(py),
        serde_json::Value::Bool(b) => b.into_bound_py_any(py),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.into_bound_py_any(py)
            } else if let Some(u) = n.as_u64() {
                u.into_bound_py_any(py)
            } else if let Some(f) = n.as_f64() {
                f.into_bound_py_any(py)
            } else {
                py.None().into_bound_py_any(py)
            }
        }
        serde_json::Value::String(s) => s.as_str().into_bound_py_any(py),
        serde_json::Value::Array(arr) => {
            let list = pyo3::types::PyList::empty(py);
            for item in arr {
                let val = json_to_py(py, item)?;
                list.append(val)?;
            }
            Ok(list.into_any())
        }
        serde_json::Value::Object(map) => {
            let dict = pyo3::types::PyDict::new(py);
            for (key, val) in map {
                let py_val = json_to_py(py, val)?;
                dict.set_item(key, py_val)?;
            }
            Ok(dict.into_any())
        }
    }
}

// ── Python → Rust conversion helpers ─────────────────────────────────────────

/// Convert a Python object to a [`quillmark_core::QuillValue`].
///
/// Supports: `None` → null, `bool`, `int`, `float`, `str`, `list`, `dict`.
fn py_to_quillvalue(value: &Bound<'_, PyAny>) -> PyResult<quillmark_core::QuillValue> {
    let json = py_to_json(value)?;
    Ok(quillmark_core::QuillValue::from_json(json))
}

fn py_to_json(value: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    use pyo3::types::{PyBool, PyFloat, PyInt, PyList, PyString};

    if value.is_none() {
        return Ok(serde_json::Value::Null);
    }
    if value.is_instance_of::<PyBool>() {
        let b: bool = value.extract()?;
        return Ok(serde_json::Value::Bool(b));
    }
    if value.is_instance_of::<PyInt>() {
        let i: i64 = value.extract()?;
        return Ok(serde_json::json!(i));
    }
    if value.is_instance_of::<PyFloat>() {
        let f: f64 = value.extract()?;
        return Ok(serde_json::json!(f));
    }
    if value.is_instance_of::<PyString>() {
        let s: String = value.extract()?;
        return Ok(serde_json::Value::String(s));
    }
    if value.is_instance_of::<PyList>() {
        let list = value.downcast::<PyList>()?;
        let arr: PyResult<Vec<serde_json::Value>> =
            list.iter().map(|item| py_to_json(&item)).collect();
        return Ok(serde_json::Value::Array(arr?));
    }
    if value.is_instance_of::<PyDict>() {
        let dict = value.downcast::<PyDict>()?;
        let mut map = serde_json::Map::new();
        for (k, v) in dict.iter() {
            let key: String = k.extract()?;
            map.insert(key, py_to_json(&v)?);
        }
        return Ok(serde_json::Value::Object(map));
    }
    // Fallback: convert to string
    let s = value.str()?.to_string();
    Ok(serde_json::Value::String(s))
}

/// Convert a Python dict `{"tag": str, "fields"?: dict, "body"?: str}` to a
/// [`quillmark_core::Leaf`].  Raises `EditError` on invalid tag or field names.
fn py_dict_to_leaf(value: &Bound<'_, PyAny>) -> PyResult<quillmark_core::Leaf> {
    let dict = value
        .downcast::<PyDict>()
        .map_err(|_| PyValueError::new_err("leaf must be a dict with a 'tag' key"))?;

    let tag: String = dict
        .get_item("tag")?
        .ok_or_else(|| PyValueError::new_err("leaf dict must have a 'tag' key"))?
        .extract()?;

    let mut leaf = quillmark_core::Leaf::new(tag).map_err(convert_edit_error)?;

    if let Some(fields_val) = dict.get_item("fields")? {
        let fields_dict = fields_val
            .downcast::<PyDict>()
            .map_err(|_| PyValueError::new_err("leaf 'fields' must be a dict"))?;
        for (k, v) in fields_dict.iter() {
            let field_name: String = k.extract()?;
            let qv = py_to_quillvalue(&v)?;
            leaf.set_field(&field_name, qv)
                .map_err(convert_edit_error)?;
        }
    }

    if let Some(body_val) = dict.get_item("body")? {
        let body: String = body_val.extract()?;
        leaf.replace_body(body);
    }

    Ok(leaf)
}
