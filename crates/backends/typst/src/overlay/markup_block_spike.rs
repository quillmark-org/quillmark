//! Spike (#795 follow-up, cascade 1): content fields as **markup blocks** in
//! the generated helper, replacing `eval(str, mode: "markup")`.
//!
//! Premise: codegen already writes each field's converted markup into the
//! generated `lib.typ` as a string literal for `eval` to re-parse at runtime.
//! Writing it as a content block instead — `#let _qm_c0 = [converted markup]`
//! — would make the content *source*: the file parser parses it once, every
//! glyph carries its own syntax node's span (word/run granularity, not the
//! eval call's uniform span), and errors inside field content get real
//! resolvable positions instead of the ephemeral-source dead end
//! `error_mapping.rs` special-cases.
//!
//! What must hold for this to be viable, each pinned by a test below:
//!   1. A module-scope block binding in the helper *package* compiles, and
//!      its glyph spans resolve into the block's byte window with per-node
//!      granularity (several distinct ranges, not one uniform range).
//!   2. Real `mark_to_typst` output embeds inside `[...]` without corrupting
//!      the parse — brackets, raw/code carrying `]`, hashes, quotes,
//!      backslashes, unicode — and its ink still classifies into the window.
//!   3. The window survives a render-body-shaped capture/replay rebuild.
//!   4. A glyph's resolved range recovers its exact source substring — the
//!      foundation for click → *cursor offset within the field*, not just
//!      click → field.
//!
//! Nothing here is wired into production; the modules compile under
//! `#[cfg(test)]` only.

use std::collections::HashMap;

use quillmark_core::{FileTreeNode, Quill};
use typst::layout::{Frame, FrameItem, Transform};
use typst::syntax::{FileId, Span};
use typst::WorldExt;
use typst_layout::PagedDocument;

use crate::compile::compile_document;
use crate::convert::mark_to_typst;
use crate::helper;
use crate::world::QuillWorld;

