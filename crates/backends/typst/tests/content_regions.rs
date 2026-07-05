//! Span-tracked content produces schema-path-keyed regions read from the
//! laid-out frames — each field's **first placement**, one region per page it
//! touches: a top-level markdown field, per-page fragments when content
//! breaks across pages, first-placement-only when a field is placed twice,
//! and the canonical card address `$cards.<kind>.<n>.<field>` (per-kind
//! 0-based ordinal, surviving interleaved kinds). Scalars need no tagging:
//! every direct `data.<field>` reference site surfaces its own region. Plus
//! the `form-field` `field:` schema-path binding, which keys a widget region
//! on a schema path rather than its `/T` name, and the forward direction —
//! `field_at` resolves a point to a field on *any* placement, not just the
//! first.

use std::collections::HashMap;

use quillmark_core::{Backend, FileTreeNode, Quill};
use quillmark_typst::TypstBackend;

/// A self-contained quill from a `Quill.yaml` + `plate.typ` pair. No fonts dir
/// is needed — Typst's embedded defaults render text — and the helper package
/// (`@local/quillmark-helper`) is injected by the backend.
fn quill(yaml: &str, plate: &str) -> Quill {
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: yaml.as_bytes().to_vec(),
        },
    );
    files.insert(
        "plate.typ".to_string(),
        FileTreeNode::File {
            contents: plate.as_bytes().to_vec(),
        },
    );
    Quill::from_tree(FileTreeNode::Directory { files }).expect("load quill")
}

/// The canonical corpus JSON the render seam carries for a richtext field —
/// these tests drive `Backend::open` directly, so they build the corpus the way
/// `compile_data` would (`import` then the canonical serializer) rather than
/// passing a raw markdown string.
fn corpus(markdown: &str) -> serde_json::Value {
    let rt = quillmark_richtext::import::from_markdown(markdown).expect("import");
    quillmark_richtext::serial::to_canonical_value(&rt)
}

#[test]
fn content_fields_emit_frame_regions() {
    const YAML: &str = r#"
quill:
  name: content_regions
  version: 0.1.0
  backend: typst
  description: content region span-tracking test
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: markdown
      description: a short intro paragraph
    body:
      type: markdown
      description: a long body that wraps and breaks across pages
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#data.intro

#data.body
"#;

    // `body` is long enough to overflow page 0 and continue; the first (only)
    // placement should surface one region per page it touches.
    let long = "This is a markdown paragraph that wraps across several lines. ".repeat(200);
    let data = serde_json::json!({
        "intro": corpus("A **short** intro paragraph on the first page."),
        "body": corpus(&long),
    });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();

    // intro: one placement, one page — exactly one region, keyed on the schema
    // path (not a widget name).
    let intro: Vec<_> = regions.iter().filter(|r| r.field == "intro").collect();
    assert_eq!(intro.len(), 1, "intro is one region: {regions:?}");
    let [x0, y0, x1, y1] = intro[0].rect;
    assert!(
        x1 > x0 && y1 > y0,
        "intro has positive area: {:?}",
        intro[0].rect
    );

    // body spans several pages: one fragment per page its (first) placement
    // touches, in page order starting at the page it opens on.
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body").collect();
    assert!(
        body.len() >= 2,
        "page-spanning body surfaces one fragment per page: {body:?}"
    );
    assert_eq!(body[0].page, 0, "first fragment on the page the body opens");
    let pages: Vec<usize> = body.iter().map(|r| r.page).collect();
    let mut sorted_pages = pages.clone();
    sorted_pages.sort();
    sorted_pages.dedup();
    assert_eq!(pages, sorted_pages, "fragments in page order, one per page");
    for r in &body {
        assert!(
            r.rect[2] - r.rect[0] > 200.0,
            "each body fragment spans most of the text column: {:?}",
            r.rect
        );
    }
}

