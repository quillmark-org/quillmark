//! The resolved-field view — [`Quill::field_states`].
//!
//! A projection that makes field resolution observable *data* rather than an
//! inferred behavior chain: for every declared field, the value the render
//! projection would use, the [`FieldSource`] rung it came from, any diagnostics
//! anchored to it, and the schema `example:` as authoring guidance. It cuts the
//! one commitment ladder (`prose/canon/SCHEMAS.md` § "Value sources and
//! projections") through the shared producer [`resolve_value_sourced`], never a
//! parallel precedence policy.
//!
//! It does not fully collapse [`Quill::validate`]: `unknown_card` and
//! `body_disabled` are card/document-level, not per-field, so the view carries a
//! diagnostics slot at each level and every `validate()` diagnostic is bucketed
//! into exactly one of them.

use std::collections::HashMap;
use std::str::FromStr;

use indexmap::IndexMap;
use serde::Serialize;

use super::compose::resolve_value_sourced;
use super::{CardSchema, FieldSchema, FieldType, Leniency, Quill, QuillConfig};
use crate::path::{DocPath, DocSeg};
use crate::{Card, Diagnostic, Document, QuillValue, Severity};

/// Engine-owned universal key for a card's body row, collision-proof against a
/// payload field literally named `body` — a user field can never be
/// `$`-prefixed, and `Payload::to_index_map` drops `$` entries.
const BODY_KEY: &str = "$body";

/// Config-space head that anchors a `$seed` overlay diagnostic; routed to the
/// document slot rather than any document field.
const SEED_KEY: &str = "$seed";

/// The rung of the commitment ladder that produced a [`FieldState::value`].
/// Serializes lowercase (`"authored" | "default" | "zero"`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldSource {
    /// The authored value — the document's own content.
    Authored,
    /// The schema `default:` (or its content form for a content field).
    Default,
    /// The type-empty [`zero_value`](crate::quill::zero_value) floor.
    Zero,
}

/// One resolved field: the value the render projection would use, its
/// [`FieldSource`], the diagnostics anchored to it, and the schema `example:`
/// (omitted from the wire when absent).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldState {
    pub value: QuillValue,
    pub source: FieldSource,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<QuillValue>,
}

/// The main card's resolved fields plus a card-level diagnostics slot for
/// diagnostics that anchor to the card but no field (main body on a
/// body-disabled main).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MainStates {
    pub fields: IndexMap<String, FieldState>,
    pub diagnostics: Vec<Diagnostic>,
}

/// One composable card's resolved fields, with its authored `kind` (present even
/// for an unknown kind, whose paths are the bare-index `cards[<i>]` form), its
/// document-array `index`, and a card-level diagnostics slot (`unknown_card`,
/// `body_disabled`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CardStates {
    pub kind: Option<String>,
    pub index: usize,
    pub fields: IndexMap<String, FieldState>,
    pub diagnostics: Vec<Diagnostic>,
}

/// The whole resolved-field view: the main card, every composable card, and a
/// document-level diagnostics slot (`$seed` overlays, unanchored diagnostics).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldStates {
    pub main: MainStates,
    pub cards: Vec<CardStates>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Quill {
    /// The resolved-field view of `doc` against this quill's schema.
    ///
    /// Values are the render projection's per-field cut of the commitment
    /// ladder — for every declared field, the value [`compile_data`] would emit
    /// into the plate, tagged with the [`FieldSource`] rung it came from
    /// (byte-for-byte with the plate on every fixture). Diagnostics are exactly
    /// the standalone [`Quill::validate`] contract, bucketed by their
    /// [`DocPath`] into the per-row, per-card, and document slots — so a
    /// consumer reads value, provenance, and completeness from one call instead
    /// of re-implementing the ladder and re-joining `validate()` by path.
    ///
    /// [`compile_data`]: Quill::compile_data
    pub fn field_states(&self, doc: &Document) -> FieldStates {
        let config = self.config();

        // Values first: the main and each card resolved through the shared
        // ladder producer. Diagnostics are attached in the bucketing pass below.
        let main = MainStates {
            fields: resolve_card_fields(&config.main, doc.main(), &DocPath::new()),
            diagnostics: Vec::new(),
        };
        let cards = doc
            .cards()
            .iter()
            .enumerate()
            .map(|(index, card)| card_states(config, card, index))
            .collect();
        let mut states = FieldStates {
            main,
            cards,
            diagnostics: Vec::new(),
        };

        // Diagnostics = `validate()` verbatim (the raw doc, so the view's
        // diagnostics are exactly the standalone contract), routed into slots.
        for diag in self.validate(doc) {
            bucket(&mut states, diag);
        }
        states
    }
}

