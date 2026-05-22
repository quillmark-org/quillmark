use pyo3::conversion::IntoPyObjectExt;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::pycell::PyRef;
use pyo3::types::PyDict;
use pyo3::Bound;

use quillmark::{
    Diagnostic, Document, Location, OutputFormat, Quill, Quillmark, RenderResult, RenderSession,
};
use std::path::PathBuf;

use crate::enums::{PyOutputFormat, PySeverity};
use crate::errors::{convert_edit_error, convert_render_error};

/// Backend identifier for the only canvas-capable backend today. Matches the
/// wasm binding's `CANVAS_BACKEND_ID`; if a second canvas backend ever ships
/// replace this with a richer check.
const CANVAS_BACKEND_ID: &str = "typst";

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

    /// `True` iff the resolved backend is canvas-capable (currently `"typst"`).
    #[getter]
    fn supports_canvas(&self) -> bool {
        self.inner.backend_id() == CANVAS_BACKEND_ID
    }

    #[getter]
    fn plate(&self) -> Option<String> {
        self.inner.source().plate().map(str::to_string)
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

    /// Document schema as a structured dict (matches the wasm `schema` shape).
    /// Includes optional `ui` keys. Describes only the user-fillable fields;
    /// the quill reference and card-kind discriminators live on the quill's
    /// metadata, not the schema.
    #[getter]
    fn schema<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let value = self.inner.source().config().schema();
        json_to_py(py, &value)
    }

    /// Document schema as a YAML string, useful for documentation or LLM prompts.
    #[getter]
    fn schema_yaml(&self) -> PyResult<String> {
        self.inner
            .source()
            .config()
            .schema_yaml()
            .map_err(|e| PyValueError::new_err(format!("schema_yaml: {}", e)))
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
        Ok(PyRenderSession {
            inner: session,
            backend_id: self.inner.backend_id().to_string(),
        })
    }

    /// Validate without backend compilation. Raises `QuillmarkError` with diagnostic payload on failure.
    fn dry_run(&self, doc: PyRef<'_, PyDocument>) -> PyResult<()> {
        self.inner.dry_run(&doc.inner).map_err(convert_render_error)
    }

    /// Schema-aware form view of `doc`.
    ///
    /// Returns a dict with keys `main`, `cards`, and `diagnostics`. `main` and each
    /// entry in `cards` contain `schema` (dict) and `values` (dict of field → value info).
    /// Each `values` entry has `value`, `default`, and `source`
    /// (`"document"` | `"default"` | `"missing"`). Read-only snapshot — call again after edits.
    /// Cards with unknown kinds are excluded and produce a `"form::unknown_card_kind"` diagnostic.
    fn form<'py>(
        &self,
        py: Python<'py>,
        doc: PyRef<'_, PyDocument>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let form = self.inner.form(&doc.inner);

        // Serialise through serde_json → Python dict to avoid writing bespoke
        // conversion for every nested type (CardSchema, FormFieldValue, etc.).
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

    /// Blank form for the main card — shaped like `form()['main']` with no document values.
    fn blank_main<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let card = self.inner.blank_main();
        let json_value = serde_json::to_value(&card).map_err(|e| {
            PyErr::new::<PyValueError, _>(format!("blank_main: serialization failed: {e}"))
        })?;
        let py_obj = json_to_py(py, &json_value)?;
        let dict = py_obj.downcast::<PyDict>().map_err(|_| {
            PyErr::new::<PyValueError, _>("blank_main: expected object at top level")
        })?;
        Ok(dict.clone())
    }

    /// Blank form for `card_kind` — shaped like a `form()['cards']` entry, or `None` if kind unknown.
    fn blank_card<'py>(
        &self,
        py: Python<'py>,
        card_kind: &str,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        let Some(card) = self.inner.blank_card(card_kind) else {
            return Ok(None);
        };
        let json_value = serde_json::to_value(&card).map_err(|e| {
            PyErr::new::<PyValueError, _>(format!("blank_card: serialization failed: {e}"))
        })?;
        let py_obj = json_to_py(py, &json_value)?;
        let dict = py_obj.downcast::<PyDict>().map_err(|_| {
            PyErr::new::<PyValueError, _>("blank_card: expected object at top level")
        })?;
        Ok(Some(dict.clone()))
    }
}

