//! SPIKE — not shipped. Investigates a typst-ide-style alternative to
//! `region_scan`'s metadata-marker bracketing: instead of inserting
//! `metadata()` sentinels (proven in the #789 investigation to be
//! categorically excluded from `par.body` by Typst's realization phase, so
//! any `show par:`-based capture-and-replay package orphans them), use the
//! [`typst::syntax::Span`] every glyph already carries intrinsically
//! (`Glyph::span`, per `typst-library`'s `text/item.rs`) — the same
//! foundation `typst-ide`'s click-to-source `jump` functions use.
//!
//! A span is a property of the glyph itself, not a sibling element, so it
//! isn't subject to the "is this content paragraph-eligible" exclusion that
//! sank the marker-splice spike (`pretag_spike.rs`) — the hypothesis here is
//! that it survives a state-buffer capture/replay (`show par: p => { ...;
//! [] }`, `render-body`'s exact shape) because the rebuild repositions the
//! *same* content value rather than re-tokenizing new text.
//!
//! Caveat #1 (solved): Typst's `eval(string, mode: "markup")` collapses every
//! span in the parsed result to ONE uniform value — the span of the `source`
//! argument at the call site (`typst-library/src/foundations/mod.rs`,
//! `eval_string`'s `SpanMode::Uniform(span)`). A shared per-field loop
//! (today's `eval-content` in `lib.typ.template`) would give every field the
//! *same* span (the one `dict.at(key)` call-site expression, executed N
//! times) — useless for distinguishing fields. Fixed by using N textually
//! distinct call sites (fine — content codegen already knows the field set),
//! confirmed by `eval_call_sites_are_span_distinguishable_and_survive_rebuild`.
//!
//! Caveat #2 (NOT solved — a real conflict, not just an open task): spans
//! identify *origin* (which field this ink came from), not *placement site*.
//! The existing region contract requires the latter too — placing the same
//! field twice must surface two independent regions
//! (`content_regions.rs::field_placed_twice_yields_independent_regions`), not
//! a spanning union that claims whatever sits between them. Distinguishing
//! "two real placements separated by other content" from "one placement
//! whose sub-parts a rebuild package decorated with its own foreign-spanned
//! chrome in between" turns out to look *identical* from pure span/order
//! data — see `adjacency_grouping_correctly_splits_two_placements_of_the_same_field`
//! vs. `adjacency_grouping_incorrectly_fragments_one_placement_through_render_body`,
//! which reach opposite correct answers using the same heuristic on the same
//! shape of input. No file- or order-based rule tried here resolves both at
//! once. Verdict: span tracking is solid for single-placement attribution —
//! which is exactly what reproduces #789's zero-regions bug — but doesn't by
//! itself replace the marker system's placement-counting guarantee.
//!
//! SCOPE UPDATE: "highlight every placement" turned out not to be a real
//! requirement — "scroll to the first placement" is. That needs only
//! `placements_by_window(..).first()`, no group-labels or box/block wrapping
//! at all, since nothing gets injected into the content stream — a pure
//! read of spans that are already there. Caveat #2 stops mattering for this
//! narrower scope: `first_placement_is_the_first_occurrence_not_a_merge_when_placed_twice`
//! confirms the first entry is genuinely the earlier occurrence, not a
//! merge; and `first_placement_survives_render_body_chrome_fragmentation`
//! confirms that even when render-body's chrome fragments one placement
//! into 3 pieces (the adjacency-grouping conflict above), taking just the
//! first fragment is still a *correct*, if smaller-than-ideal, answer — it
//! points at the true start of the field's content, which is all a scroll
//! target needs to be.
//!
//! DOMINANCE CHECK — does this cover `tagged()`'s OTHER stated use (a bare
//! scalar the plate author interpolates directly, e.g. `tagged("subject")[*
//! #data.subject*]`, per `lib.typ.template`'s own doc: "a scalar has no tag
//! of its own"), without needing `tagged()` at all? Yes, and better than
//! expected:
//! - `interpolated_scalar_gets_a_resolvable_span_at_its_own_reference_site` /
//!   `two_interpolated_scalars_are_naturally_span_distinguishable`: every
//!   glyph of an interpolated scalar resolves to ONE shared span — the
//!   *reference expression's own source position* (`data.subject`'s field
//!   access, not the value's characters, since the value itself never
//!   appears in the source — it came from JSON). Two different fields
//!   interpolated at two different plate positions get two different spans
//!   automatically, no codegen or `tagged()` call needed — unlike auto-tag's
//!   shared-loop problem, each occurrence here is already a distinct AST
//!   node the plate author wrote themselves.
//! - `same_scalar_written_twice_in_source_gets_two_distinct_spans_not_one`:
//!   goes further than "first placement." If the plate author writes
//!   `data.subject` literally at two separate textual positions (header AND
//!   footer, say — not a value bound once to a variable and reused), each
//!   occurrence is its own AST node with its own span, so BOTH resolve
//!   correctly and independently — 20 matching glyphs at each, at visibly
//!   distinct positions. Caveat #2 (placement-counting) doesn't even apply
//!   here, because the source itself already has two distinct expressions,
//!   not one shared value shown twice. This pattern gets *full* placement
//!   fidelity for free, not just "first."

use std::collections::HashMap;

use quillmark_core::{FileTreeNode, Quill};
use typst::layout::{Frame, FrameItem, Point, Transform};
use typst::syntax::{FileId, Span};
use typst::{World, WorldExt};
use typst_layout::PagedDocument;