/// Resolve one card (main or a schema-declared kind) into its ordered
/// [`FieldState`] rows. `base` roots the field paths — empty for main, the
/// `cards.<kind>[<i>]` card root otherwise.
fn resolve_card_fields(
    schema: &CardSchema,
    card: &Card,
    base: &DocPath,
) -> IndexMap<String, FieldState> {
    // Mirror `compile_data`'s pipeline per-field: coerce (the schema looked up
    // by the authored name, as `coerce_payload` does), then NFC-normalize the
    // key (`normalize_document` runs between coercion and the ladder), then
    // resolve. Every validated ingress (parse, the mutators) restricts field
    // names to ASCII — NFC-invariant — so the normalization only respells keys
    // on a directly-constructed payload (`Payload::from_index_map`), under the
    // same NFC key the plate carries.
    //
    // The coercion is the same `conform_value(Render)` `compile_data` runs over
    // the whole payload, done here per-field so a failure anchors to its row:
    // on `Ok` the coerced value feeds the ladder; on `Err` the raw authored
    // value is kept (the ladder still reads it as Authored) and a
    // `validation::coercion_failed` diagnostic is parked for the row. Same
    // code/hint as compose.rs's `coercion_error`, but WITH a path — the
    // compile-data coercion is pathless.
    let mut resolved_input: IndexMap<String, QuillValue> = IndexMap::new();
    let mut coercion_diags: HashMap<String, Diagnostic> = HashMap::new();
    for (raw_name, value) in card.payload().to_index_map() {
        let name = crate::normalize::normalize_field_name(&raw_name);
        let entry = match schema.fields.get(&raw_name) {
            Some(field_schema) => {
                let field_path = base.field(&name);
                match QuillConfig::conform_value(
                    &value,
                    field_schema,
                    &field_path.to_string(),
                    Leniency::Render,
                ) {
                    Ok(coerced_value) => coerced_value,
                    Err(e) => {
                        coercion_diags
                            .insert(name.clone(), coercion_failed_diagnostic(&e, &field_path));
                        value
                    }
                }
            }
            None => value,
        };
        resolved_input.insert(name, entry);
    }

    let mut fields = IndexMap::new();

    // Declared rows in schema **declaration order** — the canon ordering
    // contract, carried by the `IndexMap`. (Not the validation walker, which
    // sorts alphabetically.)
    for (name, field_schema) in &schema.fields {
        let (value, source) = resolve_value_sourced(resolved_input.get(name), field_schema);
        let mut diagnostics = Vec::new();
        if let Some(d) = coercion_diags.remove(name) {
            diagnostics.push(d);
        }
        fields.insert(
            name.clone(),
            FieldState {
                value,
                source,
                diagnostics,
                example: field_example(field_schema),
            },
        );
    }

    // The body row, present iff the kind enables a body.
    if schema.body_enabled() {
        fields.insert(BODY_KEY.to_string(), body_state(schema, card));
    }

    // Undeclared authored fields, appended in authored order under their NFC
    // keys (matching the plate's): the schema is a floor, not an allowlist, so
    // these reach the plate too — value verbatim, source Authored, no example.
    for (name, value) in &resolved_input {
        if !schema.fields.contains_key(name) {
            fields.insert(
                name.clone(),
                FieldState {
                    value: value.clone(),
                    source: FieldSource::Authored,
                    diagnostics: Vec::new(),
                    example: None,
                },
            );
        }
    }

    fields
}