/// Python wrapper for the typed Quillmark `Document`.
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
                    // Per the v0.81 binding-error contract, every parse
                    // exception carries the full `.diagnostics` list; the
                    // singular `.diagnostic` shim is the single entry.
                    let _ = exc.setattr("diagnostic", py_diag.clone());
                    let _ = exc.setattr("diagnostics", vec![py_diag]);
                }
            });
            py_err
        })?;
        Ok(PyDocument {
            inner: output.document,
            parse_warnings: output.warnings,
        })
    }

    /// Reconstruct a `Document` from its versioned storage DTO string.
    ///
    /// Raises `quillmark.ParseError` on malformed JSON, unknown `schema`, missing fields,
    /// or unparseable quill reference. `warnings` is always empty (DTO has no source text).
    #[staticmethod]
    fn from_json(json: &str) -> PyResult<Self> {
        let inner: Document = serde_json::from_str(json).map_err(|e| {
            PyErr::new::<crate::errors::ParseError, _>(format!("invalid storage DTO: {e}"))
        })?;
        Ok(PyDocument {
            inner,
            parse_warnings: Vec::new(),
        })
    }

    /// Like [`from_json`](PyDocument::from_json) but returns `None` instead of raising.
    /// Use to detect "is this a stored DTO or raw Markdown?" without exception control flow.
    #[staticmethod]
    fn try_from_json(json: &str) -> Option<Self> {
        let inner: Document = serde_json::from_str(json).ok()?;
        Some(PyDocument {
            inner,
            parse_warnings: Vec::new(),
        })
    }

    /// Read the `schema` version tag from a raw DTO string without a full parse, or `None`.
    /// Returns unknown future versions that `from_json` would reject — use this to distinguish
    /// "build too old" from "payload is corrupt" when [`from_json`](PyDocument::from_json) raises.
    #[staticmethod]
    fn schema_version_of(json: &str) -> Option<String> {
        quillmark_core::document::peek_schema_version(json)
    }

    /// Schema version this build writes. Compare against [`schema_version_of`](PyDocument::schema_version_of)
    /// to detect mismatches before calling [`from_json`](PyDocument::from_json).
    #[staticmethod]
    fn current_schema_version() -> &'static str {
        quillmark_core::document::SCHEMA_V0_82_0
    }

    /// Emit canonical Quillmark Markdown. Round-trip safe: re-parsing produces an equal `Document`.
    fn to_markdown(&self) -> String {
        self.inner.to_markdown()
    }

    /// Serialize to a versioned storage DTO string. Prefer this over `to_markdown` for persistence —
    /// the wire format is frozen per `schema` version. Parse-time `warnings` are not included.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner).map_err(|e| {
            PyErr::new::<crate::errors::QuillmarkError, _>(format!("serialization failed: {e}"))
        })
    }

    fn quill_ref(&self) -> String {
        self.inner.quill_reference().to_string()
    }

    /// Return a fresh `Document` handle with the same parsed state. Mutations are independent.
    fn clone(&self) -> Self {
        PyDocument {
            inner: self.inner.clone(),
            parse_warnings: self.parse_warnings.clone(),
        }
    }

    fn __copy__(&self) -> Self {
        self.clone()
    }

    fn __deepcopy__(&self, _memo: Bound<'_, PyAny>) -> Self {
        self.clone()
    }

    /// Structural equality — compares `main` and `cards` by value. Excludes `warnings`.
    fn equals(&self, other: PyRef<'_, PyDocument>) -> bool {
        self.inner == other.inner
    }

    fn __eq__(&self, other: Bound<'_, PyAny>) -> bool {
        match other.extract::<PyRef<'_, PyDocument>>() {
            Ok(other) => self.inner == other.inner,
            Err(_) => false,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Document(quill_ref={:?}, cards={})",
            self.inner.quill_reference().to_string(),
            self.inner.cards().len()
        )
    }

    /// Number of composable cards (excludes the main card). O(1).
    #[getter]
    fn card_count(&self) -> usize {
        self.inner.cards().len()
    }

    #[getter]
    fn warnings(&self) -> Vec<PyDiagnostic> {
        self.parse_warnings
            .iter()
            .map(|d| PyDiagnostic { inner: d.clone() })
            .collect()
    }

    /// Main card's global Markdown body (str, never None).
    #[getter]
    fn body(&self) -> &str {
        self.inner.main().body()
    }

    /// Main (entry) card as a dict with `kind`, `payload_items`, and `body`.
    #[getter]
    fn main<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        card_to_pydict(py, self.inner.main())
    }

    /// Ordered list of composable card blocks, each a dict with `kind`, `payload_items`, and `body`.
    #[getter]
    fn cards<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
        let mut result = Vec::new();
        for card in self.inner.cards() {
            result.push(card_to_pydict(py, card)?);
        }
        Ok(result)
    }

    /// Set a payload field on the main card. Clears any `!fill` marker.
    /// Raises `quillmark.EditError` if `name` does not match `[a-z_][a-z0-9_]*`.
    fn set_field(&mut self, name: &str, value: Bound<'_, PyAny>) -> PyResult<()> {
        let qv = py_to_quillvalue(&value)?;
        self.inner
            .main_mut()
            .set_field(name, qv)
            .map_err(convert_edit_error)
    }

    /// Set a payload field on the main card and mark it as `!fill`.
    fn set_fill(&mut self, name: &str, value: Bound<'_, PyAny>) -> PyResult<()> {
        let qv = py_to_quillvalue(&value)?;
        self.inner
            .main_mut()
            .set_fill(name, qv)
            .map_err(convert_edit_error)
    }

    /// Remove a payload field from the main card, returning the value or `None` if absent.
    /// Raises `quillmark.EditError` if `name` is invalid.
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

    /// Replace the quill reference string. Raises `ValueError` if `ref_str` is not a valid `QuillReference`.
    fn set_quill_ref(&mut self, ref_str: &str) -> PyResult<()> {
        let qr: quillmark_core::QuillReference = ref_str.parse().map_err(|e| {
            PyValueError::new_err(format!("invalid QuillReference '{}': {}", ref_str, e))
        })?;
        self.inner.set_quill_ref(qr);
        Ok(())
    }

    fn replace_body(&mut self, body: &str) {
        self.inner.main_mut().replace_body(body);
    }

    /// Append a card dict (`kind`, optional `fields`, optional `body`) to the card list.
    /// Raises `quillmark.EditError` if `card["kind"]` or any field name is invalid.
    fn push_card(&mut self, card: Bound<'_, PyAny>) -> PyResult<()> {
        let core_card = py_dict_to_card(&card)?;
        self.inner.push_card(core_card);
        Ok(())
    }

    /// Insert a card at `index` (must be in `0..=len`). Out-of-range raises `quillmark.EditError`.
    fn insert_card(&mut self, index: usize, card: Bound<'_, PyAny>) -> PyResult<()> {
        let core_card = py_dict_to_card(&card)?;
        self.inner
            .insert_card(index, core_card)
            .map_err(convert_edit_error)
    }

    /// Remove and return the card at `index`, or `None` if out of range.
    fn remove_card<'py>(
        &mut self,
        py: Python<'py>,
        index: usize,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        match self.inner.remove_card(index) {
            Some(card) => Ok(Some(card_to_pydict(py, &card)?)),
            None => Ok(None),
        }
    }

    /// Move the card at `from_idx` to `to_idx`. Both must be in `0..len`; raises `quillmark.EditError` otherwise.
    fn move_card(&mut self, from_idx: usize, to_idx: usize) -> PyResult<()> {
        self.inner
            .move_card(from_idx, to_idx)
            .map_err(convert_edit_error)
    }

    /// Replace the `$kind` of the card at `index`. Payload and body are untouched;
    /// schema-aware migration is the caller's responsibility.
    /// Raises `quillmark.EditError` if `index` is out of range or `new_kind` is invalid.
    fn set_card_kind(&mut self, index: usize, new_kind: &str) -> PyResult<()> {
        self.inner
            .set_card_kind(index, new_kind)
            .map_err(convert_edit_error)
    }

    /// Update a field on the card at `index`. Raises `quillmark.EditError` if `index` is out of range
    /// or `name` is invalid.
    fn update_card_field(
        &mut self,
        index: usize,
        name: &str,
        value: Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let qv = py_to_quillvalue(&value)?;
        let len = self.inner.cards().len();
        let card = self.inner.card_mut(index).ok_or_else(|| {
            convert_edit_error(quillmark_core::EditError::IndexOutOfRange { index, len })
        })?;
        card.set_field(name, qv).map_err(convert_edit_error)
    }

    /// Remove a field from the card at `index`, returning the value or `None` if absent.
    /// Raises `quillmark.EditError` if `index` is out of range or `name` is invalid.
    fn remove_card_field<'py>(
        &mut self,
        py: Python<'py>,
        index: usize,
        name: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let len = self.inner.cards().len();
        let card = self.inner.card_mut(index).ok_or_else(|| {
            convert_edit_error(quillmark_core::EditError::IndexOutOfRange { index, len })
        })?;
        match card.remove_field(name).map_err(convert_edit_error)? {
            Some(v) => quillvalue_to_py(py, &v),
            None => py.None().into_bound_py_any(py),
        }
    }

    /// Replace the body of the card at `index`. Raises `quillmark.EditError` if out of range.
    fn update_card_body(&mut self, index: usize, body: &str) -> PyResult<()> {
        let len = self.inner.cards().len();
        let card = self.inner.card_mut(index).ok_or_else(|| {
            convert_edit_error(quillmark_core::EditError::IndexOutOfRange { index, len })
        })?;
        card.replace_body(body);
        Ok(())
    }
}

