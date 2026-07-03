//! SPIKE — not shipped. Tests a third alternative to metadata-marker
//! bracketing and span-based tracking: label an inline `box()`/block `block()`
//! that wraps the field's *actual visible content itself* (not a decorative
//! sibling the way `metadata()` markers are), and read the label back off the
//! resulting `FrameItem::Group` (`GroupItem.label`, set by
//! `typst-layout/src/inline/box.rs:75-76` and `flow/block.rs:98-99,244-247`
//! whenever the wrapped element itself carries a label).
//!
//! Two prior findings this is trying to reconcile:
//! - `pretag_spike.rs`: `metadata()` is categorically excluded from
//!   `par.body` — Typst's realization phase treats it as non-paragraph
//!   "meta" content regardless of source adjacency.
//! - `span_scan_spike.rs`: `Span` survives capture/replay (it's intrinsic to
//!   the glyph), but is tied to *origin*, not *occurrence* — it can't
//!   distinguish two placements of the same field.
//!
//! Hypothesis: a labeled `box()`/`block()` wrapping the real content (a) is
//! NOT "meta" content — it's the paragraph's actual visible text, so it
//! should be captured into `p.body` where metadata wasn't; and (b) gets a
//! *fresh* `Group` at every layout/realization — same mechanism as
//! `Tag`/`Location`, not `Span` — so two placements of the same labeled value
//! should produce two independent `Group` occurrences.
//!
//! VERDICT: both hold, and this resolves the conflict `span_scan_spike.rs`
//! found unsolvable.
//!
//! - `labeled_box_inside_paragraph_survives_minimal_capture_and_replay`: a
//!   labeled `box()` wrapping the paragraph's entire real content — not a
//!   decorative sibling — DOES get captured into `p.body`, unlike bare
//!   `metadata()`. It's real visible content, not "meta," so the
//!   paragraph-grouping exclusion that sank the marker spike doesn't apply.
//! - `labeled_box_gives_fresh_group_per_placement_unlike_span`: the same
//!   labeled value, placed twice with unrelated content between, yields TWO
//!   independent `Group` occurrences — because `Group`s are minted at
//!   layout/realization time (same mechanism as `Tag`/`Location`), not tied
//!   to origin the way `Span` is. This is the placement-counting guarantee
//!   spans structurally cannot give.
//! - `labeled_block_output_wrap_keeps_occurrence_identity_when_placed_twice`:
//!   both properties together, against the real vendored `render-body`,
//!   using the pattern that would actually ship — label the package's
//!   *output* (`#block[#mainmatter[..]]<label>`, mirroring how `tagged()`
//!   already brackets output today) rather than its raw input. Two
//!   placements of that wrapped value → two clean, disjoint regions.
//!
//! One caveat, *not* fully root-caused: wrapping the *raw pre-rebuild*
//! multi-paragraph input in a labeled box (rather than wrapping the output)
//! produces two identically-sized spurious occurrences of unclear origin —
//! see `labeled_box_wrapping_raw_multi_paragraph_input_does_not_survive`'s
//! diagnostic dump; a likely suspect is `render-body`'s own internal
//! `measure()` call in `body.typ`, which lays content out a second time
//! purely to read its height. Not investigated further because the viable,
//! cleanly-validated pattern is output-wrapping, matching `tagged()`'s
//! existing contract — this spike doesn't need input-wrapping to work.

use std::collections::HashMap;

use quillmark_core::{FileTreeNode, Quill};
use typst::foundations::Label;
use typst::layout::{Frame, FrameItem, Point, Transform};
use typst::utils::PicoStr;
use typst_layout::PagedDocument;

use crate::compile::compile_document;
use crate::world::QuillWorld;

#[derive(Debug)]
struct GroupHit {
    #[allow(dead_code)]
    page: usize,
    rect: [f64; 4],
}

