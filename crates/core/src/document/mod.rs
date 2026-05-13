//! # Document Module
//!
//! Parsing functionality for markdown documents with YAML frontmatter.
//!
//! ## Overview
//!
//! The `document` module provides the [`Document::from_markdown`] function for parsing
//! markdown documents into a typed in-memory model.
//!
//! ## Key Types
//!
//! - [`Document`]: Typed in-memory Quillmark document — `main` leaf plus composable leaves.
//! - [`Leaf`]: A single metadata fence block, main or composable, with a sentinel,
//!   typed frontmatter, and a body.
//! - [`Sentinel`]: Discriminates `QUILL:` main leaves from `KIND:` composable leaves.
//! - [`Frontmatter`]: Ordered list of items (fields + comments) parsed from a YAML fence.
//!
//! ## Examples
//!
//! ### Basic Parsing
//!
//! ```
//! use quillmark_core::Document;
//!
//! let markdown = r#"---
//! QUILL: my_quill
//! title: My Document
//! author: John Doe
//! ---
//!
//! # Introduction
//!
//! Document content here.
//! "#;
//!
//! let doc = Document::from_markdown(markdown).unwrap();
//! let title = doc.main()
//!     .frontmatter()
//!     .get("title")
//!     .and_then(|v| v.as_str())
//!     .unwrap_or("Untitled");
//! assert_eq!(title, "My Document");
//! assert_eq!(doc.leaves().len(), 0);
//! ```
//!
//! ### Document with leaves
//!
//! ```
//! use quillmark_core::Document;
//!
//! let markdown = "---\nQUILL: my_quill\ntitle: Catalog\n---\n\nIntro.\n\n```leaf\nKIND: product\nname: Widget\n```\n";
//! let doc = Document::from_markdown(markdown).unwrap();
//! assert_eq!(doc.leaves().len(), 1);
//! assert_eq!(doc.leaves()[0].tag(), "product");
//! ```
//!
//! ### Accessing the plate wire format
//!
//! ```
//! use quillmark_core::Document;
//!
//! let doc = Document::from_markdown(
//!     "---\nQUILL: my_quill\ntitle: Hi\n---\n\nBody here.\n"
//! ).unwrap();
//! let json = doc.to_plate_json();
//! assert_eq!(json["QUILL"], "my_quill");
//! assert_eq!(json["title"], "Hi");
//! assert_eq!(json["BODY"], "\nBody here.\n");
//! assert!(json["LEAVES"].is_array());
//! ```
//!
//! ## Error Handling
//!
//! [`Document::from_markdown`] returns errors for:
//! - Malformed YAML syntax
//! - Unclosed frontmatter blocks
//! - Multiple global frontmatter blocks
//! - Both QUILL and KIND specified in the same block
//! - Reserved field name usage
//! - Name collisions
//!
//! See [PARSE.md](https://github.com/nibsbin/quillmark/blob/main/designs/PARSE.md) for
//! comprehensive documentation of the Extended YAML Metadata Standard.

use crate::error::ParseError;
use crate::version::QuillReference;
use crate::Diagnostic;

pub mod assemble;
pub mod edit;
pub mod emit;
pub mod fences;
pub mod frontmatter;
pub mod limits;
pub mod prescan;
pub mod sentinel;

pub use edit::EditError;
pub use frontmatter::{Frontmatter, FrontmatterItem};

// Re-export the sentinel type (defined below in this module file).
// `Sentinel` is exported at the crate root via `lib.rs`.

#[cfg(test)]
mod tests;

/// Parse result carrying both the parsed document and any non-fatal warnings
/// (e.g. near-miss sentinel lints emitted per spec §4.2).
#[derive(Debug)]
pub struct ParseOutput {
    /// The successfully parsed document.
    pub document: Document,
    /// Non-fatal warnings collected during parsing.
    pub warnings: Vec<Diagnostic>,
}

/// Discriminator for a [`Leaf`]'s metadata fence.
///
/// The first fence in a Quillmark document carries `QUILL: <ref>` and is the
/// document-level *main* leaf; every subsequent fence carries `KIND: <tag>`
/// and is a composable leaf. `Sentinel` captures that distinction in the typed
/// model so every fence is one uniform shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Sentinel {
    /// `QUILL: <ref>` — the document entry leaf.
    Main(QuillReference),
    /// `KIND: <tag>` — a composable leaf with the given tag.
    Leaf(String),
}

