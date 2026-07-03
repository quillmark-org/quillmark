//! Recover schema-field regions from *glyph spans* — the origin every drawn
//! frame item already carries.
//!
//! Every `Text` glyph (and `Shape`/`Image` item) in the laid-out frames
//! carries a [`Span`] pointing at the source expression that produced it.
//! Content fields are codegen'd as markup **block** bindings (`#let _qm_cN =
//! [ .. ]`) in the generated helper `lib.typ`; the file parser parses each
//! block, so every glyph carries its own syntax node's span (word/run
//! granularity) — all nested inside the block's byte range. The backend
//! records each block's **byte window** at generation time ([`FieldWindow`])
//! and the scan classifies a frame item by which window its resolved range
//! nests inside; per-node spans and a single uniform span both fall inside the
//! same window, so classification is by containment, not identity. A scalar the
//! plate interpolates directly (`#data.subject`) needs no codegen: its glyphs
//! carry a span at or around the reference expression in the plate, and
//! [`scalar_windows`] recovers those windows from the plate's syntax tree.
//! Spans survive *any* content rebuild (a `show`-rule pass that captures
//! paragraphs into a state buffer and re-emits them) because they are a
//! property of the glyph, not a sibling element a rebuild can drop.
//!
//! **Resolution goes through the compile's own helper source.** The session
//! serves reads from its last-good compile even after a failed `apply`, but a
//! failed apply has already written the *next* injection's helper text into
//! the world — resolving the served document's spans against that text would
//! shift or drop every byte range. The scan therefore resolves helper-file
//! spans against the [`Source`] snapshot the served document was compiled
//! from, and only non-helper spans (the plate, vendored packages — sources
//! that never change within a session) through the live world.
//!
//! **First placement only.** A window's region is its first maximal run of
//! consecutive matching frame items in document order — one region per page
//! that run touches, in page order. Span data cannot distinguish "package
//! chrome between two paragraphs of one placement" from "a second placement
//! of the same value" (both are a gap of foreign spans), so later runs are
//! not enumerated; the first run is the true start of the field's content,
//! and shrinks (never lies) when foreign ink interrupts it mid-page. One
//! tolerance keeps continuation pages covered: page marginals (headers,
//! footers, page numbers) walk between one page's body and the next's, so a
//! run interrupted by foreign ink may resume on the **immediately following
//! page** — a same-page gap still ends the run (that is exactly the
//! twice-placed case), at the cost that a *second* placement opening at the
//! top of the next page reads as a continuation (an over-report of that
//! field's own ink, never another field's). A scalar referenced at several
//! distinct plate sites costs nothing: each site is its own window, so each
//! surfaces independently.
//!
//! Geometry composes the group-transform stack exactly like
//! `typst_layout::introspect::discover_frame`, transforming all four corners
//! of each item box (the stack may rotate or scale). Boxes are computed only
//! for classified ink — foreign items matter to the scan solely as
//! run-breakers.

use std::collections::HashMap;
use std::ops::Range;

use typst::layout::{Frame, FrameItem, Point, Transform};
use typst::syntax::ast::{self, AstNode};
use typst::syntax::{DiagSpan, DiagSpanKind, FileId, LinkedNode, Source, Span, SyntaxKind};
use typst::World;
use typst_layout::PagedDocument;

use quillmark_core::RenderedRegion;

use crate::world::QuillWorld;

/// A tracked byte window in a compiled source: the schema field whose content
/// resolves into `range` of `file`. Content fields point at their generated
/// markup block (`#let _qm_cN = [ .. ]`) in the helper `lib.typ`; scalar
/// reference sites point at their expression in the plate.
#[derive(Debug, Clone)]
pub(crate) struct FieldWindow {
    pub path: String,
    pub file: FileId,
    pub range: Range<usize>,
}

