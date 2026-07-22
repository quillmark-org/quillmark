//! Consumer-facing operations on a [`Quill`]: validation, seeding, and the
//! zero-filled compile to backend wire JSON. All pure reads of the quill's
//! config — no backend, no engine (those live in the `quillmark` crate).

use std::str::FromStr;

use indexmap::IndexMap;

use super::field_states::FieldSource;
use super::{seed, CardSchema, FieldSchema, FieldType, Quill, QuillConfig};
use crate::normalize::normalize_document;
use crate::quill::zero_value;
use crate::path::DocPath;
use crate::{
    Card, Diagnostic, Document, Payload, QuillValue, RenderError, SeedOverlay, Severity, Version,
};

impl Quill {
    /// [`QuillConfig::compile_data`] on this quill's config.
    pub fn compile_data(&self, doc: &Document) -> Result<serde_json::Value, RenderError> {
        self.config().compile_data(doc)
    }

    /// Validate without backend compilation.
    pub fn dry_run(&self, doc: &Document) -> Result<(), RenderError> {
        self.config().dry_run(doc)
    }

    /// [`QuillConfig::check_quill_reference`] on this quill's config.
    pub fn check_quill_reference(&self, doc: &Document) -> Result<(), RenderError> {
        self.config().check_quill_reference(doc)
    }
}

/// The document→data compile is a pure config read: coercion, validation,
/// normalization, and zero-fill consult only the parsed schemas — never the
/// quill's file tree. Living on [`QuillConfig`] lets a consumer that only
/// compiles data (e.g. a live session's `apply`) retain the config alone
/// rather than the whole quill with its font/package bytes.
impl QuillConfig {
    /// Applies coercion, validation, normalization, and **zero-filled render**:
    /// every absent schema field is resolved to its authored value, else its
    /// schema default, else its type-empty zero value — in this plate-JSON
    /// projection only, never in the persisted document. A merely *incomplete*
    /// document compiles fine; only a *malformed* one (a value that won't
    /// coerce/validate) errors. A `!must_fill` placeholder never gates render —
    /// it surfaces as a non-fatal warning from `validate`. See
    /// `prose/canon/SCHEMAS.md`.
    pub fn compile_data(&self, doc: &Document) -> Result<serde_json::Value, RenderError> {
        let coerced = self.coerce_and_validate(doc)?;
        let normalized = normalize_document(coerced)?;

        let main_resolved = resolve_fields(&normalized.main().payload().to_index_map(), &self.main);
        let cards_resolved: Vec<Card> = normalized
            .cards()
            .iter()
            .map(|card| {
                let fields = match self.card_kind(card.kind().unwrap_or("")) {
                    Some(schema) => resolve_fields(&card.payload().to_index_map(), schema),
                    None => card.payload().to_index_map(),
                };
                Card::from_parts(rebuild_payload_with_meta(card, fields), card.body().clone())
            })
            .collect();

        let final_main = Card::from_parts(
            rebuild_payload_with_meta(normalized.main(), main_resolved),
            normalized.main().body().clone(),
        );
        let final_doc = Document::from_main_and_cards(final_main, cards_resolved);

        Ok(final_doc.to_plate_json())
    }

    /// Validate without backend compilation.
    pub fn dry_run(&self, doc: &Document) -> Result<(), RenderError> {
        self.check_quill_reference(doc)?;
        self.coerce_and_validate(doc).map(|_| ())
    }

