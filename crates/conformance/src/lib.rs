//! Engine-side runner for the cross-repo conformance fixture set.
//!
//! The suite is the general cure for "green upstream tests, broken pinned
//! consumer": one versioned fixture file (`conformance/conformance.json`) both
//! the engine and the downstream `borb-sh/quillmark-editor` run in CI. Freezing
//! it is what 1.0 means — post-1.0 a fixture diff is a breaking change by
//! definition; pre-1.0 the diffs are the pivot log. See
//! [`prose/canon/CONFORMANCE.md`](https://github.com/borb-sh/quillmark/blob/main/prose/canon/CONFORMANCE.md).
//!
//! This crate is the **engine** runner: it replays each fixture against
//! `quillmark-core` — the same producers the WASM surface serializes — and
//! asserts on codes, paths, sources, values, and the `DocPath` grammar. The
//! `@quillmark/conformance` npm package ships the identical JSON with a JS
//! runner over `@quillmark/wasm`; both repos run one frozen set green.
//!
//! **Never message text.** A fixture asserts a diagnostic's `code`, `path`, and
//! `severity` — never its `message`. A copyedit must not be a formal break, or
//! fixture diffs get rubber-stamped and the freeze signal dies (the Typst
//! `typst::<message-prefix>` convention — identity derived from prose — is
//! exactly what the frozen set excludes). The `Expect*` types below carry no
//! message field, so the discipline is structural, not a review rule.

use indexmap::IndexMap;
use quillmark_core::quill::FileTreeNode;
use quillmark_core::{
    doc_path_to_plate_addr, plate_addr_to_doc_path, Card, DocPath, Document, EditError, Quill,
    QuillValue, Severity,
};
use serde::Deserialize;
use serde_json::Value as Json;
use std::collections::HashMap;

/// The frozen fixture file — the single source of truth both repos load. The
/// engine embeds it at compile time; the npm package ships the same bytes.
pub const SUITE_JSON: &str = include_str!("../../../conformance/conformance.json");

// ── Fixture format (the DSL) ────────────────────────────────────────────────

/// The whole suite: a `contractVersion` stamp, the quills fixtures build
/// against, the operation-script fixtures, and the `DocPath` grammar fixtures.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Suite {
    /// The [`quillmark_core::CONTRACT_VERSION`] this set was frozen against — a
    /// consumer asserts its engine matches before trusting the fixtures.
    pub contract_version: String,
    /// Named quills, each a file tree (`path -> UTF-8 text`). A conformance
    /// quill is `Quill.yaml`-only — the suite exercises validate / fieldStates /
    /// mutators / paths, none of which render, so no plate or fonts are carried.
    pub quills: IndexMap<String, IndexMap<String, String>>,
    /// State fixtures: a document, an operation script, and expectations on the
    /// resolved view, validation diagnostics, and mutator errors.
    pub fixtures: Vec<Fixture>,
    /// Grammar fixtures: a `DocPath` string, its parsed segments, and — for a
    /// geometry address — the plate-space form it translates from.
    pub paths: Vec<PathFixture>,
}

/// One state fixture: parse `document` against `quill`, replay `steps`, then
/// assert `validate` and `fieldStates` on the resulting document.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fixture {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub quill: String,
    pub document: String,
    #[serde(default)]
    pub steps: Vec<Step>,
    /// Expected `validate()` output as a set (order-independent) of
    /// `{severity, code, path}`. Absent skips the check; `[]` asserts empty.
    #[serde(default)]
    pub validate: Option<Vec<ExpectDiag>>,
    /// Expected `fieldStates()` rows. Absent skips the check.
    #[serde(default)]
    pub field_states: Option<ExpectFieldStates>,
}

/// One operation-script step, keyed by `op` (the WASM `Document` verb name).
/// `card` absent targets the main card; `field` names the field; `value` /
/// `kind` / `index` / `body` carry op-specific data. An `error` asserts the op
/// fails with that `{code, path}`; its absence asserts the op succeeds.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Step {
    pub op: String,
    #[serde(default)]
    pub card: Option<usize>,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub value: Option<Json>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub index: Option<usize>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub error: Option<ExpectError>,
}