/// An axis-aligned box accumulated in page-space (top-left origin) pt.
#[derive(Clone, Copy)]
struct Aabb {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Aabb {
    fn of(corners: [Point; 4], ts: Transform) -> Self {
        let mut b = Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        };
        for c in corners {
            let p = c.transform(ts);
            let (x, y) = (p.x.to_pt(), p.y.to_pt());
            b.min_x = b.min_x.min(x);
            b.min_y = b.min_y.min(y);
            b.max_x = b.max_x.max(x);
            b.max_y = b.max_y.max(y);
        }
        b
    }

    fn union(&mut self, o: Aabb) {
        self.min_x = self.min_x.min(o.min_x);
        self.min_y = self.min_y.min(o.min_y);
        self.max_x = self.max_x.max(o.max_x);
        self.max_y = self.max_y.max(o.max_y);
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        self.min_x <= x && x <= self.max_x && self.min_y <= y && y <= self.max_y
    }
}

/// One drawn frame item, classified: which tracked window (if any) its span
/// resolved into, and — for classified ink only — its page-space box.
struct Hit {
    page: usize,
    window: Option<usize>,
    rect: Option<Aabb>,
}

/// Memoizing span → window-index classifier. A block's glyphs carry a handful
/// of distinct per-node spans (not one uniform span), so the range lookup runs
/// once per distinct span, not once per glyph. Helper-file spans resolve
/// against the served compile's own source snapshot (see the module doc);
/// everything else against the world.
struct Classifier<'a> {
    world: &'a QuillWorld,
    helper: &'a Source,
    windows: &'a [FieldWindow],
    memo: HashMap<Span, Option<usize>>,
}

impl Classifier<'_> {
    fn classify(&mut self, span: Span) -> Option<usize> {
        if let Some(&w) = self.memo.get(&span) {
            return w;
        }
        // The same unpack `WorldExt::range` performs, with the helper file
        // routed to the served compile's snapshot instead of the world.
        let resolved = match DiagSpan::from(span).get() {
            DiagSpanKind::Detached => None,
            DiagSpanKind::Number {
                id,
                num,
                sub_range,
            } => {
                let range = if id == self.helper.id() {
                    self.helper.range(num, sub_range)
                } else {
                    self.world.source(id).ok().and_then(|s| s.range(num, sub_range))
                };
                range.map(|r| (id, r))
            }
            DiagSpanKind::Range { id, range } => Some((id, range)),
        };
        let w = resolved.and_then(|(file, range)| {
            self.windows.iter().position(|win| {
                win.file == file && win.range.start <= range.start && range.end <= win.range.end
            })
        });
        self.memo.insert(span, w);
        w
    }
}

/// Walk one page frame in document order, emitting one [`Hit`] per drawn item
/// — per glyph for text (a text run may mix spans), per item for shapes and
/// images (each carries a single span). Boxes are computed for classified
/// ink only.
fn collect_page_hits(
    frame: &Frame,
    page: usize,
    cls: &mut Classifier,
    out: &mut Vec<Hit>,
) {
    fn walk(frame: &Frame, ts: Transform, page: usize, cls: &mut Classifier, out: &mut Vec<Hit>) {
        for (pos, item) in frame.items() {
            match item {
                FrameItem::Group(group) => {
                    let ts = ts
                        .pre_concat(Transform::translate(pos.x, pos.y))
                        .pre_concat(group.transform);
                    walk(&group.frame, ts, page, cls, out);
                }
                FrameItem::Text(text) => {
                    let bb = text.bbox();
                    let mut cursor = Point::zero();
                    for glyph in &text.glyphs {
                        let advance = Point::new(
                            glyph.x_advance.at(text.size),
                            glyph.y_advance.at(text.size),
                        );
                        let window = cls.classify(glyph.span.0);
                        let rect = window.is_some().then(|| {
                            let offset = Point::new(
                                glyph.x_offset.at(text.size),
                                glyph.y_offset.at(text.size),
                            );
                            let lo = Point::new(cursor.x + offset.x, cursor.y + bb.min.y);
                            let hi =
                                Point::new(cursor.x + offset.x + advance.x, cursor.y + bb.max.y);
                            item_aabb(*pos, lo, hi, ts)
                        });
                        out.push(Hit { page, window, rect });
                        cursor += advance;
                    }
                }
                FrameItem::Shape(shape, span) => {
                    let window = cls.classify(*span);
                    let rect = window.is_some().then(|| {
                        let bb = shape.geometry.bbox(shape.stroke.as_ref());
                        item_aabb(*pos, bb.min, bb.max, ts)
                    });
                    out.push(Hit { page, window, rect });
                }
                FrameItem::Image(_, size, span) => {
                    let window = cls.classify(*span);
                    let rect = window
                        .is_some()
                        .then(|| item_aabb(*pos, Point::zero(), size.to_point(), ts));
                    out.push(Hit { page, window, rect });
                }
                _ => {}
            }
        }
    }
    walk(frame, Transform::identity(), page, cls, out);
}

