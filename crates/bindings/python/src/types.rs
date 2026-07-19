use pyo3::conversion::IntoPyObjectExt;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::pycell::PyRef;
use pyo3::types::{PyDict, PyList};
use pyo3::Bound;

use quillmark::{
    quill_from_path, Diagnostic, Document, Location, OutputFormat, Quill, Quillmark, RenderResult,
};
use std::path::PathBuf;
use std::time::Instant;

use crate::enums::{PyOutputFormat, PySeverity};
use crate::errors::{
    convert_edit_error, convert_edit_errors, convert_render_error, raise_with_diagnostics,
};

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

    /// Render `doc` against `quill` in one shot, resolving `quill`'s backend on
    /// this engine. The default `output_format` falls back to the backend's
    /// first supported format. Raises `QuillmarkError` (`engine::backend_not_found`)
    /// when the backend is not registered. Mirrors WASM `Engine.render`.
    #[pyo3(signature = (quill, doc, format=None, ppi=None, pages=None, producer=None, regions=false))]
    #[allow(clippy::too_many_arguments)] // kwargs mirror RenderOptions 1:1; the signature IS the Python API
    fn render(
        &self,
        quill: &PyQuill,
        doc: PyRef<'_, PyDocument>,
        format: Option<PyOutputFormat>,
        ppi: Option<f32>,
        pages: Option<Vec<usize>>,
        producer: Option<String>,
        regions: bool,
    ) -> PyResult<PyRenderResult> {
        let opts = quillmark_core::RenderOptions {
            output_format: format.map(OutputFormat::from),
            ppi,
            pages,
            producer,
            regions,
        };
        let start = Instant::now();
        let mut result = self
            .inner
            .render(&quill.inner, &doc.inner, &opts)
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

    /// The output formats `quill`'s backend can emit. Raises `QuillmarkError`
    /// (`engine::backend_not_found`) for an unregistered backend. Mirrors WASM
    /// `Engine.supportedFormats`.
    fn supported_formats(&self, quill: &PyQuill) -> PyResult<Vec<PyOutputFormat>> {
        Ok(self
            .inner
            .supported_formats(&quill.inner)
            .map_err(convert_render_error)?
            .iter()
            .map(|f| (*f).into())
            .collect())
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
    /// Portable, declarative config data. The declared backend is
    /// resolved later, at render time, by the `Quillmark` engine — never here.
    pub(crate) inner: Quill,
}

#[pymethods]
impl PyQuill {
    /// Load a quill from a filesystem directory. Pure config load — no backend,
    /// no engine; the declared backend is resolved at render time by a
    /// `Quillmark` engine. Mirrors WASM `Quill.fromTree`/Rust `quill_from_path`.
    #[staticmethod]
    fn from_path(path: PathBuf) -> PyResult<PyQuill> {
        let quill = quill_from_path(&path).map_err(convert_render_error)?;
        Ok(PyQuill { inner: quill })
    }

    /// The declared backend identifier (e.g. `"typst"`). Mirrors WASM `backendId`.
    #[getter]
    fn backend_id(&self) -> String {
        self.inner.backend_id().to_string()
    }

    /// Bind this quill's schema to `doc` for typed writes — the documented front
    /// door, mirroring core `quill.writer(&mut doc)` and WASM `quill.writer(doc)`.
    /// The quill owns the schema, so it is the factory. See [`PyWriter`] for the
    /// re-borrow/ephemerality contract.
    fn writer(slf: Py<Self>, doc: Py<PyDocument>) -> PyWriter {
        PyWriter { quill: slf, doc }
    }

    /// Bind this quill's schema to `doc` for interpreted reads — the read twin of
    /// `writer`, mirroring core `quill.view(&doc)` and WASM `quill.view(doc)`.
    /// Each field reads by its declared type (a richtext field to markdown, every
    /// other type verbatim) with schema authority. See [`PyView`] for the
    /// re-borrow/ephemerality contract.
    fn view(slf: Py<Self>, doc: Py<PyDocument>) -> PyView {
        PyView { quill: slf, doc }
    }

    #[getter]
    fn quill_ref(&self) -> String {
        let source = &self.inner;
        let version = source
            .metadata()
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0");
        format!("{}@{}", source.name(), version)
    }

    /// Identity snapshot mirroring the `quill:` section of `Quill.yaml`.
    /// A pure config read — it never resolves a backend and never raises for
    /// an unregistered one. Capability lives on the engine: read
    /// `Quillmark.supported_formats(quill)`. Mirrors WASM `metadata`.
    #[getter]
    fn metadata<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let source = &self.inner;
        let config = source.config();

        let dict = PyDict::new(py);
        dict.set_item("name", &config.name)?;
        dict.set_item("version", &config.version)?;
        dict.set_item("backend", &config.backend)?;
        dict.set_item("author", &config.author)?;
        dict.set_item("description", &config.description)?;

        // Forward unstructured keys declared under `quill:` (excluding the
        // typed ones already populated above).
        for (key, value) in source.metadata() {
            if quillmark_core::STANDARD_METADATA_KEYS.contains(&key.as_str()) {
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
        let value = self.inner.config().schema();
        json_to_py(py, &value)
    }

    #[getter]
    fn blueprint(&self) -> String {
        self.inner.config().blueprint()
    }

    /// Validate `doc` against this quill's schema, returning a list of
    /// diagnostic dicts (empty when the document is valid). Mirrors WASM
    /// `validate`.
    ///
    /// Forwards the canonical `validation::*` diagnostics — same `code`,
    /// `path`, and `hint` the engine emits — including the non-fatal
    /// `validation::must_fill` warning for each `!must_fill` marker left in
    /// the document. Field values, defaults, and order are not part of this
    /// surface: read them from the `Document` payload and `Quill.schema`
    /// (schema key order is display order).
    fn validate<'py>(
        &self,
        py: Python<'py>,
        doc: PyRef<'_, PyDocument>,
    ) -> PyResult<Bound<'py, PyList>> {
        let diags = self.inner.validate(&doc.inner);
        let json_value = serde_json::to_value(&diags)
            .map_err(|e| PyValueError::new_err(format!("validate: serialization failed: {e}")))?;
        let py_obj = json_to_py(py, &json_value)?;
        let list = py_obj
            .downcast::<PyList>()
            .map_err(|_| PyValueError::new_err("validate: expected a list at top level"))?;
        Ok(list.clone())
    }

    /// Seed a starter `Document` from the schema — the main card plus one
    /// instance of each composable card kind, each committing its fields'
    /// `example` values and leaving every other field absent (interpolated at
    /// render). Illustration-first: a field with both an `example` and a
    /// `default` renders its example. Mirrors WASM `seedDocument`.
    fn seed_document(&self) -> PyDocument {
        PyDocument {
            inner: self.inner.seed_document(),
            parse_warnings: Vec::new(),
        }
    }

    /// Seed a starter main card (carries `$quill`) from the schema — the
    /// `$kind: main` card of `seed_document()` in isolation, as a dict (same
    /// shape as `Document.main`). Mirrors WASM `seedMain`.
    fn seed_main<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        card_to_pydict(py, &self.inner.seed_main())
    }

    /// Seed a starter composable card of the given kind (carries `$kind`),
    /// layering an optional per-kind seed `overlay` over the schema-example
    /// base (`overlay › example › absent`); `None` if `card_kind` is not
    /// declared. Returns the same dict shape as `Document.cards` /
    /// `remove_card`. Pass `document.main["seed"][card_kind]` as `overlay` so a
    /// card added to a template-derived document inherits its curated starting
    /// values; omit it for the bare schema seed. Mirrors WASM `seedCard`.
    #[pyo3(signature = (card_kind, overlay=None))]
    fn seed_card<'py>(
        &self,
        py: Python<'py>,
        card_kind: &str,
        overlay: Option<Bound<'_, PyAny>>,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        let overlay = match overlay {
            Some(value) => quillmark_core::SeedOverlay::from_json(&py_to_json(&value)?),
            None => None,
        };
        match self.inner.seed_card(card_kind, overlay.as_ref()) {
            Some(card) => Ok(Some(card_to_pydict(py, &card)?)),
            None => Ok(None),
        }
    }
}

