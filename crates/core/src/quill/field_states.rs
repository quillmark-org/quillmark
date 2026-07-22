//! The resolved-value view — [`Quill::field_states`].
//!
//! A projection that makes field resolution observable *data* rather than an
//! inferred behavior chain: for every declared field, the value the render
//! projection would use and the [`FieldSource`] rung it came from. It cuts the
//! one commitment ladder (`prose/canon/SCHEMAS.md` § "Value sources and
//! projections") through the shared producer [`resolve_value_sourced`], never a
//! parallel precedence policy.
//!
//! Values only: diagnostics stay [`Quill::validate`]'s job (the editor merges
//! `validate()` with its own producers regardless, so bucketing here would
//! delete no consumer code), and schema guidance (`example:`, labels, groups)
//! reads from [`Quill::schema`]. The view answers one question — what value
//! renders, and from which rung.

use indexmap::IndexMap;
use serde::Serialize;

use super::compose::resolve_value_sourced;
use super::{CardSchema, Leniency, Quill, QuillConfig};
use crate::{Card, Document, QuillValue};

/// Engine-owned universal key for a card's body row, collision-proof against a
/// payload field literally named `body` — a user field can never be
/// `$`-prefixed, and `Payload::to_index_map` drops `$` entries.
const BODY_KEY: &str = "$body";

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

/// One resolved field: the value the render projection would use and its
/// [`FieldSource`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldState {
    pub value: QuillValue,
    pub source: FieldSource,
}

/// The main card's resolved fields, keyed by name in declaration order (with
/// `$body` under its key when the main enables a body).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MainStates {
    pub fields: IndexMap<String, FieldState>,
}

/// One composable card's resolved fields, with its authored `kind` (present even
/// for an unknown kind, which carries its fields verbatim) and its
/// document-array `index`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CardStates {
    pub kind: Option<String>,
    pub index: usize,
    pub fields: IndexMap<String, FieldState>,
}

/// The whole resolved-value view: the main card and every composable card.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldStates {
    pub main: MainStates,
    pub cards: Vec<CardStates>,
}

impl Quill {
    /// The resolved-value view of `doc` against this quill's schema.
    ///
    /// For every declared field, the value [`compile_data`] would emit into the
    /// plate (byte-for-byte with the plate on every fixture), tagged with the
    /// [`FieldSource`] rung it came from — one call for value and provenance
    /// instead of re-implementing the ladder. Completeness and errors stay
    /// [`Quill::validate`]'s; this view carries no diagnostics.
    ///
    /// [`compile_data`]: Quill::compile_data
    pub fn field_states(&self, doc: &Document) -> FieldStates {
        let config = self.config();
        let main = MainStates {
            fields: resolve_card_fields(&config.main, doc.main()),
        };
        let cards = doc
            .cards()
            .iter()
            .enumerate()
            .map(|(index, card)| card_states(config, card, index))
            .collect();
        FieldStates { main, cards }
    }
}

/// Resolve one card (main or a schema-declared kind) into its ordered
/// [`FieldState`] rows.
fn resolve_card_fields(schema: &CardSchema, card: &Card) -> IndexMap<String, FieldState> {
    // Mirror `compile_data`'s pipeline per-field so the value is byte-for-byte
    // with the plate: coerce under Render leniency (the schema looked up by the
    // authored name, as `coerce_payload` does), then NFC-normalize the key
    // (`normalize_document` runs between coercion and the ladder), then resolve.
    // Every validated ingress (parse, the mutators) restricts field names to
    // ASCII — NFC-invariant — so the normalization only respells keys on a
    // directly-constructed payload (`Payload::from_index_map`), under the same
    // NFC key the plate carries. A value the render coercion cannot conform is
    // kept raw (the ladder reads it Authored), exactly as `compile_data` leaves
    // it — the failure surfaces through `validate()`, not here.
    let mut resolved_input: IndexMap<String, QuillValue> = IndexMap::new();
    for (raw_name, value) in card.payload().to_index_map() {
        let name = crate::normalize::normalize_field_name(&raw_name);
        let entry = match schema.fields.get(&raw_name) {
            Some(field_schema) => {
                QuillConfig::conform_value(&value, field_schema, &name, Leniency::Render)
                    .unwrap_or(value)
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
        fields.insert(name.clone(), FieldState { value, source });
    }

    // The body row, present iff the kind enables a body.
    if schema.body_enabled() {
        fields.insert(BODY_KEY.to_string(), body_state(card));
    }

    // Undeclared authored fields, appended in authored order under their NFC
    // keys (matching the plate's): the schema is a floor, not an allowlist, so
    // these reach the plate too — value verbatim, source Authored.
    for (name, value) in &resolved_input {
        if !schema.fields.contains_key(name) {
            fields.insert(
                name.clone(),
                FieldState {
                    value: value.clone(),
                    source: FieldSource::Authored,
                },
            );
        }
    }

    fields
}

/// Resolve one composable card. A card whose `$kind` names a schema resolves
/// through the ladder; an unknown-kind card (declared `$kind` with no schema, or
/// a kindless card) carries its authored fields verbatim — no coercion, no
/// ladder, no `$body` row.
fn card_states(config: &QuillConfig, card: &Card, index: usize) -> CardStates {
    // The raw authored kind rides the entry even when it names no schema — the
    // card reports what it *claimed* to be.
    let kind = card.kind().map(String::from);
    match card.kind().and_then(|k| config.card_kind(k)) {
        Some(schema) => CardStates {
            kind,
            index,
            fields: resolve_card_fields(schema, card),
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
                        },
                    )
                })
                .collect();
            CardStates { kind, index, fields }
        }
    }
}

/// The `$body` row. The value is byte-identical to the plate's `$body`
/// (canonical Content-JSON of the card body). A body has no `default:` rung, so
/// its source is only ever [`Authored`](FieldSource::Authored) (non-blank) or
/// [`Zero`](FieldSource::Zero) (blank).
fn body_state(card: &Card) -> FieldState {
    let value = QuillValue::from_json(quillmark_content::serial::to_canonical_value(card.body()));
    let source = if card.body().is_blank() {
        FieldSource::Zero
    } else {
        FieldSource::Authored
    };
    FieldState { value, source }
}

#[cfg(test)]
mod tests {
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
    fn body_disabled_kind_omits_body_row() {
        let quill = quill_from_yaml(BODY_DISABLED_QUILL);
        let doc = parse(
            "~~~card-yaml\n$quill: bd_test@1.0\n$kind: main\ntitle: T\n~~~\n\n\
             ~~~card-yaml\n$kind: stamp\nlabel: L\n~~~\nStray prose.\n",
        );
        let card = &quill.field_states(&doc).cards[0];
        assert!(
            !card.fields.contains_key(BODY_KEY),
            "a body-disabled kind has no `$body` row"
        );
        assert!(card.fields.contains_key("label"), "declared rows still present");
    }

    // ── Unknown-kind card ────────────────────────────────────────────────────

    #[test]
    fn unknown_kind_card_shape() {
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
    }

    // ── Wire shape ───────────────────────────────────────────────────────────

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
    fn field_state_is_value_and_source_only() {
        let state = FieldState {
            value: QuillValue::from_json(serde_json::json!("x")),
            source: FieldSource::Authored,
        };
        let json = serde_json::to_value(&state).unwrap();
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 2, "only value + source on the wire: {json}");
        assert!(obj.contains_key("value") && obj.contains_key("source"));
    }
}