/// An item box (corners `lo`..`hi` relative to the item anchor `pos`, in local
/// frame space) mapped to page space via `ts`. All four corners transform —
/// `ts` may rotate or scale.
fn item_aabb(pos: Point, lo: Point, hi: Point, ts: Transform) -> Aabb {
    Aabb::of(
        [
            Point::new(pos.x + lo.x, pos.y + lo.y),
            Point::new(pos.x + hi.x, pos.y + lo.y),
            Point::new(pos.x + lo.x, pos.y + hi.y),
            Point::new(pos.x + hi.x, pos.y + hi.y),
        ],
        ts,
    )
}

/// Per-window first-run state. The run currently accruing is not represented
/// here — at most one window can be in-run at a time (any hit forecloses
/// every other window's run), so the scan tracks it as a single cursor and
/// this enum carries only the out-of-run states.
#[derive(Clone, Copy, PartialEq)]
enum Run {
    NotSeen,
    /// Interrupted by foreign ink; may resume on page `last_page + 1` only.
    Suspended { last_page: usize },
    Done,
}

/// Scan the compiled document and return each window's **first placement** —
/// one [`RenderedRegion`] per page the placement's run touches, PDF
/// bottom-left rects, sorted (page, field, window order). Best-effort like the
/// widget path: an unresolvable span simply matches no window.
pub(crate) fn scan(
    doc: &PagedDocument,
    world: &QuillWorld,
    helper: &Source,
    windows: &[FieldWindow],
) -> Vec<RenderedRegion> {
    if windows.is_empty() {
        return Vec::new();
    }
    let mut cls = Classifier {
        world,
        helper,
        windows,
        memo: HashMap::new(),
    };

    // Single pass in document order: `current` is the one window whose first
    // run is accruing. A hit for another window (or untracked ink) suspends
    // it; a suspended run resumes only on the immediately following page
    // (page-marginal tolerance — see the module doc), otherwise it is done.
    let mut state = vec![Run::NotSeen; windows.len()];
    let mut boxes: Vec<Vec<(usize, Aabb)>> = vec![Vec::new(); windows.len()];
    let mut current: Option<(usize, usize)> = None; // (window, last_page)

    let mut hits = Vec::new();
    for (page, p) in doc.pages().iter().enumerate() {
        collect_page_hits(&p.frame, page, &mut cls, &mut hits);
    }
    for hit in &hits {
        match hit.window {
            Some(i) if current.map(|(c, _)| c) == Some(i) => {
                accrue(&mut boxes[i], hit);
                current = Some((i, hit.page));
            }
            Some(i) => {
                if let Some((c, last_page)) = current.take() {
                    state[c] = Run::Suspended { last_page };
                }
                match state[i] {
                    Run::NotSeen => {
                        accrue(&mut boxes[i], hit);
                        current = Some((i, hit.page));
                    }
                    Run::Suspended { last_page } if hit.page == last_page + 1 => {
                        accrue(&mut boxes[i], hit);
                        current = Some((i, hit.page));
                    }
                    Run::Suspended { .. } => state[i] = Run::Done,
                    Run::Done => {}
                }
            }
            None => {
                if let Some((c, last_page)) = current.take() {
                    state[c] = Run::Suspended { last_page };
                }
            }
        }
    }

    let mut out: Vec<(RenderedRegion, usize)> = Vec::new();
    for (i, window) in windows.iter().enumerate() {
        for (page, b) in &boxes[i] {
            let Some(page_h) = doc.pages().get(*page).map(|p| p.frame.size().y.to_pt()) else {
                continue;
            };
            out.push((
                RenderedRegion {
                    field: window.path.clone(),
                    page: *page,
                    rect: [
                        b.min_x as f32,
                        (page_h - b.max_y) as f32,
                        b.max_x as f32,
                        (page_h - b.min_y) as f32,
                    ],
                },
                i,
            ));
        }
    }
    out.sort_by(|(a, ai), (b, bi)| (a.page, &a.field, *ai).cmp(&(b.page, &b.field, *bi)));
    out.into_iter().map(|(r, _)| r).collect()
}