/// Resolve one composable card. A card whose `$kind` names a schema resolves
/// through the ladder; an unknown-kind card (declared `$kind` with no schema, or
/// a kindless card) carries its authored fields verbatim — no coercion, no
/// ladder, no `$body` row — and its `unknown_card` diagnostic arrives via
/// bucketing.
fn card_states(config: &QuillConfig, card: &Card, index: usize) -> CardStates {
    // The raw authored kind rides the entry even when it names no schema — the
    // card reports what it *claimed* to be, though its paths are `cards[<i>]`.
    let kind = card.kind().map(String::from);
    match card.kind().and_then(|k| config.card_kind(k)) {
        Some(schema) => CardStates {
            kind,
            index,
            fields: resolve_card_fields(schema, card, &DocPath::card(card.kind(), index)),
            diagnostics: Vec::new(),
        },
        None => {
            let fields = card
                .payload()
                .to_index_map()
                .into_iter()
                .map(|(name, value)| {
                    (
                        name,
                        FieldState {
                            value,
                            source: FieldSource::Authored,
                            diagnostics: Vec::new(),
                            example: None,
                        },
                    )
                })
                .collect();
            CardStates {
                kind,
                index,
                fields,
                diagnostics: Vec::new(),
            }
        }
    }
}

/// The `$body` row. The value is byte-identical to the plate's `$body`
/// (canonical Content-JSON of the card body). A body has no `default:` rung, so
/// its source is only ever [`Authored`](FieldSource::Authored) (non-blank) or
/// [`Zero`](FieldSource::Zero) (blank) — [`Default`](FieldSource::Default) is
/// unreachable for it. The example is the body schema's `example_content`.
fn body_state(schema: &CardSchema, card: &Card) -> FieldState {
    let value = QuillValue::from_json(quillmark_content::serial::to_canonical_value(card.body()));
    let source = if card.body().is_blank() {
        FieldSource::Zero
    } else {
        FieldSource::Authored
    };
    FieldState {
        value,
        source,
        diagnostics: Vec::new(),
        example: schema.body.as_ref().and_then(|b| b.example_content.clone()),
    }
}

/// A declared field's `example:` for the row — the same content-vs-scalar branch
/// as the `default:` rung: a content field's example is its content form
/// (`example_content`), every other field's is the raw `example`. `None` omits
/// the row's `example`.
fn field_example(field: &FieldSchema) -> Option<QuillValue> {
    if matches!(
        field.r#type,
        FieldType::RichText { .. } | FieldType::PlainText { .. }
    ) {
        field.example_content.clone()
    } else {
        field.example.clone()
    }
}

/// A `validation::coercion_failed` diagnostic for a field whose render coercion
/// errored — same code and hint as compose.rs's pathless `coercion_error`, but
/// anchored at the field's path.
fn coercion_failed_diagnostic(e: &super::CoercionError, field_path: &DocPath) -> Diagnostic {
    Diagnostic::new(Severity::Error, e.to_string())
        .with_code("validation::coercion_failed".to_string())
        .with_path(field_path.to_string())
        .with_hint("Ensure all fields can be coerced to their declared types".to_string())
}

/// Route one `validate()` diagnostic into exactly one slot by parsing its
/// `path` with [`DocPath::from_str`] — the phase-2 parser is total over every
/// path `validate()` emits, so a parse failure only ever means "no structured
/// anchor" and lands on the document slot.
fn bucket(states: &mut FieldStates, diag: Diagnostic) {
    let Some(path) = diag.path.as_deref().and_then(|p| DocPath::from_str(p).ok()) else {
        // No path, or unparseable → document slot.
        states.diagnostics.push(diag);
        return;
    };
    match path.segs() {
        // A `$seed` anchor is config-space (seed anchors never gate render), not
        // a document field. Also the empty case, defensively.
        [] => states.diagnostics.push(diag),
        [DocSeg::Field { name }, ..] if name == SEED_KEY => states.diagnostics.push(diag),

        // The main body is the sole `main`-headed form → main's `$body` row.
        [DocSeg::Main, ..] => row_or_slot(
            &mut states.main.fields,
            &mut states.main.diagnostics,
            BODY_KEY,
            diag,
        ),

        // A bare field chain heads on its field: a nested path
        // (`recipients[0].name`) buckets to the HEAD field's row.
        [DocSeg::Field { name }, ..] => row_or_slot(
            &mut states.main.fields,
            &mut states.main.diagnostics,
            name,
            diag,
        ),

        // A card-rooted path: whole-card (`cards[<i>]`), body, or a field chain.
        [DocSeg::Card { index, .. }, rest @ ..] => {
            let Some(card) = states.cards.get_mut(*index) else {
                // Out of range shouldn't happen — the walker indexes real cards.
                states.diagnostics.push(diag);
                return;
            };
            match rest {
                [] => card.diagnostics.push(diag),
                [DocSeg::Body, ..] => {
                    row_or_slot(&mut card.fields, &mut card.diagnostics, BODY_KEY, diag)
                }
                [DocSeg::Field { name }, ..] => {
                    row_or_slot(&mut card.fields, &mut card.diagnostics, name, diag)
                }
                // A card root followed by an index (unemittable) → card slot.
                _ => card.diagnostics.push(diag),
            }
        }

        // An index- or body-headed path is unemittable → document slot.
        _ => states.diagnostics.push(diag),
    }
}