    fn coerce_and_validate(&self, doc: &Document) -> Result<Document, RenderError> {
        let coerced_payload = self
            .coerce_payload(&doc.main().payload().to_index_map())
            .map_err(coercion_error)?;

        let mut coerced_cards: Vec<Card> = Vec::with_capacity(doc.cards().len());
        for card in doc.cards() {
            let coerced_fields = self
                .coerce_card(card.kind().unwrap_or(""), &card.payload().to_index_map())
                .map_err(coercion_error)?;
            coerced_cards.push(Card::from_parts(
                rebuild_payload_with_meta(card, coerced_fields),
                card.body().clone(),
            ));
        }

        let coerced_main = Card::from_parts(
            rebuild_payload_with_meta(doc.main(), coerced_payload),
            doc.main().body().clone(),
        );
        let coerced_doc = Document::from_main_and_cards(coerced_main, coerced_cards);

        // Only *malformed* input is fatal (a value that won't coerce/validate).
        // An incomplete document — absent fields or `!must_fill` placeholders —
        // renders fine via zero-fill. `validate_document` returns `Err` only
        // with a non-empty error list; each error keeps its own `path` for UI
        // navigation.
        self.validate_document(&coerced_doc).map_err(|errors| {
            RenderError::new(errors.iter().map(|e| e.to_diagnostic()).collect())
        })?;

        Ok(coerced_doc)
    }

    /// Enforce the document's `$quill` reference (`name@selector`) against this
    /// quill, failing with a `quill::name_mismatch` / `quill::version_mismatch`
    /// diagnostic if either component diverges. The document is well-formed; it
    /// was paired with the wrong quill
    /// — a different format, or an incompatible version of one — which yields
    /// undefined output, so it errors rather than warns.
    ///
    /// Name is the prerequisite (a selector belongs to a *named* quill): a name
    /// mismatch (`quill::name_mismatch`) short-circuits and the version is left
    /// unevaluated; otherwise the selector is checked (`quill::version_mismatch`).
    /// The version parses infallibly in practice (validated at load); if it
    /// somehow doesn't, the version check is skipped.
    pub fn check_quill_reference(&self, doc: &Document) -> Result<(), RenderError> {
        let doc_ref = doc.quill_reference();

        if doc_ref.name.as_str() != self.name {
            return Err(quill_mismatch(
                format!(
                    "document declares $quill '{}' but was rendered with '{}'",
                    doc_ref, self.name
                ),
                "quill::name_mismatch",
                "render with the quill named by $quill, or update the $quill name",
            ));
        }

        let Ok(quill_version) = Version::from_str(&self.version) else {
            return Ok(());
        };
        if !doc_ref.selector.matches(quill_version) {
            return Err(quill_mismatch(
                format!(
                    "document declares $quill '{}' but the loaded quill is version '{}'",
                    doc_ref, quill_version
                ),
                "quill::version_mismatch",
                "render with a quill whose version satisfies the selector, or update the $quill selector",
            ));
        }

        Ok(())
    }
}

impl Quill {
    /// Validate `doc` against this quill's schema, returning every diagnostic
    /// (an empty `Vec` when the document is valid).
    ///
    /// The editor-facing validation surface. Forwards the canonical
    /// `validation::*` diagnostics verbatim (same code, `path`, `hint`) so
    /// consumers route on the code without parsing message text: type
    /// mismatches, unknown card kinds, body-on-disabled-body, and the non-fatal
    /// `validation::must_fill` warning — the only non-fatal one; the rest are
    /// blockers. Field absence is not surfaced (it zero-fills at render).
    ///
    /// Field values, defaults, and presentation order are not part of this
    /// surface — read them from the [`Document`] payload and the quill schema
    /// (`quill.config().schema()`, whose key order is display order).
    pub fn validate(&self, doc: &Document) -> Vec<Diagnostic> {
        let mut diags = match self.config().validate_document(doc) {
            Ok(()) => Vec::new(),
            Err(errors) => errors.iter().map(|e| e.to_diagnostic()).collect(),
        };
        diags.extend(validate_fills(self.config(), doc));
        diags.extend(self.validate_seed(doc));
        diags
    }