/// A mutator failure expectation: the namespaced `edit::*` code and the
/// `DocPath` anchor (absent when the error carries none).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectError {
    pub code: String,
    #[serde(default)]
    pub path: Option<String>,
}

/// A validation-diagnostic expectation — code, path, severity; never message.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectDiag {
    pub severity: String,
    pub code: String,
    #[serde(default)]
    pub path: Option<String>,
}

/// Expected resolved-value rows: the main card and any composable cards.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExpectFieldStates {
    #[serde(default)]
    pub main: Option<ExpectCard>,
    #[serde(default)]
    pub cards: Vec<ExpectCardAt>,
}

/// One card's expected rows: an optional key-`order` assertion (the declaration
/// -order contract) and a per-field `{source, value?}` map.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectCard {
    #[serde(default)]
    pub order: Option<Vec<String>>,
    #[serde(default)]
    pub fields: IndexMap<String, ExpectFieldState>,
}

/// A composable card's expected rows, matched to the actual card by `index`;
/// `kind` optionally asserts the reported `$kind` (JSON `null` for unknown-kind).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectCardAt {
    pub index: usize,
    #[serde(default)]
    pub kind: Option<Json>,
    #[serde(default)]
    pub order: Option<Vec<String>>,
    #[serde(default)]
    pub fields: IndexMap<String, ExpectFieldState>,
}

/// One resolved row: the `source` rung (always asserted) and, when the value is
/// deterministic enough to pin, the exact `value`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectFieldState {
    pub source: String,
    #[serde(default)]
    pub value: Option<Json>,
}

/// A grammar fixture: a `DocPath` string, the segments it parses to, and — when
/// the address is a geometry one — the plate-space form the session translates
/// it from. The parse + round-trip is the universal check both repos run; the
/// `plate` translation is the engine-side seam (the editor consumes already
/// -translated addresses, so it runs only the parse half).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PathFixture {
    pub name: String,
    pub path: String,
    /// The expected `DocPathSeg[]` — the tagged-segment JSON `parseDocPath`
    /// returns and `DocPath` serializes to.
    pub segs: Json,
    #[serde(default)]
    pub plate: Option<PlateCase>,
}

/// The plate-space geometry address for a [`PathFixture`], with the ordered card
/// kinds that resolve its per-kind ordinal to an absolute index.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlateCase {
    pub addr: String,
    pub card_kinds: Vec<Option<String>>,
}

// ── Runner ──────────────────────────────────────────────────────────────────

/// Parse the embedded suite.
pub fn suite() -> Suite {
    serde_json::from_str(SUITE_JSON).expect("conformance.json is valid JSON matching the format")
}

/// Build a [`Quill`] from a `path -> text` file tree.
fn build_quill(files: &IndexMap<String, String>) -> Quill {
    let mut tree = HashMap::new();
    for (path, text) in files {
        tree.insert(
            path.clone(),
            FileTreeNode::File {
                contents: text.clone().into_bytes(),
            },
        );
    }
    Quill::from_tree(FileTreeNode::Directory { files: tree })
        .unwrap_or_else(|d| panic!("quill build failed: {d:?}"))
}

/// The `DocPath` base a step's error resolves against, mirroring the WASM
/// binding's per-verb base computed before the mutable borrow. An addressed
/// mutator uses its card root (empty for main, `cards.<kind>[i]` for a
/// composable one, kind read off the live card); the structural `insertCard`
/// anchors at its target slot `cards[index]`, and an appending `insertCard`
/// (no index) has no slot yet.
fn base_of(doc: &Document, step: &Step) -> DocPath {
    match step.op.as_str() {
        "insertCard" => match step.index {
            Some(i) => DocPath::card(None, i),
            None => DocPath::new(),
        },
        _ => match step.card {
            None => DocPath::new(),
            Some(i) => DocPath::card(doc.cards().get(i).and_then(|c| c.kind()), i),
        },
    }
}

/// The mutable card a step targets, mapping an out-of-range index to the same
/// `IndexOutOfRange` the WASM card verbs throw.
fn card_mut<'a>(doc: &'a mut Document, card: Option<usize>) -> Result<&'a mut Card, EditError> {
    match card {
        None => Ok(doc.main_mut()),
        Some(i) => {
            let len = doc.cards().len();
            doc.card_mut(i).ok_or(EditError::IndexOutOfRange { index: i, len })
        }
    }
}