use crate::compile::compile_document;
use crate::world::QuillWorld;

/// One glyph's resolved origin (file + byte range) and its page-space
/// (top-left, pt) bounding box.
#[derive(Debug)]
struct SpanHit {
    page: usize,
    file: Option<FileId>,
    range: Option<std::ops::Range<usize>>,
    rect: [f64; 4], // x0,y0,x1,y1 top-left page space
}

/// Walk every page, resolving each glyph's span via `world.range()` — the
/// same `WorldExt` helper `error_mapping.rs` already uses for diagnostics —
/// instead of looking for `Tag::Start`/`Tag::End` markers the way
/// `region_scan::scan` does.
fn collect_span_hits(doc: &PagedDocument, world: &QuillWorld) -> Vec<SpanHit> {
    fn walk(
        frame: &Frame,
        ts: Transform,
        page_idx: usize,
        world: &QuillWorld,
        out: &mut Vec<SpanHit>,
    ) {
        for (pos, item) in frame.items() {
            match item {
                FrameItem::Group(group) => {
                    let ts = ts
                        .pre_concat(Transform::translate(pos.x, pos.y))
                        .pre_concat(group.transform);
                    walk(&group.frame, ts, page_idx, world, out);
                }
                FrameItem::Text(text) => {
                    let mut cursor = Point::zero();
                    for glyph in &text.glyphs {
                        let advance = Point::new(
                            glyph.x_advance.at(text.size),
                            glyph.y_advance.at(text.size),
                        );
                        let offset =
                            Point::new(glyph.x_offset.at(text.size), glyph.y_offset.at(text.size));
                        let local = *pos + cursor + offset;
                        let p0 = local.transform(ts);
                        let p1 = (local + advance).transform(ts);
                        let span: Span = glyph.span.0;
                        out.push(SpanHit {
                            page: page_idx,
                            file: span.id(),
                            range: world.range(span),
                            rect: [
                                p0.x.to_pt().min(p1.x.to_pt()),
                                p0.y.to_pt().min(p1.y.to_pt()),
                                p0.x.to_pt().max(p1.x.to_pt()),
                                p0.y.to_pt().max(p1.y.to_pt()),
                            ],
                        });
                        cursor += advance;
                    }
                }
                _ => {}
            }
        }
    }

    let mut out = Vec::new();
    for (page_idx, page) in doc.pages().iter().enumerate() {
        walk(&page.frame, Transform::identity(), page_idx, world, &mut out);
    }
    out
}

/// Union the rects of every hit whose resolved (file, byte-offset) falls
/// inside `[range.start, range.end)` of `file`, per page. Mirrors
/// `region_scan`'s per-(field, page) accumulation, but classification is by
/// span-range membership instead of marker open/close state.
fn union_by_window(
    hits: &[SpanHit],
    file: FileId,
    range: std::ops::Range<usize>,
) -> HashMap<usize, [f64; 4]> {
    let mut boxes: HashMap<usize, [f64; 4]> = HashMap::new();
    for hit in hits {
        let Some(hfile) = hit.file else { continue };
        let Some(hrange) = &hit.range else { continue };
        if hfile != file || !(range.start <= hrange.start && hrange.end <= range.end) {
            continue;
        }
        boxes
            .entry(hit.page)
            .and_modify(|b| {
                b[0] = b[0].min(hit.rect[0]);
                b[1] = b[1].min(hit.rect[1]);
                b[2] = b[2].max(hit.rect[2]);
                b[3] = b[3].max(hit.rect[3]);
            })
            .or_insert(hit.rect);
    }
    boxes
}

/// Group hits matching `(file, range)` into separate **placement instances**
/// by walk-order adjacency: two matching hits are the same placement only if
/// nothing *else* (any other origin, matching or not) appears between them in
/// `hits`' global order. This is what a real implementation would need
/// instead of blind unioning — `union_by_window` above merges every matching
/// hit into one box regardless of what's between them, which is wrong the
/// moment the same field is placed at two separate sites (see
/// `field_placed_twice_is_not_merged_into_one_box` below, which is exactly
/// `region_scan.rs`'s existing `field_placed_twice_yields_independent_regions`
/// contract, TDD'd against this new mechanism).
fn placements_by_window(
    hits: &[SpanHit],
    file: FileId,
    range: std::ops::Range<usize>,
) -> Vec<HashMap<usize, [f64; 4]>> {
    let matches = |h: &SpanHit| {
        h.file == Some(file)
            && h.range
                .as_ref()
                .is_some_and(|r| range.start <= r.start && r.end <= range.end)
    };

    let mut placements: Vec<HashMap<usize, [f64; 4]>> = Vec::new();
    let mut in_run = false;
    for hit in hits {
        if matches(hit) {
            if !in_run {
                placements.push(HashMap::new());
                in_run = true;
            }
            let boxes = placements.last_mut().unwrap();
            boxes
                .entry(hit.page)
                .and_modify(|b: &mut [f64; 4]| {
                    b[0] = b[0].min(hit.rect[0]);
                    b[1] = b[1].min(hit.rect[1]);
                    b[2] = b[2].max(hit.rect[2]);
                    b[3] = b[3].max(hit.rect[3]);
                })
                .or_insert(hit.rect);
        } else {
            in_run = false;
        }
    }
    placements
}