#[pyclass(name = "Document")]
pub struct PyDocument {
    pub(crate) inner: Document,
    pub(crate) parse_warnings: Vec<quillmark_core::Diagnostic>,
}

#[pymethods]
impl PyDocument {
    /// `Document(quill_ref)` — a blank document: a main card carrying only
    /// `$quill`, an empty body, and no composable cards. The programmatic
    /// blank canvas: absent fields resolve at render time (`default`, else
    /// type-empty zero), so nothing the caller did not set reaches the
    /// output. For an example-filled starter use `Quill.seed_document()`.
    /// Raises `ValueError` on an invalid quill reference. Mirrors WASM
    /// `new Document(quillRef)`.
    #[new]
    fn new(quill_ref: &str) -> PyResult<Self> {
        let qr: quillmark_core::QuillReference = quill_ref.parse().map_err(|e| {
            PyValueError::new_err(format!("invalid QuillReference '{}': {}", quill_ref, e))
        })?;
        Ok(PyDocument {
            inner: Document::new(qr),
            parse_warnings: Vec::new(),
        })
    }

    #[staticmethod]
    fn from_markdown(markdown: &str) -> PyResult<Self> {
        let output = Document::parse(markdown).map_err(|e| {
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
        quillmark_core::document::SCHEMA_V0_93_0
    }

    /// Canonical card-yaml authoring rules — the core text every surface shows.
    /// Mirrors WASM `Document.formatRules`. Cache it; the value never changes.
    #[staticmethod]
    fn format_rules() -> &'static str {
        quillmark_core::document::FORMAT_RULES
    }

    /// Authoring-ergonomics header introducing a blueprint to an LLM/MCP
    /// consumer for `quill_name`. Mirrors WASM `Document.blueprintInstruction`.
    #[staticmethod]
    fn blueprint_instruction(quill_name: &str) -> String {
        quillmark_core::document::blueprint_instruction(quill_name)
    }

    /// The canonical `$quill` reference grammar as author-facing text — matches
    /// the `hint` on `parse::invalid_quill_reference`. Mirrors WASM
    /// `Document.quillRefHint`. Cache it; the value never changes.
    #[staticmethod]
    fn quill_ref_hint() -> &'static str {
        quillmark_core::quill_ref_hint()
    }

    /// Emit canonical Quillmark Markdown. Round-trip safe.
    fn to_markdown(&self) -> String {
        self.inner.to_markdown()
    }

    /// Serialize to a versioned storage DTO string. Byte-deterministic per schema version.
    fn to_json(&self) -> String {
        serde_json::to_string(&self.inner).expect("Document serialization is infallible")
    }

    #[getter]
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