    /// Advisory validation of the main card's `$seed` overlays.
    ///
    /// Seed overlays are editor-surface only: they never gate render
    /// (`compile_data` / `dry_run` ignore `$seed`), so every diagnostic here is
    /// a **warning** rooted at `$seed.<kind>[.<field>]`. An overlay keyed by a
    /// name that is not a declared `card_kind` is flagged; otherwise each
    /// overlaid field is checked against that kind's schema with the same
    /// conformance core the schema's own `example:` / `default:` literals use
    /// (partial values allowed, no null/absence gating).
    /// The reserved `$body` key is the body override, not a field, and is
    /// skipped.
    fn validate_seed(&self, doc: &Document) -> Vec<Diagnostic> {
        let Some(seed_map) = doc.main().payload().seed() else {
            return Vec::new();
        };
        let config = self.config();
        let mut diags = Vec::new();
        for (kind, overlay) in seed_map {
            let Some(card_schema) = config.card_kind(kind) else {
                diags.push(
                    Diagnostic::new(
                        Severity::Warning,
                        format!("`$seed` overlay targets unknown card kind `{kind}`"),
                    )
                    .with_code("validation::seed_unknown_kind".to_string())
                    .with_path(DocPath::new().field("$seed").field(kind).to_string())
                    .with_hint(format!(
                        "Remove the `{kind}` overlay, or rename it to a declared card kind."
                    )),
                );
                continue;
            };
            let Some(obj) = overlay.as_object() else {
                diags.push(
                    Diagnostic::new(
                        Severity::Warning,
                        format!("`$seed.{kind}` must be a mapping of field overrides"),
                    )
                    .with_code("validation::seed_overlay_shape".to_string())
                    .with_path(DocPath::new().field("$seed").field(kind).to_string()),
                );
                continue;
            };
            for (field, value) in obj {
                if field == "$body" {
                    continue;
                }
                let field_path = DocPath::new().field("$seed").field(kind).field(field);
                let Some(field_schema) = card_schema.fields.get(field) else {
                    diags.push(
                        Diagnostic::new(
                            Severity::Warning,
                            format!("`$seed.{kind}.{field}` is not a field of card kind `{kind}`"),
                        )
                        .with_code("validation::seed_unknown_field".to_string())
                        .with_path(field_path.to_string()),
                    );
                    continue;
                };
                let qv = QuillValue::from_json(value.clone());
                for violation in
                    super::validation::validate_schema_literal(field_schema, &qv, &field_path)
                {
                    diags.push(seed_violation_diagnostic(&violation));
                }
            }
        }
        diags
    }

    /// Seed a starter [`Document`]: the main card plus one instance of each
    /// declared composable card kind, each committing its fields' `example`
    /// values and leaving all other fields absent (interpolated at render:
    /// `default` → type-empty zero). The committed, structured "filled-out" twin
    /// of the [`blueprint`](crate::quill::QuillConfig::blueprint). See the
    /// `seed` module.
    pub fn seed_document(&self) -> Document {
        seed::seed_document(self)
    }

    /// Seed a starter main [`Card`] (carries `$quill`). Use as the main card of
    /// a fresh document. See [`Quill::seed_document`].
    pub fn seed_main(&self) -> Card {
        seed::seed_main(self)
    }

    /// Seed a starter composable [`Card`] of the given kind (carries `$kind`),
    /// layering an optional per-kind [`SeedOverlay`] over the schema-example
    /// base (`overlay › example › absent`); `None` if the kind is not declared.
    /// Use to add a new card to a document — pass the document's `$seed` entry
    /// for the kind (`doc.main().seed().and_then(|m| m.get(card_kind)).and_then(SeedOverlay::from_json)`)
    /// so a card spawned into a template-derived document inherits its curated
    /// starting values, and `None` for the bare schema seed.
    pub fn seed_card(&self, card_kind: &str, overlay: Option<&SeedOverlay>) -> Option<Card> {
        seed::seed_card_for_kind(self, card_kind, overlay)
    }
}

/// A single-diagnostic quill-mismatch failure. `path` is unset — the
/// mismatch is the root `$quill` line, not a field.
fn quill_mismatch(message: String, code: &str, hint: &str) -> RenderError {
    RenderError::from_diag(
        Diagnostic::new(Severity::Error, message)
            .with_code(code.to_string())
            .with_hint(hint.to_string()),
    )
}

/// Render a seed-overlay validation error as a **warning**-severity diagnostic
/// — seed overlays are advisory and never gate render. The error's `path` is
/// already rooted at `$seed.<kind>.<field>` by the caller.
fn seed_violation_diagnostic(v: &super::validation::ValidationError) -> Diagnostic {
    let mut diag = Diagnostic::new(Severity::Warning, v.to_string())
        .with_code(v.code().to_string())
        .with_path(v.path().to_string());
    if let Some(hint) = v.hint() {
        diag = diag.with_hint(hint);
    }
    diag
}

