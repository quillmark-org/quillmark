//! # Quillmark WASM
//!
//! WebAssembly bindings for Quillmark.
//!
//! Two artifacts ship from this one crate (see
//! `prose/proposals/wasm-bindings-split.md`): a Typst-less **core**
//! (`@quillmark/wasm/core`) for load / validate / schema / seed / blueprint, and
//! a Typst-backed **backend** binary (`pkg/backends/typst/`) that adds the
//! engine and canvas preview. The `typst` / `pdfform` cargo features gate the
//! engine half (the default `typst` feature enables it; a no-feature build is
//! the core).
//!
//! The Typst backend is a PRIVATE binary — it is not a public npm export. The
//! package's public root (`@quillmark/wasm`) is a hand-written canonical layer
//! (`pkg/runtime/`) exposing `Quill` / `Document` (re-exported from core) and an
//! `Engine` that lazily loads a backend and renders through it; `/core` is the
//! render-free escape hatch. The FFI types below (`Quillmark`, `RenderSession`)
//! are the backend binding the canonical `Engine` wraps.
//!
//! ## API
//!
//! - [`Quill`] - portable, declarative quill data (`fromTree` constructor;
//!   validate / schema / metadata / seed / blueprint). Present in both builds.
//! - [`Quillmark`] - render engine: `open` / `render` / `supportedFormats` /
//!   `supportsCanvas`. Render build only.
//! - [`engine::Document`] - typed parsed document (`fromMarkdown`/`fromJson` static
//!   constructors, `toMarkdown`/`toJson` emitters). Present in both builds.
//!
//! ## Example
//!
//! The public `@quillmark/wasm` package exports the engine as `Engine` (the
//! runtime wrapper around the FFI [`Quillmark`] class below); its render
//! methods are async.
//!
//! ```javascript
//! import { Document, Quill, Engine } from '@quillmark/wasm';
//!
//! const quill = Quill.fromTree(tree);
//! const engine = new Engine();
//!
//! const doc = Document.fromMarkdown(markdown);
//! const result = await engine.render(quill, doc);
//! const pdfBytes = result.artifacts[0].bytes;
//! ```

use wasm_bindgen::prelude::*;

mod engine;
mod error;
mod types;

pub use engine::{Document, Quill};
#[cfg(any(feature = "typst", feature = "pdfform"))]
pub use engine::{Quillmark, RenderSession};
pub use error::WasmError;
pub use types::*;

/// Initialize the WASM module with panic hooks for better error messages
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}
