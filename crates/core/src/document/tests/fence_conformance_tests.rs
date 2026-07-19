//! Cross-checks the hand-rolled fence scanner (`super::super::fences`) against
//! a real CommonMark parser (`pulldown-cmark`, a dev-dependency).
//!
//! ## The invariant
//!
//! Quillmark runs two parsers: the hand-rolled scanner that splits a document
//! into card-yaml blocks + prose bodies, and pulldown-cmark in the typst
//! backend that renders each body. They must agree about where fenced blocks
//! begin and end, or the splitter could slice in the middle of something the
//! renderer treats as a single code block.
//!
//! To pulldown-cmark a card-yaml block is just a fenced code block (or, for
//! the `---` root alias, a YAML metadata block). So:
//!
//! > **Every card-yaml block the scanner recognizes must coincide — same
//! > opening offset, same fence span — with a fenced code block or YAML
//! > metadata block that pulldown-cmark delimits on the same source.**
//!
//! The relationship is one-directional (⊆): pulldown legitimately sees *more*
//! fenced blocks than we do — code blocks inside prose bodies, and `~~~`
//! fences that fail our blank-line-above rule — and those are correctly left
//! in the body. We only require that none of *our* blocks invent or misplace a
//! fence relative to CommonMark.
//!
//! pulldown reports a fenced/metadata block as `[opener_line_start ..
//! end_of_closing_fence]` (the trailing newline is excluded); our block range
//! includes the closer's trailing newline. Comparing the spans with trailing
//! `\n`/`\r` trimmed makes them convention-independent.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::document::fences::find_metadata_blocks;

/// `(start, end)` byte spans of every fenced code block and YAML-style
/// metadata block pulldown-cmark finds in `md`.
fn pulldown_fence_spans(md: &str) -> Vec<(usize, usize)> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    let mut spans = Vec::new();
    // Stack of (start_offset) for the currently-open fenced/metadata block.
    let mut open: Vec<usize> = Vec::new();
    for (ev, range) in Parser::new_ext(md, opts).into_offset_iter() {
        match ev {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_)))
            | Event::Start(Tag::MetadataBlock(_)) => open.push(range.start),
            Event::End(TagEnd::CodeBlock) | Event::End(TagEnd::MetadataBlock(_)) => {
                if let Some(start) = open.pop() {
                    spans.push((start, range.end));
                }
            }
            _ => {}
        }
    }
    spans
}

fn trim_fence(s: &str) -> &str {
    s.trim_end_matches(['\n', '\r'])
}

/// Assert the ⊆ invariant for one document. Returns a human-readable failure
/// description, or `None` if conformant (or if our scanner rejects the input —
/// error paths are out of scope for this cross-check).
fn nonconformance(md: &str) -> Option<String> {
    let Ok((blocks, _warnings)) = find_metadata_blocks(md) else {
        return None;
    };
    let pd = pulldown_fence_spans(md);

    for b in &blocks {
        let ours = trim_fence(&md[b.start..b.end]);
        let matched = pd
            .iter()
            .any(|&(ps, pe)| ps == b.start && trim_fence(&md[ps..pe.min(md.len())]) == ours);
        if !matched {
            return Some(format!(
                "card-yaml block at [{}..{}] = {:?} has no matching pulldown fence \
                 (pulldown spans: {:?})",
                b.start,
                b.end,
                ours,
                pd.iter()
                    .map(|&(s, e)| trim_fence(&md[s..e.min(md.len())]))
                    .collect::<Vec<_>>()
            ));
        }
    }
    None
}

fn assert_conformant(label: &str, md: &str) {
    if let Some(why) = nonconformance(md) {
        panic!("fence non-conformance in {label}:\n{why}\n--- source ---\n{md}");
    }
}

// ── Synthetic edge-case content ────────────────────────────────────────────────

