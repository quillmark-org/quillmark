//! # Quillmark .NET binding — native C-ABI layer
//!
//! A `cdylib` exposing Quillmark over a flat C ABI that the companion C#
//! package ([`csharp/`](../csharp)) consumes via P/Invoke. It is the .NET
//! analogue of the Python binding's PyO3 extension module: the same
//! `quillmark` orchestration crate underneath, projected through an FFI
//! boundary instead of PyO3.
//!
//! ## Design (symmetry with Python)
//!
//! The Python binding marshals rich Python objects per method. C has no such
//! luxury, so this layer leans on **JSON as the structured-data currency**:
//! cards, schema, metadata, diagnostics, and field values all cross the
//! boundary as UTF-8 JSON produced by the very same `serde` types
//! (`CardWire`, `Diagnostic`, …) the other bindings use. Opaque handles carry
//! the stateful objects (`Quillmark`, `Quill`, `Document`, `RenderResult`).
//! The idiomatic, typed surface — `Document.SetField`, `Quill.Metadata`, … —
//! is reassembled in C# on top of these primitives, so the public .NET API
//! mirrors Python method-for-method.
//!
//! ## Error contract
//!
//! Mirrors Python's single-exception rule. A fallible entry point signals
//! failure by returning a null pointer (handle/string returns) or `-1`
//! (status returns) and parking a JSON `{ message, diagnostics }` payload the
//! caller drains with [`qm_last_error_take`]. C# turns that into a
//! `QuillmarkException` whose `.Diagnostics` is always non-empty.
//!
//! Optional *values* are never conflated with errors: they are encoded as JSON
//! `null` inside a valid (non-null) string, so a null pointer always means a
//! real failure.

mod abi;

use std::ffi::c_char;
use std::path::PathBuf;
use std::time::Instant;

use quillmark::{quill_from_path, Document, OutputFormat, Quill, Quillmark, RenderResult};
use quillmark_core::{Card, CardWire, Diagnostic, EditError, PayloadItemWire, RenderError};

use abi::{
    borrow_mut, borrow_ref, borrow_str, clear_error, drop_handle, panic_message, set_error,
    set_error_message, to_c_string, QmBytes,
};

/// Run a fallible entry point's body under `catch_unwind`, converting a panic
/// into the binding's error contract (a parked diagnostic plus `$default`
/// sentinel) instead of letting it cross the `extern "C"` boundary and abort
/// the host process. This is the hand-rolled-FFI analogue of the panic trapping
/// PyO3 and the WASM panic hook provide, so a backend panic surfaces to .NET as
/// a `QuillmarkException` rather than killing the host. The body is wrapped in
/// `unsafe` because every guarded entry point is itself `unsafe extern "C"` and
/// dereferences caller pointers.
macro_rules! ffi_try {
    ($default:expr, $body:block) => {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { $body })) {
            Ok(value) => value,
            Err(payload) => {
                set_error_message(format!(
                    "internal error: panic caught at FFI boundary: {}",
                    panic_message(payload.as_ref())
                ));
                $default
            }
        }
    };
}

// ── Handle wrappers ─────────────────────────────────────────────────────────

/// A parsed document plus its parse-time warnings, matching `PyDocument`.
pub struct DocHandle {
    inner: Document,
    parse_warnings: Vec<Diagnostic>,
}

/// A render result plus the wall-clock time spent in `render`, matching
/// `PyRenderResult`.
pub struct RenderResultHandle {
    inner: RenderResult,
    render_time_ms: f64,
}

// ── Format marshaling ───────────────────────────────────────────────────────

fn format_to_str(f: OutputFormat) -> &'static str {
    match f {
        OutputFormat::Pdf => "pdf",
        OutputFormat::Svg => "svg",
        OutputFormat::Txt => "txt",
        OutputFormat::Png => "png",
    }
}

fn format_from_str(s: &str) -> Option<OutputFormat> {
    match s {
        "pdf" => Some(OutputFormat::Pdf),
        "svg" => Some(OutputFormat::Svg),
        "txt" => Some(OutputFormat::Txt),
        "png" => Some(OutputFormat::Png),
        _ => None,
    }
}

fn mime_for(f: OutputFormat) -> &'static str {
    match f {
        OutputFormat::Pdf => "application/pdf",
        OutputFormat::Svg => "image/svg+xml",
        OutputFormat::Txt => "text/plain",
        OutputFormat::Png => "image/png",
    }
}

// ── Error conversion (mirrors Python errors.rs) ─────────────────────────────

fn report_edit_error(err: EditError) -> i32 {
    let variant = match &err {
        EditError::InvalidFieldName(_) => "InvalidFieldName",
        EditError::InvalidKindName(_) => "InvalidKindName",
        EditError::ReservedKind => "ReservedKind",
        EditError::IndexOutOfRange { .. } => "IndexOutOfRange",
        EditError::ValueTooDeep { .. } => "ValueTooDeep",
    };
    set_error_message(format!("[EditError::{}] {}", variant, err))
}

