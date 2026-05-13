//! Renderable `Quill` — the engine-constructed composition of a
//! [`QuillSource`] with a resolved backend.

use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;

use quillmark_core::{
    normalize::normalize_document, Backend, Leaf, Diagnostic, Document, Frontmatter, OutputFormat,
    QuillSource, QuillValue, RenderError, RenderOptions, RenderResult, RenderSession, Sentinel,
    Severity,
};

use crate::form::{self, Form, FormLeaf};

/// Renderable quill. Composes an [`Arc<QuillSource>`] with a resolved
/// [`Backend`]. Constructed by the engine; immutable once created.
#[derive(Clone)]
pub struct Quill {
    source: Arc<QuillSource>,
    backend: Arc<dyn Backend>,
}

impl Quill {
    /// Construct a Quill from a source and a resolved backend.
    ///
    /// Engine-internal; external callers should use
    /// [`crate::Quillmark::quill`] or [`crate::Quillmark::quill_from_path`].
    pub(crate) fn new(source: Arc<QuillSource>, backend: Arc<dyn Backend>) -> Self {
        Self { source, backend }
    }

    /// The underlying quill source.
    pub fn source(&self) -> &QuillSource {
        &self.source
    }

    /// The resolved backend identifier (e.g. `"typst"`).
    pub fn backend_id(&self) -> &str {
        self.backend.id()
    }

