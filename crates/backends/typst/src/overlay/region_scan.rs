//! Recover schema-field regions for *auto-tagged content* by walking the
//! compiled frame tree, not by reading a fixed-size widget box.
//!
//! Where [`extract`](super::extract) handles `form-field` widgets — each a
//! single fixed-size box whose rect is `marker_position + (width, height)` — a
//! content field (a markdown body) has no declared size and may wrap across
//! many lines and break across pages. Its geometry can only be read from the
//! laid-out frames.
//!
//! The helper (`lib.typ`'s `qm-region`) brackets each auto-tagged content field
//! between two zero-size `<__qm_region__>` metadata markers (`role: start` /
//! `role: end`, carrying the schema `field` path). The walk, mirroring
//! `typst_layout::introspect::discover_frame` (the same transform composition
//! Typst uses to place elements):
//!   1. Query the markers → `location → (role, field)`.
//!   2. Walk every page frame in document order, composing the group-transform
//!      stack. A `start` tag opens a field; an `end` tag closes it. Every drawn
//!      leaf (text/shape/image) seen while a field is open contributes its
//!      transformed bbox to that field's running union, partitioned by page.
//!   3. Flip each page-space (top-left) union to the PDF bottom-left origin the
//!      region model uses.
//!
//! A field that crosses a page boundary yields one box per page it touches, in
//! page order (the open-field set persists across pages: a field opened on one
//! page stays open until its `end` marker, which may live on a later page). The
//! session keeps only the first — the field's lowest-page anchor — so callers
//! see one region per logical field; the per-page boxes are produced here so the
//! anchor is the page where the field actually starts.

use std::collections::{HashMap, HashSet};

use typst::foundations::{Label, Selector, Value};
use typst::introspection::{Introspector, Tag};
use typst::layout::{Frame, FrameItem, Point, Transform};
use typst::utils::PicoStr;
use typst_layout::PagedDocument;

use quillmark_core::RenderedRegion;

const REGION_LABEL: &str = "__qm_region__";

/// An axis-aligned bounding box accumulated in page-space (top-left origin) pt.
#[derive(Clone, Copy)]
struct Aabb {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Aabb {
    fn empty() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        }
    }

    fn add(&mut self, p: Point) {
        let (x, y) = (p.x.to_pt(), p.y.to_pt());
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    fn is_empty(&self) -> bool {
        self.min_x > self.max_x || self.min_y > self.max_y
    }
}

/// Marker role read from the metadata dict.
#[derive(Clone, Copy, PartialEq)]
enum Role {
    Start,
    End,
}

/// Walk the compiled document and return one [`RenderedRegion`] per
/// (auto-tagged content field, page it occupies). Best-effort: a malformed
/// marker is skipped rather than failing the whole scan, since this rides
/// alongside the (already-validated) form-field path and a render would surface
/// any real compilation error first.
pub(crate) fn scan(doc: &PagedDocument) -> Vec<RenderedRegion> {
    let intro = doc.introspector();
    let Some(label) = Label::new(PicoStr::intern(REGION_LABEL)) else {
        return Vec::new();
    };
    let markers = intro.query(&Selector::Label(label));
    if markers.is_empty() {
        return Vec::new();
    }

    // location id → (role, field). Keyed by the marker's location so the frame
    // walk can recognise a tag without re-reading the metadata dict.
    let mut by_loc: HashMap<u128, (Role, String)> = HashMap::new();
    for c in markers.iter() {
        let Ok(Value::Dict(dict)) = c.get_by_name("value") else {
            continue;
        };
        let role = match dict.get("role") {
            Ok(Value::Str(s)) if s.as_str() == "start" => Role::Start,
            Ok(Value::Str(s)) if s.as_str() == "end" => Role::End,
            _ => continue,
        };
        let Ok(Value::Str(field)) = dict.get("field") else {
            continue;
        };
        if let Some(loc) = c.location() {
            by_loc.insert(loc.hash(), (role, field.to_string()));
        }
    }

    // (field, page) → union box, in page-space top-left pt.
    let mut boxes: HashMap<(String, usize), Aabb> = HashMap::new();
    // `active` persists across pages: a field opened on one page stays open
    // until its `end` marker, which may live on a later page. Resetting per
    // page would drop the continuation fragments.
    let mut active: HashSet<String> = HashSet::new();
    for (page_idx, page) in doc.pages().iter().enumerate() {
        walk(
            &page.frame,
            Transform::identity(),
            page_idx,
            &by_loc,
            &mut active,
            &mut boxes,
        );
    }

    // Flip each page-space box to PDF bottom-left and emit. Sort for stable output.
    let mut out: Vec<RenderedRegion> = boxes
        .into_iter()
        .filter(|(_, b)| !b.is_empty())
        .filter_map(|((field, page), b)| {
            let page_h = doc.pages().get(page)?.frame.size().y.to_pt();
            Some(RenderedRegion {
                field,
                page,
                rect: [
                    b.min_x as f32,
                    (page_h - b.max_y) as f32,
                    b.max_x as f32,
                    (page_h - b.min_y) as f32,
                ],
            })
        })
        .collect();
    out.sort_by(|a, b| (a.page, &a.field).cmp(&(b.page, &b.field)));
    out
}