fn report_render_error(err: RenderError) -> i32 {
    let message = match &err {
        RenderError::CompilationFailed { diags } => {
            format!("Compilation failed with {} error(s)", diags.len())
        }
        RenderError::QuillConfig { diags } => summary("Quill configuration has", diags),
        RenderError::ValidationFailed { diags } => summary("Validation failed with", diags),
        RenderError::InvalidPayload { diags }
        | RenderError::EngineCreation { diags }
        | RenderError::FormatNotSupported { diags }
        | RenderError::UnsupportedBackend { diags }
        | RenderError::QuillMismatch { diags } => diags
            .first()
            .map(|d| d.message.clone())
            .unwrap_or_else(|| "render error".to_string()),
    };
    set_error(err.into_diagnostics(), message)
}

fn summary(prefix: &str, diags: &[Diagnostic]) -> String {
    match diags {
        [only] => only.message.clone(),
        _ => format!("{} {} error(s)", prefix, diags.len()),
    }
}

// ── Card JSON bridge (mirrors Python card_to_pydict / py_dict_to_card) ───────

fn card_to_json(card: &Card) -> *mut c_char {
    match serde_json::to_string(&CardWire::from(card)) {
        Ok(s) => to_c_string(s),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Parse a `Card` JSON object into a core [`Card`], reporting failure through
/// the thread-local error. `deny_unknown_fields` on `CardWire` rejects a stale
/// flat `{ kind, fields }` shape loudly, matching the other bindings.
fn card_from_json(json: &str) -> Result<Card, ()> {
    let wire: CardWire = serde_json::from_str(json).map_err(|e| {
        set_error_message(format!(
            "card must be a Card object {{ kind, payloadItems?, body? }}: {e}"
        ));
    })?;
    Card::try_from(wire).map_err(|e| {
        set_error_message(e.to_string());
    })
}

// ══════════════════════════════════════════════════════════════════════════
// Quillmark engine
// ══════════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn qm_engine_new() -> *mut Quillmark {
    Box::into_raw(Box::new(Quillmark::new()))
}

#[no_mangle]
pub extern "C" fn qm_engine_free(ptr: *mut Quillmark) {
    unsafe { drop_handle(ptr) }
}

/// Render `doc` against `quill`. `opts_json` is a JSON object (or null/`"null"`
/// for defaults): `{ format?, ppi?, pages?, producer? }` where `format` is a
/// lowercase format string. Returns a `RenderResultHandle`, or null on error.
#[no_mangle]
pub unsafe extern "C" fn qm_engine_render(
    engine: *mut Quillmark,
    quill: *mut Quill,
    doc: *mut DocHandle,
    opts_json: *const c_char,
) -> *mut RenderResultHandle {
    // The Typst backend is not panic-free, so this is the entry point most
    // likely to unwind; guard it so a backend panic becomes a QuillmarkException.
    ffi_try!(std::ptr::null_mut(), {
        clear_error();
        let (Some(engine), Some(quill), Some(doc)) =
            (borrow_ref(engine), borrow_ref(quill), borrow_ref(doc))
        else {
            set_error_message("render: null engine, quill, or document handle");
            return std::ptr::null_mut();
        };

        let opts = match parse_render_options(opts_json) {
            Ok(o) => o,
            Err(()) => return std::ptr::null_mut(),
        };

        let start = Instant::now();
        let mut result = match engine.render(quill, &doc.inner, &opts) {
            Ok(r) => r,
            Err(e) => {
                report_render_error(e);
                return std::ptr::null_mut();
            }
        };
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        // Parse-time warnings lead, mirroring the Python/WASM splice.
        result
            .warnings
            .splice(0..0, doc.parse_warnings.iter().cloned());

        Box::into_raw(Box::new(RenderResultHandle {
            inner: result,
            render_time_ms: elapsed_ms,
        }))
    })
}

unsafe fn parse_render_options(
    opts_json: *const c_char,
) -> Result<quillmark_core::RenderOptions, ()> {
    let mut opts = quillmark_core::RenderOptions::default();
    let Some(s) = borrow_str(opts_json) else {
        return Ok(opts); // null pointer → defaults
    };
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(opts);
    }
    let v: serde_json::Value = serde_json::from_str(trimmed).map_err(|e| {
        set_error_message(format!("render: invalid options JSON: {e}"));
    })?;
    if let Some(fmt) = v.get("format").and_then(|f| f.as_str()) {
        match format_from_str(fmt) {
            Some(f) => opts.output_format = Some(f),
            None => {
                set_error_message(format!("render: unknown output format '{fmt}'"));
                return Err(());
            }
        }
    }
    if let Some(ppi) = v.get("ppi").and_then(|p| p.as_f64()) {
        opts.ppi = Some(ppi as f32);
    }
    if let Some(pages) = v.get("pages").and_then(|p| p.as_array()) {
        opts.pages = Some(
            pages
                .iter()
                .filter_map(|p| p.as_u64().map(|u| u as usize))
                .collect(),
        );
    }
    if let Some(producer) = v.get("producer").and_then(|p| p.as_str()) {
        opts.producer = Some(producer.to_string());
    }
    Ok(opts)
}