/// Walk every page for `FrameItem::Group` items whose label matches `want`,
/// recording each occurrence's own frame bounds (the group's frame size,
/// transformed into page space) — no manual leaf-union needed, unlike
/// `region_scan`'s marker approach, since the group's frame already bounds
/// its content.
fn collect_group_hits(doc: &PagedDocument, want: Label) -> Vec<GroupHit> {
    fn walk(frame: &Frame, ts: Transform, page_idx: usize, want: Label, out: &mut Vec<GroupHit>) {
        for (pos, item) in frame.items() {
            match item {
                FrameItem::Group(group) => {
                    let inner_ts = ts
                        .pre_concat(Transform::translate(pos.x, pos.y))
                        .pre_concat(group.transform);
                    if group.label == Some(want) {
                        let size = group.frame.size();
                        let corners = [
                            Point::zero(),
                            Point::new(size.x, Point::zero().y),
                            Point::new(Point::zero().x, size.y),
                            Point::new(size.x, size.y),
                        ];
                        let mut min_x = f64::INFINITY;
                        let mut min_y = f64::INFINITY;
                        let mut max_x = f64::NEG_INFINITY;
                        let mut max_y = f64::NEG_INFINITY;
                        for c in corners {
                            let p = c.transform(inner_ts);
                            min_x = min_x.min(p.x.to_pt());
                            min_y = min_y.min(p.y.to_pt());
                            max_x = max_x.max(p.x.to_pt());
                            max_y = max_y.max(p.y.to_pt());
                        }
                        out.push(GroupHit {
                            page: page_idx,
                            rect: [min_x, min_y, max_x, max_y],
                        });
                    }
                    walk(&group.frame, inner_ts, page_idx, want, out);
                }
                _ => {}
            }
        }
    }

    let mut out = Vec::new();
    for (page_idx, page) in doc.pages().iter().enumerate() {
        walk(&page.frame, Transform::identity(), page_idx, want, &mut out);
    }
    out
}

fn label_for(name: &str) -> Label {
    Label::new(PicoStr::intern(name)).expect("non-empty label")
}

fn minimal_quill() -> Quill {
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: br#"
quill:
  name: group_label_spike
  version: 0.1.0
  backend: typst
  description: group-label scan spike
typst:
  plate_file: plate.typ
main:
  fields: {}
"#
            .to_vec(),
        },
    );
    Quill::from_tree(FileTreeNode::Directory { files }).expect("load quill")
}

fn compile(plate: &str) -> PagedDocument {
    let quill = minimal_quill();
    let world = QuillWorld::new(&quill, plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    doc
}

fn host_tree() -> FileTreeNode {
    fn walk(dir: &std::path::Path) -> std::io::Result<FileTreeNode> {
        let mut files = HashMap::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let p = entry.path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if p.is_file() {
                files.insert(
                    name,
                    FileTreeNode::File {
                        contents: std::fs::read(&p)?,
                    },
                );
            } else if p.is_dir() {
                files.insert(name, walk(&p)?);
            }
        }
        Ok(FileTreeNode::Directory { files })
    }
    let quill_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("resources")
        .join("quills")
        .join("usaf_memo")
        .join("0.2.0");
    walk(&quill_path).expect("walk fixture")
}

#[test]
fn labeled_box_survives_direct_placement() {
    let plate = r#"
#set page(width: 400pt, height: 400pt, margin: 40pt)
#box[FIRSTFIELD one two three.]<qm-field-first>
"#;
    let doc = compile(plate);
    let hits = collect_group_hits(&doc, label_for("qm-field-first"));
    eprintln!("direct placement hits: {hits:?}");
    assert_eq!(hits.len(), 1, "one direct placement, one group: {hits:?}");
    assert!(hits[0].rect[2] > hits[0].rect[0] && hits[0].rect[3] > hits[0].rect[1]);
}

#[test]
fn labeled_box_gives_fresh_group_per_placement_unlike_span() {
    // The critical test: two placements of the SAME labeled value. If group
    // creation is occurrence-based (like Tag/Location) rather than
    // origin-based (like Span), this must yield 2 independent hits.
    let plate = r#"
#set page(width: 400pt, height: 700pt, margin: 40pt)
#let content = [#box[FIRSTFIELD placed here.]<qm-field-first>]
#content

#lorem(40)

#content
"#;
    let doc = compile(plate);
    let hits = collect_group_hits(&doc, label_for("qm-field-first"));
    eprintln!("two-placement hits: {hits:?}");
    assert_eq!(
        hits.len(),
        2,
        "the same labeled value placed twice must yield two independent group occurrences: {hits:?}"
    );
    assert!(
        hits[0].rect[1] > hits[1].rect[3] || hits[1].rect[1] > hits[0].rect[3],
        "the two occurrences must not overlap/collapse: {hits:?}"
    );
}