impl Sentinel {
    /// String form of this sentinel's value: the quill reference for `Main`,
    /// the tag for `Leaf`.
    pub fn as_str(&self) -> String {
        match self {
            Sentinel::Main(r) => r.to_string(),
            Sentinel::Leaf(t) => t.clone(),
        }
    }

    /// Returns `true` if this is a `Main` sentinel.
    pub fn is_main(&self) -> bool {
        matches!(self, Sentinel::Main(_))
    }
}

/// A single metadata fence parsed from a Quillmark Markdown document.
///
/// A `Leaf` is the uniform shape for both the document entry (main) fence and
/// composable leaf fences. `sentinel` distinguishes the two.
///
/// Every leaf has:
/// - `sentinel` — the `QUILL` reference (for main) or `KIND` tag (for composable).
/// - `frontmatter` — ordered items parsed from the YAML fence body (with the
///   sentinel key already removed).
/// - `body` — the Markdown text that follows the closing fence, up to the next
///   fence (or EOF).
///
/// ## Leaf body absence
///
/// If a leaf block has no trailing Markdown content (e.g. the next block or
/// EOF immediately follows the closing fence), `body` is the empty string `""`.
/// It is never `None`; callers that need to distinguish "absent" from "empty"
/// should check `leaf.body().is_empty()`.
#[derive(Debug, Clone, PartialEq)]
pub struct Leaf {
    sentinel: Sentinel,
    frontmatter: Frontmatter,
    body: String,
}

impl Leaf {
    /// Create a `Leaf` directly from a sentinel, a typed frontmatter, and a
    /// body. Does **not** validate the sentinel tag or any field names —
    /// callers are responsible for providing already-valid data. For
    /// user-facing construction of composable leaves use [`Leaf::new`]
    /// (defined in `edit.rs`).
    pub fn new_with_sentinel(sentinel: Sentinel, frontmatter: Frontmatter, body: String) -> Self {
        Self {
            sentinel,
            frontmatter,
            body,
        }
    }

    /// The sentinel discriminating this leaf as main or composable.
    pub fn sentinel(&self) -> &Sentinel {
        &self.sentinel
    }

    /// The leaf tag — the `KIND:` value for composable leaves, or the string
    /// form of the quill reference for main leaves.
    pub fn tag(&self) -> String {
        self.sentinel.as_str()
    }

    /// Typed frontmatter (map-keyed view and ordered item list).
    pub fn frontmatter(&self) -> &Frontmatter {
        &self.frontmatter
    }

    /// Mutable access to the frontmatter.
    pub fn frontmatter_mut(&mut self) -> &mut Frontmatter {
        &mut self.frontmatter
    }

    /// Markdown body that follows this leaf's closing fence.
    ///
    /// Empty string when no trailing content is present.
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Returns `true` if this is the document entry (main) leaf.
    pub fn is_main(&self) -> bool {
        self.sentinel.is_main()
    }

    /// Replace this leaf's sentinel. Internal helper; public mutators
    /// ([`Document::set_quill_ref`], the parser) call this.
    pub(crate) fn replace_sentinel(&mut self, sentinel: Sentinel) {
        self.sentinel = sentinel;
    }

    /// Overwrite the body string. Internal helper used by [`Leaf::replace_body`].
    pub(crate) fn overwrite_body(&mut self, body: String) {
        self.body = body;
    }
}

/// A fully-parsed, typed in-memory Quillmark document.
///
/// `Document` is the canonical representation of a Quillmark Markdown file.
/// Markdown is both the import and export format; the structured data here
/// is primary.
///
/// ## Structure
///
/// - `main` — the entry `Leaf` (sentinel is `Sentinel::Main(reference)`).
/// - `leaves` — ordered composable leaves (each with `Sentinel::Leaf(tag)`).
///
/// Backend plates consume the flat JSON wire shape produced by
/// [`Document::to_plate_json`]. That method is the **only** place in core
/// that reconstructs `{"QUILL": ..., "LEAVES": [...], "BODY": "..."}`.
#[derive(Debug, Clone)]
pub struct Document {
    main: Leaf,
    leaves: Vec<Leaf>,
    warnings: Vec<Diagnostic>,
}

// Equality is defined over the structural content only — `warnings` are
// parse-time observations that depend on what the source text happened to
// contain (near-miss sentinels, unsupported tag drops, etc.) and so differ
// between a source document and its round-tripped emission. Two documents
// are equal when their `main` and `leaves` match.
impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        self.main == other.main && self.leaves == other.leaves
    }
}