/// Wrap a coercion error into a `validation::coercion_failed` failure.
/// `Diagnostic::path` is unset — coercion runs before structured validation.
fn coercion_error(e: impl std::fmt::Display) -> RenderError {
    RenderError::from_diag(
        Diagnostic::new(Severity::Error, e.to_string())
            .with_code("validation::coercion_failed".to_string())
            .with_hint("Ensure all fields can be coerced to their declared types".to_string()),
    )
}

/// Resolve every schema field absent from `fields`, by precedence: an authored
/// value wins; else the schema `default:`; else the type-empty [`zero_value`].
/// This is the zero-filled render projection — the fill lives only here and is
/// never persisted (see `prose/canon/SCHEMAS.md`). Non-schema fields already
/// present are preserved untouched.
///
/// Null ≡ absent **at every level**: a null or absent value — top-level field,
/// typed-dictionary property, or typed-table cell — carries no data and resolves
/// like an omitted one (default, else zero) rather than projecting a bare null
/// into the plate. A `!must_fill` placeholder is a present-null (or a suggested
/// value) on this path; its marker is render-irrelevant.
fn resolve_fields(
    fields: &IndexMap<String, QuillValue>,
    schema: &CardSchema,
) -> IndexMap<String, QuillValue> {
    let mut result = fields.clone();
    for (name, field) in &schema.fields {
        let resolved = resolve_value(result.get(name), field);
        result.insert(name.clone(), resolved);
    }
    result
}

/// The value half of [`resolve_value_sourced`], discarding the rung tag — the
/// zero-filled render projection consumes only the value. `field_states`
/// consumes both, so the source is produced by the one branch here rather than
/// re-derived against a parallel ladder.
fn resolve_value(value: Option<&QuillValue>, field: &FieldSchema) -> QuillValue {
    resolve_value_sourced(value, field).0
}

/// Resolve one (possibly absent or null) value against its field schema,
/// reporting the [`FieldSource`] rung that produced it, and applying null ≡
/// absent recursively so no bare null reaches the plate:
///
/// - A null or absent value becomes the schema `default:`
///   ([`Default`](FieldSource::Default)), else the type-empty [`zero_value`]
///   ([`Zero`](FieldSource::Zero)).
/// - A present **typed dictionary** is rebuilt from its declared properties so a
///   null/absent property zero-fills and the projection matches the schema shape.
///   Source keys the schema does not declare pass through verbatim, matching
///   `config::coerce_object_props`'s coercion-time behavior — the schema is a
///   floor, not an allowlist, so an undeclared `note:` on a typed dict reaches
///   the plate instead of being silently dropped.
/// - A present **typed array** resolves each element against the item schema, so
///   a null element zero-fills in place.
/// - Any other present value is returned unchanged.
///
/// Every present shape is [`Authored`](FieldSource::Authored) (the nested
/// zero-fill inside a dict/array is a projection detail, not a source change).
/// The source is the byproduct of the same branch that computes the value, so
/// the render projection ([`resolve_value`]) and the field-state view cut the
/// one commitment ladder rather than each re-deriving precedence
/// (`prose/canon/SCHEMAS.md` § "Value sources and projections").
pub(crate) fn resolve_value_sourced(
    value: Option<&QuillValue>,
    field: &FieldSchema,
) -> (QuillValue, FieldSource) {
    let present = value.filter(|v| !v.as_json().is_null());
    let Some(v) = present else {
        // A content-bearing field (`richtext` or its literal sibling `plaintext`)
        // commits the *content* form of its default (`default_content`, cached at
        // load by `from_yaml`), so the seam carries canonical Content-JSON the
        // backend can classify. It must NOT fall through to the raw `default`:
        // `resolve_fields` runs after `coerce_and_validate`, so a bare authored
        // string injected here would reach the plate uncoerced and be misread. A
        // content field with no cached `default_content` (only reachable via a
        // serde-built `QuillConfig`, never `from_yaml`) zero-fills to the empty
        // content.
        if matches!(
            field.r#type,
            FieldType::RichText { .. } | FieldType::PlainText { .. }
        ) {
            return match field.default_content.clone() {
                Some(content) => (content, FieldSource::Default),
                None => (zero_value(field), FieldSource::Zero),
            };
        }
        // Non-content: `default_content` is always `None`, so use the raw
        // `default`, then the type-empty zero.
        return match field.default.clone() {
            Some(default) => (default, FieldSource::Default),
            None => (zero_value(field), FieldSource::Zero),
        };
    };
    let resolved = match (&field.r#type, &field.properties, &field.items) {
        (FieldType::Object, Some(props), _) => {
            let obj = v.as_json().as_object();
            let mut out = serde_json::Map::new();
            for (pname, pschema) in props {
                let pv = obj
                    .and_then(|o| o.get(pname))
                    .map(|j| QuillValue::from_json(j.clone()));
                out.insert(
                    pname.clone(),
                    resolve_value(pv.as_ref(), pschema).into_json(),
                );
            }
            // Preserve undeclared keys verbatim; only rebuild the ones the
            // schema names. Skips keys already emitted above so a declared
            // property keeps its resolved (zero-filled) value.
            if let Some(o) = obj {
                for (k, v) in o {
                    if !props.contains_key(k) {
                        out.insert(k.clone(), v.clone());
                    }
                }
            }
            QuillValue::from_json(serde_json::Value::Object(out))
        }
        (FieldType::Array, _, Some(items)) => {
            let arr = v.as_json().as_array().cloned().unwrap_or_default();
            let out: Vec<serde_json::Value> = arr
                .into_iter()
                .map(|e| resolve_value(Some(&QuillValue::from_json(e)), items).into_json())
                .collect();
            QuillValue::from_json(serde_json::Value::Array(out))
        }
        _ => v.clone(),
    };
    (resolved, FieldSource::Authored)
}