    /// Main card's global body as canonical Content-JSON — the source-of-truth
    /// content model (a content dict, `{text, lines, marks, islands}`). The
    /// quill-free total-read snapshot; for the markdown projection use
    /// `quill.view(doc).get_body()`.
    #[getter]
    fn body<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let wire = quillmark_core::CardWire::from(self.inner.main());
        json_to_py(py, &wire.body)
    }

    /// Main (entry) card as a dict with `kind`, `quill`, `id`, `payload_items`,
    /// `ext`, `seed`, and `body`.
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


    /// Remove a payload field, returning its previous value (or `None` when
    /// absent). `card` selects the target: `None` the main card, `Some(i)` the
    /// composable card at `i` (out-of-range raises `IndexOutOfRange`). `remove`
    /// has no lane — one verb serves every write path.
    #[pyo3(signature = (name, card=None))]
    fn remove_field<'py>(
        &mut self,
        py: Python<'py>,
        name: &str,
        card: Option<usize>,
    ) -> PyResult<Bound<'py, PyAny>> {
        match self
            .addr_card_mut(card)?
            .remove_field(name)
            .map_err(convert_edit_error)?
        {
            Some(v) => quillvalue_to_py(py, &v),
            None => py.None().into_bound_py_any(py),
        }
    }

    /// Replace the opaque `$ext` map on a card. `value` must be a dict; raises
    /// `ValueError` otherwise. `card` selects the target: `None` the main card,
    /// `Some(i)` the composable card at `i` (out-of-range raises `IndexOutOfRange`).
    /// `$ext` carries out-of-band consumer state and never reaches the rendered
    /// output; pass `{}` for an explicit empty `$ext`. Prefer `store_ext_namespace`
    /// to write one slot without clobbering sibling consumers'.
    #[pyo3(signature = (value, card=None))]
    fn store_ext(&mut self, value: Bound<'_, PyAny>, card: Option<usize>) -> PyResult<()> {
        let map = py_to_object(&value, "store_ext")?;
        self.addr_card_mut(card)?
            .store_ext(map)
            .map_err(convert_edit_error)?;
        Ok(())
    }

    /// Remove the `$ext` map from a card *entirely*, returning the previous map or
    /// `None`. `card` selects the target (`None` main, `Some(i)` composable,
    /// out-of-range raises). A blunt escape hatch that discards every namespace at
    /// once — prefer `remove_ext_namespace` to clear only your own slot.
    #[pyo3(signature = (card=None))]
    fn remove_ext<'py>(
        &mut self,
        py: Python<'py>,
        card: Option<usize>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let prev = self.addr_card_mut(card)?.remove_ext();
        ext_map_to_py(py, prev)
    }

    /// Merge `value` into a card's `$ext` map under `namespace`, creating the map
    /// when absent and replacing any existing value at that key. Sibling
    /// namespaces are preserved so independent consumers don't clobber each other.
    /// `card` selects the target (`None` main, `Some(i)` composable, out-of-range
    /// raises).
    #[pyo3(signature = (namespace, value, card=None))]
    fn store_ext_namespace(
        &mut self,
        namespace: &str,
        value: Bound<'_, PyAny>,
        card: Option<usize>,
    ) -> PyResult<()> {
        let json = py_to_json(&value)?;
        self.addr_card_mut(card)?
            .store_ext_namespace(namespace, json)
            .map_err(convert_edit_error)?;
        Ok(())
    }

    /// Remove `namespace` from a card's `$ext` map, returning the value stored
    /// there or `None`; sibling namespaces survive, and the `$ext` entry drops
    /// entirely once its last namespace is removed (not left as `$ext: {}`).
    /// `card` selects the target (`None` main, `Some(i)` composable, out-of-range
    /// raises).
    #[pyo3(signature = (namespace, card=None))]
    fn remove_ext_namespace<'py>(
        &mut self,
        py: Python<'py>,
        namespace: &str,
        card: Option<usize>,
    ) -> PyResult<Bound<'py, PyAny>> {
        ext_value_to_py(py, self.addr_card_mut(card)?.remove_ext_namespace(namespace))
    }

    /// Merge a card-kind's seed `overlay` into the main card's `$seed` map
    /// under `card_kind`, preserving sibling kinds. Sets the starting values
    /// new cards of that kind spawn with.
    fn store_seed_namespace(&mut self, card_kind: &str, overlay: Bound<'_, PyAny>) -> PyResult<()> {
        let json = py_to_json(&overlay)?;
        self.inner
            .main_mut()
            .store_seed_namespace(card_kind, json)
            .map_err(convert_edit_error)?;
        Ok(())
    }

    /// Remove `card_kind` from the main card's `$seed` map, returning its
    /// overlay or `None`; drops `$seed` entirely once empty. Sibling kinds
    /// survive.
    fn remove_seed_namespace<'py>(
        &mut self,
        py: Python<'py>,
        card_kind: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        ext_value_to_py(py, self.inner.main_mut().remove_seed_namespace(card_kind))
    }

    fn set_quill_ref(&mut self, ref_str: &str) -> PyResult<()> {
        let qr: quillmark_core::QuillReference = ref_str.parse().map_err(|e| {
            PyValueError::new_err(format!("invalid QuillReference '{}': {}", ref_str, e))
        })?;
        self.inner.set_quill_ref(qr);
        Ok(())
    }

    /// Build a fresh `Card` dict from a kind and a flat field mapping — the
    /// ergonomic constructor for `insert_card`. `fields` maps field name → value
    /// (each becomes a card field, in insertion order); `body` defaults to `""`.
    /// Kind validity is checked by `insert_card`, not here. Mirrors WASM
    /// `Document.makeCard`.
    #[staticmethod]
    #[pyo3(signature = (kind, fields=None, body=None))]
    fn make_card<'py>(
        py: Python<'py>,
        kind: String,
        fields: Option<Bound<'_, PyDict>>,
        body: Option<String>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let mut payload_items = Vec::new();
        if let Some(fields) = fields {
            for (k, v) in fields.iter() {
                let key: String = k.extract()?;
                payload_items.push(quillmark_core::PayloadItemWire::Field {
                    key,
                    value: py_to_json(&v)?,
                    fill: false,
                    nested_fills: Vec::new(),
                });
            }
        }
        let wire = quillmark_core::CardWire {
            kind,
            quill: None,
            id: None,
            ext: None,
            seed: None,
            payload_items,
            // The `body` argument is markdown; `Card::try_from` imports it to the
            // content, and `card_to_pydict` re-emits the content body.
            body: serde_json::Value::String(body.unwrap_or_default()),
        };
        let card = quillmark_core::Card::try_from(wire)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        card_to_pydict(py, &card)
    }

    /// Place a composable card. `at` picks the position: `None` appends, `Some(i)`
    /// inserts at index `i` (`0..=card_count`; out of range raises
    /// `IndexOutOfRange`). `card` is a `Card` dict — from `make_card`, `cards`,
    /// `remove_card`, or `seed_card`. The one insertion verb per lane, folding
    /// core's `push_card` + `insert_card`. Mirrors WASM `insertCard(card, at?)`.
    #[pyo3(signature = (card, at=None))]
    fn insert_card(&mut self, card: Bound<'_, PyAny>, at: Option<usize>) -> PyResult<()> {
        let core_card = py_dict_to_card(&card)?;
        match at {
            None => self.inner.push_card(core_card),
            Some(index) => self.inner.insert_card(index, core_card),
        }
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

}