/// Union `hit` into the run's box for its page, opening a new per-page box at
/// a page transition (pages are nondecreasing in walk order).
fn accrue(boxes: &mut Vec<(usize, Aabb)>, hit: &Hit) {
    let rect = hit.rect.expect("classified hits carry a box");
    match boxes.last_mut() {
        Some((page, b)) if *page == hit.page => b.union(rect),
        _ => boxes.push((hit.page, rect)),
    }
}

/// The schema field under a point — the forward (click → field) direction.
/// `x`/`y` are PDF points with a **bottom-left** origin, the same convention
/// as [`RenderedRegion::rect`]. Unlike [`scan`], every placement answers, not
/// just the first: a concrete point identifies one frame item, whose span is
/// unambiguous however many times its field is placed. Among tracked ink the
/// later-painted item wins; untracked ink never occludes — a decorative
/// overlay does not swallow clicks on the field beneath it.
pub(crate) fn field_at(
    doc: &PagedDocument,
    world: &QuillWorld,
    helper: &Source,
    windows: &[FieldWindow],
    page: usize,
    x: f32,
    y: f32,
) -> Option<String> {
    if windows.is_empty() {
        return None;
    }
    let frame = &doc.pages().get(page)?.frame;
    let page_h = frame.size().y.to_pt();
    let (x, y) = (x as f64, page_h - y as f64);

    let mut cls = Classifier {
        world,
        helper,
        windows,
        memo: HashMap::new(),
    };
    let mut hits = Vec::new();
    collect_page_hits(frame, page, &mut cls, &mut hits);

    hits.iter()
        .rev()
        .find(|h| h.rect.is_some_and(|r| r.contains(x, y)))
        .and_then(|h| h.window)
        .map(|w| windows[w].path.clone())
}