/// Build a minimal in-memory quill (no packages) for span-survival probes
/// that don't need the real `tonguetoquill-usaf-memo` package.
fn minimal_quill() -> Quill {
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: br#"
quill:
  name: span_spike
  version: 0.1.0
  backend: typst
  description: span-scan spike
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

fn compile(plate: &str) -> (PagedDocument, QuillWorld) {
    let quill = minimal_quill();
    let world = QuillWorld::new(&quill, plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    (doc, world)
}

#[test]
fn baseline_span_resolves_to_literal_source_position() {
    let plate = "#set page(width: 400pt, height: 400pt, margin: 40pt)\n[FIRSTFIELD one two three]";
    let (doc, world) = compile(plate);
    let hits = collect_span_hits(&doc, &world);
    assert!(!hits.is_empty(), "expected glyph hits");
    let start = plate.find("FIRSTFIELD").unwrap();
    let end = start + "FIRSTFIELD one two three".len();
    let boxes = union_by_window(&hits, world.main(), start..end);
    assert!(
        !boxes.is_empty(),
        "expected glyphs whose span resolves inside the literal text's own byte range: {hits:?}"
    );
}

#[test]
fn two_distinct_literal_call_sites_are_span_distinguishable() {
    // Field discrimination requires distinct textual call sites — a shared
    // loop would collapse both to one span (see module doc). Two literal
    // blocks at different source positions is the minimal case that isn't
    // collapsed.
    let plate = "#set page(width: 400pt, height: 400pt, margin: 40pt)\n[FIRSTFIELD alpha]\n\n[SECONDFIELD beta]";
    let (doc, world) = compile(plate);
    let hits = collect_span_hits(&doc, &world);

    let f_start = plate.find("FIRSTFIELD alpha").unwrap();
    let f_end = f_start + "FIRSTFIELD alpha".len();
    let s_start = plate.find("SECONDFIELD beta").unwrap();
    let s_end = s_start + "SECONDFIELD beta".len();

    let first_boxes = union_by_window(&hits, world.main(), f_start..f_end);
    let second_boxes = union_by_window(&hits, world.main(), s_start..s_end);
    assert!(!first_boxes.is_empty(), "first field must resolve: {hits:?}");
    assert!(!second_boxes.is_empty(), "second field must resolve: {hits:?}");
}

#[test]
fn span_survives_minimal_capture_and_replay() {
    // The decisive test, mirroring `pretag_spike.rs`'s
    // `minimal_capture_replay_inspect_repr`: a `show par: p => { ...; [] }`
    // rule captures `p.body` into a state buffer and replays it later via a
    // fresh `block[#c]`. The metadata-marker spike found the marker excluded
    // from `p.body` entirely; here we check whether the SURVIVING captured
    // text's glyphs keep their original span through the same rebuild.
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

#capture([FIRSTFIELD one two three.

SECONDFIELD four five six.])

#context {
  for c in BUF.get() {
    block[#c]
  }
}
"#;
    let (doc, world) = compile(plate);
    let hits = collect_span_hits(&doc, &world);
    eprintln!("hits: {hits:?}");

    let f_start = plate.find("FIRSTFIELD one two three.").unwrap();
    let f_end = f_start + "FIRSTFIELD one two three.".len();
    let s_start = plate.find("SECONDFIELD four five six.").unwrap();
    let s_end = s_start + "SECONDFIELD four five six.".len();

    let first_boxes = union_by_window(&hits, world.main(), f_start..f_end);
    let second_boxes = union_by_window(&hits, world.main(), s_start..s_end);
    eprintln!("first_boxes={first_boxes:?} second_boxes={second_boxes:?}");
    assert!(
        !first_boxes.is_empty(),
        "span-based tracking must survive the capture/replay for paragraph 1: {hits:?}"
    );
    assert!(
        !second_boxes.is_empty(),
        "span-based tracking must survive the capture/replay for paragraph 2: {hits:?}"
    );
}

#[test]
fn eval_call_sites_are_span_distinguishable_and_survive_rebuild() {
    // Closes the loop on the module doc's `SpanMode::Uniform` caveat: auto-tag
    // codegen would need N textually distinct `eval(...)` call sites (not a
    // shared runtime loop) for fields to carry different spans. This checks
    // that precisely — two separate literal `eval(..., mode: "markup")`
    // calls (standing in for what per-field codegen would emit), each still
    // distinguishable by which call site produced it, and surviving the same
    // capture-and-replay rebuild as the earlier test.
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

#capture(eval("FIRSTFIELD one two three.", mode: "markup"))

#capture(eval("SECONDFIELD four five six.", mode: "markup"))

#context {
  for c in BUF.get() {
    block[#c]
  }
}
"#;
    let (doc, world) = compile(plate);
    let hits = collect_span_hits(&doc, &world);

    // Each eval() call's span is uniform to the *call site* — the byte range
    // of its own `source` argument expression, i.e. the string literal
    // itself in this plate (codegen would instead point at wherever it
    // writes each field's `eval(...)` invocation).
    let f_call = plate.find("\"FIRSTFIELD one two three.\"").unwrap();
    let f_call_end = f_call + "\"FIRSTFIELD one two three.\"".len();
    let s_call = plate.find("\"SECONDFIELD four five six.\"").unwrap();
    let s_call_end = s_call + "\"SECONDFIELD four five six.\"".len();

    let first_boxes = union_by_window(&hits, world.main(), f_call..f_call_end);
    let second_boxes = union_by_window(&hits, world.main(), s_call..s_call_end);
    eprintln!("eval-call first_boxes={first_boxes:?} second_boxes={second_boxes:?}");
    assert!(
        !first_boxes.is_empty(),
        "field produced via its own eval() call site must resolve after rebuild: {hits:?}"
    );
    assert!(
        !second_boxes.is_empty(),
        "second field's distinct eval() call site must resolve independently: {hits:?}"
    );
}

