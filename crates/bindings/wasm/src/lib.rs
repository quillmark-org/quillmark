//! # Quillmark WASM
//!
//! WebAssembly bindings for Quillmark.
//!
//! Three build variants ship from this one crate (see
//! `docs/migrations/0.89-to-0.90.md`): a Typst-less **core** build
//! (`pkg/core/`) for load / validate / schema / seed / blueprint, and two
//! engine-carrying **backend** binaries — `pkg/backends/typst/` (Typst) and
//! `pkg/backends/pdfform/` (Typst-free PDF-form) — each adding the engine and
//! canvas preview. The `typst` / `pdfform` cargo features gate the engine half
//! (the default `typst` feature enables it; a no-feature build is the core).
//!
//! Both backend builds are PRIVATE binaries — neither is a public npm
//! export. The package's public root (`@quillmark/wasm`) is a hand-written
//! canonical layer (`pkg/runtime/`) exposing `Quill` / `Document`
//! (re-exported from the core build) and an `Engine` that lazily loads a
//! backend and renders through it. `pkg/core` is likewise not a public
//! subpath — it is the internal build the canonical layer re-exports from.
//! The FFI types below (`Quillmark`, `LiveSession`) are the backend binding
//! the canonical `Engine` wraps.
//!
//! ## API
//!
//! - [`Quill`] - portable, declarative quill data (`fromTree` constructor;
//!   validate / schema / metadata / seed / blueprint). Present in every build.
//! - [`Quillmark`] - render engine: `open` / `render` / `supportedFormats` /
//!   `supportsCanvas`. Render builds only (`typst` or `pdfform`).
//! - [`engine::Document`] - typed parsed document (`fromMarkdown`/`fromJson` static
//!   constructors, `toMarkdown`/`toJson` emitters). Present in every build.
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
pub use engine::{LiveSession, Quillmark};
pub use error::WasmError;
pub use types::*;

/// Initialize the WASM module with panic hooks for better error messages
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}
