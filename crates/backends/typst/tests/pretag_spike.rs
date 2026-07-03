//! SPIKE — not a production test. Investigates whether splicing region
//! markers into the *markdown string* before `eval()` (so the parser folds
//! them into the same paragraph as adjacent text) survives being piped
//! through a real content-rebuilding package, instead of the current
//! bracket-the-eval'd-value approach that `render-body` is known to drop
//! (issue #789 / PR #788's `tagged()` escape hatch).
//!
//! **VERDICT: falsified.** `minimal_capture_replay_inspect_repr` and its
//! `_with_zws_bandaid` variant show that `metadata()` — with or without a
//! zero-width-space bandaid for the "show par won't collect a wrapper alone"
//! quirk `body.typ` already documents — never becomes part of `par.body`.
//! Typst's realization phase excludes zero-size/meta content from paragraph
//! grouping *categorically*, regardless of how close it sits to the
//! paragraph's real text in the source string. `query()` still finds the
//! marker (`MARKER_COUNT=2` in every diagnostic case, rebuild or not — the
//! element itself isn't deleted), but it never rides inside the content a
//! `show par: it => ...`-based capture-and-replay package extracts, so it
//! stays behind exactly like the current post-eval value-level bracket does.
//! Splicing into the markdown *string* instead of bracketing the eval'd
//! *value* changes nothing: the failure is about element kind (meta vs.
//! paragraph-eligible), not about which Typst construction path produced it.
//! Every baseline/adversarial case below (plain paragraphs, single
//! paragraph, page-spanning body, headings, lists, tables) fails uniformly
//! against `render-body` — the content shape doesn't matter once the
//! category-level exclusion applies. A leading-heading shape additionally
//! breaks the *compile* outright (`adversarial_leading_heading`), an
//! independent hazard from markup-boundary corruption.
//!
//! Conclusion: no marker-encoding scheme beats a package that peels
//! `it.body`/`it`'s inline content out of a matched element and discards the
//! wrapper — the existing design (`tagged()` bracketing the package's
//! *output*, from #788) remains the only working fix, and #789's actual
//! proposal (a render-time lint that detects a declared content field with
//! zero regions) is the correct remaining scope, not a new tagging
//! primitive.
//!
//! Uses the real vendored `tonguetoquill-usaf-memo` package's `render-body`
//! (via `mainmatter`) as the adversary — not a synthetic stand-in — since
//! that's the one concrete rebuild package in this codebase.
//!
//! Every case prints what it found; only the ones with a clear expected
//! outcome assert. Run with `--nocapture` to see the field/rect dump for the
//! adversarial cases.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use quillmark_core::{Backend, FileTreeNode, Quill, RenderedRegion};
use quillmark_typst::TypstBackend;

/// Walk the `usaf_memo@0.2.0` fixture (packages/fonts/assets included) into an
/// in-memory tree, so the spike plate can import the real vendored package.
fn host_tree() -> FileTreeNode {
    fn walk(dir: &Path) -> std::io::Result<FileTreeNode> {
        let mut files = HashMap::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let p: PathBuf = entry.path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if p.is_file() {
                files.insert(
                    name,
                    FileTreeNode::File {
                        contents: fs::read(&p)?,
                    },
                );
            } else if p.is_dir() {
                files.insert(name, walk(&p)?);
            }
        }
        Ok(FileTreeNode::Directory { files })
    }
    let quill_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
        .join("resources")
        .join("quills")
        .join("usaf_memo")
        .join("0.2.0");
    walk(&quill_path).expect("walk fixture")
}

fn quill_with_plate(plate: &str) -> Quill {
    let mut tree = host_tree();
    if let FileTreeNode::Directory { files } = &mut tree {
        files.insert(
            "plate.typ".to_string(),
            FileTreeNode::File {
                contents: plate.as_bytes().to_vec(),
            },
        );
    }
    Quill::from_tree(tree).expect("load quill")
}