impl PyDocument {
    /// Resolve a mutable composable card by index, raising the same
    /// `IndexOutOfRange` error the other card mutators raise.
    fn card_mut_or_raise(&mut self, index: usize) -> PyResult<&mut quillmark_core::Card> {
        let len = self.inner.cards().len();
        self.inner.card_mut(index).ok_or_else(|| {
            convert_edit_error(quillmark_core::EditError::IndexOutOfRange { index, len })
        })
    }

    /// Resolve the card a `card=` selector targets: the main card when `None`,
    /// else the composable card at that index (out-of-range raises). The shared
    /// address axis of the `card=`-parametrized `$ext` / `remove_field` verbs.
    fn addr_card_mut(&mut self, card: Option<usize>) -> PyResult<&mut quillmark_core::Card> {
        match card {
            None => Ok(self.inner.main_mut()),
            Some(index) => self.card_mut_or_raise(index),
        }
    }
}

/// A `Document` bound to its `Quill` for typed writes — the schema-bound writer,
/// from `Quill.writer(doc)`. Speaks names, values, and markdown; a consumer
/// here never meets an address, a content dict, or a delta. It holds both objects
/// by reference and re-borrows them per call (pyo3 objects carry no lifetime, so
/// unlike core's `TypedWriter` it cannot keep the borrow) — so it is ephemeral by
/// convention: bind, write, discard. Mirrors WASM `quill.writer(doc)`.
#[pyclass(name = "Writer")]
pub struct PyWriter {
    quill: Py<PyQuill>,
    doc: Py<PyDocument>,
}

#[pymethods]
impl PyWriter {
    /// The bound document — the same object passed in, mutated in place.
    #[getter]
    fn document(&self, py: Python<'_>) -> Py<PyDocument> {
        self.doc.clone_ref(py)
    }

    /// Typed-commit one main-card field (strict coerce, mismatch raises now).
    /// Raises `[EditError::UnknownField]` for a name the schema does not declare.
    fn set(&self, py: Python<'_>, name: &str, value: Bound<'_, PyAny>) -> PyResult<()> {
        let qv = py_to_quillvalue(&value)?;
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        quill
            .inner
            .writer(&mut doc.inner)
            .set(name, qv)
            .map_err(convert_edit_error)
    }

    /// Typed-commit several main-card fields atomically — nothing is applied on
    /// error, and the raised `QuillmarkError` carries one diagnostic per offending
    /// field (an `[EditError::UnknownField]` per undeclared name).
    fn set_all(&self, py: Python<'_>, fields: Bound<'_, PyDict>) -> PyResult<()> {
        let batch = pydict_to_field_batch(&fields)?;
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        quill
            .inner
            .writer(&mut doc.inner)
            .set_all(batch)
            .map_err(convert_edit_errors)
    }

    /// Set the main body from markdown (edit semantics: anchors rebase),
    /// discarding the delta — the receipt-free body write.
    fn set_body(&self, py: Python<'_>, markdown: &str) -> PyResult<()> {
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        quill
            .inner
            .writer(&mut doc.inner)
            .set_body(markdown)
            .map_err(convert_edit_error)
    }

    /// Revise the richtext main-card field `name` from markdown — typed *and*
    /// anchor-preserving. Surviving anchors rebase, then the diffed result is
    /// schema-conformed (a `richtext(inline)` field rejects a multi-block result
    /// with `[EditError::FieldRichtextNotInline]`). Raises
    /// `[EditError::UnknownField]` for a name the schema does not declare. The
    /// only field write that preserves a JS editor's anchors on a shared document
    /// (`set` cold-imports). The text `Delta` is discarded — the position-mapping
    /// receipt is an editor concern, and that lane is WASM-only; core and WASM
    /// return it.
    fn revise_field(&self, py: Python<'_>, name: &str, markdown: &str) -> PyResult<()> {
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        quill
            .inner
            .writer(&mut doc.inner)
            .revise_field(name, markdown)
            .map(|_| ())
            .map_err(convert_edit_error)
    }