/// Push `diag` onto the row named `key` if it exists, else onto the fallback
/// `slot`.
fn row_or_slot(
    fields: &mut IndexMap<String, FieldState>,
    slot: &mut Vec<Diagnostic>,
    key: &str,
    diag: Diagnostic,
) {
    match fields.get_mut(key) {
        Some(row) => row.diagnostics.push(diag),
        None => slot.push(diag),
    }
}

#[cfg(test)]
mod tests {
    //! Coercion-failure coverage uses a `richtext` field authored as a
    //! non-content JSON object: `conform_value(Render)` genuinely errors on it
    //! (`from_canonical_value` rejects the shape), which is the reachable
    //! render-leniency coercion failure.

    use super::*;
    use crate::quill::FileTreeNode;
    use crate::{Card, Document, Payload, Quill};
    use std::collections::HashMap as StdHashMap;

    /// Build a minimal [`Quill`] from inline `Quill.yaml` with no filesystem deps.
    fn quill_from_yaml(yaml: &str) -> Quill {
        let mut files = StdHashMap::new();
        files.insert(
            "Quill.yaml".to_string(),
            FileTreeNode::File {
                contents: yaml.as_bytes().to_vec(),
            },
        );
        let root = FileTreeNode::Directory { files };
        Quill::from_tree(root).expect("quill_from_yaml: from_tree failed")
    }

    fn parse(md: &str) -> Document {
        Document::parse(md).expect("document should parse").document
    }

    const QUILL: &str = r#"
quill:
  name: fs_test
  version: "1.0"
  backend: typst
  description: Field-state tests
main:
  body:
    example: "Example body prose."
  fields:
    title:
      type: string
    status:
      type: string
      default: draft
    notes:
      type: string
    intro:
      type: richtext
      default: "**hi**"
    recipients:
      type: array
      items:
        type: object
        properties:
          name: { type: string }
card_kinds:
  note:
    fields:
      author:
        type: string
        example: A. Author
      tag:
        type: string
"#;

    // ── Sources ──────────────────────────────────────────────────────────────

    #[test]
    fn scalar_sources_authored_default_zero() {
        let quill = quill_from_yaml(QUILL);
        // title authored; status absent (has a default); notes absent (no default).
        let doc = parse("~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: Hello\n~~~\n");
        let states = quill.field_states(&doc);
        let f = &states.main.fields;

        assert_eq!(f["title"].source, FieldSource::Authored);
        assert_eq!(f["title"].value.as_json(), &serde_json::json!("Hello"));

        assert_eq!(f["status"].source, FieldSource::Default);
        assert_eq!(f["status"].value.as_json(), &serde_json::json!("draft"));

        assert_eq!(f["notes"].source, FieldSource::Zero);
        assert_eq!(f["notes"].value.as_json(), &serde_json::json!(""));
    }