    /// Supported output formats for this quill's backend.
    pub fn supported_formats(&self) -> &'static [OutputFormat] {
        self.backend.supported_formats()
    }

    /// The quill's declared name.
    pub fn name(&self) -> &str {
        self.source.name()
    }

    /// Render a document to final artifacts.
    ///
    /// Pass `&RenderOptions::default()` for backend defaults (first supported
    /// format, backend-chosen ppi, all pages).
    pub fn render(
        &self,
        doc: &Document,
        opts: &RenderOptions,
    ) -> Result<RenderResult, RenderError> {
        let session = self.open(doc)?;
        let resolved = RenderOptions {
            output_format: opts
                .output_format
                .or_else(|| self.backend.supported_formats().first().copied()),
            ppi: opts.ppi,
            pages: opts.pages.clone(),
        };
        session.render(&resolved)
    }

    /// Open an iterative render session for this document.
    pub fn open(&self, doc: &Document) -> Result<RenderSession, RenderError> {
        let json_data = self.compile_data(doc)?;
        let plate_content = self
            .source
            .plate()
            .filter(|s| !s.is_empty())
            .unwrap_or("")
            .to_string();
        let warnings: Vec<_> = self.ref_mismatch_warning(doc).into_iter().collect();
        let session = self.backend.open(&plate_content, &self.source, &json_data)?;
        Ok(session.with_warnings(warnings))
    }

    /// Compile a Document to JSON data suitable for the backend.
    ///
    /// Applies coercion, validation, normalization, and schema defaults, then
    /// calls [`Document::to_plate_json`] to produce the wire format.
    pub fn compile_data(&self, doc: &Document) -> Result<serde_json::Value, RenderError> {
        let coerced = self.coerce_and_validate(doc)?;

        // Normalize: strip bidi + fix HTML comment fences in body regions.
        let normalized = normalize_document(coerced)?;

        // Apply schema defaults to main + per-leaf frontmatter.
        let main_with_defaults = apply_defaults(
            &normalized.main().frontmatter().to_index_map(),
            self.source.config().main.defaults(),
        );
        let leaves_with_defaults: Vec<Leaf> = normalized
            .leaves()
            .iter()
            .map(|leaf| {
                let defaults = self
                    .source
                    .config()
                    .leaf_kind(&leaf.tag())
                    .map(|c| c.defaults())
                    .unwrap_or_default();
                let fields =
                    apply_defaults(&leaf.frontmatter().to_index_map(), defaults);
                Leaf::new_with_sentinel(
                    Sentinel::Leaf(leaf.tag()),
                    Frontmatter::from_index_map(fields),
                    leaf.body().to_string(),
                )
            })
            .collect();

        let final_main = Leaf::new_with_sentinel(
            Sentinel::Main(normalized.quill_reference().clone()),
            Frontmatter::from_index_map(main_with_defaults),
            normalized.main().body().to_string(),
        );
        let final_doc = Document::from_main_and_leaves(
            final_main,
            leaves_with_defaults,
            normalized.warnings().to_vec(),
        );

        Ok(final_doc.to_plate_json())
    }

    /// Perform a dry-run validation without backend compilation.
    pub fn dry_run(&self, doc: &Document) -> Result<(), RenderError> {
        self.coerce_and_validate(doc).map(|_| ())
    }

    /// Coerce main + leaf fields against their schemas, then validate the
    /// resulting document. Shared entry point for [`Self::compile_data`]
    /// (which then normalizes and applies defaults) and [`Self::dry_run`]
    /// (which stops here).
    fn coerce_and_validate(&self, doc: &Document) -> Result<Document, RenderError> {
        let config = self.source.config();

        let coerced_frontmatter = config
            .coerce_frontmatter(&doc.main().frontmatter().to_index_map())
            .map_err(coercion_error)?;

        let mut coerced_leaves: Vec<Leaf> = Vec::with_capacity(doc.leaves().len());
        for leaf in doc.leaves() {
            let coerced_fields = config
                .coerce_leaf(&leaf.tag(), &leaf.frontmatter().to_index_map())
                .map_err(coercion_error)?;
            coerced_leaves.push(Leaf::new_with_sentinel(
                Sentinel::Leaf(leaf.tag()),
                Frontmatter::from_index_map(coerced_fields),
                leaf.body().to_string(),
            ));
        }

        let coerced_main = Leaf::new_with_sentinel(
            Sentinel::Main(doc.quill_reference().clone()),
            Frontmatter::from_index_map(coerced_frontmatter),
            doc.main().body().to_string(),
        );
        let coerced_doc =
            Document::from_main_and_leaves(coerced_main, coerced_leaves, doc.warnings().to_vec());

        self.validate_document(&coerced_doc)?;

        Ok(coerced_doc)
    }

    fn ref_mismatch_warning(&self, doc: &Document) -> Option<Diagnostic> {
        let doc_ref = doc.quill_reference().name.as_str();
        if doc_ref != self.source.name() {
            Some(
                Diagnostic::new(
                    Severity::Warning,
                    format!(
                        "document declares QUILL '{}' but was rendered with '{}'",
                        doc_ref,
                        self.source.name()
                    ),
                )
                .with_code("quill::ref_mismatch".to_string())
                .with_hint(
                    "the QUILL field is informational; ensure you are rendering with the intended quill"
                        .to_string(),
                ),
            )
        } else {
            None
        }
    }

    /// The schema-aware form view of `doc` — the whole-document snapshot
    /// rendered through this quill's schema.
    ///
    /// For each schema-declared field on the main leaf and on every
    /// recognised leaf, the returned [`Form`] records the current value, the
    /// schema default, and a [`form::FormFieldSource`] label.
    ///
    /// **Snapshot semantics.** The result is a read-only snapshot — re-call
    /// after editing `doc`.
    ///
    /// **Unknown leaf tags** are dropped from [`Form::leaves`] and surface as
    /// `form::unknown_card_tag` diagnostics. Validation errors are appended
    /// as `form::validation_error` diagnostics; the view itself is never
    /// altered or filtered by validation failures.
    pub fn form(&self, doc: &Document) -> Form {
        form::build_form(self, doc)
    }

    /// A blank form for the main leaf — no document values supplied. Every
    /// declared field's source is [`form::FormFieldSource::Default`] (when
    /// the schema declares a default) or [`form::FormFieldSource::Missing`].
    ///
    /// Useful as a starting state for a fresh document, or for previewing the
    /// main-leaf form without a document in hand.
    pub fn blank_main(&self) -> FormLeaf {
        FormLeaf::blank(&self.source.config().main)
    }

    /// A blank form for a leaf of the given type — no document values
    /// supplied. Returns `None` if `leaf_kind` is not declared in the
    /// quill's schema.
    ///
    /// This is the "user is about to add a new leaf" view: the UI can render
    /// the form before the leaf is committed to the document.
    pub fn blank_leaf(&self, leaf_kind: &str) -> Option<FormLeaf> {
        form::blank_card_for_tag(self, leaf_kind)
    }

    fn validate_document(&self, doc: &Document) -> Result<(), RenderError> {
        match self.source.config().validate_document(doc) {
            Ok(_) => Ok(()),
            Err(errors) => {
                // One diagnostic per ValidationError so each carries its own
                // `path` anchor for UI navigation. Consumers should iterate
                // `RenderError::diagnostics()` rather than reading a single
                // flattened message.
                //
                // `validate_document` only returns `Err` with a non-empty
                // error list, but the `ValidationFailed` variant documents
                // the same invariant — assert it here so any future
                // refactor of the underlying validator can't quietly
                // produce an empty-diags error.
                debug_assert!(!errors.is_empty(), "ValidationFailed must carry at least one diagnostic");
                let diags: Vec<Diagnostic> = errors.iter().map(|e| e.to_diagnostic()).collect();
                Err(RenderError::ValidationFailed { diags })
            }
        }
    }
}

/// Wrap a coercion error from `QuillConfig::coerce_frontmatter` /
/// `coerce_leaf` into a `RenderError::ValidationFailed` with a uniform hint.
///
/// Coercion happens before validation walks the typed document, so we don't
/// have a structured `path` here — `Diagnostic::path` is left unset.
fn coercion_error(e: impl std::fmt::Display) -> RenderError {
    RenderError::ValidationFailed {
        diags: vec![Diagnostic::new(Severity::Error, e.to_string())
            .with_code("validation::coercion_failed".to_string())
            .with_hint("Ensure all fields can be coerced to their declared types".to_string())],
    }
}

/// Merge schema `defaults` into `fields`. Fields already present in `fields`
/// win — defaults only fill gaps. Insertion order of existing fields is
/// preserved; new default keys append at the end (in `HashMap` iteration
/// order, which is fine for downstream wire serialization).
fn apply_defaults(
    fields: &IndexMap<String, QuillValue>,
    defaults: HashMap<String, QuillValue>,
) -> IndexMap<String, QuillValue> {
    let mut result = fields.clone();
    for (k, v) in defaults {
        result.entry(k).or_insert(v);
    }
    result
}

impl std::fmt::Debug for Quill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Quill")
            .field("name", &self.source.name())
            .field("backend", &self.backend.id())
            .finish()
    }
}