#[test]
fn field_placed_twice_surfaces_first_region_but_field_at_resolves_every_placement() {
    // One content value placed at two sites with unrelated ink between them.
    // Span data cannot distinguish a genuine second placement from package
    // chrome interrupting one placement, so `regions()` promises the first
    // placement only — never a spanning union that would claim the middle
    // content. The forward direction differs: a click identifies one drawn
    // item, so *both* placements resolve via `field_at` — including the second
    // one `regions()` deliberately does not enumerate — while a point on
    // unrelated ink resolves to nothing.
    const YAML: &str = r#"
quill:
  name: two_placements
  version: 0.1.0
  backend: typst
  description: first-placement region + click-to-field test
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: markdown
      description: a short paragraph placed twice
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

#data.intro

#lorem(40)

#data.intro
"#;
    let data = serde_json::json!({ "intro": corpus("The same intro, placed twice.") });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();
    let intro: Vec<_> = regions.iter().filter(|r| r.field == "intro").collect();
    assert_eq!(
        intro.len(),
        1,
        "a twice-placed content value surfaces its first placement only: {regions:?}"
    );
    // The surfaced region is the *first* placement: near the top margin, and
    // not a union stretching down over the lorem filler to the second copy.
    let first = intro[0];
    let [_, y0, _, y1] = first.rect;
    assert!(
        y1 - y0 < 100.0,
        "the region is one placement's extent, not a spanning union: {:?}",
        first.rect
    );
    assert!(
        y1 > 600.0,
        "the region is the top-of-page first placement (bottom-left origin): {:?}",
        first.rect
    );

    // A point inside the surfaced first placement resolves to the field.
    let cx = (first.rect[0] + first.rect[2]) / 2.0;
    let cy = (first.rect[1] + first.rect[3]) / 2.0;
    assert_eq!(
        session.field_at(first.page, cx, cy).as_deref(),
        Some("intro"),
        "a click inside the first placement resolves"
    );

    // The second placement sits below the lorem filler — probe downward from
    // the first placement until ink resolves again; it must still say `intro`.
    let mut second_hit = None;
    let mut y = first.rect[1] - 12.0;
    while y > 40.0 {
        if let Some(f) = session.field_at(first.page, cx, y) {
            second_hit = Some(f);
        }
        y -= 6.0;
    }
    assert_eq!(
        second_hit.as_deref(),
        Some("intro"),
        "the second, un-surfaced placement still resolves point-wise"
    );

    // Far outside any ink: nothing.
    assert_eq!(
        session.field_at(first.page, 5.0, 5.0),
        None,
        "a click off any field's ink resolves to nothing"
    );
}

#[test]
fn scalar_reference_sites_each_surface_a_region() {
    // A plain scalar needs no tagging: every direct `data.<field>` reference
    // site in the plate is its own tracked window, so a field shown twice
    // (e.g. header and body) surfaces both sites — full per-site fidelity,
    // because two source expressions are two origins, not one value counted
    // twice.
    const YAML: &str = r#"
quill:
  name: scalar_sites
  version: 0.1.0
  backend: typst
  description: scalar reference-site region test
main:
  fields:
    subject:
      type: string
      description: a plain scalar referenced at two sites
typst:
  plate_file: plate.typ
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

*#data.subject*

#lorem(20)

#data.at("subject")
"#;
    let data = serde_json::json!({ "subject": "Request for Quarters" });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();
    let subject: Vec<_> = regions.iter().filter(|r| r.field == "subject").collect();
    assert_eq!(
        subject.len(),
        2,
        "each scalar reference site surfaces independently: {regions:?}"
    );
    for r in &subject {
        assert!(r.rect[2] > r.rect[0], "positive width: {:?}", r.rect);
    }
    // Disjoint vertically (bottom-left origin: the later site sits lower).
    assert!(
        subject[0].rect[1] > subject[1].rect[3] || subject[1].rect[1] > subject[0].rect[3],
        "sites do not union: {:?} vs {:?}",
        subject[0].rect,
        subject[1].rect
    );
}

#[test]
fn widget_and_tracked_content_both_surface_widget_ordered_first() {
    // A field bound to both a `field:`-bound widget and a tracked content
    // placement surfaces both (they route to the same field; a consumer
    // groups by `field`), deterministically ordered widget-first — the
    // contract documented on `SessionHandle::regions` and `TypstSession::regions`.
    const YAML: &str = r#"
quill:
  name: widget_and_content
  version: 0.1.0
  backend: typst
  description: widget-before-content ordering test
main:
  fields:
    signature_block:
      type: string
      description: bound to both a widget and a scalar reference site
typst:
  plate_file: plate.typ
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, signature-field
#set page(width: 612pt, height: 792pt, margin: 72pt)

#data.signature_block
#signature-field("Signature", field: "signature_block", width: 137pt)
"#;
    let data = serde_json::json!({ "signature_block": "FIRST M. LAST, Rank, USAF" });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();
    let matches: Vec<_> = regions
        .iter()
        .filter(|r| r.field == "signature_block")
        .collect();
    assert_eq!(
        matches.len(),
        2,
        "a widget-bound field with tracked content surfaces both: {regions:?}"
    );

    // The widget is a fixed-size box at the test-owned 137pt width; the
    // content region is the ink of the placed string, distinguishing which
    // entry is which without relying on a `source` field the type doesn't
    // carry.
    assert!(
        (matches[0].rect[2] - matches[0].rect[0] - 137.0).abs() < 0.01,
        "the widget region sorts first: {regions:?}"
    );
}

