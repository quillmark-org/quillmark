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
use std::time::Instant;

use crate::enums::{PyOutputFormat, PySeverity};
use crate::errors::{convert_edit_error, convert_render_error, raise_with_diagnostics};

/// Backend identifier for the only canvas-capable backend today. Matches the
/// wasm binding's `CANVAS_BACKEND_ID`.
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
    /// The resolved backend identifier (e.g. `"typst"`). Mirrors WASM `backendId`.
    #[getter]
    fn backend_id(&self) -> String {
        self.inner.backend_id().to_string()
    }

    /// `True` iff the resolved backend is canvas-capable (currently `"typst"`).
    #[getter]
    fn supports_canvas(&self) -> bool {
        self.inner.backend_id() == CANVAS_BACKEND_ID
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

    /// Identity snapshot mirroring the `quill:` section of `Quill.yaml`,
    /// plus `supportedFormats`. Mirrors WASM `metadata`.
    #[getter]
    fn metadata<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let source = self.inner.source();
        let config = source.config();

        let dict = PyDict::new(py);
        dict.set_item("name", &config.name)?;
        dict.set_item("version", &config.version)?;
        dict.set_item("backend", &config.backend)?;
        dict.set_item("author", &config.author)?;
        dict.set_item("description", &config.description)?;

        let formats: Vec<PyOutputFormat> = self
            .inner
            .supported_formats()
            .iter()
            .map(|f| (*f).into())
            .collect();
        dict.set_item("supportedFormats", formats)?;

        // Forward unstructured keys declared under `quill:` (excluding the
        // typed ones already populated above).
        for (key, value) in source.metadata() {
            if matches!(
                key.as_str(),
                "name" | "backend" | "description" | "version" | "author"
            ) {
                continue;
            }
            if dict.contains(key)? {
                continue;
            }
            dict.set_item(key, quillvalue_to_py(py, value)?)?;
        }

        Ok(dict)
    }

    /// Document schema as a structured dict (matches the wasm `schema` shape).
    #[getter]
    fn schema<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let value = self.inner.source().config().schema();
        json_to_py(py, &value)
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

    #[pyo3(signature = (doc, format=None, ppi=None, pages=None, producer=None))]
    fn render(
        &self,
        doc: PyRef<'_, PyDocument>,
        format: Option<PyOutputFormat>,
        ppi: Option<f32>,
        pages: Option<Vec<usize>>,
        producer: Option<String>,
    ) -> PyResult<PyRenderResult> {
        let opts = quillmark_core::RenderOptions {
            output_format: format.map(OutputFormat::from),
            ppi,
            pages,
            producer,
        };
        let start = Instant::now();
        let mut result = self
            .inner
            .render(&doc.inner, &opts)
            .map_err(convert_render_error)?;
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        result
            .warnings
            .splice(0..0, doc.parse_warnings.iter().cloned());
        Ok(PyRenderResult {
            inner: result,
            render_time_ms: elapsed_ms,
        })
    }

    fn open(&self, doc: PyRef<'_, PyDocument>) -> PyResult<PyRenderSession> {
        let session = self.inner.open(&doc.inner).map_err(convert_render_error)?;
        Ok(PyRenderSession {
            inner: session,
            backend_id: self.inner.backend_id().to_string(),
        })
    }

    /// Schema-aware form view of `doc`.
    ///
    /// Returns a dict with keys `main`, `cards`, and `diagnostics`. Mirrors WASM `form`.
    fn form<'py>(
        &self,
        py: Python<'py>,
        doc: PyRef<'_, PyDocument>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let form = self.inner.form(&doc.inner);
        let json_value = serde_json::to_value(&form).map_err(|e| {
            PyValueError::new_err(format!("form: serialization failed: {e}"))
        })?;
        let py_obj = json_to_py(py, &json_value)?;
        let dict = py_obj.downcast::<PyDict>().map_err(|_| {
            PyValueError::new_err("form: expected object at top level")
        })?;
        Ok(dict.clone())
    }

    /// Blank form for the main card. Mirrors WASM `blankMain`.
    fn blank_main<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let card = self.inner.blank_main();
        let json_value = serde_json::to_value(&card).map_err(|e| {
            PyValueError::new_err(format!("blank_main: serialization failed: {e}"))
        })?;
        let py_obj = json_to_py(py, &json_value)?;
        let dict = py_obj.downcast::<PyDict>().map_err(|_| {
            PyValueError::new_err("blank_main: expected object at top level")
        })?;
        Ok(dict.clone())
    }

    /// Blank form for `card_kind`, or `None` if the kind is not declared.
    /// Mirrors WASM `blankCard`.
    fn blank_card<'py>(
        &self,
        py: Python<'py>,
        card_kind: &str,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        let Some(card) = self.inner.blank_card(card_kind) else {
            return Ok(None);
        };
        let json_value = serde_json::to_value(&card).map_err(|e| {
            PyValueError::new_err(format!("blank_card: serialization failed: {e}"))
        })?;
        let py_obj = json_to_py(py, &json_value)?;
        let dict = py_obj.downcast::<PyDict>().map_err(|_| {
            PyValueError::new_err("blank_card: expected object at top level")
        })?;
        Ok(Some(dict.clone()))
    }
}

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
            let diag = e.to_diagnostic();
            let message = diag.message.clone();
            raise_with_diagnostics(vec![diag], message)
        })?;
        Ok(PyDocument {
            inner: output.document,
            parse_warnings: output.warnings,
        })
    }

    /// Reconstruct a `Document` from its versioned storage DTO string.
    /// Raises `QuillmarkError` on malformed JSON, unknown `schema`, missing fields,
    /// or unparseable quill reference.
    #[staticmethod]
    fn from_json(json: &str) -> PyResult<Self> {
        let inner: Document = serde_json::from_str(json).map_err(|e| {
            let msg = format!("invalid storage DTO: {e}");
            raise_with_diagnostics(
                vec![quillmark_core::Diagnostic::new(
                    quillmark_core::Severity::Error,
                    msg.clone(),
                )],
                msg,
            )
        })?;
        Ok(PyDocument {
            inner,
            parse_warnings: Vec::new(),
        })
    }

    /// Like [`from_json`] but returns `None` instead of raising. Mirrors WASM `tryFromJson`.
    #[staticmethod]
    fn try_from_json(json: &str) -> Option<Self> {
        let inner: Document = serde_json::from_str(json).ok()?;
        Some(PyDocument {
            inner,
            parse_warnings: Vec::new(),
        })
    }

    /// Read the `schema` version tag from a raw DTO string without a full parse, or `None`.
    #[staticmethod]
    fn schema_version_of(json: &str) -> Option<String> {
        quillmark_core::document::peek_schema_version(json)
    }

    /// Schema version this build writes.
    #[staticmethod]
    fn current_schema_version() -> &'static str {
        quillmark_core::document::SCHEMA_V0_82_0
    }

    /// Emit canonical Quillmark Markdown. Round-trip safe.
    fn to_markdown(&self) -> String {
        self.inner.to_markdown()
    }

    /// Serialize to a versioned storage DTO string. Byte-deterministic per schema version.
    fn to_json(&self) -> String {
        serde_json::to_string(&self.inner).expect("Document serialization is infallible")
    }

    fn quill_ref(&self) -> String {
        self.inner.quill_reference().to_string()
    }

    /// Return a fresh `Document` handle with the same parsed state.
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

    /// Main card's global Markdown body.
    #[getter]
    fn body(&self) -> &str {
        self.inner.main().body()
    }

    /// Main (entry) card as a dict with `kind`, `payload_items`, `ext`, and `body`.
    #[getter]
    fn main<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        card_to_pydict(py, self.inner.main())
    }

    /// Ordered list of composable card blocks.
    #[getter]
    fn cards<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
        let mut result = Vec::new();
        for card in self.inner.cards() {
            result.push(card_to_pydict(py, card)?);
        }
        Ok(result)
    }

    fn set_field(&mut self, name: &str, value: Bound<'_, PyAny>) -> PyResult<()> {
        let qv = py_to_quillvalue(&value)?;
        self.inner
            .main_mut()
            .set_field(name, qv)
            .map_err(convert_edit_error)
    }

    fn set_fill(&mut self, name: &str, value: Bound<'_, PyAny>) -> PyResult<()> {
        let qv = py_to_quillvalue(&value)?;
        self.inner
            .main_mut()
            .set_fill(name, qv)
            .map_err(convert_edit_error)
    }

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

    fn push_card(&mut self, card: Bound<'_, PyAny>) -> PyResult<()> {
        let core_card = py_dict_to_card(&card)?;
        self.inner.push_card(core_card);
        Ok(())
    }

    fn insert_card(&mut self, index: usize, card: Bound<'_, PyAny>) -> PyResult<()> {
        let core_card = py_dict_to_card(&card)?;
        self.inner
            .insert_card(index, core_card)
            .map_err(convert_edit_error)
    }

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

    fn move_card(&mut self, from_idx: usize, to_idx: usize) -> PyResult<()> {
        self.inner
            .move_card(from_idx, to_idx)
            .map_err(convert_edit_error)
    }

    fn set_card_kind(&mut self, index: usize, new_kind: &str) -> PyResult<()> {
        self.inner
            .set_card_kind(index, new_kind)
            .map_err(convert_edit_error)
    }

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
    pub(crate) render_time_ms: f64,
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

    #[getter]
    fn backend_id(&self) -> &str {
        &self.backend_id
    }

    #[getter]
    fn supports_canvas(&self) -> bool {
        self.backend_id == CANVAS_BACKEND_ID
    }

    #[getter]
    fn warnings(&self) -> Vec<PyDiagnostic> {
        self.inner
            .warnings()
            .iter()
            .map(|d| PyDiagnostic { inner: d.clone() })
            .collect()
    }

    #[pyo3(signature = (format=None, ppi=None, pages=None, producer=None))]
    fn render(
        &self,
        format: Option<PyOutputFormat>,
        ppi: Option<f32>,
        pages: Option<Vec<usize>>,
        producer: Option<String>,
    ) -> PyResult<PyRenderResult> {
        let opts = quillmark::RenderOptions {
            output_format: format.map(OutputFormat::from),
            ppi,
            pages,
            producer,
        };
        let start = Instant::now();
        let result = self.inner.render(&opts).map_err(convert_render_error)?;
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        Ok(PyRenderResult {
            inner: result,
            render_time_ms: elapsed_ms,
        })
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
                format: a.output_format,
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
    fn format(&self) -> PyOutputFormat {
        self.inner.output_format.into()
    }

    /// Wall-clock time spent inside `render`, in milliseconds. Mirrors WASM `renderTimeMs`.
    #[getter]
    fn render_time_ms(&self) -> f64 {
        self.render_time_ms
    }
}