    /// Build a composable card of `kind`, typed-commit `fields` onto it, set its
    /// body from optional markdown, and place it — the fused `make_card` + typed
    /// commit + insertion. `at` picks the position: `None` appends, `Some(i)`
    /// inserts at index `i`, so a positioned typed insert is one atomic call
    /// rather than `add_card` + `move_card`. Transactional: a rejected field
    /// (raises a per-field diagnostic bundle) or an invalid kind/body/position
    /// leaves the document untouched.
    #[pyo3(signature = (kind, fields=None, body=None, at=None))]
    fn add_card(
        &self,
        py: Python<'_>,
        kind: &str,
        fields: Option<Bound<'_, PyDict>>,
        body: Option<String>,
        at: Option<usize>,
    ) -> PyResult<()> {
        let batch = match fields {
            Some(f) => pydict_to_field_batch(&f)?,
            None => Vec::new(),
        };
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        quill
            .inner
            .writer(&mut doc.inner)
            .add_card(kind, batch, body.as_deref(), at)
            .map_err(convert_edit_errors)
    }

    /// Remove the composable card at `index`, returning it as a dict (or `None`
    /// if the index is out of range) — the writer spelling of
    /// `Document.remove_card`.
    fn remove_card<'py>(
        &self,
        py: Python<'py>,
        index: usize,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        let mut doc = self.doc.borrow_mut(py);
        match doc.inner.remove_card(index) {
            Some(card) => Ok(Some(card_to_pydict(py, &card)?)),
            None => Ok(None),
        }
    }

    /// A `CardWriter` for the composable card at `index`. The index is checked
    /// lazily at the write, so this never raises. The cursor is ephemeral — a
    /// `remove_card`/`add_card` between binding and writing silently retargets
    /// it; for durable addressing stamp `$id` and re-resolve at write time.
    fn card(&self, py: Python<'_>, index: usize) -> PyCardWriter {
        PyCardWriter {
            quill: self.quill.clone_ref(py),
            doc: self.doc.clone_ref(py),
            index,
        }
    }
}

/// A composable card bound to its `Quill` for typed writes, from `Writer.card`.
/// Same verbs as `Writer`, targeting the card at its bound index; each write
/// raises `[EditError::IndexOutOfRange]` if that index is out of range.
#[pyclass(name = "CardWriter")]
pub struct PyCardWriter {
    quill: Py<PyQuill>,
    doc: Py<PyDocument>,
    index: usize,
}

#[pymethods]
impl PyCardWriter {
    /// The bound card index.
    #[getter]
    fn index(&self) -> usize {
        self.index
    }

    /// The bound card's `$kind`, or `None` when it carries none. Raises
    /// `[EditError::IndexOutOfRange]` if the bound index is out of range. Mirrors
    /// core `CardWriter::kind()` / WASM `writer.card(i).kind`.
    #[getter]
    fn kind(&self, py: Python<'_>) -> PyResult<Option<String>> {
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        let mut writer = quill.inner.writer(&mut doc.inner);
        let card = writer.card(self.index).map_err(convert_edit_error)?;
        Ok(card.kind().map(|k| k.to_string()))
    }

    /// Typed-commit one field on this card, resolving its type from the card's
    /// `$kind` schema. Raises `[EditError::UnknownField]` for an undeclared name
    /// and `[EditError::IndexOutOfRange]` for a bad index.
    fn set(&self, py: Python<'_>, name: &str, value: Bound<'_, PyAny>) -> PyResult<()> {
        let qv = py_to_quillvalue(&value)?;
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        quill
            .inner
            .writer(&mut doc.inner)
            .card(self.index)
            .map_err(convert_edit_error)?
            .set(name, qv)
            .map_err(convert_edit_error)
    }

    /// Typed-commit several fields on this card atomically — same per-field
    /// diagnostic bundle as `Writer.set_all`.
    fn set_all(&self, py: Python<'_>, fields: Bound<'_, PyDict>) -> PyResult<()> {
        let batch = pydict_to_field_batch(&fields)?;
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        let mut writer = quill.inner.writer(&mut doc.inner);
        writer
            .card(self.index)
            .map_err(convert_edit_error)?
            .set_all(batch)
            .map_err(convert_edit_errors)
    }

    /// Set this card's body from markdown (edit semantics), discarding the delta.
    fn set_body(&self, py: Python<'_>, markdown: &str) -> PyResult<()> {
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        quill
            .inner
            .writer(&mut doc.inner)
            .card(self.index)
            .map_err(convert_edit_error)?
            .set_body(markdown)
            .map_err(convert_edit_error)
    }

    /// Revise the richtext field `name` on this card from markdown — typed *and*
    /// anchor-preserving; the card twin of `Writer.revise_field`. Raises
    /// `[EditError::UnknownField]` for an undeclared name and
    /// `[EditError::IndexOutOfRange]` for a bad index. The `Delta` is discarded
    /// (see `Writer.revise_field`).
    fn revise_field(&self, py: Python<'_>, name: &str, markdown: &str) -> PyResult<()> {
        let quill = self.quill.borrow(py);
        let mut doc = self.doc.borrow_mut(py);
        let mut writer = quill.inner.writer(&mut doc.inner);
        writer
            .card(self.index)
            .map_err(convert_edit_error)?
            .revise_field(name, markdown)
            .map(|_| ())
            .map_err(convert_edit_error)
    }
}

/// A `Document` bound to its `Quill` for interpreted reads — the schema-plane
/// read view, from `Quill.view(doc)` and the read twin of `Writer`. One `get`
/// reads each field by its declared type: a richtext field to its markdown
/// projection, a plaintext field to its literal text, every other type its
/// canonical value verbatim. The schema authority is the point: a name the schema
/// does not declare raises `[EditError::UnknownField]` (a typo, as on the write
/// side) rather than reading back `None`, and a content field holding an
/// undecodable value raises `[EditError::FieldRichtextDecode]`. This is the field
/// read surface — `Document` carries no quill-free field read. Holds both objects
/// by reference and re-borrows them per call (pyo3 objects carry no lifetime), so
/// it is ephemeral by convention: bind, read, discard. Mirrors WASM
/// `quill.view(doc)`.
#[pyclass(name = "View")]
pub struct PyView {
    quill: Py<PyQuill>,
    doc: Py<PyDocument>,
}

