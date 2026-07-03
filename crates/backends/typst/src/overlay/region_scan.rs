//! Recover schema-field regions for *marker-tagged content* — auto-tagged
//! content fields and explicit `tagged(..)` placements — by walking the
//! compiled frame tree, not by reading a fixed-size widget box.
//!
//! Where [`extract`](super::extract) handles `form-field` widgets — each a
//! single fixed-size box whose rect is `marker_position + (width, height)` — a
//! tagged body has no declared size and may wrap across many lines and break
//! across pages. Its geometry can only be read from the laid-out frames.
//!
//! The helper (`lib.typ`'s `_qm-tag`, reached via auto-tagging or the public
//! `tagged`) brackets each placement between two zero-size `<__qm_region__>`
//! metadata markers (`role: start` / `role: end`, carrying the schema `field`
//! path). The walk, mirroring `typst_layout::introspect::discover_frame` (the
//! same transform composition Typst uses to place elements):
//!   1. Query the markers → `location → (role, field)`.
//!   2. Walk every page frame in document order, composing the group-transform
//!      stack. A `start` marker opens a **placement** of its field; the
//!      matching `end` marker closes it. Every drawn leaf (text/shape/image)
//!      seen while a placement is open contributes its transformed bbox to
//!      that placement's running union, partitioned by page.
//!   3. Flip each page-space (top-left) union to the PDF bottom-left origin
//!      the region model uses.
//!
//! **One region per (placement, page).** A field placed twice yields two
//! independent regions — a spanning union would claim the ink of whatever sits
//! between the placements. A placement that crosses a page boundary yields one
//! region per page it touches, in page order (a placement opened on one page
//! stays open until its `end` marker, which may live on a later page) — every
//! fragment surfaces, so a consumer highlighting a field covers its
//! continuation pages too. Consumers group by `field`.
//!
//! Nested markers for the *same* field collapse into the outer placement
//! (depth-counted): a plate that wraps an already-auto-tagged verbatim body in
//! an explicit `tagged(..)` is double-tagging one placement, not placing the
//! field twice, and gets one region for it.

use std::collections::HashMap;

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

/// Per-field open state: the placement instance leaves currently accrue to,
/// and the marker nesting depth that keeps same-field markers collapsed into
/// it.
struct OpenField {
    instance: usize,
    depth: usize,
}

/// Walk state threaded through the frame recursion.
struct Scan<'a> {
    /// Marker location hash → (role, field), from the introspector query.
    by_loc: &'a HashMap<u128, (Role, String)>,
    /// field → its open placement. Persists across pages: a placement opened
    /// on one page stays open until its `end` marker, which may live on a
    /// later page — resetting per page would drop the continuation fragments.
    open: HashMap<String, OpenField>,
    /// instance id → field. Allocation order is document order.
    instances: Vec<String>,
    /// (instance, page) → union box, in page-space top-left pt.
    boxes: HashMap<(usize, usize), Aabb>,
}

/// Walk the compiled document and return one [`RenderedRegion`] per
/// (placement of a tagged field, page it occupies). Best-effort: a malformed
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
    // walk can recognise a marker without re-reading the metadata dict.
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

    let mut scan = Scan {
        by_loc: &by_loc,
        open: HashMap::new(),
        instances: Vec::new(),
        boxes: HashMap::new(),
    };
    for (page_idx, page) in doc.pages().iter().enumerate() {
        walk(&page.frame, Transform::identity(), page_idx, &mut scan);
    }

    // Flip each page-space box to PDF bottom-left and emit. Sort for stable
    // output: page order, then field, then placement document order (two
    // same-field placements on one page keep the order they were opened in).
    let mut out: Vec<(RenderedRegion, usize)> = scan
        .boxes
        .into_iter()
        .filter(|(_, b)| !b.is_empty())
        .filter_map(|((instance, page), b)| {
            let page_h = doc.pages().get(page)?.frame.size().y.to_pt();
            Some((
                RenderedRegion {
                    field: scan.instances[instance].clone(),
                    page,
                    rect: [
                        b.min_x as f32,
                        (page_h - b.max_y) as f32,
                        b.max_x as f32,
                        (page_h - b.min_y) as f32,
                    ],
                },
                instance,
            ))
        })
        .collect();
    out.sort_by(|(a, ai), (b, bi)| (a.page, &a.field, *ai).cmp(&(b.page, &b.field, *bi)));
    out.into_iter().map(|(r, _)| r).collect()
}

/// Recursive frame walk. `ts` maps this frame's local coordinates to page space;
/// composed exactly as `discover_frame` does (translate by item pos, then the
/// group's own transform).
fn walk(frame: &Frame, ts: Transform, page_idx: usize, scan: &mut Scan) {
    for (pos, item) in frame.items() {
        match item {
            // Each `metadata` element contributes a `Tag::Start` and a
            // `Tag::End` frame item with the *same* location. Reacting to
            // `Tag::Start` only fires exactly once per marker, in document
            // order — required now that a `role: start` marker allocates a
            // placement instance (a double fire would open two).
            FrameItem::Tag(tag @ Tag::Start(..)) => {
                if let Some((role, field)) = scan.by_loc.get(&tag.location().hash()) {
                    match role {
                        Role::Start => {
                            let next_instance = scan.instances.len();
                            let entry =
                                scan.open
                                    .entry(field.clone())
                                    .or_insert_with(|| OpenField {
                                        instance: next_instance,
                                        depth: 0,
                                    });
                            if entry.depth == 0 {
                                scan.instances.push(field.clone());
                            }
                            entry.depth += 1;
                        }
                        Role::End => {
                            if let Some(entry) = scan.open.get_mut(field) {
                                entry.depth -= 1;
                                if entry.depth == 0 {
                                    scan.open.remove(field);
                                }
                            }
                        }
                    }
                }
            }
            FrameItem::Tag(Tag::End(..)) => {}
            FrameItem::Group(group) => {
                let ts = ts
                    .pre_concat(Transform::translate(pos.x, pos.y))
                    .pre_concat(group.transform);
                walk(&group.frame, ts, page_idx, scan);
            }
            FrameItem::Text(text) if !scan.open.is_empty() => {
                let bb = text.bbox();
                add_rect(scan, page_idx, *pos, bb.min, bb.max, ts);
            }
            FrameItem::Shape(shape, _) if !scan.open.is_empty() => {
                let bb = shape.geometry.bbox(shape.stroke.as_ref());
                add_rect(scan, page_idx, *pos, bb.min, bb.max, ts);
            }
            FrameItem::Image(_, size, _) if !scan.open.is_empty() => {
                add_rect(scan, page_idx, *pos, Point::zero(), size.to_point(), ts);
            }
            _ => {}
        }
    }
}

/// Union a leaf item's box (corners `lo`..`hi`, relative to the item anchor
/// `pos`, in this frame's local space) into every currently-open placement,
/// after mapping to page space via `ts`.
fn add_rect(scan: &mut Scan, page_idx: usize, pos: Point, lo: Point, hi: Point, ts: Transform) {
    // Transform all four corners (ts may rotate/scale), take the AABB.
    let corners = [
        Point::new(pos.x + lo.x, pos.y + lo.y),
        Point::new(pos.x + hi.x, pos.y + lo.y),
        Point::new(pos.x + lo.x, pos.y + hi.y),
        Point::new(pos.x + hi.x, pos.y + hi.y),
    ];
    for open in scan.open.values() {
        let entry = scan
            .boxes
            .entry((open.instance, page_idx))
            .or_insert_with(Aabb::empty);
        for c in corners {
            entry.add(c.transform(ts));
        }
    }
}