/// Byte windows for the plate's direct scalar references. Two windows per
/// reference site where they differ:
///
/// - the **chain** window — the `data.<field>` / `data.at("<field>")` access
///   widened to the outermost postfix chain it heads (`data.refs.at(0)`,
///   `data.name.upper()`) — matching ink whose span is the reference
///   expression itself; and
/// - the **enclosing-expression** window — widened through surrounding call
///   arguments and operators (`#upper(data.subject)`, `#str(data.count)`) —
///   matching ink stamped with the whole wrapping expression's span. Emitted
///   only when exactly one reference sits inside it: an expression mixing two
///   fields (`data.a + data.b`) has no single owner and is not attributed.
///
/// Chain windows sort first, so ink resolving to the reference itself is
/// never claimed by a wider window. Each reference site is independent — a
/// field shown in both header and footer surfaces both sites. Not chased: a
/// value laundered through `#let s = data.x` carries the binding's span, and
/// card fields read from the per-card loop variable (`card.from`) have one
/// shared expression site across every card instance — no per-instance
/// identity exists in span data; a card *content* field is covered by its
/// per-instance generated eval site instead. Content-field references also
/// match harmlessly: their glyphs carry the helper eval-site span, which no
/// plate window contains.
pub(crate) fn scalar_windows(source: &Source, fields: &[String]) -> Vec<(String, Range<usize>)> {
    let mut anchors: Vec<(String, Range<usize>, Range<usize>)> = Vec::new();
    collect_anchors(&LinkedNode::new(source.root()), fields, &mut anchors);

    let mut out: Vec<(String, Range<usize>)> = anchors
        .iter()
        .map(|(path, chain, _)| (path.clone(), chain.clone()))
        .collect();
    for (path, chain, wide) in &anchors {
        if wide == chain {
            continue;
        }
        let inside = anchors
            .iter()
            .filter(|(_, c, _)| wide.start <= c.start && c.end <= wide.end)
            .count();
        if inside == 1 {
            out.push((path.clone(), wide.clone()));
        }
    }
    out
}

/// Recurse the whole tree collecting `(path, chain range, enclosing range)`
/// per reference site. Recursion continues into matched subtrees — a
/// reference nested in another chain's arguments is its own site.
fn collect_anchors(
    node: &LinkedNode,
    fields: &[String],
    out: &mut Vec<(String, Range<usize>, Range<usize>)>,
) {
    if let Some((path, anchor)) = data_access(node, fields) {
        // Chain: the outermost postfix chain headed by this access.
        let mut chain = anchor.clone();
        while let Some(parent) = chain.parent() {
            match parent.kind() {
                SyntaxKind::FieldAccess | SyntaxKind::FuncCall => chain = parent.clone(),
                _ => break,
            }
        }
        // Enclosing expression: widened through argument and operator
        // context, stopping at any statement/markup boundary.
        let mut wide = chain.clone();
        while let Some(parent) = wide.parent() {
            match parent.kind() {
                SyntaxKind::FieldAccess
                | SyntaxKind::FuncCall
                | SyntaxKind::Args
                | SyntaxKind::Named
                | SyntaxKind::Spread
                | SyntaxKind::Parenthesized
                | SyntaxKind::Unary
                | SyntaxKind::Binary => wide = parent.clone(),
                _ => break,
            }
        }
        out.push((path, chain.range(), wide.range()));
    }
    for child in node.children() {
        collect_anchors(&child, fields, out);
    }
}