fn qv(step: &Step) -> QuillValue {
    QuillValue::from_json(step.value.clone().unwrap_or(Json::Null))
}

/// Run one operation-script step against `doc`, dispatching `op` to the core
/// mutator the identically-named WASM verb wraps.
fn run_step(quill: &Quill, doc: &mut Document, step: &Step) -> Result<(), EditError> {
    let field = || step.field.clone().expect("op needs a field");
    match step.op.as_str() {
        "storeField" => card_mut(doc, step.card)?.store_field(&field(), qv(step)),
        "storeFill" => card_mut(doc, step.card)?.store_fill(&field(), qv(step)),
        "removeField" => card_mut(doc, step.card)?.remove_field(&field()).map(|_| ()),
        "commitField" => {
            let f = field();
            let v = qv(step);
            let mut w = quill.writer(doc);
            match step.card {
                None => w.set(&f, v),
                Some(i) => w.card(i)?.set(&f, v),
            }
        }
        "insertCard" => {
            let mut card = Card::new(step.kind.clone().expect("insertCard needs a kind"))?;
            if let Some(body) = &step.body {
                card.revise_body(body.clone())?;
            }
            match step.index {
                Some(i) => doc.insert_card(i, card),
                None => doc.push_card(card),
            }
        }
        other => panic!("fixture uses an unknown op `{other}`"),
    }
}

/// Replay a fixture's steps, asserting each step's success or expected error.
fn run_steps(quill: &Quill, doc: &mut Document, fx: &Fixture) {
    for (i, step) in fx.steps.iter().enumerate() {
        let base = base_of(doc, step);
        let result = run_step(quill, doc, step);
        let at = format!("{}: step {} (`{}`)", fx.name, i, step.op);
        match (&step.error, result) {
            (None, Ok(())) => {}
            (None, Err(e)) => panic!("{at}: expected success, got {} ({e})", e.code()),
            (Some(exp), Ok(())) => panic!("{at}: expected error {}, got success", exp.code),
            (Some(exp), Err(e)) => {
                assert_eq!(e.code(), exp.code, "{at}: error code");
                let got = e.doc_path(&base).map(|p| p.to_string());
                assert_eq!(got, exp.path, "{at}: error path");
            }
        }
    }
}

/// Assert `validate()` matches the expected diagnostic set (code/path/severity,
/// order-independent). Compares as sorted multisets so a missing *or* extra
/// diagnostic fails.
fn check_validate(quill: &Quill, doc: &Document, fx: &Fixture, expected: &[ExpectDiag]) {
    let mut actual: Vec<(String, Option<String>, String)> = quill
        .validate(doc)
        .into_iter()
        .map(|d| {
            let sev = match d.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            (
                d.code.unwrap_or_default(),
                d.path,
                sev.to_string(),
            )
        })
        .collect();
    let mut want: Vec<(String, Option<String>, String)> = expected
        .iter()
        .map(|e| (e.code.clone(), e.path.clone(), e.severity.clone()))
        .collect();
    actual.sort();
    want.sort();
    assert_eq!(actual, want, "{}: validate() diagnostics", fx.name);
}

/// Assert one card's resolved rows against `expect`, reading the serialized
/// `fieldStates` (`serde_json` preserves the `IndexMap` key order, the
/// declaration-order contract).
fn check_card(card: &Json, order: &Option<Vec<String>>, fields: &IndexMap<String, ExpectFieldState>, at: &str) {
    let actual = card["fields"]
        .as_object()
        .unwrap_or_else(|| panic!("{at}: fields is not an object"));
    if let Some(order) = order {
        let keys: Vec<&String> = actual.keys().collect();
        let want: Vec<&String> = order.iter().collect();
        assert_eq!(keys, want, "{at}: field order (declaration-order contract)");
    }
    for (name, exp) in fields {
        let row = actual
            .get(name)
            .unwrap_or_else(|| panic!("{at}: missing row `{name}`"));
        assert_eq!(row["source"], Json::String(exp.source.clone()), "{at}: `{name}` source");
        if let Some(value) = &exp.value {
            assert_eq!(&row["value"], value, "{at}: `{name}` value");
        }
    }
}