#[test]
fn labeled_box_inside_paragraph_survives_minimal_capture_and_replay() {
    // Does a labeled box, when it constitutes the paragraph's ENTIRE inline
    // content (not a decorative sibling), get captured into p.body — unlike
    // bare metadata()? Uses the same minimal show-par capture rule as
    // pretag_spike.rs / span_scan_spike.rs.
    let plate = r#"
#set page(width: 400pt, height: 400pt, margin: 40pt)

#let BUF = state("BUF", ())
#let capture(it) = {
  show par: p => {
    BUF.update(buf => buf + (text([#p.body]),))
    []
  }
  it
}

#capture([#box[FIRSTFIELD one two three.]<qm-field-first>])

#context {
  for c in BUF.get() {
    block[#c]
  }
}
"#;
    let doc = compile(plate);
    let hits = collect_group_hits(&doc, label_for("qm-field-first"));
    eprintln!("minimal capture/replay hits: {hits:?}");
    assert!(
        !hits.is_empty(),
        "a labeled box wrapping the paragraph's real content must survive the capture/replay: {hits:?}"
    );
}

#[test]
fn labeled_box_wrapping_raw_single_paragraph_input_survives_by_accident() {
    // Correction to the module doc's original hypothesis: for a SINGLE
    // paragraph, wrapping the raw pre-rebuild input in a labeled box DOES
    // survive — because that one paragraph's `p.body`, when render-body's
    // `show par:` captures it, IS the whole labeled box (nothing else is in
    // that paragraph), so the label rides along into the capture exactly
    // like `labeled_box_inside_paragraph_survives_minimal_capture_and_replay`
    // already showed. This isn't the general case — see the next test.
    let plate = r#"
#import "@local/tonguetoquill-usaf-memo:3.0.0": frontmatter, mainmatter

#show: frontmatter.with(subject: "Spike Memo", memo_for: ("TEST/SYMB",));

#mainmatter[#box[FIRSTFIELD one two three.]<qm-field-first>]
"#;
    let quill = Quill::from_tree(host_tree()).expect("load usaf_memo host quill");
    let world = QuillWorld::new(&quill, plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    let hits = collect_group_hits(&doc, label_for("qm-field-first"));
    eprintln!("single-paragraph raw-input-wrapped hits: {hits:?}");
    assert!(
        !hits.is_empty(),
        "single-paragraph case survives (the label rides inside p.body): {hits:?}"
    );
}

#[test]
fn labeled_box_wrapping_raw_multi_paragraph_input_does_not_survive() {
    // The real auto-tag shape: a field's markdown is usually multiple
    // paragraphs. Wrapping ALL of them in ONE outer labeled box before
    // handing it to render-body: render-body's `show par:` fires once per
    // paragraph, each capturing its OWN `p.body` — the label lives on the
    // OUTER wrapper spanning all of them, not on any individual paragraph,
    // so no single capture carries it, and the outer wrapper ends up empty
    // once its paragraph children are replaced with `[]` at the original
    // site. Same structural reason `tagged()` has to wrap the package's
    // *output*, not its input.
    let plate = r#"
#import "@local/tonguetoquill-usaf-memo:3.0.0": frontmatter, mainmatter

#show: frontmatter.with(subject: "Spike Memo", memo_for: ("TEST/SYMB",));

#mainmatter[#box[FIRSTFIELD one two three.

SECONDFIELD four five six.]<qm-field-first>]
"#;
    let quill = Quill::from_tree(host_tree()).expect("load usaf_memo host quill");
    let world = QuillWorld::new(&quill, plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    // Diagnostic: dump every Group frame item (labeled or not) to understand
    // where the second, unexpected occurrence comes from.
    fn dump_groups(frame: &Frame, depth: usize) {
        for (_, item) in frame.items() {
            if let FrameItem::Group(g) = item {
                eprintln!(
                    "{}group label={:?} size={:?}",
                    "  ".repeat(depth),
                    g.label,
                    g.frame.size()
                );
                dump_groups(&g.frame, depth + 1);
            }
        }
    }
    for page in doc.pages() {
        dump_groups(&page.frame, 0);
    }

    let hits = collect_group_hits(&doc, label_for("qm-field-first"));
    eprintln!("multi-paragraph raw-input-wrapped hits: {hits:?}");
    // OPEN QUESTION, not a clean confirmation either way: two identically-
    // sized labeled groups appear, at different positions, both matching the
    // FIRST paragraph's dimensions ("SECONDFIELD ..." doesn't appear at all
    // in either). The group dump above shows this isn't simple survival —
    // something in render-body's own pipeline (a likely suspect: its
    // `measure(final_par, ...)` call in body.typ, which lays content out a
    // second time purely to read its height for the sticky/breakable
    // decision) produces a duplicate. Not root-caused here; NOT the pattern
    // to build on without further digging. The clean, validated pattern is
    // the next test: label the package's OUTPUT (mirroring tagged() today),
    // which gives exactly one correct hit.
    assert_eq!(
        hits.len(),
        2,
        "documents the open question rather than asserting either clean outcome: {hits:?}"
    );
}

#[test]
fn labeled_block_wrapping_render_body_output_survives_and_keeps_occurrence_identity() {
    // The viable pattern: label render-body's OUTPUT (exactly how `tagged()`
    // brackets the package's output today) with a `block` (not `box` — the
    // output is itself multi-paragraph, block-level content). Also the
    // pattern `flow/block.rs` documents labeling every FRAGMENT of a
    // breakable block, matching the "one region per page fragment"
    // contract, for free.
    let plate = r#"
#import "@local/tonguetoquill-usaf-memo:3.0.0": frontmatter, mainmatter

#show: frontmatter.with(subject: "Spike Memo", memo_for: ("TEST/SYMB",));

#block[#mainmatter[FIRSTFIELD one two three.

SECONDFIELD four five six.]]<qm-field-body>
"#;
    let quill = Quill::from_tree(host_tree()).expect("load usaf_memo host quill");
    let world = QuillWorld::new(&quill, plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    let hits = collect_group_hits(&doc, label_for("qm-field-body"));
    eprintln!("output-wrapped (block) render-body hits: {hits:?}");
    assert!(
        !hits.is_empty(),
        "labeling render-body's output with a block (like tagged() already does) survives: {hits:?}"
    );
}

#[test]
fn labeled_block_output_wrap_keeps_occurrence_identity_when_placed_twice() {
    // Combines both properties for the pattern that would actually ship: the
    // SAME labeled-and-wrapped render-body output, referenced twice with
    // unrelated content between (mainmatter is called once, building one
    // value; that value is placed twice) — must survive AND stay two
    // independent occurrences, the way `field_placed_twice_yields_independent_regions`
    // requires, unlike span identity.
    let plate = r#"
#import "@local/tonguetoquill-usaf-memo:3.0.0": frontmatter, mainmatter

#show: frontmatter.with(subject: "Spike Memo", memo_for: ("TEST/SYMB",));

#let wrapped = [#block[#mainmatter[FIRSTFIELD one two three.]]<qm-field-body>]
#wrapped

#lorem(40)

#wrapped
"#;
    let quill = Quill::from_tree(host_tree()).expect("load usaf_memo host quill");
    let world = QuillWorld::new(&quill, plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    let hits = collect_group_hits(&doc, label_for("qm-field-body"));
    eprintln!("twice-placed output-wrapped hits: {hits:?}");
    assert_eq!(
        hits.len(),
        2,
        "the same rebuilt-and-labeled value, placed twice, must stay two independent occurrences: {hits:?}"
    );
    assert!(
        hits[0].rect[1] > hits[1].rect[3] || hits[1].rect[1] > hits[0].rect[3],
        "the two occurrences must not overlap/collapse: {hits:?}"
    );
}
