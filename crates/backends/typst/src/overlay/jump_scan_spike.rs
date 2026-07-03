//! SPIKE — not shipped. Tests a typst-ide-shaped click→source navigation
//! (`jump_from_click`'s direction) as a *replacement scope*, not a fix, for
//! the placement-counting problem `span_scan_spike.rs` found unsolvable.
//!
//! Reframing: `region.rs`'s own docs describe two distinct consumer needs —
//! "click a rendered field → focus it in the editor" (forward: point →
//! field) and "highlight the page rectangle for the focused field" (reverse:
//! field → all its boxes). Every mechanism spiked so far (metadata markers,
//! `Span`, labeled `Group`) was aimed at the reverse direction, which is
//! where placement-counting bites — enumerating N boxes for one field
//! requires telling separate placements apart.
//!
//! The forward direction never has that problem: given ONE specific clicked
//! point, there is exactly one frame item there, and its `Span` unambiguously
//! answers "which field produced this ink" — regardless of how many OTHER
//! places that field is also placed. Same mechanism `typst-ide`'s
//! `jump_from_click` already uses in production (click → frame item → its
//! span → source position); here the last step maps to a known field window
//! instead of a source location.
//!
//! Hypothesis: this fully and robustly solves click-to-navigate, including
//! through `render-body`'s rebuild (spans already proven to survive that),
//! *without* needing markers, group-labels, or any placement-counting logic
//! at all.

use std::collections::HashMap;

use quillmark_core::{FileTreeNode, Quill};
use typst::layout::{Frame, FrameItem, Point, Transform};
use typst::syntax::FileId;
use typst::{World, WorldExt};
use typst_layout::PagedDocument;

use crate::compile::compile_document;
use crate::world::QuillWorld;

#[derive(Debug, Clone)]
struct SpanHit {
    page: usize,
    file: Option<FileId>,
    range: Option<std::ops::Range<usize>>,
    rect: [f64; 4],
}