#[pyclass(name = "Artifact")]
#[derive(Clone)]
pub struct PyArtifact {
    pub(crate) inner: Vec<u8>,
    pub(crate) format: OutputFormat,
}

#[pymethods]
impl PyArtifact {
    #[getter]
    fn bytes(&self) -> Vec<u8> {
        self.inner.clone()
    }

    #[getter]
    fn format(&self) -> PyOutputFormat {
        self.format.into()
    }

    fn save(&self, path: String) -> PyResult<()> {
        std::fs::write(&path, &self.inner).map_err(|e| {
            let msg = format!("Failed to save artifact to {}: {}", path, e);
            raise_with_diagnostics(
                vec![quillmark_core::Diagnostic::new(
                    quillmark_core::Severity::Error,
                    msg.clone(),
                )],
                msg,
            )
        })
    }

    #[getter]
    fn mime_type(&self) -> &'static str {
        match self.format {
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
            quillmark_core::PayloadItem::Field {
                key, value, fill, ..
            } => {
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
            | quillmark_core::PayloadItem::Id { .. }
            | quillmark_core::PayloadItem::Ext { .. } => continue,
        }
        items.append(entry)?;
    }
    d.set_item("payload_items", items)?;

    match card.ext() {
        Some(ext_map) => {
            let ext_value = serde_json::Value::Object(ext_map.clone());
            d.set_item("ext", json_to_py(py, &ext_value)?)?;
        }
        None => d.set_item("ext", py.None())?,
    }

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