    #[test]
    fn richtext_default_reports_default_and_matches_plate() {
        let quill = quill_from_yaml(QUILL);
        // intro absent → its richtext `default:` (committed as content).
        let doc = parse("~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\n~~~\n");
        let states = quill.field_states(&doc);
        let intro = &states.main.fields["intro"];

        assert_eq!(intro.source, FieldSource::Default);
        // The value is the content form of the default, byte-equal to the plate.
        let plate = quill.compile_data(&doc).expect("compile");
        assert_eq!(intro.value.as_json(), &plate["intro"]);
        // And it is content, not the raw markdown string.
        assert!(intro.value.as_json().is_object());
    }

    #[test]
    fn present_null_is_absent_takes_default_rung() {
        let quill = quill_from_yaml(QUILL);
        // `status:` is a present-null → treated as absent → default rung.
        let doc = parse("~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\nstatus:\n~~~\n");
        let status = &quill.field_states(&doc).main.fields["status"];
        assert_eq!(status.source, FieldSource::Default);
        assert_eq!(status.value.as_json(), &serde_json::json!("draft"));
    }

    // ── Byte-for-byte with the render projection (the phase's acceptance) ────

    #[test]
    fn every_row_is_byte_for_byte_with_compile_data() {
        let quill = quill_from_yaml(QUILL);
        let md = "~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\n\
                  title: Hello\nintro: \"**bold**\"\nrecipients:\n  - name: Alice\n~~~\n\n\
                  Body prose here.\n\n\
                  ~~~card-yaml\n$kind: note\nauthor: Zed\n~~~\nNote body.\n";
        let doc = parse(md);
        let states = quill.field_states(&doc);
        let plate = quill.compile_data(&doc).expect("compile");

        // Every declared main row equals its plate field; `$body` equals plate `$body`.
        for name in ["title", "status", "notes", "intro", "recipients"] {
            assert_eq!(
                states.main.fields[name].value.as_json(),
                &plate[name],
                "main row `{name}` must be byte-for-byte with the plate"
            );
        }
        assert_eq!(states.main.fields[BODY_KEY].value.as_json(), &plate["$body"]);

        // The card's declared rows equal its plate card; `$body` equals plate `$body`.
        let plate_card = &plate["$cards"][0];
        let card = &states.cards[0];
        for name in ["author", "tag"] {
            assert_eq!(
                card.fields[name].value.as_json(),
                &plate_card[name],
                "card row `{name}` must be byte-for-byte with the plate"
            );
        }
        assert_eq!(card.fields[BODY_KEY].value.as_json(), &plate_card["$body"]);
    }

    #[test]
    fn non_nfc_key_on_a_constructed_payload_rows_under_its_nfc_spelling() {
        // Every validated ingress (parse, the mutators) restricts field names
        // to ASCII, so a non-NFC key only enters through direct construction
        // (`Payload::from_index_map`). Render NFC-normalizes it between
        // coercion and the ladder; the view mirrors that, rowing it under the
        // NFC key the plate carries — not the raw decomposed one.
        let quill = quill_from_yaml(QUILL);
        let mut map = IndexMap::new();
        // `e` + U+0301 combining acute — NFC-composes to U+00E9.
        map.insert(
            "cafe\u{301}".to_string(),
            QuillValue::from_json(serde_json::json!("hot")),
        );
        let mut payload = Payload::from_index_map(map);
        payload.set_quill("fs_test@1.0".parse().unwrap());
        payload.set_kind("main");
        let main = Card::from_parts(payload, quillmark_content::Content::empty());
        let doc = Document::from_main_and_cards(main, Vec::new());
        let states = quill.field_states(&doc);

        assert!(!states.main.fields.contains_key("cafe\u{301}"));
        let row = &states.main.fields["caf\u{e9}"];
        assert_eq!(row.source, FieldSource::Authored);
        assert_eq!(row.value.as_json(), &serde_json::json!("hot"));
    }

    // ── The body row ─────────────────────────────────────────────────────────

    #[test]
    fn body_row_authored_vs_blank_source() {
        let quill = quill_from_yaml(QUILL);

        let authored =
            parse("~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\n~~~\n\nHello body.\n");
        assert_eq!(
            quill.field_states(&authored).main.fields[BODY_KEY].source,
            FieldSource::Authored
        );

        let blank = parse("~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\n~~~\n");
        let body = &quill.field_states(&blank).main.fields[BODY_KEY];
        assert_eq!(body.source, FieldSource::Zero);
        assert!(body.value.as_json().is_object(), "blank body is empty content");
    }