#[test]
fn span_survives_real_render_body() {
    // Full adversary: the real vendored `tonguetoquill-usaf-memo` package.
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

    let plate = r#"
#import "@local/tonguetoquill-usaf-memo:3.0.0": frontmatter, mainmatter

#show: frontmatter.with(subject: "Spike Memo", memo_for: ("TEST/SYMB",))

#mainmatter[FIRSTFIELD one two three.

SECONDFIELD four five six.

THIRDFIELD seven eight nine, forcing AFH numbering with three paragraphs.]
"#;
    let quill = host_tree();
    let quill = Quill::from_tree(quill).expect("load usaf_memo host quill");
    let world = QuillWorld::new(&quill, plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    let hits = collect_span_hits(&doc, &world);

    let windows = [
        ("FIRSTFIELD one two three.", "first"),
        ("SECONDFIELD four five six.", "second"),
        (
            "THIRDFIELD seven eight nine, forcing AFH numbering with three paragraphs.",
            "third",
        ),
    ];
    for (needle, label) in windows {
        let start = plate.find(needle).unwrap();
        let end = start + needle.len();
        let boxes = union_by_window(&hits, world.main(), start..end);
        eprintln!("{label} boxes: {boxes:?}");
        assert!(
            !boxes.is_empty(),
            "{label} paragraph's span must survive render-body's rebuild: total hits={}",
            hits.len()
        );
    }
}

// ---------------------------------------------------------------------------
// Derisk #1 (the ranked top risk): does span identity distinguish two
// SEPARATE placements of the same field, the way `region_scan.rs`'s
// `field_placed_twice_yields_independent_regions` contract requires? The
// same eval() result, referenced twice, is the same content value with the
// SAME uniform span both times — blind window-matching can't tell them
// apart. `placements_by_window` is the proposed fix: group by walk-order
// adjacency instead of blindly unioning every match.
// ---------------------------------------------------------------------------

#[test]
fn diagnostic_lorem_span_file() {
    // Does the intervening `lorem(40)` content resolve to our plate file
    // (main.typ) or somewhere untracked (Typst's embedded corpus data)? This
    // decides whether "only break on a hit in a tracked content file"
    // (rather than "any non-matching hit") could distinguish "another
    // placement interrupting" from "package chrome decorating the same
    // placement" (the render-body number-text case).
    let plate = r#"
#set page(width: 400pt, height: 700pt, margin: 40pt)
#let content = eval("FIRSTFIELD placed here.", mode: "markup")
#content

#lorem(10)

#content
"#;
    let (doc, world) = compile(plate);
    let hits = collect_span_hits(&doc, &world);
    for h in &hits {
        let text = h.file.and_then(|f| {
            let r = h.range.as_ref()?;
            let src = world.source(f).ok()?;
            Some(src.text().get(r.clone())?.to_string())
        });
        eprintln!("file={:?} range={:?} text={:?}", h.file, h.range, text);
    }
}

#[test]
fn naive_union_incorrectly_merges_two_placements_of_the_same_field() {
    // Reproduces the risk: `content` is ONE eval() result (one uniform span,
    // exactly how auto-tagging would bind a field once), placed twice with
    // unrelated content between — mirroring
    // `content_regions.rs::field_placed_twice_yields_independent_regions`.
    let plate = r#"
#set page(width: 400pt, height: 700pt, margin: 40pt)
#let content = eval("FIRSTFIELD placed here.", mode: "markup")
#content

#lorem(40)

#content
"#;
    let (doc, world) = compile(plate);
    let hits = collect_span_hits(&doc, &world);

    let call = plate.find("\"FIRSTFIELD placed here.\"").unwrap();
    let call_end = call + "\"FIRSTFIELD placed here.\"".len();

    let merged = union_by_window(&hits, world.main(), call..call_end);
    eprintln!("naive merged boxes (expected to wrongly collapse to 1): {merged:?}");
    // Confirms the risk: exactly one page, one box — the two placements'
    // vertical extents got unioned into a single spanning rect that also
    // claims the `lorem(40)` ink sitting between them. This is the wrong
    // answer region_scan.rs's marker-based open/close was built to avoid.
    assert_eq!(
        merged.len(),
        1,
        "naive window union collapses both placements onto one page entry: {merged:?}"
    );
}

#[test]
fn adjacency_grouping_correctly_splits_two_placements_of_the_same_field() {
    let plate = r#"
#set page(width: 400pt, height: 700pt, margin: 40pt)
#let content = eval("FIRSTFIELD placed here.", mode: "markup")
#content

#lorem(40)

#content
"#;
    let (doc, world) = compile(plate);
    let hits = collect_span_hits(&doc, &world);

    let call = plate.find("\"FIRSTFIELD placed here.\"").unwrap();
    let call_end = call + "\"FIRSTFIELD placed here.\"".len();

    let placements = placements_by_window(&hits, world.main(), call..call_end);
    eprintln!("adjacency-grouped placements: {placements:?}");
    assert_eq!(
        placements.len(),
        2,
        "two separate placements of the same field must stay independent: {placements:?}"
    );
    let p0 = &placements[0][&0];
    let p1 = &placements[1][&0];
    assert!(
        p0[1] > p1[3] || p1[1] > p0[3],
        "the two placements must not vertically overlap/union: {p0:?} vs {p1:?}"
    );
}

