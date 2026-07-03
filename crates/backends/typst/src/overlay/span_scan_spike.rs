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
//! Caveat surfaced along the way: Typst's `eval(string, mode: "markup")`
//! collapses every span in the parsed result to ONE uniform value — the
//! span of the `source` argument at the call site
//! (`typst-library/src/foundations/mod.rs`, `eval_string`'s
//! `SpanMode::Uniform(span)`). A shared per-field loop (today's
//! `eval-content` in `lib.typ.template`) would give every field the *same*
//! span (the one `dict.at(key)` call-site expression, executed N times) —
//! useless for distinguishing fields. Field-level discrimination would need
//! either N textually distinct call sites (fine — content codegen already
//! knows the field set) or `SpanMode::Mapped`, not the shared-loop shape
//! used today. Tested directly below via two distinct literal call sites.

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