/// Assert the whole `fieldStates()` projection against `expect`.
fn check_field_states(quill: &Quill, doc: &Document, fx: &Fixture, expect: &ExpectFieldStates) {
    let states = quill.field_states(doc);
    let json = serde_json::to_value(&states).expect("field_states serializes");
    if let Some(main) = &expect.main {
        check_card(&json["main"], &main.order, &main.fields, &format!("{}: main", fx.name));
    }
    let cards = json["cards"].as_array().expect("cards is an array");
    for exp in &expect.cards {
        let card = cards
            .iter()
            .find(|c| c["index"] == Json::from(exp.index))
            .unwrap_or_else(|| panic!("{}: no card at index {}", fx.name, exp.index));
        if let Some(kind) = &exp.kind {
            assert_eq!(&card["kind"], kind, "{}: card[{}] kind", fx.name, exp.index);
        }
        check_card(
            card,
            &exp.order,
            &exp.fields,
            &format!("{}: card[{}]", fx.name, exp.index),
        );
    }
}

/// Run one state fixture end to end.
fn run_fixture(quills: &IndexMap<String, Quill>, fx: &Fixture) {
    let quill = quills
        .get(&fx.quill)
        .unwrap_or_else(|| panic!("{}: unknown quill `{}`", fx.name, fx.quill));
    let mut doc = Document::parse(&fx.document)
        .unwrap_or_else(|e| panic!("{}: document parse failed: {e}", fx.name))
        .document;
    run_steps(quill, &mut doc, fx);
    if let Some(expected) = &fx.validate {
        check_validate(quill, &doc, fx, expected);
    }
    if let Some(expect) = &fx.field_states {
        check_field_states(quill, &doc, fx, expect);
    }
}

/// Run one grammar fixture: parse + round-trip (the universal check), then the
/// plate-space translation (the engine-side seam) when present.
fn run_path_fixture(fx: &PathFixture) {
    let parsed: DocPath = fx
        .path
        .parse()
        .unwrap_or_else(|e| panic!("{}: `{}` does not parse: {e}", fx.name, fx.path));
    assert_eq!(parsed.to_string(), fx.path, "{}: round-trip", fx.name);
    let segs = serde_json::to_value(&parsed).expect("DocPath serializes");
    assert_eq!(segs, fx.segs, "{}: parsed segments", fx.name);

    if let Some(plate) = &fx.plate {
        let kinds: Vec<Option<&str>> = plate.card_kinds.iter().map(|k| k.as_deref()).collect();
        assert_eq!(
            plate_addr_to_doc_path(&plate.addr, &kinds),
            Some(parsed.clone()),
            "{}: plate `{}` -> DocPath",
            fx.name,
            plate.addr,
        );
        assert_eq!(
            doc_path_to_plate_addr(&parsed, &kinds),
            Some(plate.addr.clone()),
            "{}: DocPath -> plate (inverse)",
            fx.name,
        );
    }
}

/// Run the whole suite — the entry point the workspace test drives.
pub fn run() {
    let suite = suite();
    assert_eq!(
        suite.contract_version,
        quillmark_core::CONTRACT_VERSION,
        "conformance.json contractVersion must match the engine's CONTRACT_VERSION",
    );
    let quills: IndexMap<String, Quill> = suite
        .quills
        .iter()
        .map(|(name, files)| (name.clone(), build_quill(files)))
        .collect();
    for fx in &suite.fixtures {
        run_fixture(&quills, fx);
    }
    for fx in &suite.paths {
        run_path_fixture(fx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole frozen set runs green against the engine — the acceptance
    /// gate's engine half (the editor runs the identical JSON against
    /// `@quillmark/wasm`).
    #[test]
    fn conformance_suite_is_green() {
        run();
    }

    /// Every fixture is uniquely named — a duplicate would let a broken fixture
    /// hide behind a passing namesake.
    #[test]
    fn fixture_names_are_unique() {
        let suite = suite();
        let mut seen = std::collections::HashSet::new();
        for name in suite
            .fixtures
            .iter()
            .map(|f| &f.name)
            .chain(suite.paths.iter().map(|p| &p.name))
        {
            assert!(seen.insert(name), "duplicate fixture name `{name}`");
        }
    }
}