#[test]
fn adjacency_grouping_incorrectly_fragments_one_placement_through_render_body() {
    // VERDICT: adjacency grouping does NOT generalize — this is the
    // counter-finding to the previous test, and it's a genuine conflict, not
    // a fixable implementation gap. render-body decorates each paragraph
    // with an auto-number prefix (`number-text` in the *package's own*
    // `body.typ`) between one field's own paragraphs. That prefix's glyphs
    // carry a span into `body.typ` — foreign to our field's window — so the
    // "break on any non-matching hit" rule (needed to correctly SPLIT two
    // real placements in the test above) also WRONGLY splits one field's own
    // paragraphs into 3 fragments here.
    //
    // A tighter rule — "only break on a hit that matches a *different known
    // field's* window, tolerate everything else" — fixes this specific case
    // (body.typ isn't a tracked field-content file) but then *regresses* the
    // test above: `lorem(40)`'s span there also resolves outside any tracked
    // field window (to the plate's own `main.typ`, at the `lorem()` call
    // site — see `diagnostic_lorem_span_file`), so it would be tolerated
    // too, re-merging two genuinely separate placements — reproducing
    // exactly the bug `region_scan.rs`'s marker-based open/close exists to
    // avoid, and regressing `content_regions.rs`'s existing
    // `field_placed_twice_yields_independent_regions` contract.
    //
    // Both scenarios present identically from pure span/file/order data: a
    // run of matching hits, a gap of non-matching hits, another run of
    // matching hits. Nothing in that data distinguishes "gap is package
    // chrome decorating the same placement" from "gap is the plate's own
    // unrelated content separating two placements" — that requires a signal
    // outside span/order (e.g. an explicit boundary marker, which reintroduces
    // exactly the fragility the marker approach has against rebuilds; or a
    // geometry heuristic like gap size relative to line height, which is
    // fuzzy and has its own edge cases). Conclusion: span tracking is solid
    // for single-placement attribution (the common case, and the one that
    // actually reproduces #789's zero-regions bug), but does not by itself
    // extend to safely disambiguating repeated placements of the same field.
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

    let plate = r#"
#import "@local/tonguetoquill-usaf-memo:3.0.0": frontmatter, mainmatter

#show: frontmatter.with(subject: "Spike Memo", memo_for: ("TEST/SYMB",))

#mainmatter[FIELDBODY paragraph one.

FIELDBODY paragraph two.

FIELDBODY paragraph three, forcing AFH numbering.]
"#;
    let quill = host_tree();
    let quill = Quill::from_tree(quill).expect("load usaf_memo host quill");
    let world = QuillWorld::new(&quill, plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    let hits = collect_span_hits(&doc, &world);

    // The whole body is one field in the real system (one eval() call over
    // the full markdown value) — so its window spans the *entire* literal
    // text block, all three paragraphs included, not per-paragraph.
    let start = plate.find("FIELDBODY paragraph one.").unwrap();
    let end = plate.find("forcing AFH numbering.").unwrap() + "forcing AFH numbering.".len();

    // Diagnostic: dump what actually sits between matching hits in the full
    // walk order, to see what's breaking adjacency.
    let matches = |h: &SpanHit| {
        h.file == Some(world.main())
            && h.range.as_ref().is_some_and(|r| start <= r.start && r.end <= end)
    };
    let mut prev_match = false;
    for h in &hits {
        let m = matches(h);
        if m != prev_match {
            let text_range = h.range.as_ref().map(|r| {
                let src = world.source(h.file.unwrap()).unwrap();
                src.text()[r.clone()].to_string()
            });
            eprintln!(
                "transition matches={m} file={:?} range={:?} text={:?} rect={:?}",
                h.file, h.range, text_range, h.rect
            );
        }
        prev_match = m;
    }

    let placements = placements_by_window(&hits, world.main(), start..end);
    eprintln!("single-field multi-paragraph placements: {placements:?}");
    assert_eq!(
        placements.len(),
        3,
        "documents the conflict: walk-order adjacency wrongly fragments one placement into \
         one-per-paragraph because render-body's own number-text prefix (body.typ) sits between \
         them with a foreign span: {placements:?}"
    );
}

// ---------------------------------------------------------------------------
// Scoped-down requirement: "highlight every placement" is out of scope;
// "scroll to the first placement" is the actual target. This needs only
// `placements_by_window(..).first()` — no group-labels, no box/block
// wrapping, no layout-neutrality or page-spanning concerns, since nothing is
// injected into the content stream at all. The two risks that mattered for
// *enumerating all* placements are checked here to confirm they don't matter
// for *finding the first* one:
// - a field placed twice must resolve to the FIRST occurrence, not a merge
//   of both (already true — `placements_by_window` never merges instances).
// - render-body's own chrome fragmenting one placement into 3 pieces
//   (the immediately preceding test) is harmless here: `.first()` still
//   points at the true start of the field's content — a smaller-than-ideal
//   box, not a wrong one, which is all "scroll to it" needs.
// ---------------------------------------------------------------------------