#[pymethods]
impl PyView {
    /// The bound document — the same object passed in.
    #[getter]
    fn document(&self, py: Python<'_>) -> Py<PyDocument> {
        self.doc.clone_ref(py)
    }

    /// Read a main-card field, interpreted by its declared type: a richtext field
    /// to its markdown projection (a `str`), every other type its canonical value
    /// (scalar/list/dict), or `None` when the field is absent. Raises
    /// `[EditError::UnknownField]` for a name the schema does not declare (a typo,
    /// as on the write side) and `[EditError::FieldRichtextDecode]` for a richtext
    /// field holding a value that does not decode.
    fn get<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        let quill = self.quill.borrow(py);
        let doc = self.doc.borrow(py);
        let read = quill
            .inner
            .view(&doc.inner)
            .get(name)
            .map_err(convert_edit_error)?;
        read_value_to_py(py, read)
    }

    /// The main body's markdown — the quill-free body read (a body's type is a
    /// format fact, not a schema fact), never raising.
    fn get_body(&self, py: Python<'_>) -> String {
        let doc = self.doc.borrow(py);
        doc.inner.main().body_markdown()
    }

    /// A `CardView` for the composable card at `index`. The index is checked
    /// lazily at the read, so this never raises. The cursor is ephemeral — a
    /// `remove_card`/`add_card` between binding and reading silently retargets it.
    fn card(&self, py: Python<'_>, index: usize) -> PyCardView {
        PyCardView {
            quill: self.quill.clone_ref(py),
            doc: self.doc.clone_ref(py),
            index,
        }
    }
}

/// A composable card bound to its `Quill` for interpreted reads, from
/// `View.card`. Same verbs as `View`, reading the card at its bound index; each
/// read raises `[EditError::IndexOutOfRange]` if that index is out of range.
#[pyclass(name = "CardView")]
pub struct PyCardView {
    quill: Py<PyQuill>,
    doc: Py<PyDocument>,
    index: usize,
}

#[pymethods]
impl PyCardView {
    /// The bound card index.
    #[getter]
    fn index(&self) -> usize {
        self.index
    }

    /// The bound card's `$kind`, or `None` when it carries none. Raises
    /// `[EditError::IndexOutOfRange]` if the bound index is out of range.
    #[getter]
    fn kind(&self, py: Python<'_>) -> PyResult<Option<String>> {
        let quill = self.quill.borrow(py);
        let doc = self.doc.borrow(py);
        let view = quill.inner.view(&doc.inner);
        let card = view.card(self.index).map_err(convert_edit_error)?;
        Ok(card.kind().map(|k| k.to_string()))
    }

    /// Read a field on this card, interpreted by its declared type — the
    /// card-indexed twin of `View.get`. Raises `[EditError::UnknownField]` for an
    /// undeclared name and `[EditError::IndexOutOfRange]` for a bad index.
    fn get<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        let quill = self.quill.borrow(py);
        let doc = self.doc.borrow(py);
        let view = quill.inner.view(&doc.inner);
        let read = view
            .card(self.index)
            .map_err(convert_edit_error)?
            .get(name)
            .map_err(convert_edit_error)?;
        read_value_to_py(py, read)
    }

    /// This card's body markdown — the card twin of `View.get_body`. Raises
    /// `[EditError::IndexOutOfRange]` if the bound index is out of range.
    fn get_body(&self, py: Python<'_>) -> PyResult<String> {
        let doc = self.doc.borrow(py);
        let card = doc
            .inner
            .card(self.index)
            .ok_or_else(|| {
                convert_edit_error(quillmark_core::EditError::IndexOutOfRange {
                    index: self.index,
                    len: doc.inner.cards().len(),
                })
            })?;
        Ok(card.body_markdown())
    }
}

#[pyclass(name = "RenderResult")]
pub struct PyRenderResult {
    pub(crate) inner: RenderResult,
    pub(crate) render_time_ms: f64,
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

    /// Schema-field geometry sidecar — populated only when `render(...,
    /// regions=True)` requested it; empty otherwise. One dict per entry:
    /// `{"field": str, "page": int, "rect": [x0, y0, x1, y1], "span":
    /// [start, end] | None}` with rect in PDF points, bottom-left origin, page
    /// indices document-space. Content fields carry one entry per **segment**
    /// (paragraph, heading, code fence) and page, each `span` the covered USV
    /// content range; widgets and scalar reference sites carry `span: None`. A
    /// field may still appear more than once; group by `field` and union the
    /// segment rects for the whole-field box.
    #[getter]
    fn regions<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
        self.inner
            .regions
            .iter()
            .map(|r| {
                let d = PyDict::new(py);
                d.set_item("field", &r.field)?;
                d.set_item("page", r.page)?;
                d.set_item("rect", r.rect.to_vec())?;
                d.set_item("span", r.span.map(|s| s.to_vec()))?;
                Ok(d)
            })
            .collect()
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
        self.format.mime_type()
    }
}

#[pyclass(name = "Diagnostic")]
#[derive(Clone)]
pub struct PyDiagnostic {
    pub(crate) inner: Diagnostic,
}

