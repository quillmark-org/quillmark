//! C-ABI marshaling primitives shared by every exported function.
//!
//! The boundary speaks three currencies: **owned C strings** (UTF-8, usually
//! carrying JSON), **owned byte buffers** ([`QmBytes`], for artifact bytes),
//! and **opaque handles** (`*mut T` from [`Box::into_raw`]). Fallibility is
//! out-of-band: a function signals failure by returning a null pointer (or a
//! non-zero status `int`) and parks a JSON error payload in a thread-local the
//! caller drains with [`qm_last_error_take`]. This mirrors the Python binding's
//! single-exception contract (`QuillmarkError` carrying `.diagnostics`) without
//! needing a richer ABI than C provides.

use std::cell::RefCell;
use std::ffi::{c_char, CStr, CString};

use quillmark_core::{Diagnostic, Severity};

thread_local! {
    /// The most recent error, as the JSON payload C# turns into a
    /// `QuillmarkException`: `{ "message": string, "diagnostics": Diagnostic[] }`.
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

/// Store `diags`/`message` as the pending thread-local error. Always returns
/// the conventional failure sentinel for `int`-returning entry points (`-1`),
/// so call sites can `return set_error(..)`.
pub(crate) fn set_error(diags: Vec<Diagnostic>, message: String) -> i32 {
    let payload = serde_json::json!({
        "message": message,
        "diagnostics": diags,
    });
    let json = payload.to_string();
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = CString::new(json).ok();
    });
    -1
}

/// Record a single-diagnostic error from a bare message (parse/IO failures that
/// don't already carry a structured diagnostic).
pub(crate) fn set_error_message(message: impl Into<String>) -> i32 {
    let message = message.into();
    set_error(
        vec![Diagnostic::new(Severity::Error, message.clone())],
        message,
    )
}

/// Clear any pending error. Entry points that can return a legitimate null
/// (`try_from_json` → "not a DTO") clear first so the caller can tell a real
/// failure from a benign absence.
pub(crate) fn clear_error() {
    LAST_ERROR.with(|slot| *slot.borrow_mut() = None);
}

/// Extract a human-readable message from a `catch_unwind` payload (the common
/// `&str` / `String` panic shapes), falling back to a generic label.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic".to_string()
    }
}

/// Drain the pending error payload, transferring ownership to the caller (free
/// with [`qm_free_string`]). Null when no error is pending.
#[no_mangle]
pub extern "C" fn qm_last_error_take() -> *mut c_char {
    LAST_ERROR.with(|slot| match slot.borrow_mut().take() {
        Some(s) => s.into_raw(),
        None => std::ptr::null_mut(),
    })
}

/// Allocate an owned C string for return across the boundary. An interior NUL
/// (impossible for JSON/UTF-8 text we produce) collapses to a null return.
pub(crate) fn to_c_string(s: impl Into<Vec<u8>>) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Borrow a `&str` from a caller-provided pointer. Null and invalid UTF-8 both
/// surface as `None`, which entry points translate into a boundary error.
pub(crate) unsafe fn borrow_str<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    CStr::from_ptr(ptr).to_str().ok()
}

/// Borrow a handle reference from a raw pointer, or `None` when null.
pub(crate) unsafe fn borrow_ref<'a, T>(ptr: *mut T) -> Option<&'a T> {
    ptr.as_ref()
}

/// Borrow a mutable handle reference from a raw pointer, or `None` when null.
pub(crate) unsafe fn borrow_mut<'a, T>(ptr: *mut T) -> Option<&'a mut T> {
    ptr.as_mut()
}

/// An owned byte buffer handed to the caller (artifact bytes). The caller must
/// return it with [`qm_free_bytes`]. The buffer is allocated as a boxed slice
/// (capacity == `len` by construction), so `qm_free_bytes` can soundly
/// reconstruct and drop it from `ptr` + `len` alone.
#[repr(C)]
pub struct QmBytes {
    pub ptr: *mut u8,
    pub len: usize,
}

impl QmBytes {
    pub(crate) fn from_vec(v: Vec<u8>) -> QmBytes {
        // Into a boxed slice so the allocation's capacity equals `len` — freeing
        // a `Vec` with a mismatched capacity (which `shrink_to_fit` does not
        // guarantee) hands the allocator the wrong layout, which is UB.
        let mut boxed = v.into_boxed_slice();
        let ptr = boxed.as_mut_ptr();
        let len = boxed.len();
        std::mem::forget(boxed);
        QmBytes { ptr, len }
    }

    pub(crate) fn empty() -> QmBytes {
        QmBytes {
            ptr: std::ptr::null_mut(),
            len: 0,
        }
    }
}

/// Free a string previously returned by any `qm_*` entry point.
#[no_mangle]
pub extern "C" fn qm_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe { drop(CString::from_raw(ptr)) };
    }
}

/// Free a byte buffer previously returned by any `qm_*` entry point.
#[no_mangle]
pub extern "C" fn qm_free_bytes(bytes: QmBytes) {
    if !bytes.ptr.is_null() && bytes.len != 0 {
        // Reconstruct the boxed slice produced by `QmBytes::from_vec` (capacity
        // == len), so the allocator gets the exact layout it handed out.
        let slice = std::ptr::slice_from_raw_parts_mut(bytes.ptr, bytes.len);
        unsafe { drop(Box::from_raw(slice)) };
    }
}

/// Reclaim a boxed handle of type `T`. The typed `qm_*_free` entry points are
/// thin wrappers so the C# side frees against the right layout.
pub(crate) unsafe fn drop_handle<T>(ptr: *mut T) {
    if !ptr.is_null() {
        drop(Box::from_raw(ptr));
    }
}