#[test]
fn first_placement_is_the_first_occurrence_not_a_merge_when_placed_twice() {
    let plate = r#"
#set page(width: 400pt, height: 700pt, margin: 40pt)
#let content = eval("FIRSTFIELD placed here.", mode: "markup")
#content

#lorem(40)

#content
"#;
    let (doc, world) = compile(plate);
    let hits = collect_span_hits(&doc, &world);

    let call = plate.find("\"FIRSTFIELD placed here.\"").unwrap();
    let call_end = call + "\"FIRSTFIELD placed here.\"".len();

    let placements = placements_by_window(&hits, world.main(), call..call_end);
    let first = placements.first().expect("at least one placement");
    eprintln!("first placement: {first:?}, total placements found: {}", placements.len());

    // "First" must mean the earlier one in document/page order (smaller y —
    // top-left origin), not whichever the union happened to produce.
    let second = &placements[1];
    assert!(
        first[&0][1] < second[&0][1],
        "the FIRST placement must be the one earlier in reading order: {first:?} vs {second:?}"
    );
}

#[test]
fn first_placement_survives_render_body_chrome_fragmentation() {
    // Reuses the exact scenario that fragmented into 3 pieces above — for
    // "scroll to the first placement," taking just the first fragment is
    // correct, not a degraded answer: it's the true start of the field's
    // content on the page it first appears.
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

    let plate = r#"
#import "@local/tonguetoquill-usaf-memo:3.0.0": frontmatter, mainmatter

#show: frontmatter.with(subject: "Spike Memo", memo_for: ("TEST/SYMB",));

#mainmatter[FIELDBODY paragraph one.

FIELDBODY paragraph two.

FIELDBODY paragraph three, forcing AFH numbering.]
"#;
    let quill = Quill::from_tree(host_tree()).expect("load usaf_memo host quill");
    let world = QuillWorld::new(&quill, plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    let hits = collect_span_hits(&doc, &world);

    let start = plate.find("FIELDBODY paragraph one.").unwrap();
    let end = plate.find("forcing AFH numbering.").unwrap() + "forcing AFH numbering.".len();

    let placements = placements_by_window(&hits, world.main(), start..end);
    let first = placements.first().expect("at least one placement, fragmented or not");
    eprintln!(
        "first placement (of {} fragments): {first:?}",
        placements.len()
    );

    // Whatever fragment count, the first one must be non-empty and on the
    // page the field's content actually starts on — sufficient to scroll to.
    let (page, rect) = first.iter().next().expect("first placement has a page entry");
    assert_eq!(*page, 0, "the field's content starts on page 0");
    assert!(rect[2] > rect[0], "the first fragment has real (non-zero) extent: {rect:?}");
}

// ---------------------------------------------------------------------------
// Closes the remaining unverified claim: does `tagged()`'s OTHER use case — a
// bare scalar the plate author interpolates directly (`tagged("subject")[*
// #data.subject*]`, per lib.typ.template's own doc comment: "a scalar has no
// tag of its own") — also get a naturally-distinguishable, resolvable span
// WITHOUT needing `tagged()` at all? If the plate author's own `#data.x`
// reference is a distinct call site in their own file (unlike auto-tag's
// shared per-field loop), it should be — dominance over the current design
// would extend to scalars too, not just rebuilt content.
// ---------------------------------------------------------------------------

#[test]
fn interpolated_scalar_gets_a_resolvable_span_at_its_own_reference_site() {
    // `subject` here stands in for a JSON-sourced string value (exactly what
    // `data.subject` is in the real pipeline) — bound as a plain Typst
    // string, not literal markup, then interpolated directly.
    let plate = r#"
#set page(width: 400pt, height: 400pt, margin: 40pt)
#let subject = "Request for Quarters"
SUBJECT: #subject
"#;
    let (doc, world) = compile(plate);
    let hits = collect_span_hits(&doc, &world);

    // Where does the interpolation SITE sit? Just the `subject` identifier
    // (Typst's span for a bare identifier reference is exactly its own
    // text, not the leading `#` — confirmed empirically).
    let ref_site = plate.rfind("subject").unwrap();
    let ref_end = ref_site + "subject".len();

    let matches: Vec<&SpanHit> = hits
        .iter()
        .filter(|h| {
            h.file == Some(world.main())
                && h.range.as_ref().is_some_and(|r| r.start == ref_site && r.end == ref_end)
        })
        .collect();
    eprintln!("hits near the #subject reference site: {matches:?}");
    assert!(
        !matches.is_empty(),
        "the interpolated scalar's glyphs must resolve to a span near its own reference site: \
         ref_site={ref_site}..{ref_end}, all hits={:?}",
        hits.iter().map(|h| (h.file, h.range.clone())).collect::<Vec<_>>()
    );
    // Every glyph of the interpolated value must resolve to the SAME range
    // (the reference expression, not individual characters of the value —
    // the value itself never appears in the source at all, it came from
    // JSON/a variable).
    let distinct_ranges: std::collections::HashSet<_> =
        matches.iter().filter_map(|h| h.range.clone()).collect();
    assert_eq!(
        distinct_ranges.len(),
        1,
        "all glyphs of one interpolated scalar must share one span: {distinct_ranges:?}"
    );
}