#[test]
fn content_survives_a_rebuilding_show_rule() {
    // A `show`-rule pass that captures paragraphs into a state buffer and
    // re-emits them (the shape of render-body's auto-numbering) must not
    // lose the field: spans are a property of the glyphs and ride through
    // the rebuild, so the field surfaces with no plate-author recovery step.
    const YAML: &str = r#"
quill:
  name: rebuild_survival
  version: 0.1.0
  backend: typst
  description: content-rebuild span survival test
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: markdown
      description: a body piped through a capture-and-replay package shape
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

#let BUF = state("BUF", ())
#let capture(it) = {
  show par: p => {
    BUF.update(buf => buf + (text([#p.body]),))
    []
  }
  it
}

#capture(data.body)

#context {
  for c in BUF.get() {
    block[#c]
  }
}
"#;
    let data = serde_json::json!({ "body": corpus("A body paragraph the package rebuilds.") });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();
    assert!(
        regions.iter().any(|r| r.field == "body"),
        "a rebuilt body still surfaces a region, with no explicit tagging: {regions:?}"
    );
}

#[test]
fn markdown_array_elements_surface_indexed_regions() {
    const YAML: &str = r#"
quill:
  name: array_regions
  version: 0.1.0
  backend: typst
  description: markdown[] element region test
main:
  fields:
    refs:
      type: array
      items:
        type: markdown
      description: a markdown[] field
typst:
  plate_file: plate.typ
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

#for r in data.refs {
  block(r)
}
"#;
    let data =
        serde_json::json!({ "refs": [corpus("First reference."), corpus("Second reference.")] });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();
    for expected in ["refs.0", "refs.1"] {
        assert!(
            regions.iter().any(|r| r.field == expected),
            "each markdown[] element gets its own eval site and region {expected:?}: {regions:?}"
        );
    }
    // Elements are distinct placements, not one unioned `refs` blob.
    assert!(
        !regions.iter().any(|r| r.field == "refs"),
        "the array itself is not a region key: {regions:?}"
    );
}

#[test]
fn card_regions_use_canonical_kind_ordinal_path() {
    // Two kinds, interleaved alpha/beta/alpha. The card address is kind + 0-based
    // ordinal *within that kind*, so the second alpha is `.1` even though it is
    // the third card overall, and beta's ordinal is unaffected by alpha.
    const YAML: &str = r#"
quill:
  name: card_regions
  version: 0.1.0
  backend: typst
  description: card region path test
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: markdown
      description: a top-level intro
card_kinds:
  alpha:
    description: alpha card
    fields:
      note:
        type: markdown
        description: alpha note
  beta:
    description: beta card
    fields:
      note:
        type: markdown
        description: beta note
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#data.intro

#for card in data.at("$cards", default: ()) {
  card.at("note", default: [])
  parbreak()
}
"#;
    let data = serde_json::json!({
        "intro": corpus("Top-level intro."),
        "$cards": [
            {"$kind": "alpha", "note": corpus("Alpha one.")},
            {"$kind": "beta",  "note": corpus("Beta one.")},
            {"$kind": "alpha", "note": corpus("Alpha two.")},
        ],
    });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let fields: std::collections::HashSet<String> =
        session.regions().into_iter().map(|r| r.field).collect();

    for expected in [
        "intro",
        "$cards.alpha.0.note",
        "$cards.beta.0.note",
        "$cards.alpha.1.note",
    ] {
        assert!(
            fields.contains(expected),
            "expected a region keyed {expected:?}; got {fields:?}"
        );
    }
    // No positional/absolute card address leaks through.
    assert!(
        !fields.iter().any(|f| f.starts_with("$cards.0.")
            || f.starts_with("$cards.1.")
            || f.starts_with("$cards.2.")),
        "card regions must use kind+ordinal, not positional index: {fields:?}"
    );
}

#[test]
fn form_field_unknown_path_fails_the_compile() {
    // `field:` validates against the schema address tables — a typo'd path
    // is a loud compile error, not a silent no-region widget.
    const YAML: &str = r#"
quill:
  name: widget_typo
  version: 0.1.0
  backend: typst
  description: form-field path validation test
typst:
  plate_file: plate.typ
main:
  fields:
    subject:
      type: string
      description: the only schema field
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#form-field("S", type: "text", value: "x", field: "subjcet")
"#;
    let err = TypstBackend
        .open(
            &quill(YAML, PLATE),
            &serde_json::json!({ "subject": "typo'd" }),
        )
        .err()
        .expect("a typo'd field binding must fail the compile");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("subjcet"),
        "the compile error names the bad path: {msg}"
    );
}