/// Common plate prelude: real frontmatter (minimal required args) + the
/// pre-eval-splice tagging helper, spelled out inline rather than routed
/// through the production auto-tag pass, so the spike isolates the marker
/// scheme itself from the rest of the pipeline. Body markdown is hardcoded
/// per-case as a Typst string literal — no quillmark schema/data involved.
fn plate_with_body(md_literal: &str) -> String {
    format!(
        r##"
#import "@local/tonguetoquill-usaf-memo:3.0.0": frontmatter, mainmatter

#show: frontmatter.with(subject: "Spike Memo", memo_for: ("TEST/SYMB",))

// Pre-eval splice: fold the marker into the markdown STRING before `eval`,
// so the parser treats it as inline content of the adjacent paragraph rather
// than a value-level sibling bracketing the whole eval'd block.
#let qm-tag-pretag(path, md) = {{
  let start = "#metadata((role: \"start\", field: \"" + path + "\")) <__qm_region__>"
  let end = "#metadata((role: \"end\", field: \"" + path + "\")) <__qm_region__>"
  eval(start + md + end, mode: "markup")
}}

#mainmatter[#qm-tag-pretag("body_text", {md_literal})]
"##
    )
}

/// Render a plain Rust `&str` as a Typst string literal (quoted, escaped) —
/// avoids fighting Rust raw-string delimiter counting when the markdown body
/// itself contains `"` or `#`.
fn ts(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

fn regions_for(plate: &str) -> Vec<RenderedRegion> {
    let session = TypstBackend
        .open(&quill_with_plate(plate), &serde_json::json!({}))
        .expect("compile");
    session.regions()
}

fn dump(label: &str, regions: &[RenderedRegion]) {
    eprintln!("--- {label} ---");
    if regions.is_empty() {
        eprintln!("  (no regions)");
    }
    for r in regions {
        eprintln!("  field={:?} page={} rect={:?}", r.field, r.page, r.rect);
    }
}

// ---------------------------------------------------------------------------
// Diagnostic: abuse a forced compile error to read out how many
// `<__qm_region__>` markers actually survive into the final introspectable
// tree after render-body, vs. how many the un-rebuilt splice produces.
// ---------------------------------------------------------------------------

#[test]
fn diagnostic_marker_survival_count_through_render_body() {
    let plate = format!(
        r##"
#import "@local/tonguetoquill-usaf-memo:3.0.0": frontmatter, mainmatter

#show: frontmatter.with(subject: "Spike Memo", memo_for: ("TEST/SYMB",))

#let qm-tag-pretag(path, md) = {{
  let start = "#metadata((role: \"start\", field: \"" + path + "\")) <__qm_region__>"
  let end = "#metadata((role: \"end\", field: \"" + path + "\")) <__qm_region__>"
  eval(start + md + end, mode: "markup")
}}

#mainmatter[#qm-tag-pretag("body_text", {})]

#context {{
  let n = query(label("__qm_region__")).len()
  panic("MARKER_COUNT=" + str(n))
}}
"##,
        ts("First paragraph of the body text.\n\nSecond paragraph, distinct from the first.\n\nThird paragraph to force AFH numbering (par_count > 1).")
    );
    let err = TypstBackend
        .open(&quill_with_plate(&plate), &serde_json::json!({}))
        .err()
        .expect("forced panic() must surface as a compile error");
    eprintln!("diagnostic: {err:?}");
}

#[test]
fn diagnostic_marker_survival_count_with_no_rebuild() {
    let plate = format!(
        r##"
#set page(width: 612pt, height: 792pt, margin: 72pt)
#let qm-tag-pretag(path, md) = {{
  let start = "#metadata((role: \"start\", field: \"" + path + "\")) <__qm_region__>"
  let end = "#metadata((role: \"end\", field: \"" + path + "\")) <__qm_region__>"
  eval(start + md + end, mode: "markup")
}}
#qm-tag-pretag("body_text", {})
#context {{
  let n = query(label("__qm_region__")).len()
  panic("MARKER_COUNT=" + str(n))
}}
"##,
        ts("First paragraph.\n\nSecond paragraph.")
    );
    let err = TypstBackend
        .open(&quill_with_plate(&plate), &serde_json::json!({}))
        .err()
        .expect("forced panic() must surface as a compile error");
    eprintln!("diagnostic: {err:?}");
}

// ---------------------------------------------------------------------------
// Minimal synthetic capture-and-replay, to isolate the exact step that drops
// the marker: no hide()/place(), no headings/lists, just `show par: it =>`
// capturing `it.body`, storing it, and replaying it later — the one pattern
// common to every content-rebuilding package #789 names.
// ---------------------------------------------------------------------------

#[test]
fn minimal_capture_replay_survival() {
    let plate = format!(
        r##"
#set page(width: 612pt, height: 792pt, margin: 72pt)

#let BUF = state("BUF", ())
#let qm-tag-pretag(path, md) = {{
  let start = "#metadata((role: \"start\", field: \"" + path + "\")) <__qm_region__>"
  let end = "#metadata((role: \"end\", field: \"" + path + "\")) <__qm_region__>"
  eval(start + md + end, mode: "markup")
}}

