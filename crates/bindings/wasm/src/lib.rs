//! # Quillmark WASM
//!
//! WebAssembly bindings for Quillmark.
//!
//! ## API
//!
//! - [`Quillmark`] - engine for loading render-ready quills from in-memory trees
//! - [`Quill`] - quill handle for rendering/compiling
//! - [`engine::Document`] - typed parsed document (`fromMarkdown`/`fromJson` static
//!   constructors, `toMarkdown`/`toJson` emitters)
//!
//! ## Usage
//!
//! 1. Build a render-ready quill with `engine.quill(...)`
//! 2. Parse markdown via `Document.fromMarkdown(...)`
//! 3. Render with `quill.render(...)`
//!
//! ## Example
//!
//! ```javascript
//! import { Document, Quillmark } from '@quillmark-test/wasm';
//!
//! const engine = new Quillmark();
//! const quill = engine.quill(tree);
//!
//! const doc = Document.fromMarkdown(markdown);
//! const result = quill.render(doc);
//! const pdfBytes = result.artifacts[0].bytes;
//! ```

use wasm_bindgen::prelude::*;

mod engine;
mod error;
mod types;

pub use engine::{Document, Quill, Quillmark, RenderSession};
pub use error::WasmError;
pub use types::*;

/// Initialize the WASM module with panic hooks for better error messages
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}
