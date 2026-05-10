//! SPIKE — not for merge. Probes whether Typst's introspector exposes
//! enough geometry for a labelled `box` to recover both position and size.
//!
//! Run with: `cargo test -p quillmark-typst --lib spike_a -- --nocapture`

#![cfg(test)]
#![allow(unused_imports)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use quillmark_core::{FileTreeNode, QuillSource};
use typst::foundations::{Label, Selector};
use typst::introspection::Location;
use typst::layout::PagedDocument;
use typst::utils::PicoStr;
use typst::Document;

use crate::world::QuillWorld;

fn load_fixture() -> QuillSource {
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
        .join("0.1.0");

    let tree = walk(&quill_path).expect("walk fixture");
    QuillSource::from_tree(tree).expect("load source")
}

fn compile(main: &str) -> PagedDocument {
    let world = QuillWorld::new(&load_fixture(), main).expect("world");
    typst::compile::<PagedDocument>(&world)
        .output
        .expect("compile ok")
}

fn label(name: &str) -> Selector {
    let l = Label::new(PicoStr::intern(name)).expect("non-empty label");
    Selector::Label(l)
}

/// Probe 1: query a labelled bracketed scope around a box.
#[test]
fn spike_a1_labelled_scope() {
    let main = r#"
#set page(width: 600pt, height: 400pt, margin: 50pt)

Some leading text.

#[
  #box(width: 200pt, height: 50pt, stroke: 0.5pt + gray)[ ]
] <qm-sig>

Trailing text.
"#;

    let doc = compile(main);
    let intro = doc.introspector();
    let elems = intro.query(&label("qm-sig"));

    println!("\n--- spike_a1_labelled_scope ---");
    println!("query returned {} element(s)", elems.len());
    for (i, c) in elems.iter().enumerate() {
        println!("[{i}] elem = {}", c.func().name());
        if let Some(loc) = c.location() {
            let pos = intro.position(loc);
            println!(
                "    position = page {:?}, ({:.2}pt, {:.2}pt)",
                pos.page,
                pos.point.x.to_pt(),
                pos.point.y.to_pt()
            );
        } else {
            println!("    no location");
        }
    }
}

/// Probe 2: label attached directly to `#box(...)` in markup.
#[test]
fn spike_a2_box_inline_label() {
    let main = r#"
#set page(width: 600pt, height: 400pt, margin: 50pt)

Leading.
#box(width: 200pt, height: 50pt, stroke: 0.5pt + gray)[ ] <qm-sig>
Trailing.
"#;

    let doc = compile(main);
    let intro = doc.introspector();
    let elems = intro.query(&label("qm-sig"));

    println!("\n--- spike_a2_box_inline_label ---");
    println!("query returned {} element(s)", elems.len());
    for (i, c) in elems.iter().enumerate() {
        println!("[{i}] elem = {}", c.func().name());
        if let Some(loc) = c.location() {
            let pos = intro.position(loc);
            println!(
                "    position = page {:?}, ({:.2}pt, {:.2}pt)",
                pos.page,
                pos.point.x.to_pt(),
                pos.point.y.to_pt()
            );
        }
    }
}

/// Probe 3: walk the page frame trees looking for Tag(loc) entries that
/// match the labelled element's Location, AND for the surrounding
/// non-Tag frame items (Group, Shape) that follow — those are the box's
/// actual rendered geometry.
#[test]
fn spike_a3_walk_frames_for_size() {
    use typst::layout::{Frame, FrameItem, Point};

    let main = r#"
#set page(width: 600pt, height: 400pt, margin: 50pt)
Leading.
#box(width: 200pt, height: 50pt, stroke: 0.5pt + gray)[ ] <qm-sig>
Trailing.
"#;

    let doc = compile(main);
    let intro = doc.introspector();
    let elems = intro.query(&label("qm-sig"));

    let target_loc = elems
        .iter()
        .next()
        .and_then(|c| c.location())
        .expect("at least one labelled element with a location");

    println!("\n--- spike_a3_walk_frames_for_size ---");
    println!("target location = {:?}", target_loc);

    // Walk: report every Tag whose location matches, with absolute coords,
    // AND the immediately-following item at the same position (which is
    // typically the Group/Shape carrying the actual geometry).
    fn walk(
        frame: &Frame,
        offset: Point,
        target: Location,
        depth: usize,
    ) -> Vec<(Point, &'static str)> {
        let mut hits = Vec::new();
        let items: Vec<_> = frame.items().collect();
        for (i, (pos, item)) in items.iter().enumerate() {
            let abs = Point::new(offset.x + pos.x, offset.y + pos.y);
            match item {
                FrameItem::Group(group) => {
                    hits.extend(walk(&group.frame, abs, target, depth + 1));
                }
                FrameItem::Tag(tag) => {
                    if tag.location() == target {
                        let label = match tag {
                            typst::introspection::Tag::Start(..) => "Start",
                            typst::introspection::Tag::End(..) => "End",
                        };
                        println!(
                            "{}TAG/{} at ({:.2}, {:.2})",
                            "  ".repeat(depth),
                            label,
                            abs.x.to_pt(),
                            abs.y.to_pt(),
                        );
                        hits.push((abs, label));

                        // Look at the next item — if it's a Group, that's
                        // probably the box's frame.
                        if let Some((next_pos, next_item)) = items.get(i + 1) {
                            let next_abs =
                                Point::new(offset.x + next_pos.x, offset.y + next_pos.y);
                            match next_item {
                                FrameItem::Group(g) => {
                                    println!(
                                        "{}  ↳ next: Group at ({:.2},{:.2}) size ({:.2}x{:.2})",
                                        "  ".repeat(depth),
                                        next_abs.x.to_pt(),
                                        next_abs.y.to_pt(),
                                        g.frame.size().x.to_pt(),
                                        g.frame.size().y.to_pt(),
                                    );
                                }
                                FrameItem::Shape(_, _) => {
                                    println!(
                                        "{}  ↳ next: Shape at ({:.2},{:.2})",
                                        "  ".repeat(depth),
                                        next_abs.x.to_pt(),
                                        next_abs.y.to_pt(),
                                    );
                                }
                                other => {
                                    println!(
                                        "{}  ↳ next: {:?}",
                                        "  ".repeat(depth),
                                        std::mem::discriminant(other)
                                    );
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        hits
    }

    for (page_idx, page) in doc.pages.iter().enumerate() {
        println!(
            "page {page_idx} size = ({:.2}, {:.2})",
            page.frame.size().x.to_pt(),
            page.frame.size().y.to_pt()
        );
        let hits = walk(&page.frame, Point::zero(), target_loc, 0);
        println!("  → {} matching tag(s) on page {page_idx}", hits.len());
    }

    let pos = intro.position(target_loc);
    println!(
        "\nintrospector.position = page {:?}, ({:.2}, {:.2})",
        pos.page,
        pos.point.x.to_pt(),
        pos.point.y.to_pt()
    );
}