/// JSON array of the lowercase format strings `quill`'s backend can emit, or
/// null on error (unregistered backend).
#[no_mangle]
pub unsafe extern "C" fn qm_engine_supported_formats(
    engine: *mut Quillmark,
    quill: *mut Quill,
) -> *mut c_char {
    clear_error();
    let (Some(engine), Some(quill)) = (borrow_ref(engine), borrow_ref(quill)) else {
        set_error_message("supported_formats: null handle");
        return std::ptr::null_mut();
    };
    match engine.supported_formats(quill) {
        Ok(formats) => {
            let names: Vec<&str> = formats.iter().map(|f| format_to_str(*f)).collect();
            to_c_string(serde_json::to_string(&names).unwrap_or_else(|_| "[]".into()))
        }
        Err(e) => {
            report_render_error(e);
            std::ptr::null_mut()
        }
    }
}

/// JSON array of registered backend ids (e.g. `["typst"]`).
#[no_mangle]
pub unsafe extern "C" fn qm_engine_registered_backends(engine: *mut Quillmark) -> *mut c_char {
    let Some(engine) = borrow_ref(engine) else {
        return to_c_string("[]");
    };
    let names: Vec<&str> = engine.registered_backends();
    to_c_string(serde_json::to_string(&names).unwrap_or_else(|_| "[]".into()))
}

// ══════════════════════════════════════════════════════════════════════════
// Quill
// ══════════════════════════════════════════════════════════════════════════