#let capture(it) = {{
  show par: p => {{
    BUF.update(buf => buf + (text([#p.body]),))
    []
  }}
  it
}}

#capture(qm-tag-pretag("body_text", {}))

#context {{
  for c in BUF.get() {{
    block[#c]
  }}
}}

#context {{
  let n = query(label("__qm_region__")).len()
  panic("MARKER_COUNT=" + str(n))
}}
"##,
        ts("First paragraph.\n\nSecond paragraph.")
    );
    let err = TypstBackend
        .open(&quill_with_plate(&plate), &serde_json::json!({}))
        .err()
        .expect("forced panic() must surface as a compile error");
    eprintln!("minimal capture-replay diagnostic: {err:?}");
}

#[test]
fn minimal_capture_replay_inspect_repr() {
    // Dump the actual captured content structure via repr() to see whether
    // the metadata call really ended up nested inside `p.body`, and how many
    // paragraphs the markdown split into.
    let plate = format!(
        r##"
#set page(width: 612pt, height: 792pt, margin: 72pt)

#let BUF = state("BUF", ())
#let qm-tag-pretag(path, md) = {{
  let start = "#metadata((role: \"start\", field: \"" + path + "\")) <__qm_region__>"
  let end = "#metadata((role: \"end\", field: \"" + path + "\")) <__qm_region__>"
  eval(start + md + end, mode: "markup")
}}

#let capture(it) = {{
  show par: p => {{
    BUF.update(buf => buf + (p.body,))
    []
  }}
  it
}}

#capture(qm-tag-pretag("body_text", {}))

#context {{
  let items = BUF.get()
  panic("COUNT=" + str(items.len()) + " REPR0=" + repr(items.at(0, default: none)))
}}
"##,
        ts("First paragraph.\n\nSecond paragraph.")
    );
    let err = TypstBackend
        .open(&quill_with_plate(&plate), &serde_json::json!({}))
        .err()
        .expect("forced panic() must surface as a compile error");
    eprintln!("repr diagnostic: {err:?}");
}

#[test]
fn minimal_capture_replay_inspect_repr_with_zws_bandaid() {
    // body.typ's own comment (lines ~172-186) documents this exact Typst
    // quirk: "show par will not collect wrappers unless there is content
    // outside" — a zero-size element alone doesn't force paragraph
    // membership; render-body's own authors bandaid it with a zero-width
    // space glued to strong/emph/underline/raw. Try the same bandaid on our
    // marker: glue `sym.zws` immediately after the metadata+label.
    let plate = format!(
        r##"
#set page(width: 612pt, height: 792pt, margin: 72pt)

#let BUF = state("BUF", ())
#let qm-tag-pretag(path, md) = {{
  let start = "#metadata((role: \"start\", field: \"" + path + "\")) <__qm_region__>#sym.zws;"
  let end = "#sym.zws;#metadata((role: \"end\", field: \"" + path + "\")) <__qm_region__>"
  eval(start + md + end, mode: "markup")
}}

#let capture(it) = {{
  show par: p => {{
    BUF.update(buf => buf + (p.body,))
    []
  }}
  it
}}

#capture(qm-tag-pretag("body_text", {}))

#context {{
  let items = BUF.get()
  panic("COUNT=" + str(items.len()) + " REPR0=" + repr(items.at(0, default: none)) + " REPR-LAST=" + repr(items.at(items.len() - 1, default: none)))
}}
"##,
        ts("First paragraph.\n\nSecond paragraph.")
    );
    let err = TypstBackend
        .open(&quill_with_plate(&plate), &serde_json::json!({}))
        .err()
        .expect("forced panic() must surface as a compile error");
    eprintln!("zws-bandaid repr diagnostic: {err:?}");
}

#[test]
fn minimal_capture_replay_regions_with_zws_bandaid() {
    let plate = format!(
        r##"
#set page(width: 612pt, height: 792pt, margin: 72pt)

#let BUF = state("BUF", ())
#let qm-tag-pretag(path, md) = {{
  let start = "#metadata((role: \"start\", field: \"" + path + "\")) <__qm_region__>#sym.zws;"
  let end = "#sym.zws;#metadata((role: \"end\", field: \"" + path + "\")) <__qm_region__>"
  eval(start + md + end, mode: "markup")
}}