impl Document {
    /// Create a `Document` from a pre-built main `Leaf` and composable leaves.
    ///
    /// The caller must guarantee that `main.sentinel` is `Sentinel::Main(_)`
    /// and every leaf in `leaves` has `sentinel` = `Sentinel::Leaf(_)`.
    pub fn from_main_and_leaves(main: Leaf, leaves: Vec<Leaf>, warnings: Vec<Diagnostic>) -> Self {
        debug_assert!(main.sentinel.is_main(), "main leaf must be Sentinel::Main");
        debug_assert!(
            leaves.iter().all(|c| !c.sentinel.is_main()),
            "composable leaves must be Sentinel::Leaf"
        );
        Self {
            main,
            leaves,
            warnings,
        }
    }

    /// Parse a Quillmark Markdown document, discarding any non-fatal warnings.
    pub fn from_markdown(markdown: &str) -> Result<Self, ParseError> {
        assemble::decompose(markdown)
    }

    /// Parse a Quillmark Markdown document, returning warnings alongside the document.
    pub fn from_markdown_with_warnings(markdown: &str) -> Result<ParseOutput, ParseError> {
        assemble::decompose_with_warnings(markdown)
            .map(|(document, warnings)| ParseOutput { document, warnings })
    }

    // ── Accessors ──────────────────────────────────────────────────────────────

    /// The document's main (entry) leaf.
    pub fn main(&self) -> &Leaf {
        &self.main
    }

    /// Mutable access to the main leaf.
    pub fn main_mut(&mut self) -> &mut Leaf {
        &mut self.main
    }

    /// The quill reference (`name@version-selector`) carried by the main leaf's
    /// sentinel. Convenience reader over `doc.main().sentinel()`.
    pub fn quill_reference(&self) -> &QuillReference {
        match &self.main.sentinel {
            Sentinel::Main(r) => r,
            Sentinel::Leaf(_) => {
                unreachable!("main leaf must carry Sentinel::Main by construction")
            }
        }
    }

    /// Ordered list of composable leaf blocks.
    pub fn leaves(&self) -> &[Leaf] {
        &self.leaves
    }

    /// Mutable access to the composable leaves slice.
    pub fn leaves_mut(&mut self) -> &mut [Leaf] {
        &mut self.leaves
    }

    /// Internal mutable access to the backing `Vec<Leaf>`. Used by edit
    /// operations ([`Document::push_leaf`], etc.) that need to insert or
    /// remove elements.
    pub(crate) fn leaves_vec_mut(&mut self) -> &mut Vec<Leaf> {
        &mut self.leaves
    }

    /// Non-fatal warnings collected during parsing.
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }

    // ── Wire format ────────────────────────────────────────────────────────────

    /// Serialize this document to the JSON shape expected by backend plates.
    ///
    /// The output has the following top-level keys, which match what
    /// `lib.typ.template` reads at Typst runtime:
    ///
    /// ```json
    /// {
    ///   "QUILL": "<ref>",
    ///   "<field>": <value>,
    ///   ...
    ///   "BODY": "<global-body>",
    ///   "LEAVES": [
    ///     { "KIND": "<tag>", "<field>": <value>, ..., "BODY": "<leaf-body>" },
    ///     ...
    ///   ]
    /// }
    /// ```
    ///
    /// This is the **only** place in `quillmark-core` that knows about the plate
    /// wire format. All internal consumers (Quill, backends) call this instead
    /// of constructing the shape by hand.
    pub fn to_plate_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();

        // QUILL first — plate authors expect this at the top.
        map.insert(
            "QUILL".to_string(),
            serde_json::Value::String(self.quill_reference().to_string()),
        );

        // Frontmatter fields in insertion order.
        for (key, value) in self.main.frontmatter.iter() {
            map.insert(key.clone(), value.as_json().clone());
        }

        // Global body.
        map.insert(
            "BODY".to_string(),
            serde_json::Value::String(self.main.body.clone()),
        );

        // Leaves array.
        let leaves_array: Vec<serde_json::Value> = self
            .leaves
            .iter()
            .map(|leaf| {
                let mut leaf_map = serde_json::Map::new();
                leaf_map.insert("KIND".to_string(), serde_json::Value::String(leaf.tag()));
                for (key, value) in leaf.frontmatter.iter() {
                    leaf_map.insert(key.clone(), value.as_json().clone());
                }
                leaf_map.insert(
                    "BODY".to_string(),
                    serde_json::Value::String(leaf.body.clone()),
                );
                serde_json::Value::Object(leaf_map)
            })
            .collect();

        map.insert("LEAVES".to_string(), serde_json::Value::Array(leaves_array));

        serde_json::Value::Object(map)
    }
}