/// If `node` is a `data.<field>` access or a `data.at("<field>")` call head
/// with a declared field, its schema path and the node to widen from.
fn data_access<'a>(
    node: &LinkedNode<'a>,
    fields: &[String],
) -> Option<(String, LinkedNode<'a>)> {
    if node.kind() != SyntaxKind::FieldAccess {
        return None;
    }
    let access = node.cast::<ast::FieldAccess>()?;
    let ast::Expr::Ident(target) = access.target() else {
        return None;
    };
    if target.as_str() != "data" {
        return None;
    }
    let field = access.field();
    if fields.iter().any(|f| f == field.as_str()) {
        return Some((field.as_str().to_string(), node.clone()));
    }
    // `data.at("field")`: the parent call carries the field name as its first
    // positional string argument.
    if field.as_str() == "at" {
        let parent = node.parent()?;
        let call = parent.cast::<ast::FuncCall>()?;
        let ast::Expr::FieldAccess(callee) = call.callee() else {
            return None;
        };
        if callee.to_untyped() != node.get() {
            return None;
        }
        let first = call.args().items().find_map(|arg| match arg {
            ast::Arg::Pos(ast::Expr::Str(s)) => Some(s.get().to_string()),
            _ => None,
        })?;
        if fields.contains(&first) {
            return Some((first, parent.clone()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::compile_document;
    use crate::world::QuillWorld;
    use quillmark_core::{FileTreeNode, Quill};
    use std::collections::HashMap as Map;
    use typst::World;

    fn quill(yaml: &str, plate: &str) -> Quill {
        let mut files = Map::new();
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

    /// The premise the whole mechanism stands on: content produced by a
    /// generated markup **block** binding (`#let _qm_cN = [ .. ]`) resolves
    /// into that block's recorded byte window in the helper `lib.typ` — a
    /// *package* source, not a plate file — through the production classifier.
    #[test]
    fn block_output_spans_resolve_into_the_helper_file() {
        const YAML: &str = r#"
quill:
  name: span_probe
  version: 0.1.0
  backend: typst
  description: helper-file span resolution probe
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: markdown
      description: a probe field
"#;
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 400pt, margin: 40pt)
#data.intro
"#;
        let q = quill(YAML, PLATE);
        let plate = crate::read_plate(&q).expect("plate");
        let schema = quillmark_core::quill::build_transform_schema(q.config());
        let meta = crate::SchemaMeta::from_schema_json(schema.as_json());
        let data = serde_json::json!({ "intro": "A probe paragraph, PROBETOKEN." });
        let transformed = crate::transformed_data(&schema, &meta, &data).expect("transform");
        let mut world = QuillWorld::new(&q, &plate).expect("world");
        let windows = world.inject_helper_package(&transformed, &meta);
        let (doc, _) = compile_document(&world).expect("compile");
        let helper = world
            .source(QuillWorld::helper_fid("lib.typ"))
            .expect("helper source");

        let intro_idx = windows
            .iter()
            .position(|w| w.path == "intro")
            .expect("intro window");
        let mut cls = Classifier {
            world: &world,
            helper: &helper,
            windows: &windows,
            memo: HashMap::new(),
        };
        let mut hits = Vec::new();
        for (page, p) in doc.pages().iter().enumerate() {
            collect_page_hits(&p.frame, page, &mut cls, &mut hits);
        }
        assert!(
            hits.iter().any(|h| h.window == Some(intro_idx)),
            "block output glyphs must classify into the helper file's recorded window {:?}",
            windows[intro_idx].range
        );
    }

    #[test]
    fn scalar_windows_track_chains_and_single_owner_enclosing_expressions() {
        let src = Source::detached(
            r#"
#import "@local/quillmark-helper:0.1.0": data
#data.subject
#data.at("subject")
#data.refs.at(0)
#upper(data.subject)
#(data.subject + data.other)
#let s = data.other
"#,
        );
        let fields = vec![
            "subject".to_string(),
            "refs".to_string(),
            "other".to_string(),
        ];
        let wins = scalar_windows(&src, &fields);
        let text = src.text();
        let spans: Vec<(&str, &str)> = wins
            .iter()
            .map(|(p, r)| (p.as_str(), &text[r.clone()]))
            .collect();
        for expected in [
            ("subject", "data.subject"),
            ("subject", "data.at(\"subject\")"),
            ("refs", "data.refs.at(0)"),
            ("other", "data.other"),
            // A wrapping call with a single reference owns its whole
            // expression: ink stamped with the outer call's span attributes
            // to the field.
            ("subject", "upper(data.subject)"),
        ] {
            assert!(spans.contains(&expected), "missing {expected:?}: {spans:?}");
        }
        // An expression mixing two fields has no single owner — no enclosing
        // window for either.
        assert!(
            !spans
                .iter()
                .any(|(_, t)| t.contains("data.subject + data.other")),
            "multi-reference expressions are not attributed: {spans:?}"
        );
        // Chain windows precede enclosing-expression windows, so ink at the
        // reference itself is never claimed by a wider window.
        let chain_pos = spans
            .iter()
            .position(|s| *s == ("subject", "data.subject"))
            .unwrap();
        let wide_pos = spans
            .iter()
            .position(|s| *s == ("subject", "upper(data.subject)"))
            .unwrap();
        assert!(chain_pos < wide_pos, "chains sort before wides: {spans:?}");
    }
}