#let capture(it) = {{
  show par: p => {{
    BUF.update(buf => buf + (text([#p.body]),))
    []
  }}
  it
}}

#capture(qm-tag-pretag("body_text", {}))

#context {{
  for c in BUF.get() {{
    block[#c]
  }}
}}
"##,
        ts("First paragraph.\n\nSecond paragraph.")
    );
    let regions = regions_for(&plate);
    dump("minimal capture-replay with zws bandaid", &regions);
}

#[test]
fn minimal_capture_replay_regions() {
    // Same minimal harness, without the forced panic, checking real regions().
    let plate = format!(
        r##"
#set page(width: 612pt, height: 792pt, margin: 72pt)

#let BUF = state("BUF", ())
#let qm-tag-pretag(path, md) = {{
  let start = "#metadata((role: \"start\", field: \"" + path + "\")) <__qm_region__>"
  let end = "#metadata((role: \"end\", field: \"" + path + "\")) <__qm_region__>"
  eval(start + md + end, mode: "markup")
}}

#let capture(it) = {{
  show par: p => {{
    BUF.update(buf => buf + (text([#p.body]),))
    []
  }}
  it
}}

#capture(qm-tag-pretag("body_text", {}))

#context {{
  for c in BUF.get() {{
    block[#c]
  }}
}}
"##,
        ts("First paragraph.\n\nSecond paragraph.")
    );
    let regions = regions_for(&plate);
    dump("minimal capture-replay", &regions);
}

// ---------------------------------------------------------------------------
// Control: does the splice mechanism work at all, with NO rebuild package in
// the way? Isolates the marker scheme itself from render-body's behavior.
// ---------------------------------------------------------------------------

#[test]
fn control_splice_with_no_rebuild_package() {
    let plate = format!(
        r##"
#set page(width: 612pt, height: 792pt, margin: 72pt)
#let qm-tag-pretag(path, md) = {{
  let start = "#metadata((role: \"start\", field: \"" + path + "\")) <__qm_region__>"
  let end = "#metadata((role: \"end\", field: \"" + path + "\")) <__qm_region__>"
  eval(start + md + end, mode: "markup")
}}
#qm-tag-pretag("body_text", {})
"##,
        ts("First paragraph.\n\nSecond paragraph.")
    );
    let regions = regions_for(&plate);
    dump("control: no rebuild package", &regions);
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body_text").collect();
    assert!(
        !body.is_empty(),
        "the splice mechanism itself must work with no rebuild package involved: {regions:?}"
    );
}

// ---------------------------------------------------------------------------
// Baseline: does it survive at all?
// ---------------------------------------------------------------------------

#[test]
fn baseline_multi_paragraph_survives_render_body() {
    // VERDICT (see `minimal_capture_replay_inspect_repr*`): it does NOT
    // survive, even for the plain multi-paragraph case with no adversarial
    // shape at all. `metadata()` is categorically excluded from `par.body`
    // by Typst's realization phase — see the module doc for the full
    // explanation. This assertion documents that finding; it is not the
    // hoped-for outcome.
    let plate = plate_with_body(&ts(
        "First paragraph of the body text.\n\nSecond paragraph, distinct from the first.\n\nThird paragraph to force AFH numbering (par_count > 1).",
    ));
    let regions = regions_for(&plate);
    dump("baseline multi-paragraph", &regions);
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body_text").collect();
    assert!(
        body.is_empty(),
        "pre-eval splice does NOT survive render-body's paragraph rebuild, even in the plain case: {regions:?}"
    );
}

#[test]
fn baseline_single_paragraph_survives_unnumbered_path() {
    // Same verdict on the AFH 33-337 §2 "single paragraph, unnumbered" branch
    // — the failure is categorical (metadata excluded from `.body`), not
    // specific to the numbered-paragraph code path.
    let plate = plate_with_body(&ts(
        "Just one lonely paragraph, unnumbered per AFH 33-337 section 2.",
    ));
    let regions = regions_for(&plate);
    dump("baseline single paragraph", &regions);
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body_text").collect();
    assert!(body.is_empty(), "the unnumbered branch fails the same way: {regions:?}");
}