#[no_mangle]
pub unsafe extern "C" fn qm_quill_from_path(path: *const c_char) -> *mut Quill {
    ffi_try!(std::ptr::null_mut(), {
        clear_error();
        let Some(path) = borrow_str(path) else {
            set_error_message("Quill.from_path: null or non-UTF-8 path");
            return std::ptr::null_mut();
        };
        match quill_from_path(&PathBuf::from(path)) {
            Ok(q) => Box::into_raw(Box::new(q)),
            Err(e) => {
                report_render_error(e);
                std::ptr::null_mut()
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn qm_quill_free(ptr: *mut Quill) {
    unsafe { drop_handle(ptr) }
}

#[no_mangle]
pub unsafe extern "C" fn qm_quill_backend_id(quill: *mut Quill) -> *mut c_char {
    match borrow_ref(quill) {
        Some(q) => to_c_string(q.backend_id().to_string()),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_quill_quill_ref(quill: *mut Quill) -> *mut c_char {
    let Some(q) = borrow_ref(quill) else {
        return std::ptr::null_mut();
    };
    let version = q
        .metadata()
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0");
    to_c_string(format!("{}@{}", q.name(), version))
}

/// JSON object mirroring the `quill:` section: typed keys plus forwarded
/// unstructured keys. Pure config read; never errors for an unregistered
/// backend. Matches `PyQuill.metadata`.
#[no_mangle]
pub unsafe extern "C" fn qm_quill_metadata_json(quill: *mut Quill) -> *mut c_char {
    let Some(q) = borrow_ref(quill) else {
        return std::ptr::null_mut();
    };
    let config = q.config();
    let mut obj = serde_json::Map::new();
    obj.insert("name".into(), config.name.clone().into());
    obj.insert("version".into(), config.version.clone().into());
    obj.insert("backend".into(), config.backend.clone().into());
    obj.insert("author".into(), config.author.clone().into());
    obj.insert("description".into(), config.description.clone().into());
    for (key, value) in q.metadata() {
        if matches!(
            key.as_str(),
            "name" | "backend" | "description" | "version" | "author"
        ) || obj.contains_key(key)
        {
            continue;
        }
        obj.insert(key.clone(), value.as_json().clone());
    }
    to_c_string(serde_json::Value::Object(obj).to_string())
}

#[no_mangle]
pub unsafe extern "C" fn qm_quill_schema_json(quill: *mut Quill) -> *mut c_char {
    match borrow_ref(quill) {
        Some(q) => to_c_string(q.config().schema().to_string()),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_quill_blueprint(quill: *mut Quill) -> *mut c_char {
    match borrow_ref(quill) {
        Some(q) => to_c_string(q.config().blueprint()),
        None => std::ptr::null_mut(),
    }
}

/// JSON array of `validation::*` diagnostics (empty when valid). Matches
/// `PyQuill.validate`.
#[no_mangle]
pub unsafe extern "C" fn qm_quill_validate_json(
    quill: *mut Quill,
    doc: *mut DocHandle,
) -> *mut c_char {
    ffi_try!(std::ptr::null_mut(), {
        clear_error();
        let (Some(q), Some(doc)) = (borrow_ref(quill), borrow_ref(doc)) else {
            set_error_message("validate: null handle");
            return std::ptr::null_mut();
        };
        let diags = q.validate(&doc.inner);
        match serde_json::to_string(&diags) {
            Ok(s) => to_c_string(s),
            Err(e) => {
                set_error_message(format!("validate: serialization failed: {e}"));
                std::ptr::null_mut()
            }
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn qm_quill_seed_document(quill: *mut Quill) -> *mut DocHandle {
    ffi_try!(std::ptr::null_mut(), {
        let Some(q) = borrow_ref(quill) else {
            return std::ptr::null_mut();
        };
        Box::into_raw(Box::new(DocHandle {
            inner: q.seed_document(),
            parse_warnings: Vec::new(),
        }))
    })
}

#[no_mangle]
pub unsafe extern "C" fn qm_quill_seed_main_json(quill: *mut Quill) -> *mut c_char {
    ffi_try!(std::ptr::null_mut(), {
        match borrow_ref(quill) {
            Some(q) => card_to_json(&q.seed_main()),
            None => std::ptr::null_mut(),
        }
    })
}

/// Card JSON for a starter composable card of `kind`, layering an optional
/// per-kind seed `overlay` (a JSON object, or null/`"null"` for none) over the
/// schema-example base, or JSON `null` when the kind is not declared. Matches
/// `PyQuill.seed_card`.
#[no_mangle]
pub unsafe extern "C" fn qm_quill_seed_card_json(
    quill: *mut Quill,
    kind: *const c_char,
    overlay_json: *const c_char,
) -> *mut c_char {
    ffi_try!(std::ptr::null_mut(), {
        let (Some(q), Some(kind)) = (borrow_ref(quill), borrow_str(kind)) else {
            return std::ptr::null_mut();
        };
        let overlay = match borrow_str(overlay_json).map(str::trim) {
            Some(s) if !s.is_empty() && s != "null" => {
                match serde_json::from_str::<serde_json::Value>(s) {
                    Ok(value) => quillmark_core::SeedOverlay::from_json(&value),
                    Err(e) => {
                        set_error_message(format!("seed_card: invalid `overlay` JSON: {e}"));
                        return std::ptr::null_mut();
                    }
                }
            }
            _ => None,
        };
        match q.seed_card(kind, overlay.as_ref()) {
            Some(card) => card_to_json(&card),
            None => to_c_string("null"),
        }
    })
}

// ══════════════════════════════════════════════════════════════════════════
// Document — constructors & statics
// ══════════════════════════════════════════════════════════════════════════

#[no_mangle]
pub unsafe extern "C" fn qm_document_from_markdown(markdown: *const c_char) -> *mut DocHandle {
    ffi_try!(std::ptr::null_mut(), {
        clear_error();
        let Some(markdown) = borrow_str(markdown) else {
            set_error_message("from_markdown: null or non-UTF-8 input");
            return std::ptr::null_mut();
        };
        match Document::from_markdown_with_warnings(markdown) {
            Ok(output) => Box::into_raw(Box::new(DocHandle {
                inner: output.document,
                parse_warnings: output.warnings,
            })),
            Err(e) => {
                let diag = e.to_diagnostic();
                set_error(vec![diag.clone()], diag.message);
                std::ptr::null_mut()
            }
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_from_json(json: *const c_char) -> *mut DocHandle {
    ffi_try!(std::ptr::null_mut(), {
        clear_error();
        let Some(json) = borrow_str(json) else {
            set_error_message("from_json: null or non-UTF-8 input");
            return std::ptr::null_mut();
        };
        match serde_json::from_str::<Document>(json) {
            Ok(inner) => Box::into_raw(Box::new(DocHandle {
                inner,
                parse_warnings: Vec::new(),
            })),
            Err(e) => {
                set_error_message(format!("invalid storage DTO: {e}"));
                std::ptr::null_mut()
            }
        }
    })
}

/// Like `from_json` but returns null (with **no** pending error) when `json`
/// is not a storage DTO. Matches `PyDocument.try_from_json`.
#[no_mangle]
pub unsafe extern "C" fn qm_document_try_from_json(json: *const c_char) -> *mut DocHandle {
    ffi_try!(std::ptr::null_mut(), {
        clear_error();
        let Some(json) = borrow_str(json) else {
            return std::ptr::null_mut();
        };
        match serde_json::from_str::<Document>(json) {
            Ok(inner) => Box::into_raw(Box::new(DocHandle {
                inner,
                parse_warnings: Vec::new(),
            })),
            Err(_) => std::ptr::null_mut(),
        }
    })
}

/// The `schema` version tag of a raw DTO string as JSON (a string or `null`).
#[no_mangle]
pub unsafe extern "C" fn qm_document_schema_version_of(json: *const c_char) -> *mut c_char {
    let Some(json) = borrow_str(json) else {
        return to_c_string("null");
    };
    match quillmark_core::document::peek_schema_version(json) {
        Some(v) => to_c_string(serde_json::Value::String(v).to_string()),
        None => to_c_string("null"),
    }
}

#[no_mangle]
pub extern "C" fn qm_document_current_schema_version() -> *mut c_char {
    to_c_string(quillmark_core::document::SCHEMA_V0_82_0)
}

#[no_mangle]
pub extern "C" fn qm_document_format_rules() -> *mut c_char {
    to_c_string(quillmark_core::document::FORMAT_RULES)
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_blueprint_instruction(
    quill_name: *const c_char,
) -> *mut c_char {
    let name = borrow_str(quill_name).unwrap_or("");
    to_c_string(quillmark_core::document::blueprint_instruction(name))
}

#[no_mangle]
pub extern "C" fn qm_document_quill_ref_hint() -> *mut c_char {
    to_c_string(quillmark_core::quill_ref_hint())
}

/// Build a `Card` JSON from a kind and a JSON object of fields (or null/`"null"`)
/// and an optional body. Matches `PyDocument.make_card`.
#[no_mangle]
pub unsafe extern "C" fn qm_document_make_card_json(
    kind: *const c_char,
    fields_json: *const c_char,
    body: *const c_char,
) -> *mut c_char {
    clear_error();
    let Some(kind) = borrow_str(kind) else {
        set_error_message("make_card: null kind");
        return std::ptr::null_mut();
    };
    let mut payload_items = Vec::new();
    if let Some(fields) = borrow_str(fields_json) {
        let trimmed = fields.trim();
        if !trimmed.is_empty() && trimmed != "null" {
            match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(trimmed) {
                Ok(map) => {
                    for (key, value) in map {
                        payload_items.push(PayloadItemWire::Field {
                            key,
                            value,
                            fill: false,
                            nested_fills: Vec::new(),
                        });
                    }
                }
                Err(e) => {
                    set_error_message(format!("make_card: `fields` must be an object: {e}"));
                    return std::ptr::null_mut();
                }
            }
        }
    }
    let wire = CardWire {
        kind: kind.to_string(),
        quill: None,
        id: None,
        ext: None,
        seed: None,
        payload_items,
        body: borrow_str(body).unwrap_or("").to_string(),
    };
    match Card::try_from(wire) {
        Ok(card) => card_to_json(&card),
        Err(e) => {
            set_error_message(e.to_string());
            std::ptr::null_mut()
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Document — lifecycle & readers
// ══════════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn qm_document_free(ptr: *mut DocHandle) {
    unsafe { drop_handle(ptr) }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_clone(doc: *mut DocHandle) -> *mut DocHandle {
    let Some(doc) = borrow_ref(doc) else {
        return std::ptr::null_mut();
    };
    Box::into_raw(Box::new(DocHandle {
        inner: doc.inner.clone(),
        parse_warnings: doc.parse_warnings.clone(),
    }))
}

/// Structural equality (parse warnings excluded). Returns 1 / 0; -1 on null.
#[no_mangle]
pub unsafe extern "C" fn qm_document_equals(a: *mut DocHandle, b: *mut DocHandle) -> i32 {
    match (borrow_ref(a), borrow_ref(b)) {
        (Some(a), Some(b)) => (a.inner == b.inner) as i32,
        _ => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_to_markdown(doc: *mut DocHandle) -> *mut c_char {
    match borrow_ref(doc) {
        Some(doc) => to_c_string(doc.inner.to_markdown()),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_to_json(doc: *mut DocHandle) -> *mut c_char {
    clear_error();
    let Some(doc) = borrow_ref(doc) else {
        set_error_message("to_json: null handle");
        return std::ptr::null_mut();
    };
    // Document serialization is infallible in practice; surface the
    // theoretical error as a QuillmarkException rather than panicking across
    // the FFI boundary.
    match serde_json::to_string(&doc.inner) {
        Ok(s) => to_c_string(s),
        Err(e) => {
            set_error_message(format!("to_json: serialization failed: {e}"));
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_quill_ref(doc: *mut DocHandle) -> *mut c_char {
    match borrow_ref(doc) {
        Some(doc) => to_c_string(doc.inner.quill_reference().to_string()),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_body(doc: *mut DocHandle) -> *mut c_char {
    match borrow_ref(doc) {
        Some(doc) => to_c_string(doc.inner.main().body()),
        None => std::ptr::null_mut(),
    }
}

/// Count of composable cards. -1 on null handle.
#[no_mangle]
pub unsafe extern "C" fn qm_document_card_count(doc: *mut DocHandle) -> isize {
    match borrow_ref(doc) {
        Some(doc) => doc.inner.cards().len() as isize,
        None => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_warnings_json(doc: *mut DocHandle) -> *mut c_char {
    let Some(doc) = borrow_ref(doc) else {
        return std::ptr::null_mut();
    };
    to_c_string(serde_json::to_string(&doc.parse_warnings).unwrap_or_else(|_| "[]".into()))
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_main_json(doc: *mut DocHandle) -> *mut c_char {
    match borrow_ref(doc) {
        Some(doc) => card_to_json(doc.inner.main()),
        None => std::ptr::null_mut(),
    }
}

/// JSON array of the composable cards (Card shape each).
#[no_mangle]
pub unsafe extern "C" fn qm_document_cards_json(doc: *mut DocHandle) -> *mut c_char {
    let Some(doc) = borrow_ref(doc) else {
        return std::ptr::null_mut();
    };
    let wires: Vec<CardWire> = doc.inner.cards().iter().map(CardWire::from).collect();
    to_c_string(serde_json::to_string(&wires).unwrap_or_else(|_| "[]".into()))
}

// ══════════════════════════════════════════════════════════════════════════
// Document — main-card mutators
// ══════════════════════════════════════════════════════════════════════════

/// Shared parse of a JSON field value into a `QuillValue`.
unsafe fn quill_value_arg(value_json: *const c_char) -> Result<quillmark_core::QuillValue, ()> {
    let Some(s) = borrow_str(value_json) else {
        set_error_message("value: null or non-UTF-8 JSON");
        return Err(());
    };
    let json: serde_json::Value = serde_json::from_str(s).map_err(|e| {
        set_error_message(format!("value: invalid JSON: {e}"));
    })?;
    Ok(quillmark_core::QuillValue::from_json(json))
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_set_field(
    doc: *mut DocHandle,
    name: *const c_char,
    value_json: *const c_char,
) -> i32 {
    clear_error();
    let (Some(doc), Some(name)) = (borrow_mut(doc), borrow_str(name)) else {
        return set_error_message("set_field: null handle or name");
    };
    let qv = match quill_value_arg(value_json) {
        Ok(v) => v,
        Err(()) => return -1,
    };
    match doc.inner.main_mut().set_field(name, qv) {
        Ok(()) => 0,
        Err(e) => report_edit_error(e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_set_fill(
    doc: *mut DocHandle,
    name: *const c_char,
    value_json: *const c_char,
) -> i32 {
    clear_error();
    let (Some(doc), Some(name)) = (borrow_mut(doc), borrow_str(name)) else {
        return set_error_message("set_fill: null handle or name");
    };
    let qv = match quill_value_arg(value_json) {
        Ok(v) => v,
        Err(()) => return -1,
    };
    match doc.inner.main_mut().set_fill(name, qv) {
        Ok(()) => 0,
        Err(e) => report_edit_error(e),
    }
}

/// Remove a main-card field, returning the removed value as JSON (`null` when
/// absent). Null pointer means error.
#[no_mangle]
pub unsafe extern "C" fn qm_document_remove_field(
    doc: *mut DocHandle,
    name: *const c_char,
) -> *mut c_char {
    clear_error();
    let (Some(doc), Some(name)) = (borrow_mut(doc), borrow_str(name)) else {
        set_error_message("remove_field: null handle or name");
        return std::ptr::null_mut();
    };
    match doc.inner.main_mut().remove_field(name) {
        Ok(Some(v)) => to_c_string(v.as_json().to_string()),
        Ok(None) => to_c_string("null"),
        Err(e) => {
            report_edit_error(e);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_set_quill_ref(
    doc: *mut DocHandle,
    ref_str: *const c_char,
) -> i32 {
    clear_error();
    let (Some(doc), Some(ref_str)) = (borrow_mut(doc), borrow_str(ref_str)) else {
        return set_error_message("set_quill_ref: null handle or reference");
    };
    match ref_str.parse::<quillmark_core::QuillReference>() {
        Ok(qr) => {
            doc.inner.set_quill_ref(qr);
            0
        }
        Err(e) => set_error_message(format!("invalid QuillReference '{}': {}", ref_str, e)),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_replace_body(doc: *mut DocHandle, body: *const c_char) -> i32 {
    clear_error();
    let (Some(doc), Some(body)) = (borrow_mut(doc), borrow_str(body)) else {
        return set_error_message("replace_body: null handle or body");
    };
    doc.inner.main_mut().replace_body(body);
    0
}

// ── $ext on the main card ───────────────────────────────────────────────────

unsafe fn json_object_arg(
    value_json: *const c_char,
    ctx: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, ()> {
    let Some(s) = borrow_str(value_json) else {
        set_error_message(format!("{ctx}: null or non-UTF-8 JSON"));
        return Err(());
    };
    match serde_json::from_str::<serde_json::Value>(s) {
        Ok(serde_json::Value::Object(map)) => Ok(map),
        Ok(_) => {
            set_error_message(format!("{ctx}: $ext must be an object"));
            Err(())
        }
        Err(e) => {
            set_error_message(format!("{ctx}: invalid JSON: {e}"));
            Err(())
        }
    }
}

unsafe fn json_value_arg(value_json: *const c_char, ctx: &str) -> Result<serde_json::Value, ()> {
    let Some(s) = borrow_str(value_json) else {
        set_error_message(format!("{ctx}: null or non-UTF-8 JSON"));
        return Err(());
    };
    serde_json::from_str(s).map_err(|e| {
        set_error_message(format!("{ctx}: invalid JSON: {e}"));
    })
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_set_ext(
    doc: *mut DocHandle,
    value_json: *const c_char,
) -> i32 {
    clear_error();
    let Some(doc) = borrow_mut(doc) else {
        return set_error_message("set_ext: null handle");
    };
    let map = match json_object_arg(value_json, "set_ext") {
        Ok(m) => m,
        Err(()) => return -1,
    };
    match doc.inner.main_mut().set_ext(map) {
        Ok(()) => 0,
        Err(e) => report_edit_error(e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_remove_ext(doc: *mut DocHandle) -> *mut c_char {
    clear_error();
    let Some(doc) = borrow_mut(doc) else {
        set_error_message("remove_ext: null handle");
        return std::ptr::null_mut();
    };
    match doc.inner.main_mut().remove_ext() {
        Some(map) => to_c_string(serde_json::Value::Object(map).to_string()),
        None => to_c_string("null"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_set_ext_namespace(
    doc: *mut DocHandle,
    namespace: *const c_char,
    value_json: *const c_char,
) -> i32 {
    clear_error();
    let (Some(doc), Some(namespace)) = (borrow_mut(doc), borrow_str(namespace)) else {
        return set_error_message("set_ext_namespace: null handle or namespace");
    };
    let value = match json_value_arg(value_json, "set_ext_namespace") {
        Ok(v) => v,
        Err(()) => return -1,
    };
    match doc.inner.main_mut().set_ext_namespace(namespace, value) {
        Ok(()) => 0,
        Err(e) => report_edit_error(e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_remove_ext_namespace(
    doc: *mut DocHandle,
    namespace: *const c_char,
) -> *mut c_char {
    clear_error();
    let (Some(doc), Some(namespace)) = (borrow_mut(doc), borrow_str(namespace)) else {
        set_error_message("remove_ext_namespace: null handle or namespace");
        return std::ptr::null_mut();
    };
    match doc.inner.main_mut().remove_ext_namespace(namespace) {
        Some(v) => to_c_string(v.to_string()),
        None => to_c_string("null"),
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Document — composable card mutators
// ══════════════════════════════════════════════════════════════════════════

#[no_mangle]
pub unsafe extern "C" fn qm_document_push_card(
    doc: *mut DocHandle,
    card_json: *const c_char,
) -> i32 {
    clear_error();
    let (Some(doc), Some(card_json)) = (borrow_mut(doc), borrow_str(card_json)) else {
        return set_error_message("push_card: null handle or card");
    };
    let card = match card_from_json(card_json) {
        Ok(c) => c,
        Err(()) => return -1,
    };
    match doc.inner.push_card(card) {
        Ok(()) => 0,
        Err(e) => report_edit_error(e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_insert_card(
    doc: *mut DocHandle,
    index: usize,
    card_json: *const c_char,
) -> i32 {
    clear_error();
    let (Some(doc), Some(card_json)) = (borrow_mut(doc), borrow_str(card_json)) else {
        return set_error_message("insert_card: null handle or card");
    };
    let card = match card_from_json(card_json) {
        Ok(c) => c,
        Err(()) => return -1,
    };
    match doc.inner.insert_card(index, card) {
        Ok(()) => 0,
        Err(e) => report_edit_error(e),
    }
}

/// Remove and return the card at `index` as Card JSON, or JSON `null` when out
/// of range. Null pointer means a null handle.
#[no_mangle]
pub unsafe extern "C" fn qm_document_remove_card(
    doc: *mut DocHandle,
    index: usize,
) -> *mut c_char {
    let Some(doc) = borrow_mut(doc) else {
        return std::ptr::null_mut();
    };
    match doc.inner.remove_card(index) {
        Some(card) => card_to_json(&card),
        None => to_c_string("null"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_move_card(
    doc: *mut DocHandle,
    from_idx: usize,
    to_idx: usize,
) -> i32 {
    clear_error();
    let Some(doc) = borrow_mut(doc) else {
        return set_error_message("move_card: null handle");
    };
    match doc.inner.move_card(from_idx, to_idx) {
        Ok(()) => 0,
        Err(e) => report_edit_error(e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_set_card_kind(
    doc: *mut DocHandle,
    index: usize,
    new_kind: *const c_char,
) -> i32 {
    clear_error();
    let (Some(doc), Some(new_kind)) = (borrow_mut(doc), borrow_str(new_kind)) else {
        return set_error_message("set_card_kind: null handle or kind");
    };
    match doc.inner.set_card_kind(index, new_kind) {
        Ok(()) => 0,
        Err(e) => report_edit_error(e),
    }
}

/// Resolve a mutable composable card, reporting the same `IndexOutOfRange`
/// edit error the other card mutators raise.
unsafe fn card_mut_or_report<'a>(
    doc: &'a mut DocHandle,
    index: usize,
) -> Result<&'a mut Card, ()> {
    let len = doc.inner.cards().len();
    doc.inner.card_mut(index).ok_or_else(|| {
        report_edit_error(EditError::IndexOutOfRange { index, len });
    })
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_update_card_field(
    doc: *mut DocHandle,
    index: usize,
    name: *const c_char,
    value_json: *const c_char,
) -> i32 {
    clear_error();
    let (Some(doc), Some(name)) = (borrow_mut(doc), borrow_str(name)) else {
        return set_error_message("update_card_field: null handle or name");
    };
    let qv = match quill_value_arg(value_json) {
        Ok(v) => v,
        Err(()) => return -1,
    };
    let card = match card_mut_or_report(doc, index) {
        Ok(c) => c,
        Err(()) => return -1,
    };
    match card.set_field(name, qv) {
        Ok(()) => 0,
        Err(e) => report_edit_error(e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_remove_card_field(
    doc: *mut DocHandle,
    index: usize,
    name: *const c_char,
) -> *mut c_char {
    clear_error();
    let (Some(doc), Some(name)) = (borrow_mut(doc), borrow_str(name)) else {
        set_error_message("remove_card_field: null handle or name");
        return std::ptr::null_mut();
    };
    let card = match card_mut_or_report(doc, index) {
        Ok(c) => c,
        Err(()) => return std::ptr::null_mut(),
    };
    match card.remove_field(name) {
        Ok(Some(v)) => to_c_string(v.as_json().to_string()),
        Ok(None) => to_c_string("null"),
        Err(e) => {
            report_edit_error(e);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_update_card_body(
    doc: *mut DocHandle,
    index: usize,
    body: *const c_char,
) -> i32 {
    clear_error();
    let (Some(doc), Some(body)) = (borrow_mut(doc), borrow_str(body)) else {
        return set_error_message("update_card_body: null handle or body");
    };
    let card = match card_mut_or_report(doc, index) {
        Ok(c) => c,
        Err(()) => return -1,
    };
    card.replace_body(body);
    0
}

// ── $ext on composable cards ────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn qm_document_set_card_ext(
    doc: *mut DocHandle,
    index: usize,
    value_json: *const c_char,
) -> i32 {
    clear_error();
    let Some(doc) = borrow_mut(doc) else {
        return set_error_message("set_card_ext: null handle");
    };
    let map = match json_object_arg(value_json, "set_card_ext") {
        Ok(m) => m,
        Err(()) => return -1,
    };
    let card = match card_mut_or_report(doc, index) {
        Ok(c) => c,
        Err(()) => return -1,
    };
    match card.set_ext(map) {
        Ok(()) => 0,
        Err(e) => report_edit_error(e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_remove_card_ext(
    doc: *mut DocHandle,
    index: usize,
) -> *mut c_char {
    clear_error();
    let Some(doc) = borrow_mut(doc) else {
        set_error_message("remove_card_ext: null handle");
        return std::ptr::null_mut();
    };
    let card = match card_mut_or_report(doc, index) {
        Ok(c) => c,
        Err(()) => return std::ptr::null_mut(),
    };
    match card.remove_ext() {
        Some(map) => to_c_string(serde_json::Value::Object(map).to_string()),
        None => to_c_string("null"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_set_card_ext_namespace(
    doc: *mut DocHandle,
    index: usize,
    namespace: *const c_char,
    value_json: *const c_char,
) -> i32 {
    clear_error();
    let (Some(doc), Some(namespace)) = (borrow_mut(doc), borrow_str(namespace)) else {
        return set_error_message("set_card_ext_namespace: null handle or namespace");
    };
    let value = match json_value_arg(value_json, "set_card_ext_namespace") {
        Ok(v) => v,
        Err(()) => return -1,
    };
    let card = match card_mut_or_report(doc, index) {
        Ok(c) => c,
        Err(()) => return -1,
    };
    match card.set_ext_namespace(namespace, value) {
        Ok(()) => 0,
        Err(e) => report_edit_error(e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_document_remove_card_ext_namespace(
    doc: *mut DocHandle,
    index: usize,
    namespace: *const c_char,
) -> *mut c_char {
    clear_error();
    let (Some(doc), Some(namespace)) = (borrow_mut(doc), borrow_str(namespace)) else {
        set_error_message("remove_card_ext_namespace: null handle or namespace");
        return std::ptr::null_mut();
    };
    let card = match card_mut_or_report(doc, index) {
        Ok(c) => c,
        Err(()) => return std::ptr::null_mut(),
    };
    match card.remove_ext_namespace(namespace) {
        Some(v) => to_c_string(v.to_string()),
        None => to_c_string("null"),
    }
}

// ══════════════════════════════════════════════════════════════════════════
// RenderResult / Artifact
// ══════════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn qm_render_result_free(ptr: *mut RenderResultHandle) {
    unsafe { drop_handle(ptr) }
}

#[no_mangle]
pub unsafe extern "C" fn qm_render_result_format(result: *mut RenderResultHandle) -> *mut c_char {
    match borrow_ref(result) {
        Some(r) => to_c_string(format_to_str(r.inner.output_format)),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_render_result_render_time_ms(result: *mut RenderResultHandle) -> f64 {
    borrow_ref(result).map(|r| r.render_time_ms).unwrap_or(-1.0)
}

#[no_mangle]
pub unsafe extern "C" fn qm_render_result_warnings_json(
    result: *mut RenderResultHandle,
) -> *mut c_char {
    let Some(r) = borrow_ref(result) else {
        return std::ptr::null_mut();
    };
    to_c_string(serde_json::to_string(&r.inner.warnings).unwrap_or_else(|_| "[]".into()))
}

/// Number of artifacts; -1 on null.
#[no_mangle]
pub unsafe extern "C" fn qm_render_result_artifact_count(result: *mut RenderResultHandle) -> isize {
    match borrow_ref(result) {
        Some(r) => r.inner.artifacts.len() as isize,
        None => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_render_result_artifact_format(
    result: *mut RenderResultHandle,
    index: usize,
) -> *mut c_char {
    let Some(r) = borrow_ref(result) else {
        return std::ptr::null_mut();
    };
    match r.inner.artifacts.get(index) {
        Some(a) => to_c_string(format_to_str(a.output_format)),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qm_render_result_artifact_mime(
    result: *mut RenderResultHandle,
    index: usize,
) -> *mut c_char {
    let Some(r) = borrow_ref(result) else {
        return std::ptr::null_mut();
    };
    match r.inner.artifacts.get(index) {
        Some(a) => to_c_string(mime_for(a.output_format)),
        None => std::ptr::null_mut(),
    }
}

/// Copy of artifact `index`'s bytes. Empty buffer when out of range.
#[no_mangle]
pub unsafe extern "C" fn qm_render_result_artifact_bytes(
    result: *mut RenderResultHandle,
    index: usize,
) -> QmBytes {
    let Some(r) = borrow_ref(result) else {
        return QmBytes::empty();
    };
    match r.inner.artifacts.get(index) {
        Some(a) => QmBytes::from_vec(a.bytes.clone()),
        None => QmBytes::empty(),
    }
}