/// Build a [`Payload`] from a coerced/defaulted field map, re-attaching `$quill`
/// / `$kind` / `$id` from `source`. Comments are dropped — this payload feeds
/// backend rendering, not round-trip storage.
fn rebuild_payload_with_meta(source: &Card, fields: IndexMap<String, QuillValue>) -> Payload {
    let mut payload = Payload::from_index_map(fields);
    if let Some(q) = source.quill() {
        payload.set_quill(q.clone());
    }
    if let Some(k) = source.kind() {
        payload.set_kind(k.to_string());
    }
    if let Some(id) = source.id() {
        payload.set_id(id.to_string());
    }
    payload
}

/// Surface every `!must_fill` marker as a non-fatal **warning**, root-and-nested
/// across the main card and every composable card.
///
/// The marker fires whether or not the cell carries a suggested value, and never
/// gates render (the cell zero-fills or uses its suggested value). A strict
/// consumer treats any outstanding marker as "not done".
fn validate_fills(config: &QuillConfig, doc: &Document) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    collect_fill_diags(doc.main(), &DocPath::new(), &mut diags);
    for (index, card) in doc.cards().iter().enumerate() {
        // A card whose declared `$kind` has no schema drops the kind segment and
        // stays `cards[<i>]`, matching `validate_typed_document`; a
        // schema-declared kind qualifies as `cards.<kind>[<i>]`.
        let kind = card.kind().filter(|k| config.card_kind(k).is_some());
        collect_fill_diags(card, &DocPath::card(kind, index), &mut diags);
    }
    diags
}

/// Append a `validation::must_fill` warning for each marker in `card`'s fields.
fn collect_fill_diags(card: &Card, base: &DocPath, out: &mut Vec<Diagnostic>) {
    let payload = card.payload();
    for (key, value) in payload {
        let field_path = base.field(key);
        // Root marker (the field-level `fill` flag) plus any nested markers
        // carried on the value tree, each rebased onto the field path.
        if payload.is_fill(key) {
            out.push(fill_warning(&field_path));
        }
        for nested in value.nonroot_fill_paths() {
            let nested_path = nested.iter().fold(field_path.clone(), |p, s| p.segment(s));
            out.push(fill_warning(&nested_path));
        }
    }
}