#[test]
fn scanner_agrees_with_commonmark_on_synthetic_inputs() {
    let cases: &[(&str, &str)] = &[
        ("bare root", "~~~\n$quill: q\n$kind: main\n~~~\n\nBody.\n"),
        (
            "legacy info string",
            "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\nBody.\n",
        ),
        (
            "root + composable card",
            "~~~\n$quill: q\n$kind: main\n~~~\n\nB\n\n~~~\n$kind: note\nx: 1\n~~~\n\nAfter.\n",
        ),
        (
            "three blocks",
            "~~~\n$quill: q\n$kind: main\n~~~\n\n~~~\n$kind: a\n~~~\n\nfirst\n\n~~~\n$kind: b\n~~~\n\nsecond\n",
        ),
        (
            "--- root alias",
            "---\n$quill: q\n$kind: main\n---\n\nBody.\n",
        ),
        (
            "--- root then bare card",
            "---\n$quill: q\n$kind: main\n---\n\nBody.\n\n~~~\n$kind: note\nx: 1\n~~~\n",
        ),
        (
            "shielded fences inside a backtick block",
            "~~~\n$quill: q\n$kind: main\n~~~\n\n```text\n~~~\n$kind: nope\n~~~\n```\n\nBody.\n",
        ),
        (
            "longer-tilde card (shorter inner ~~~ stays payload)",
            "~~~\n$quill: q\n$kind: main\n~~~\n\n~~~~\n$kind: c\nx: 1\n~~~~\n",
        ),
        (
            "five-tilde root",
            "~~~~~\n$quill: q\n$kind: main\n~~~~~\n\nBody.\n",
        ),
        (
            "backtick code block shielding tildes",
            "~~~\n$quill: q\n$kind: main\n~~~\n\n```\n~~~\nx\n~~~\n```\n\nBody.\n",
        ),
        (
            "unclosed `~~~` in body is CommonMark code to EOF, not a card",
            "~~~\n$quill: q\n$kind: main\n~~~\n\nIntro.\n\n~~~\nstray\n",
        ),
        (
            "tilde fence with language info in body",
            "~~~\n$quill: q\n$kind: main\n~~~\n\n~~~rust\nlet x = 1;\n~~~\n",
        ),
        (
            "card after a paragraph",
            "~~~\n$quill: q\n$kind: main\n~~~\n\nParagraph one.\n\n~~~\n$kind: c\nk: v\n~~~\n",
        ),
        (
            "card with nested yaml payload",
            "~~~\n$quill: q\n$kind: main\nmeta:\n  a: 1\n  b:\n    - x\n    - y\n~~~\n\nBody.\n",
        ),
        (
            "empty payload block",
            "~~~\n$quill: q\n$kind: main\n~~~\n\n~~~\n$kind: marker\n~~~\n",
        ),
        (
            "CRLF line endings",
            "~~~\r\n$quill: q\r\n$kind: main\r\n~~~\r\n\r\nBody.\r\n",
        ),
        (
            "trailing newline absent on closer",
            "~~~\n$quill: q\n$kind: main\n~~~",
        ),
        (
            "indented opener is not a card (stays a CommonMark code block)",
            "~~~\n$quill: q\n$kind: main\n~~~\n\nB\n\n   ~~~\n$kind: c\nx: 1\n   ~~~\n",
        ),
        (
            "opener with trailing spaces",
            "~~~   \n$quill: q\n$kind: main\n~~~\n\nBody.\n",
        ),
        (
            "indented closer on root",
            "~~~\n$quill: q\n$kind: main\n  ~~~\n\nBody.\n",
        ),
        (
            "closer longer than opener",
            "~~~\n$quill: q\n$kind: main\n~~~~~\n\nBody.\n",
        ),
    ];

    for (label, md) in cases {
        assert_conformant(label, md);
    }
}

// ── Fixture content ────────────────────────────────────────────────────────────

fn collect_md(root: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

#[test]
fn scanner_agrees_with_commonmark_on_fixtures() {
    let fixtures_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("crates/fixtures/resources");

    let mut files = Vec::new();
    collect_md(&fixtures_root, &mut files);

    let mut checked = 0;
    let mut failures = Vec::new();
    for path in &files {
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        let label = path
            .strip_prefix(&fixtures_root)
            .unwrap_or(path)
            .display()
            .to_string();
        if let Some(why) = nonconformance(&src) {
            failures.push(format!("{label}: {why}"));
        } else {
            checked += 1;
        }
    }

    assert!(
        failures.is_empty(),
        "fence non-conformance in {} fixture(s):\n{}",
        failures.len(),
        failures.join("\n\n")
    );
    assert!(checked > 0, "no fixtures exercised the cross-check");
}