#[test]
fn failed_apply_keeps_serving_last_good_regions() {
    // apply is transactional for regions too: a failed compile has already
    // written the next injection's helper source into the world, but the
    // served document's spans must keep resolving against the compile they
    // came from — regions and clicks may not shift or vanish.
    const YAML: &str = r#"
quill:
  name: failed_apply_regions
  version: 0.1.0
  backend: typst
  description: transactional regions test
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: markdown
      description: a paragraph
    when:
      type: datetime
      description: a date the template parses at data-assembly time
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

#data.intro
"#;
    let good = serde_json::json!({
        "intro": corpus("A stable paragraph the session keeps serving."),
        "when": "2026-07-03",
    });
    let mut session = TypstBackend.open(&quill(YAML, PLATE), &good).expect("open");
    let before = session.regions();
    assert!(
        before.iter().any(|r| r.field == "intro"),
        "baseline intro region: {before:?}"
    );

    // Shorter content shifts every byte offset in the regenerated helper,
    // and the unparseable date fails the compile at data-assembly time.
    let bad = serde_json::json!({ "intro": corpus("X"), "when": "not-a-date" });
    session
        .apply(&bad)
        .expect_err("the bad date must fail the compile");

    assert_eq!(
        session.regions(),
        before,
        "a failed apply must not move or drop the served compile's regions"
    );
    let intro = before.iter().find(|r| r.field == "intro").unwrap();
    let cx = (intro.rect[0] + intro.rect[2]) / 2.0;
    let cy = (intro.rect[1] + intro.rect[3]) / 2.0;
    assert_eq!(
        session.field_at(intro.page, cx, cy).as_deref(),
        Some("intro"),
        "clicks keep resolving against the served compile"
    );
}

#[test]
fn continuation_fragments_survive_page_marginals() {
    // Headers and footers walk between one page's body and the next's. That
    // foreign ink suspends a placement's run at the page boundary; the run
    // resumes on the immediately following page, so a page-spanning body on
    // a chrome-bearing plate still surfaces its continuation fragments.
    const YAML: &str = r#"
quill:
  name: marginal_fragments
  version: 0.1.0
  backend: typst
  description: continuation fragments under page chrome
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: markdown
      description: a long body under headers and footers
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(
  width: 612pt,
  height: 300pt,
  margin: 60pt,
  header: [Running Header],
  footer: [Running Footer],
)
#set text(size: 11pt)

#data.body
"#;
    let long = "A paragraph that wraps and flows across pages. ".repeat(120);
    let data = serde_json::json!({ "body": corpus(&long) });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body").collect();
    assert!(
        body.len() >= 2,
        "page chrome must not truncate the placement to its first page: {body:?}"
    );
    let pages: Vec<usize> = body.iter().map(|r| r.page).collect();
    assert_eq!(pages[0], 0);
    assert!(
        pages.windows(2).all(|w| w[1] == w[0] + 1),
        "fragments cover consecutive pages: {pages:?}"
    );
}

#[test]
fn wrapped_scalar_expression_attributes_to_its_field() {
    // A reference wrapped in an expression (`#upper(data.subject)`) stamps
    // its ink with the whole expression's span; the enclosing-expression
    // window attributes it as long as the field is the expression's only
    // reference.
    const YAML: &str = r#"
quill:
  name: wrapped_scalar
  version: 0.1.0
  backend: typst
  description: wrapped scalar attribution test
main:
  fields:
    subject:
      type: string
      description: a scalar shown through a wrapping call
typst:
  plate_file: plate.typ
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

#upper(data.subject)
"#;
    let data = serde_json::json!({ "subject": "request for quarters" });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();
    let subject: Vec<_> = regions.iter().filter(|r| r.field == "subject").collect();
    assert_eq!(
        subject.len(),
        1,
        "a single-reference wrapping expression attributes to the field: {regions:?}"
    );
    let cx = (subject[0].rect[0] + subject[0].rect[2]) / 2.0;
    let cy = (subject[0].rect[1] + subject[0].rect[3]) / 2.0;
    assert_eq!(
        session.field_at(subject[0].page, cx, cy).as_deref(),
        Some("subject"),
        "clicks on the wrapped ink route to the field"
    );
}