/// Recursive frame walk. `ts` maps this frame's local coordinates to page space;
/// composed exactly as `discover_frame` does (translate by item pos, then the
/// group's own transform).
fn walk(
    frame: &Frame,
    ts: Transform,
    page_idx: usize,
    by_loc: &HashMap<u128, (Role, String)>,
    active: &mut HashSet<String>,
    boxes: &mut HashMap<(String, usize), Aabb>,
) {
    for (pos, item) in frame.items() {
        match item {
            // Each `metadata` element emits both a `Tag::Start` and a `Tag::End`
            // with the *same* location hash, so every marker fires twice here.
            // `insert`/`remove` are idempotent and the body is zero-size (the two
            // tags are adjacent), so the double-fire is a no-op.
            FrameItem::Tag(tag) => {
                if let Some((role, field)) = by_loc.get(&tag_loc(tag)) {
                    match role {
                        Role::Start => {
                            active.insert(field.clone());
                        }
                        Role::End => {
                            active.remove(field);
                        }
                    }
                }
            }
            FrameItem::Group(group) => {
                let ts = ts
                    .pre_concat(Transform::translate(pos.x, pos.y))
                    .pre_concat(group.transform);
                walk(&group.frame, ts, page_idx, by_loc, active, boxes);
            }
            FrameItem::Text(text) if !active.is_empty() => {
                let bb = text.bbox();
                add_rect(active, page_idx, boxes, *pos, bb.min, bb.max, ts);
            }
            FrameItem::Shape(shape, _) if !active.is_empty() => {
                let bb = shape.geometry.bbox(shape.stroke.as_ref());
                add_rect(active, page_idx, boxes, *pos, bb.min, bb.max, ts);
            }
            FrameItem::Image(_, size, _) if !active.is_empty() => {
                add_rect(
                    active,
                    page_idx,
                    boxes,
                    *pos,
                    Point::zero(),
                    size.to_point(),
                    ts,
                );
            }
            _ => {}
        }
    }
}

/// Union a leaf item's box (corners `lo`..`hi`, relative to the item anchor
/// `pos`, in this frame's local space) into every currently-open field, after
/// mapping to page space via `ts`.
fn add_rect(
    active: &HashSet<String>,
    page_idx: usize,
    boxes: &mut HashMap<(String, usize), Aabb>,
    pos: Point,
    lo: Point,
    hi: Point,
    ts: Transform,
) {
    // Transform all four corners (ts may rotate/scale), take the AABB.
    let corners = [
        Point::new(pos.x + lo.x, pos.y + lo.y),
        Point::new(pos.x + hi.x, pos.y + lo.y),
        Point::new(pos.x + lo.x, pos.y + hi.y),
        Point::new(pos.x + hi.x, pos.y + hi.y),
    ];
    for field in active.iter() {
        let entry = boxes
            .entry((field.clone(), page_idx))
            .or_insert_with(Aabb::empty);
        for c in corners {
            entry.add(c.transform(ts));
        }
    }
}

fn tag_loc(tag: &Tag) -> u128 {
    tag.location().hash()
}