fn fill_warning(path: &DocPath) -> Diagnostic {
    let path = path.to_string();
    Diagnostic::new(
        Severity::Warning,
        format!("Field `{path}` is marked `!must_fill` — a placeholder awaiting a value."),
    )
    .with_code("validation::must_fill".to_string())
    .with_path(path)
    .with_hint(
        "Replace the value and drop the `!must_fill` marker, or remove the marker if the \
         current value is intended."
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn field(yaml: &str) -> FieldSchema {
        let value = QuillValue::from_yaml_str(yaml).unwrap();
        FieldSchema::from_quill_value("field".to_string(), &value).unwrap()
    }

    // A typed dictionary carrying a key the schema does not declare keeps that
    // key in the resolved projection (regression guard for #803: the schema is
    // a floor, not an allowlist). Declared-but-absent properties still zero-fill.
    #[test]
    fn typed_dict_preserves_undeclared_keys() {
        let schema = field(
            r#"
type: object
properties:
  street: { type: string }
  zip: { type: integer }
"#,
        );
        let input = QuillValue::from_json(json!({ "street": "1 Infinite Loop", "note": "extra" }));

        let resolved = resolve_value(Some(&input), &schema).into_json();

        assert_eq!(
            resolved,
            json!({ "street": "1 Infinite Loop", "zip": 0, "note": "extra" })
        );
    }

    // A card whose declared `$kind` has no schema anchors its `!must_fill`
    // warning at the bare-index root `cards[<i>].<field>` — matching
    // `validate_typed_document`'s unknown-card path — never `cards.<kind>[<i>]`.
    // A truly kindless card (no `$kind`) stays bare-index the same way.
    #[test]
    fn unknown_kind_card_fill_path_is_bare_index() {
        use crate::document::Payload;

        let config = QuillConfig::from_yaml(
            r#"
quill:
  name: fills_test
  backend: typst
  description: fill path tests
  version: 1.0.0
main:
  fields:
    title:
      type: string
      default: ""
card_kinds:
  known:
    fields:
      note:
        type: string
"#,
        )
        .unwrap();

        let mut main = Payload::new();
        main.set_quill("fills_test@1.0.0".parse().unwrap());
        main.set_kind("main");
        let main = Card::from_parts(main, quillmark_content::Content::empty());

        // Index 0: a card whose `$kind` ("mystery") is not a declared card kind.
        let mut unknown = Card::new("mystery").unwrap();
        unknown
            .store_fill("note", QuillValue::from_json(json!(null)))
            .unwrap();

        // Index 1: a kindless card (no `$kind` at all).
        let mut kindless =
            Card::from_parts(Payload::new(), quillmark_content::Content::empty());
        kindless
            .store_fill("memo", QuillValue::from_json(json!(null)))
            .unwrap();

        let doc = Document::from_main_and_cards(main, vec![unknown, kindless]);
        let paths: Vec<String> = validate_fills(&config, &doc)
            .iter()
            .filter_map(|d| d.path.clone())
            .collect();

        assert!(
            paths.contains(&"cards[0].note".to_string()),
            "unknown-kind card fill must anchor at the bare index; got {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.starts_with("cards.mystery")),
            "unknown-kind card fill must NOT carry the kind segment; got {paths:?}"
        );
        assert!(
            paths.contains(&"cards[1].memo".to_string()),
            "kindless card fill must anchor at the bare index; got {paths:?}"
        );
    }

    // Same pass-through inside a typed-table row (the Array→Object recursion).
    #[test]
    fn typed_table_row_preserves_undeclared_keys() {
        let schema = field(
            r#"
type: array
items:
  type: object
  properties:
    name: { type: string }
"#,
        );
        let input = QuillValue::from_json(json!([{ "name": "ACME", "year": 2020 }]));

        let resolved = resolve_value(Some(&input), &schema).into_json();

        assert_eq!(resolved, json!([{ "name": "ACME", "year": 2020 }]));
    }
}