fn quill(plate: &str) -> Quill {
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: br#"
quill:
  name: markup_block_spike
  version: 0.1.0
  backend: typst
  description: markup-block content spike
typst:
  plate_file: plate.typ
main:
  fields: {}
"#
            .to_vec(),
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

/// Compile `plate` against a hand-built helper `lib.typ` standing in for the
/// codegen end-state (block bindings instead of eval sites).
fn compile_with_helper(helper_src: &str, plate: &str) -> (PagedDocument, QuillWorld) {
    let q = quill(plate);
    let mut world = QuillWorld::new(&q, plate).expect("world");
    world.set_source(QuillWorld::helper_fid("lib.typ"), helper_src);
    world.set_binary(
        QuillWorld::helper_fid("typst.toml"),
        helper::generate_typst_toml().into_bytes(),
    );
    let (doc, _) = compile_document(&world).expect("compile");
    (doc, world)
}

/// Every text glyph's resolved (file, byte range), in frame-walk order.
fn glyph_origins(doc: &PagedDocument, world: &QuillWorld) -> Vec<(FileId, std::ops::Range<usize>)> {
    fn walk(
        frame: &Frame,
        ts: Transform,
        world: &QuillWorld,
        out: &mut Vec<(FileId, std::ops::Range<usize>)>,
    ) {
        for (pos, item) in frame.items() {
            match item {
                FrameItem::Group(g) => {
                    let ts = ts
                        .pre_concat(Transform::translate(pos.x, pos.y))
                        .pre_concat(g.transform);
                    walk(&g.frame, ts, world, out);
                }
                FrameItem::Text(text) => {
                    for glyph in &text.glyphs {
                        let span: Span = glyph.span.0;
                        if let (Some(id), Some(range)) = (span.id(), world.range(span)) {
                            out.push((id, range));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for page in doc.pages() {
        walk(&page.frame, Transform::identity(), world, &mut out);
    }
    out
}

/// Distinct resolved ranges nested inside `window` of the helper file.
fn distinct_ranges_in_window(
    doc: &PagedDocument,
    world: &QuillWorld,
    window: std::ops::Range<usize>,
) -> Vec<std::ops::Range<usize>> {
    let helper_id = QuillWorld::helper_fid("lib.typ");
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    for (id, r) in glyph_origins(doc, world) {
        if id == helper_id && window.start <= r.start && r.end <= window.end {
            if !ranges.contains(&r) {
                ranges.push(r);
            }
        }
    }
    ranges
}

const IMPORT_PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": _qm_c0
#set page(width: 500pt, height: 500pt, margin: 40pt)
#_qm_c0
"#;

/// Premise 1: a module-scope block binding in the helper package parses,
/// places, and yields **per-node** spans — several distinct ranges nested in
/// the block window, ordered like the source — where the eval design yields
/// exactly one uniform range for the whole field.
#[test]
fn module_scope_block_has_per_node_spans_in_window() {
    let helper_src = "#let _qm_c0 = [Alpha #strong[beta] gamma #emph[delta] omega.]\n";
    let (doc, world) = compile_with_helper(helper_src, IMPORT_PLATE);

    let window = helper_src.find('[').unwrap()..helper_src.rfind(']').unwrap() + 1;
    let ranges = distinct_ranges_in_window(&doc, &world, window.clone());
    assert!(
        ranges.len() >= 4,
        "expected per-node span granularity (text runs + strong + emph), got {ranges:?}"
    );
    // Ranges appear in source order as the walk proceeds — the property a
    // click → cursor-offset mapping stands on.
    let starts: Vec<usize> = ranges.iter().map(|r| r.start).collect();
    let mut sorted = starts.clone();
    sorted.sort();
    assert_eq!(starts, sorted, "walk order matches source order: {ranges:?}");
}

/// Premise 4: a resolved range recovers its exact source substring — span →
/// byte offset *within the field's markup*, the cursor-fidelity foundation.
#[test]
fn resolved_ranges_recover_exact_source_substrings() {
    let helper_src = "#let _qm_c0 = [Alpha #strong[beta] gamma.]\n";
    let (doc, world) = compile_with_helper(helper_src, IMPORT_PLATE);

    let window = helper_src.find('[').unwrap()..helper_src.rfind(']').unwrap() + 1;
    let texts: Vec<&str> = distinct_ranges_in_window(&doc, &world, window)
        .into_iter()
        .map(|r| &helper_src[r])
        .collect();
    assert!(
        texts.contains(&"beta"),
        "the strong word's glyphs resolve to exactly its source text: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("Alpha")),
        "the leading text run resolves to its source text: {texts:?}"
    );
}

/// Premise 2: real converter output embeds inside `[...]` without corrupting
/// the parse, across the shapes that could plausibly break a block context —
/// brackets in text, inline code carrying `]` and backticks, fenced code with
/// Typst syntax inside, links, tables, headings, blockquotes, escapes,
/// unicode — and the embedded content still inks glyphs that classify into
/// the block window.
#[test]
fn converter_output_embeds_as_block_source() {
    let corpus: &[&str] = &[
        "Plain paragraph with **bold**, _italic_, and `code`.",
        "Literal brackets ] and [ in prose, plus # hash and \\ backslash.",
        "Inline code with a bracket: `a[0] = b]` and ``nested ` tick``.",
        "```typst\n#import \"@x/y:1.0.0\": z\n#let a = [unbalanced ]\n```\n\nAfter the fence.",
        "A [link](https://example.com/a?b=c&d=\"e\") in text.",
        "# Heading\n\n- item one\n- item two\n\n> a blockquote line",
        "| a | b |\n|---|---|\n| 1 | ] |",
        "Unicode: naïve café — “quotes” 你好 🎉",
        "Line one  \nline two after a hard break.\n\nSecond paragraph.",
    ];

    for md in corpus {
        let markup = mark_to_typst(md).expect("convert");
        // The newline wrap matters: line-anchored markup (headings, list
        // items) must start at a line boundary, which eval'ing a standalone
        // string gave for free and a block must provide explicitly.
        let helper_src = format!("#let _qm_c0 = [\n{markup}\n]\n");
        let (doc, world) = compile_with_helper(&helper_src, IMPORT_PLATE);
        let window = helper_src.find('[').unwrap()..helper_src.rfind(']').unwrap() + 1;
        let ranges = distinct_ranges_in_window(&doc, &world, window);
        assert!(
            !ranges.is_empty(),
            "converted markup must ink glyphs inside its block window\n\
             markdown: {md:?}\nmarkup: {markup:?}"
        );
    }
}

/// Premise 3: block-borne spans ride a render-body-shaped capture/replay
/// rebuild exactly like eval-borne ones do — the survival property is the
/// glyph's, not the mechanism's.
#[test]
fn block_window_survives_capture_replay_rebuild() {
    let helper_src =
        "#let _qm_c0 = [First paragraph of the body.\n\nSecond paragraph, rebuilt later.]\n";
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": _qm_c0
#set page(width: 500pt, height: 500pt, margin: 40pt)

#let BUF = state("BUF", ())
#let capture(it) = {
  show par: p => {
    BUF.update(buf => buf + (text([#p.body]),))
    []
  }
  it
}

#capture(_qm_c0)

#context {
  for c in BUF.get() {
    block[#c]
  }
}
"#;
    let (doc, world) = compile_with_helper(helper_src, plate);
    let window = helper_src.find('[').unwrap()..helper_src.rfind(']').unwrap() + 1;
    let ranges = distinct_ranges_in_window(&doc, &world, window);
    assert!(
        ranges.len() >= 2,
        "both rebuilt paragraphs' glyphs keep block-resolved spans: {ranges:?}"
    );
}