/// Same walk as `span_scan_spike.rs::collect_span_hits`, duplicated here to
/// keep this spike self-contained.
fn collect_span_hits(doc: &PagedDocument, world: &QuillWorld) -> Vec<SpanHit> {
    fn walk(frame: &Frame, ts: Transform, page_idx: usize, world: &QuillWorld, out: &mut Vec<SpanHit>) {
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
                        out.push(SpanHit {
                            page: page_idx,
                            file: glyph.span.0.id(),
                            range: world.range(glyph.span.0),
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

/// The forward, click→field query: find whichever glyph's rect contains
/// `(x, y)` on `page`, resolve its span, and return the field whose known
/// window contains that resolved range. Mirrors `typst-ide::jump_from_click`
/// — find the frame item under the click point, read its span — except the
/// last step resolves against known field windows instead of a source Jump.
fn jump_from_point(
    hits: &[SpanHit],
    page: usize,
    x: f64,
    y: f64,
    windows: &[(&str, FileId, std::ops::Range<usize>)],
) -> Option<String> {
    let hit = hits.iter().find(|h| {
        h.page == page && h.rect[0] <= x && x <= h.rect[2] && h.rect[1] <= y && y <= h.rect[3]
    })?;
    let hfile = hit.file?;
    let hrange = hit.range.as_ref()?;
    for (name, file, range) in windows {
        if hfile == *file && range.start <= hrange.start && hrange.end <= range.end {
            return Some(name.to_string());
        }
    }
    None
}

fn minimal_quill() -> Quill {
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: br#"
quill:
  name: jump_spike
  version: 0.1.0
  backend: typst
  description: jump-from-click scan spike
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
fn click_resolves_to_the_correct_field_among_several() {
    let plate = r#"
#set page(width: 400pt, height: 400pt, margin: 40pt)
#let a = eval("FIRSTFIELD text here.", mode: "markup")
#let b = eval("SECONDFIELD text here.", mode: "markup")
#a

#b
"#;
    let (doc, world) = compile(plate);
    let hits = collect_span_hits(&doc, &world);

    let a_call = plate.find("\"FIRSTFIELD text here.\"").unwrap();
    let a_end = a_call + "\"FIRSTFIELD text here.\"".len();
    let b_call = plate.find("\"SECONDFIELD text here.\"").unwrap();
    let b_end = b_call + "\"SECONDFIELD text here.\"".len();
    let windows = [
        ("field_a", world.main(), a_call..a_end),
        ("field_b", world.main(), b_call..b_end),
    ];

    // Click squarely inside each field's rendered text.
    let a_hit = hits.iter().find(|h| h.rect[0] > 0.0).unwrap();
    let click_a = ((a_hit.rect[0] + a_hit.rect[2]) / 2.0, (a_hit.rect[1] + a_hit.rect[3]) / 2.0);
    let field_at_a = jump_from_point(&hits, 0, click_a.0, click_a.1, &windows);
    eprintln!("click on first field's ink -> {field_at_a:?}");
    assert_eq!(field_at_a.as_deref(), Some("field_a"));

    let b_hit = hits.iter().rev().find(|h| h.rect[1] > a_hit.rect[3]).unwrap();
    let click_b = ((b_hit.rect[0] + b_hit.rect[2]) / 2.0, (b_hit.rect[1] + b_hit.rect[3]) / 2.0);
    let field_at_b = jump_from_point(&hits, 0, click_b.0, click_b.1, &windows);
    eprintln!("click on second field's ink -> {field_at_b:?}");
    assert_eq!(field_at_b.as_deref(), Some("field_b"));
}

#[test]
fn click_on_either_of_two_placements_resolves_to_the_same_field_no_disambiguation_needed() {
    // The actual point of this reframing: a field placed twice needs NO
    // placement-counting for click-to-navigate. Clicking on EITHER
    // occurrence must resolve to the same field — trivially true here,
    // unlike the reverse (enumerate-all-boxes) direction.
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
    let windows = [("field_a", world.main(), call..call_end)];

    let matching: Vec<&SpanHit> = hits
        .iter()
        .filter(|h| {
            h.file == Some(world.main())
                && h.range.as_ref().is_some_and(|r| call <= r.start && r.end <= call_end)
        })
        .collect();
    assert!(matching.len() >= 2, "expect glyphs from both placements: {matching:?}");

    // Take one hit from each visually-separated cluster (top vs bottom half
    // of the page) and confirm both resolve to field_a with zero
    // disambiguation logic.
    let top = matching.iter().min_by(|a, b| a.rect[1].total_cmp(&b.rect[1])).unwrap();
    let bottom = matching.iter().max_by(|a, b| a.rect[1].total_cmp(&b.rect[1])).unwrap();
    assert!(bottom.rect[1] > top.rect[3] + 20.0, "sanity: these are genuinely two separate placements");

    let click_top = ((top.rect[0] + top.rect[2]) / 2.0, (top.rect[1] + top.rect[3]) / 2.0);
    let click_bottom = ((bottom.rect[0] + bottom.rect[2]) / 2.0, (bottom.rect[1] + bottom.rect[3]) / 2.0);
    let field_top = jump_from_point(&hits, 0, click_top.0, click_top.1, &windows);
    let field_bottom = jump_from_point(&hits, 0, click_bottom.0, click_bottom.1, &windows);
    eprintln!("click on placement 1 -> {field_top:?}, click on placement 2 -> {field_bottom:?}");
    assert_eq!(field_top.as_deref(), Some("field_a"));
    assert_eq!(field_bottom.as_deref(), Some("field_a"));
}

#[test]
fn click_survives_render_body_rebuild_on_the_real_package() {
    // The other half of the hypothesis: does the forward direction still
    // work after render-body's full state-buffer capture/renumber/replay?
    // Reuses span_scan_spike.rs's already-validated survival finding, now
    // through an actual point click rather than a byte-range window union.
    let plate = r#"
#import "@local/tonguetoquill-usaf-memo:3.0.0": frontmatter, mainmatter

#show: frontmatter.with(subject: "Spike Memo", memo_for: ("TEST/SYMB",));

#mainmatter[FIRSTFIELD one two three.

SECONDFIELD four five six.]
"#;
    let quill = Quill::from_tree(host_tree()).expect("load usaf_memo host quill");
    let world = QuillWorld::new(&quill, plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    let hits = collect_span_hits(&doc, &world);

    let a_call = plate.find("FIRSTFIELD one two three.").unwrap();
    let a_end = a_call + "FIRSTFIELD one two three.".len();
    let b_call = plate.find("SECONDFIELD four five six.").unwrap();
    let b_end = b_call + "SECONDFIELD four five six.".len();
    let windows = [
        ("body_text", world.main(), a_call..a_end),
        ("body_text", world.main(), b_call..b_end),
    ];

    // render-body prepends an AFH auto-number prefix ("1.", "2.") — glyphs
    // from body.typ's own `numbering()` call, a DIFFERENT source than our
    // field. Pick click points from hits we independently know (via their
    // own resolved span) belong to the field, rather than blindly clicking
    // "the first line" (which is very likely that number prefix, not field
    // text) — this isolates "does clicking on real field ink work" from
    // "did we click on the right pixel."
    let is_field_hit = |h: &&SpanHit| {
        h.file == Some(world.main())
            && h.range.as_ref().is_some_and(|r| {
                (a_call <= r.start && r.end <= a_end) || (b_call <= r.start && r.end <= b_end)
            })
    };
    let field_hits: Vec<&SpanHit> = hits.iter().filter(is_field_hit).collect();
    eprintln!("field-content hits found: {}", field_hits.len());
    assert!(
        !field_hits.is_empty(),
        "expected at least some glyphs whose span resolves to the field, post-rebuild"
    );

    for h in &field_hits {
        let cx = (h.rect[0] + h.rect[2]) / 2.0;
        let cy = (h.rect[1] + h.rect[3]) / 2.0;
        let field = jump_from_point(&hits, 0, cx, cy, &windows);
        eprintln!("click at ({cx:.1},{cy:.1}) on known field ink -> {field:?}");
        assert_eq!(
            field.as_deref(),
            Some("body_text"),
            "clicking on ink whose own span resolves to the field must round-trip through jump_from_point too"
        );
    }
}