#[test]
fn two_interpolated_scalars_are_naturally_span_distinguishable() {
    let plate = r#"
#set page(width: 400pt, height: 400pt, margin: 40pt)
#let subject = "Request for Quarters"
#let signee = "FIRST M. LAST"
SUBJECT: #subject

SIGNED: #signee
"#;
    let (doc, world) = compile(plate);
    let hits = collect_span_hits(&doc, &world);
    for h in &hits {
        let text = h.file.and_then(|f| {
            let r = h.range.as_ref()?;
            let src = world.source(f).ok()?;
            Some(src.text().get(r.clone())?.to_string())
        });
        eprintln!(
            "page={} range={:?} text={:?} rect={:?}",
            h.page, h.range, text, h.rect
        );
    }

    let subject_ref = plate.rfind("#subject").unwrap() + 1; // skip '#'
    let signee_ref = plate.rfind("#signee").unwrap() + 1;

    let subject_hits: Vec<_> = hits
        .iter()
        .filter(|h| {
            h.range
                .as_ref()
                .is_some_and(|r| r.start == subject_ref && r.end == subject_ref + "subject".len())
        })
        .collect();
    let signee_hits: Vec<_> = hits
        .iter()
        .filter(|h| {
            h.range
                .as_ref()
                .is_some_and(|r| r.start == signee_ref && r.end == signee_ref + "signee".len())
        })
        .collect();
    assert!(!subject_hits.is_empty(), "expected glyphs resolving exactly to the `subject` reference");
    assert!(!signee_hits.is_empty(), "expected glyphs resolving exactly to the `signee` reference");
    assert_ne!(
        subject_hits[0].range, signee_hits[0].range,
        "two different scalar fields must resolve to two different spans"
    );
}

#[test]
fn same_scalar_written_twice_in_source_gets_two_distinct_spans_not_one() {
    // The variant that matters for "does this need placement-counting too":
    // NOT a value bound once to a variable and shown twice (that recreates
    // the shared-span problem), but the plate author literally writing
    // `#data.subject`-shaped field access at two SEPARATE textual positions
    // — the ordinary way a plate would reference the same field twice (a
    // header and a footer, say). Each occurrence is a distinct AST node
    // parsed at its own source position, unlike a bound variable reused.
    let plate = r#"
#set page(width: 400pt, height: 700pt, margin: 40pt)
#let data = (subject: "Request for Quarters")
Header — #data.subject

#lorem(40)

Footer — #data.subject
"#;
    let (doc, world) = compile(plate);
    let hits = collect_span_hits(&doc, &world);

    // Two distinct textual occurrences of `data.subject` field access — the
    // resolved span covers the whole field-access expression (confirmed
    // empirically: it starts at "data", not just ".subject").
    let first_ref = plate.find("data.subject").unwrap();
    let first_ref_end = first_ref + "data.subject".len();
    let second_ref = plate.rfind("data.subject").unwrap();
    let second_ref_end = second_ref + "data.subject".len();
    assert_ne!(first_ref, second_ref, "sanity: these must be different source positions");

    let ranges: std::collections::HashSet<_> = hits
        .iter()
        .filter(|h| h.file == Some(world.main()))
        .filter_map(|h| h.range.clone())
        .collect();
    eprintln!("distinct resolved ranges seen: {ranges:?}");
    eprintln!("first ref site: {:?}, second ref site: {:?}", first_ref..first_ref_end, second_ref..second_ref_end);

    let at_first: Vec<_> = hits
        .iter()
        .filter(|h| {
            h.file == Some(world.main())
                && h.range.as_ref().is_some_and(|r| r.start >= first_ref && r.end <= first_ref_end + 1)
        })
        .collect();
    let at_second: Vec<_> = hits
        .iter()
        .filter(|h| {
            h.file == Some(world.main())
                && h.range.as_ref().is_some_and(|r| r.start >= second_ref && r.end <= second_ref_end + 1)
        })
        .collect();
    eprintln!("hits at first .subject occurrence: {}", at_first.len());
    eprintln!("hits at second .subject occurrence: {}", at_second.len());

    assert!(!at_first.is_empty(), "expected glyphs resolving to the first occurrence's own span");
    assert!(!at_second.is_empty(), "expected glyphs resolving to the second occurrence's own span");

    // The two occurrences' hits must be at visually different y-positions
    // (header vs. footer), confirming they're genuinely two separate
    // placements distinguishable purely from span identity — no
    // placement-counting logic needed, because the SOURCE itself already
    // has two distinct expressions, not one value shown twice.
    let y_first = at_first[0].rect[1];
    let y_second = at_second[0].rect[1];
    assert!(
        (y_first - y_second).abs() > 20.0,
        "the two occurrences must be at visually distinct positions: {y_first} vs {y_second}"
    );
}

// ---------------------------------------------------------------------------
// Derisk: does the "N distinct eval() call sites" codegen fix actually scale
// to a realistic schema — several top-level fields, an array field with a
// runtime-determined element count, and a card kind with several instances
// each carrying its own content field (the exact `$cards.<kind>.<n>.<field>`
// shape `content_regions.rs::card_regions_use_canonical_kind_ordinal_path`
// and `usaf_memo_regions_test.rs` already exercise for the CURRENT marker
// mechanism)? Only tested with 1-2 handpicked fields so far.
// ---------------------------------------------------------------------------