#[pymethods]
impl PyDiagnostic {
    /// Canonical pretty-printed diagnostic text — the same rendering the CLI
    /// and WASM (`Document.formatDiagnostic`) emit, so a diagnostic reads
    /// identically no matter which surface shows it.
    fn __str__(&self) -> String {
        self.inner.fmt_pretty()
    }

    fn __repr__(&self) -> String {
        format!(
            "Diagnostic(severity={:?}, code={:?}, message={:?})",
            self.inner.severity, self.inner.code, self.inner.message,
        )
    }

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

/// Flatten a [`ReadValue`](quillmark_core::ReadValue) into a Python object for
/// the `View.get` surfaces: a richtext projection becomes a `str`, a canonical
/// value its scalar/list/dict shape, and an absent field `None`. The schema-plane
/// twin of `quillvalue_to_py`, which knows only the transport shape.
fn read_value_to_py<'py>(
    py: Python<'py>,
    read: Option<quillmark_core::ReadValue>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    match read {
        None => Ok(None),
        // Both content projections flatten to a Python str at the boundary.
        Some(quillmark_core::ReadValue::Markdown(s))
        | Some(quillmark_core::ReadValue::Plaintext(s)) => Ok(Some(s.into_bound_py_any(py)?)),
        Some(quillmark_core::ReadValue::Value(v)) => Ok(Some(quillvalue_to_py(py, &v)?)),
    }
}

/// Project a core [`Card`](quillmark_core::Card) to its Python dict shape via
/// the canonical [`CardWire`](quillmark_core::CardWire) (core owns the
/// field/comment/`$`-entry mapping). The dict keeps Python's snake_case
/// `payload_items`; item entries (`type`/`key`/`value`/`fill`/`text`/`inline`)
/// match the WASM `Card` shape verbatim.
fn card_to_pydict<'py>(
    py: Python<'py>,
    card: &quillmark_core::Card,
) -> PyResult<Bound<'py, PyDict>> {
    let wire = quillmark_core::CardWire::from(card);
    let d = PyDict::new(py);
    d.set_item("kind", &wire.kind)?;
    d.set_item("quill", wire.quill.as_deref())?;
    d.set_item("id", wire.id.as_deref())?;

    let items = PyList::empty(py);
    for item in &wire.payload_items {
        let entry = PyDict::new(py);
        match item {
            quillmark_core::PayloadItemWire::Field {
                key,
                value,
                fill,
                nested_fills,
            } => {
                entry.set_item("type", "field")?;
                entry.set_item("key", key)?;
                entry.set_item("value", json_to_py(py, value)?)?;
                entry.set_item("fill", *fill)?;
                // Paths to `!must_fill` markers nested inside `value` (the JSON
                // projection is fill-free). Mirrors the WASM `nestedFills` field;
                // omitted when empty so simple cards stay clean. The serde-based
                // reverse path (`py_dict_to_card`) reads it back.
                if !nested_fills.is_empty() {
                    let nf = serde_json::to_value(nested_fills)
                        .map_err(|e| PyValueError::new_err(e.to_string()))?;
                    entry.set_item("nestedFills", json_to_py(py, &nf)?)?;
                }
            }
            quillmark_core::PayloadItemWire::Comment { text, inline } => {
                entry.set_item("type", "comment")?;
                entry.set_item("text", text)?;
                entry.set_item("inline", *inline)?;
            }
        }
        items.append(entry)?;
    }
    d.set_item("payload_items", items)?;

    match &wire.ext {
        Some(ext_map) => {
            d.set_item(
                "ext",
                json_to_py(py, &serde_json::Value::Object(ext_map.clone()))?,
            )?;
        }
        None => d.set_item("ext", py.None())?,
    }

    match &wire.seed {
        Some(seed_map) => {
            d.set_item(
                "seed",
                json_to_py(py, &serde_json::Value::Object(seed_map.clone()))?,
            )?;
        }
        None => d.set_item("seed", py.None())?,
    }

    // `body` is the canonical content (source of truth); the markdown projection
    // is the schema-plane `quill.view(doc).get_body()` read. The reverse path
    // (`py_dict_to_card`) reads `body`.
    d.set_item("body", json_to_py(py, &wire.body)?)?;
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

/// Convert a Python mapping to the `(name, value)` batch the typed writer's
/// `set_all` / `add_card` consume. Value-conversion failures (depth bound,
/// unsupported type) are collected — not fail-fast — into one `QuillmarkError`
/// with a per-field `path`, matching the batch contract of the writer itself.
/// Non-string keys are a caller bug and raise `ValueError` directly.
fn pydict_to_field_batch(
    fields: &Bound<'_, PyDict>,
) -> PyResult<Vec<(String, quillmark_core::QuillValue)>> {
    let mut batch = Vec::new();
    let mut diags = Vec::new();
    for (key, value) in fields.iter() {
        let name: String = key
            .extract()
            .map_err(|_| PyValueError::new_err("field names must be strings"))?;
        match py_to_quillvalue(&value) {
            Ok(qv) => batch.push((name, qv)),
            Err(e) => diags.push(
                quillmark_core::Diagnostic::new(
                    quillmark_core::Severity::Error,
                    format!("invalid value: {e}"),
                )
                .with_path(name),
            ),
        }
    }
    if !diags.is_empty() {
        let message = quillmark_core::RenderError::summary_message(&diags);
        return Err(raise_with_diagnostics(diags, message));
    }
    Ok(batch)
}

fn py_to_json(value: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    py_to_json_at(value, 0)
}