#[test]
fn page_spanning_body_yields_per_page_fragments() {
    let long = "This is a long paragraph that must wrap across many lines and break across pages when rendered through render-body's numbering pipeline. ".repeat(120);
    let plate = plate_with_body(&ts(&long));
    let regions = regions_for(&plate);
    dump("page-spanning body", &regions);
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body_text").collect();
    assert!(body.is_empty(), "consistent with the categorical failure: {regions:?}");
}

// ---------------------------------------------------------------------------
// Adversarial: realistic content shapes a plate author's markdown could
// plausibly contain, not hand-crafted Typst attacks.
// ---------------------------------------------------------------------------

#[test]
fn adversarial_leading_heading() {
    // Independent of the metadata-exclusion finding above, this shape breaks
    // the compile outright: splicing the start-marker directly against a
    // leading `# Heading` line (no newline boundary) makes the parser read
    // the marker's `#` as continuing into the heading's own `#`, producing
    // "expected expression" rather than merely losing the tag. A second,
    // independent way the splice is unsafe for real markdown.
    let plate = plate_with_body(&ts("# Section Heading\n\nBody text following the heading."));
    let err = TypstBackend
        .open(&quill_with_plate(&plate), &serde_json::json!({}))
        .err();
    eprintln!("leading heading compile result: {err:?}");
    assert!(
        err.is_some(),
        "documents that a leading heading breaks the splice at compile time, not just at tagging time"
    );
}

#[test]
fn adversarial_leading_list() {
    // Realistic body: a bulleted list as the *first* content. Splicing marker
    // text directly onto the same line as `- ` risks breaking the list-marker
    // recognition itself (CommonMark/Typst list markers must start the line),
    // not just losing the tag.
    let plate = plate_with_body(&ts("- First bullet\n- Second bullet\n\nParagraph after the list."));
    let regions = regions_for(&plate);
    dump("leading list", &regions);
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body_text").collect();
    eprintln!("leading list survived: {}", !body.is_empty());
}

#[test]
fn adversarial_trailing_list() {
    // Symmetric case: list is the *last* content, so the end-marker is the
    // one glued onto a list line.
    let plate = plate_with_body(&ts("Paragraph before the list.\n\n- Only bullet"));
    let regions = regions_for(&plate);
    dump("trailing list", &regions);
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body_text").collect();
    eprintln!("trailing list survived: {}", !body.is_empty());
}

#[test]
fn adversarial_body_is_only_a_list() {
    // Nothing but a list, no plain paragraph anywhere for the marker to
    // piggyback on.
    let plate = plate_with_body(&ts("- Solitary bullet one\n- Solitary bullet two"));
    let regions = regions_for(&plate);
    dump("body is only a list", &regions);
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body_text").collect();
    eprintln!("list-only body survived: {}", !body.is_empty());
}

#[test]
fn adversarial_body_is_only_a_table() {
    // render-body captures tables via a *separate* `show table:` rule
    // (`kind: "table"`), entirely disjoint from the paragraph-capture path.
    // A marker can't be "inside" a table the way it can inside a paragraph,
    // so this checks what happens when it can only end up adjacent.
    let plate = plate_with_body(&ts("| A | B |\n| --- | --- |\n| 1 | 2 |"));
    let regions = regions_for(&plate);
    dump("body is only a table", &regions);
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body_text").collect();
    eprintln!("table-only body survived: {}", !body.is_empty());
}

#[test]
fn adversarial_leading_table_then_paragraph() {
    let plate = plate_with_body(&ts(
        "| A | B |\n| --- | --- |\n| 1 | 2 |\n\nParagraph after the table.",
    ));
    let regions = regions_for(&plate);
    dump("leading table then paragraph", &regions);
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body_text").collect();
    eprintln!("leading-table-then-paragraph survived: {}", !body.is_empty());
}

#[test]
fn adversarial_nested_list_continuation() {
    // A multi-block list item (continuation kind in render-body's buffer) —
    // the marker rides on the *first* paragraph of a nested item.
    let plate = plate_with_body(&ts(
        "Top paragraph.\n\n- Item with a continuation:\n\n  Second block inside the same item.\n\nClosing paragraph.",
    ));
    let regions = regions_for(&plate);
    dump("nested list continuation", &regions);
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body_text").collect();
    eprintln!("nested-list-continuation survived: {}", !body.is_empty());
}

#[test]
fn adversarial_blank_body() {
    // Already a documented no-ink case for the *current* scheme too — just
    // confirming the splice doesn't crash or misbehave on it.
    let plate = plate_with_body(&ts("   "));
    let regions = regions_for(&plate);
    dump("blank body", &regions);
}