/// Simulates the codegen `lib.typ`'s auto-tag pass would need: one textually
/// distinct `#let _field_<n> = eval("...", mode: "markup")` binding per
/// field/array-element/card-field, instead of today's shared loop. Returns
/// the generated source plus each binding's field path and the exact byte
/// window `eval`'s string argument occupies (what codegen would track).
fn generate_distinct_eval_bindings(
    entries: &[(&str, &str)], // (field_path, markdown_value)
) -> (String, Vec<(String, std::ops::Range<usize>)>) {
    let mut src = String::new();
    let mut windows = Vec::new();
    for (i, (path, value)) in entries.iter().enumerate() {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        let prefix = format!("#let _field_{i} = eval(");
        // Typst's span for a string-literal argument covers the literal
        // INCLUDING its surrounding quotes (empirically confirmed: expected
        // window was one byte narrow on each side, off by exactly the quote
        // characters) — so the window starts at the opening `"`, not after it.
        let arg_start = src.len() + prefix.len();
        src.push_str(&prefix);
        src.push('"');
        src.push_str(&escaped);
        src.push('"');
        let arg_end = src.len();
        src.push_str(", mode: \"markup\")\n");
        windows.push((path.to_string(), arg_start..arg_end));
    }
    (src, windows)
}

#[test]
fn realistic_multi_field_array_and_card_codegen_all_resolve_distinctly() {
    let entries = [
        ("subject", "Request for Quarters"),
        ("intro", "A short introductory paragraph."),
        ("refs.0", "First reference document."),
        ("refs.1", "Second reference document."),
        ("refs.2", "Third reference document."),
        ("$cards.indorsement.0.$body", "Indorsement zero body text."),
        ("$cards.indorsement.1.$body", "Indorsement one body text."),
    ];
    let (bindings_src, windows) = generate_distinct_eval_bindings(&entries);

    let mut placements_src = String::new();
    for i in 0..entries.len() {
        placements_src.push_str(&format!("#_field_{i}\n\n"));
    }

    // `bindings_src` must come FIRST in the plate — the windows above were
    // computed relative to its own start at byte 0; anything prepended
    // (like `#set page(..)`) would silently shift every window and produce
    // false negatives that look like a Typst problem but are a test-harness
    // bug (caught empirically: every field "failed" identically).
    let plate = format!(
        "{bindings_src}\n#set page(width: 400pt, height: 900pt, margin: 40pt)\n{placements_src}"
    );

    let (doc, world) = compile(&plate);
    let hits = collect_span_hits(&doc, &world);

    for (path, window) in &windows {
        let boxes = union_by_window(&hits, world.main(), window.clone());
        eprintln!("field {path:?} window {window:?} -> boxes {boxes:?}");
        assert!(
            !boxes.is_empty(),
            "field {path:?} must resolve to a non-empty region at realistic schema scale"
        );
    }

    // Cross-check: no two DIFFERENT fields' windows produce overlapping
    // (indistinguishable) hit sets — every field's matching hits must be
    // disjoint from every other field's.
    let per_field_hits: Vec<(String, Vec<&SpanHit>)> = windows
        .iter()
        .map(|(path, window)| {
            let matches: Vec<&SpanHit> = hits
                .iter()
                .filter(|h| {
                    h.file == Some(world.main())
                        && h.range
                            .as_ref()
                            .is_some_and(|r| window.start <= r.start && r.end <= window.end)
                })
                .collect();
            (path.clone(), matches)
        })
        .collect();
    for i in 0..per_field_hits.len() {
        for j in (i + 1)..per_field_hits.len() {
            let (name_i, hits_i) = &per_field_hits[i];
            let (name_j, hits_j) = &per_field_hits[j];
            let overlap = hits_i.iter().any(|a| hits_j.iter().any(|b| a.rect == b.rect));
            assert!(
                !overlap,
                "fields {name_i:?} and {name_j:?} must not share any hit: {hits_i:?} vs {hits_j:?}"
            );
        }
    }
}

#[test]
fn realistic_card_body_field_survives_render_body_at_scale() {
    // Combines the multi-field/card codegen shape above with the real
    // adversary: one of the card body fields is piped through mainmatter
    // (matching usaf_memo's `$cards.indorsement.<n>.$body` -> `indorsement`
    // package call), verifying the whole codegen shape still survives the
    // rebuild, not just a single isolated field.
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

    let entries = [
        ("subject", "Request for Quarters"),
        ("refs.0", "First reference document."),
        (
            "$cards.indorsement.0.$body",
            "Indorsement paragraph one.\n\nIndorsement paragraph two.",
        ),
    ];
    let (bindings_src, windows) = generate_distinct_eval_bindings(&entries);

    // `bindings_src` must start the plate at byte 0 — see the comment on the
    // sibling test above; `#let` bindings don't need to follow `#import`
    // textually, only their USE (in `#mainmatter[..]`) does.
    let plate = format!(
        r#"{bindings_src}
#import "@local/tonguetoquill-usaf-memo:3.0.0": frontmatter, mainmatter

#show: frontmatter.with(subject: "Spike Memo", memo_for: ("TEST/SYMB",));

#_field_0

#_field_1

#mainmatter[#_field_2]
"#
    );

    let quill = Quill::from_tree(host_tree()).expect("load usaf_memo host quill");
    let world = QuillWorld::new(&quill, &plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    let hits = collect_span_hits(&doc, &world);

    for (path, window) in &windows {
        let boxes = union_by_window(&hits, world.main(), window.clone());
        eprintln!("field {path:?} -> boxes {boxes:?}");
        assert!(
            !boxes.is_empty(),
            "field {path:?} must resolve even when one sibling field goes through render-body"
        );
    }
}