    const BODY_DISABLED_QUILL: &str = r#"
quill:
  name: bd_test
  version: "1.0"
  backend: typst
  description: Body-disabled test
main:
  fields:
    title:
      type: string
card_kinds:
  stamp:
    body:
      enabled: false
    fields:
      label:
        type: string
"#;

    #[test]
    fn body_disabled_kind_omits_body_row_and_buckets_to_card_slot() {
        let quill = quill_from_yaml(BODY_DISABLED_QUILL);
        // Stray prose authored on a body-disabled card.
        let doc = parse(
            "~~~card-yaml\n$quill: bd_test@1.0\n$kind: main\ntitle: T\n~~~\n\n\
             ~~~card-yaml\n$kind: stamp\nlabel: L\n~~~\nStray prose.\n",
        );
        let states = quill.field_states(&doc);
        let card = &states.cards[0];

        assert!(
            !card.fields.contains_key(BODY_KEY),
            "a body-disabled kind has no `$body` row"
        );
        assert!(card.fields.contains_key("label"), "declared rows still present");
        assert!(
            card.diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("validation::body_disabled")),
            "body_disabled lands in the card slot: {:?}",
            card.diagnostics
        );
    }

    // ── Unknown-kind card ────────────────────────────────────────────────────

    #[test]
    fn unknown_kind_card_shape_and_slot() {
        let quill = quill_from_yaml(QUILL);
        let doc = parse(
            "~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\n~~~\n\n\
             ~~~card-yaml\n$kind: mystery\nfoo: bar\n~~~\nUnread body.\n",
        );
        let card = &quill.field_states(&doc).cards[0];

        assert_eq!(card.kind.as_deref(), Some("mystery"));
        assert_eq!(card.index, 0);
        // Authored fields only — no `$body` row, no ladder.
        assert_eq!(card.fields["foo"].source, FieldSource::Authored);
        assert_eq!(card.fields["foo"].value.as_json(), &serde_json::json!("bar"));
        assert!(!card.fields.contains_key(BODY_KEY));
        assert!(
            card.diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("validation::unknown_card")),
            "unknown_card lands in the card slot: {:?}",
            card.diagnostics
        );
    }

    // ── Bucketing ────────────────────────────────────────────────────────────

    #[test]
    fn type_mismatch_buckets_to_its_row() {
        // A bare-field type mismatch routes to the field's row.
        let q = quill_from_yaml(
            "quill:\n  name: tm\n  version: \"1.0\"\n  backend: typst\n  description: tm\n\
             main:\n  fields:\n    count:\n      type: integer\n",
        );
        let d = parse("~~~card-yaml\n$quill: tm@1.0\n$kind: main\ncount: \"not-a-number\"\n~~~\n");
        let states = q.field_states(&d);
        assert!(
            states.main.fields["count"]
                .diagnostics
                .iter()
                .any(|x| x.code.as_deref() == Some("validation::type_mismatch")),
            "type_mismatch must land on the `count` row: {:?}",
            states.main.fields["count"].diagnostics
        );
    }

    #[test]
    fn seed_warning_buckets_to_document_slot() {
        let quill = quill_from_yaml(QUILL);
        // A `$seed` overlay for an unknown kind → an advisory warning anchored at
        // `$seed.bogus`, which is config-space → the document slot.
        let doc = parse(
            "~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\n$seed:\n  bogus:\n    x: 1\n~~~\n",
        );
        let states = quill.field_states(&doc);
        assert!(
            states
                .diagnostics
                .iter()
                .any(|d| d.path.as_deref() == Some("$seed.bogus")),
            "a $seed warning belongs on the document slot: {:?}",
            states.diagnostics
        );
    }

    #[test]
    fn nested_must_fill_buckets_to_head_field_row() {
        let quill = quill_from_yaml(QUILL);
        // `!must_fill` on `recipients[0].name` — a nested marker on the
        // `recipients` field.
        let doc = parse(
            "~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\n\
             recipients:\n  - name: !must_fill\n~~~\n",
        );
        let states = quill.field_states(&doc);
        let row = &states.main.fields["recipients"];
        assert!(
            row.diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("validation::must_fill")),
            "a nested must_fill buckets to the head field's row: {:?}",
            row.diagnostics
        );
        // And nowhere else.
        assert!(states.main.diagnostics.is_empty());
        assert!(states.diagnostics.is_empty());
    }

    // ── Undeclared authored field ────────────────────────────────────────────

    #[test]
    fn undeclared_authored_field_row_is_authored() {
        let quill = quill_from_yaml(QUILL);
        let doc = parse(
            "~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\nextra: whatever\n~~~\n",
        );
        let row = &quill.field_states(&doc).main.fields["extra"];
        assert_eq!(row.source, FieldSource::Authored);
        assert_eq!(row.value.as_json(), &serde_json::json!("whatever"));
        assert!(row.example.is_none());
    }

    // ── Completeness ─────────────────────────────────────────────────────────

    #[test]
    fn every_validate_diagnostic_appears_exactly_once() {
        let quill = quill_from_yaml(QUILL);
        // A document that raises several diagnostics without any coercion
        // failure (so no extra `coercion_failed` rows are added by the view):
        // a nested must_fill, a $seed warning, and an unknown-kind card.
        let doc = parse(
            "~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\n\
             recipients:\n  - name: !must_fill\n$seed:\n  bogus:\n    x: 1\n~~~\n\n\
             ~~~card-yaml\n$kind: mystery\nfoo: bar\n~~~\n",
        );
        let expected = quill.validate(&doc);
        assert!(!expected.is_empty(), "the fixture must raise diagnostics");

        let states = quill.field_states(&doc);
        let mut collected: Vec<Diagnostic> = Vec::new();
        collected.extend(states.diagnostics.iter().cloned());
        collected.extend(states.main.diagnostics.iter().cloned());
        for row in states.main.fields.values() {
            collected.extend(row.diagnostics.iter().cloned());
        }
        for card in &states.cards {
            collected.extend(card.diagnostics.iter().cloned());
            for row in card.fields.values() {
                collected.extend(row.diagnostics.iter().cloned());
            }
        }

        assert_eq!(
            collected.len(),
            expected.len(),
            "every validate() diagnostic lands in exactly one slot (no more, no less)"
        );
        for d in &expected {
            assert!(
                collected.contains(d),
                "validate() diagnostic missing from the view: {d:?}"
            );
        }
    }

    // ── Coercion-failure row ─────────────────────────────────────────────────

    #[test]
    fn render_coercion_failure_parks_diagnostic_on_its_row() {
        let quill = quill_from_yaml(QUILL);
        // `intro` is richtext; a non-content JSON object cannot be coerced under
        // Render leniency (`from_canonical_value` rejects the shape).
        let doc = parse(
            "~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\nintro:\n  not: content\n~~~\n",
        );
        let intro = &quill.field_states(&doc).main.fields["intro"];
        assert!(
            intro
                .diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("validation::coercion_failed")
                    && d.path.as_deref() == Some("intro")),
            "a Render coercion failure parks a path-anchored diagnostic on the row: {:?}",
            intro.diagnostics
        );
        // The raw authored value is kept (Authored), not dropped.
        assert_eq!(intro.source, FieldSource::Authored);
        assert_eq!(intro.value.as_json(), &serde_json::json!({ "not": "content" }));
    }

    #[test]
    fn field_source_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&FieldSource::Authored).unwrap(),
            "\"authored\""
        );
        assert_eq!(
            serde_json::to_string(&FieldSource::Default).unwrap(),
            "\"default\""
        );
        assert_eq!(serde_json::to_string(&FieldSource::Zero).unwrap(), "\"zero\"");
    }

    #[test]
    fn field_state_omits_absent_example_on_the_wire() {
        let state = FieldState {
            value: QuillValue::from_json(serde_json::json!("x")),
            source: FieldSource::Authored,
            diagnostics: Vec::new(),
            example: None,
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(!json.contains("example"), "absent example is skipped: {json}");
    }
}