/// Recursive worker for [`py_to_json`], depth-bounded at the core §8 nesting
/// limit. The bound serves two purposes: this function's own recursion cannot
/// overflow the native stack on an adversarially deep Python object, and the
/// produced value is rejected at the *same shape* the core payload boundary
/// would reject it.
///
/// The canonical cutoff is **container levels**, matching core's
/// [`json_depth_exceeds`](quillmark_core::json_depth_exceeds): a scalar leaf is
/// never charged a level, so `MAX_YAML_DEPTH` nested containers are accepted
/// whether the deepest one is empty, holds a scalar, or holds another
/// container — and `MAX_YAML_DEPTH + 1` is rejected in every case. The guard
/// therefore fires only on the recursing (container) branches, never the scalar
/// leaves: `depth` is the 0-based depth of the current node, so a container at
/// `depth` is the `(depth + 1)`-th nesting level and `depth >= MAX_YAML_DEPTH`
/// rejects exactly the level-`MAX_YAML_DEPTH + 1` container core also rejects.
fn py_to_json_at(value: &Bound<'_, PyAny>, depth: usize) -> PyResult<serde_json::Value> {
    use pyo3::types::{PyBool, PyFloat, PyInt, PyList, PyString};

    // Charged only when about to recurse into a container (see doc comment):
    // scalar leaves below fall through without consuming a level.
    let reject_too_deep = || {
        Err(PyValueError::new_err(format!(
            "value nests deeper than the maximum of {} levels",
            quillmark_core::document::limits::MAX_YAML_DEPTH
        )))
    };

    if value.is_none() {
        return Ok(serde_json::Value::Null);
    }
    if value.is_instance_of::<PyBool>() {
        let b: bool = value.extract()?;
        return Ok(serde_json::Value::Bool(b));
    }
    if value.is_instance_of::<PyInt>() {
        // Python ints are unbounded; map to i64, then u64, before giving up so
        // large positive values still convert losslessly. Report overflow as a
        // ValueError rather than letting PyO3's raw OverflowError leak across
        // the binding boundary.
        if let Ok(i) = value.extract::<i64>() {
            return Ok(serde_json::json!(i));
        }
        if let Ok(u) = value.extract::<u64>() {
            return Ok(serde_json::json!(u));
        }
        return Err(pyo3::exceptions::PyValueError::new_err(
            "integer value is out of range for JSON conversion (exceeds 64-bit)",
        ));
    }
    if value.is_instance_of::<PyFloat>() {
        let f: f64 = value.extract()?;
        if !f.is_finite() {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "non-finite float value '{}' cannot be represented in JSON",
                f
            )));
        }
        return Ok(serde_json::json!(f));
    }
    if value.is_instance_of::<PyString>() {
        let s: String = value.extract()?;
        return Ok(serde_json::Value::String(s));
    }
    if value.is_instance_of::<PyList>() {
        if depth >= quillmark_core::document::limits::MAX_YAML_DEPTH {
            return reject_too_deep();
        }
        let list = value.downcast::<PyList>()?;
        let arr: PyResult<Vec<serde_json::Value>> = list
            .iter()
            .map(|item| py_to_json_at(&item, depth + 1))
            .collect();
        return Ok(serde_json::Value::Array(arr?));
    }
    if value.is_instance_of::<PyDict>() {
        if depth >= quillmark_core::document::limits::MAX_YAML_DEPTH {
            return reject_too_deep();
        }
        let dict = value.downcast::<PyDict>()?;
        let mut map = serde_json::Map::new();
        for (k, v) in dict.iter() {
            let key: String = k.extract()?;
            map.insert(key, py_to_json_at(&v, depth + 1)?);
        }
        return Ok(serde_json::Value::Object(map));
    }
    let s = value.str()?.to_string();
    Ok(serde_json::Value::String(s))
}

/// Convert a Python value into a JSON object map, rejecting non-objects. Used
/// by the `$ext` mutators, whose value must be a dict.
fn py_to_object(
    value: &Bound<'_, PyAny>,
    ctx: &str,
) -> PyResult<serde_json::Map<String, serde_json::Value>> {
    match py_to_json(value)? {
        serde_json::Value::Object(map) => Ok(map),
        _ => Err(PyValueError::new_err(format!(
            "{}: $ext must be a dict",
            ctx
        ))),
    }
}

/// Convert an optional JSON value to a Python object, or `None`. Backs both the
/// namespaced reads (any value) and the whole-map reads (via `ext_map_to_py`).
fn ext_value_to_py<'py>(
    py: Python<'py>,
    value: Option<serde_json::Value>,
) -> PyResult<Bound<'py, PyAny>> {
    match value {
        Some(v) => json_to_py(py, &v),
        None => py.None().into_bound_py_any(py),
    }
}

/// Convert an optional `$ext` map to a Python dict, or `None`.
fn ext_map_to_py<'py>(
    py: Python<'py>,
    map: Option<serde_json::Map<String, serde_json::Value>>,
) -> PyResult<Bound<'py, PyAny>> {
    ext_value_to_py(py, map.map(serde_json::Value::Object))
}

/// Build a core [`Card`](quillmark_core::Card) from a Python `Card` dict via
/// the canonical [`CardWire`](quillmark_core::CardWire) (core owns the
/// construction). Accepts the snake_case `payload_items` key; a flat
/// `{ kind, fields }` dict fails loudly (`deny_unknown_fields`) rather than
/// yielding an empty card.
fn py_dict_to_card(value: &Bound<'_, PyAny>) -> PyResult<quillmark_core::Card> {
    let json = py_to_json(value)?;
    let wire: quillmark_core::CardWire = serde_json::from_value(json).map_err(|e| {
        PyValueError::new_err(format!(
            "card must be a Card dict {{ kind, payload_items?, body? }}: {e}"
        ))
    })?;
    quillmark_core::Card::try_from(wire).map_err(|e| PyValueError::new_err(e.to_string()))
}