#[test]
fn form_field_path_rejected_when_address_tables_are_empty() {
    // `__meta__` present with empty address tables (a body-disabled main
    // with no fields and no cards) validates against the empty set — every
    // address rejects. Only `__meta__` *absent* is permissive.
    const YAML: &str = r#"
quill:
  name: empty_tables
  version: 0.1.0
  backend: typst
  description: empty-tables-vs-absent-meta guard test
typst:
  plate_file: plate.typ
main:
  body:
    enabled: false
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#form-field("S", type: "text", value: "x", field: "subject")
"#;
    let err = TypstBackend
        .open(&quill(YAML, PLATE), &serde_json::json!({}))
        .err()
        .expect("an address must still fail when the tables are empty, not absent");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("subject"),
        "the compile error names the bad path: {msg}"
    );
}

#[test]
fn adversarial_codegen_inputs_still_compile() {
    // The generated helper is Typst source built from document data, so a data
    // value must never produce source that fails to parse. Two edges the string
    // shape of the codegen could hide but a real compile catches:
    //   - an unterminated `<u>` yields unbalanced `#underline[` markup that, in
    //     a `[ .. ]` content block, would break the whole helper file;
    //   - `i64::MIN` cannot be a Typst int literal (its magnitude overflows).
    const YAML: &str = r#"
quill:
  name: adversarial_codegen
  version: 0.1.0
  backend: typst
  description: codegen robustness
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: markdown
      description: a body that may carry malformed inline markup
"#;
    // The plate proves the int round-tripped: a wrong `i64::MIN` literal makes
    // the assert fail, which fails the compile, which fails `open`.
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 400pt, margin: 40pt)
#assert(data.at("n") == -9223372036854775807 - 1)
#data.body
"#;
    let data = serde_json::json!({
        // Import balances the unterminated `<u>` into a closed underline mark, so
        // the emitter's `#underline[ .. ]` is bracket-balanced by construction.
        "body": corpus("Please <u>sign here"),
        "n": i64::MIN,
    });
    // Compile success is the assertion: a broken literal or unbalanced block
    // would make `open` return Err.
    TypstBackend
        .open(&quill(YAML, PLATE), &data)
        .expect("adversarial data (unterminated <u>, i64::MIN) must still compile");
}

/// Two spatially-overlapping widgets resolve `field_at` by paint order — the
/// later-painted widget wins — not by their alphabetical `/T` names. The widget
/// hit-test once sorted placements by `(page, name)` and `find`-first, silently
/// violating the "later-painted wins" rule the content-field path documents
/// (`span_scan::field_at`). `aaa` is painted first, `zzz` on top of it; a click
/// in the shared box must resolve to `zzz`'s field, not `aaa`'s.
#[test]
fn overlapping_widgets_resolve_field_at_by_paint_order_not_name() {
    const YAML: &str = r#"
quill:
  name: overlap_widgets
  version: 0.1.0
  backend: typst
  description: overlapping widget hit-test
typst:
  plate_file: plate.typ
main:
  fields:
    aaa_early:
      type: string
      description: alphabetically first, painted first (underneath)
    zzz_late:
      type: string
      description: alphabetically last, painted last (on top)
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#set page(width: 300pt, height: 200pt, margin: 0pt)
#place(top + left, dx: 60pt, dy: 60pt,
  form-field("aaa", type: "checkbox", field: "aaa_early", width: 40pt, height: 40pt))
#place(top + left, dx: 60pt, dy: 60pt,
  form-field("zzz", type: "checkbox", field: "zzz_late", width: 40pt, height: 40pt))
"#;
    let session = TypstBackend
        .open(&quill(YAML, PLATE), &serde_json::json!({}))
        .expect("open");
    let regions = session.regions();
    let a = regions
        .iter()
        .find(|r| r.field == "aaa_early")
        .expect("aaa_early widget region");
    let z = regions
        .iter()
        .find(|r| r.field == "zzz_late")
        .expect("zzz_late widget region");
    assert_eq!(a.page, z.page, "both widgets on the same page");
    let cx = (z.rect[0] + z.rect[2]) / 2.0;
    let cy = (z.rect[1] + z.rect[3]) / 2.0;
    assert_eq!(
        session.field_at(z.page, cx, cy).as_deref(),
        Some("zzz_late"),
        "the later-painted widget wins the click, not the alphabetically-first name"
    );
}