#[pyclass(name = "RenderResult")]
pub struct PyRenderResult {
    pub(crate) inner: RenderResult,
}

#[pyclass(name = "RenderSession")]
pub struct PyRenderSession {
    pub(crate) inner: RenderSession,
    pub(crate) backend_id: String,
}

#[pymethods]
impl PyRenderSession {
    #[getter]
    fn page_count(&self) -> usize {
        self.inner.page_count()
    }

    /// The backend that produced this session (e.g. `"typst"`).
    #[getter]
    fn backend_id(&self) -> &str {
        &self.backend_id
    }

    /// Whether this session's backend supports canvas preview (currently `backend_id == "typst"`).
    #[getter]
    fn supports_canvas(&self) -> bool {
        self.backend_id == CANVAS_BACKEND_ID
    }

    /// Non-fatal diagnostics from `quill.open(...)` (e.g. version-compatibility shims).
    /// Also appended to `RenderResult.warnings` on each `render()` call.
    #[getter]
    fn warnings(&self) -> Vec<PyDiagnostic> {
        self.inner
            .warnings()
            .iter()
            .map(|d| PyDiagnostic { inner: d.clone() })
            .collect()
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

    /// Document-model path anchor (e.g. `"cards.indorsement[0].signature_block"`).
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

fn quillvalue_to_py<'py>(
    py: Python<'py>,
    value: &quillmark_core::QuillValue,
) -> PyResult<Bound<'py, PyAny>> {
    json_to_py(py, value.as_json())
}

fn card_to_pydict<'py>(
    py: Python<'py>,
    card: &quillmark_core::Card,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("kind", card.kind().unwrap_or(""))?;

    let items = pyo3::types::PyList::empty(py);
    for item in card.payload().items() {
        let entry = PyDict::new(py);
        match item {
            quillmark_core::PayloadItem::Field { key, value, fill } => {
                entry.set_item("type", "field")?;
                entry.set_item("key", key)?;
                entry.set_item("value", quillvalue_to_py(py, value)?)?;
                entry.set_item("fill", *fill)?;
            }
            quillmark_core::PayloadItem::Comment { text, inline } => {
                entry.set_item("type", "comment")?;
                entry.set_item("text", text)?;
                entry.set_item("inline", *inline)?;
            }
            quillmark_core::PayloadItem::Quill { .. }
            | quillmark_core::PayloadItem::Kind { .. }
            | quillmark_core::PayloadItem::Id { .. } => continue,
        }
        items.append(entry)?;
    }
    d.set_item("payload_items", items)?;

    d.set_item("body", card.body())?;
    Ok(d)
}

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
    let s = value.str()?.to_string();
    Ok(serde_json::Value::String(s))
}

fn py_dict_to_card(value: &Bound<'_, PyAny>) -> PyResult<quillmark_core::Card> {
    let dict = value
        .downcast::<PyDict>()
        .map_err(|_| PyValueError::new_err("card must be a dict with a 'kind' key"))?;

    let kind: String = dict
        .get_item("kind")?
        .ok_or_else(|| PyValueError::new_err("card dict must have a 'kind' key"))?
        .extract()?;

    let mut card = quillmark_core::Card::new(kind).map_err(convert_edit_error)?;

    if let Some(fields_val) = dict.get_item("fields")? {
        let fields_dict = fields_val
            .downcast::<PyDict>()
            .map_err(|_| PyValueError::new_err("card 'fields' must be a dict"))?;
        for (k, v) in fields_dict.iter() {
            let field_name: String = k.extract()?;
            let qv = py_to_quillvalue(&v)?;
            card.set_field(&field_name, qv)
                .map_err(convert_edit_error)?;
        }
    }

    if let Some(body_val) = dict.get_item("body")? {
        let body: String = body_val.extract()?;
        card.replace_body(body);
    }

    Ok(card)
}
