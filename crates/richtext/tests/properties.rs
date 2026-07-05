//! Phase-1 property suite (issue #831 step 1).
//!
//! The four properties the freeze rests on:
//!
//! 1. **Round-trip modulo loss class** — for a corpus from import,
//!    `import(export(rt)) == rt`. Markdown source is not canonical; the corpus
//!    is, so round-trip is defined at the corpus. Our generator emits only
//!    lossless islands, so equality is exact.
//! 2. **Canonical serialization** — byte-deterministic and a fixed point;
//!    insensitive to mark/island discovery order.
//! 3. **Diff-import preserves identity marks** — an anchor over text that
//!    survives a rewrite is carried forward.
//! 4. **USV boundary** — char↔UTF-16 round-trips at char boundaries; a
//!    mid-surrogate index rounds down.

use proptest::prelude::*;
use quillmark_richtext::delta::diff_import;
use quillmark_richtext::export::to_markdown;
use quillmark_richtext::import::from_markdown;
use quillmark_richtext::model::{Mark, MarkKind};
use quillmark_richtext::usv::{char_to_utf16, utf16_to_char};
use quillmark_richtext::RichText;

// ---------------------------------------------------------------------------
// A constrained markdown generator: combinations of the phase-1 constructs,
// with inline tokens space-separated so the property exercises structure and
// marks without depending on CommonMark's delimiter-adjacency corners (those
// are pinned by explicit unit tests, not fuzzed here).
// ---------------------------------------------------------------------------

// `clean_word` is safe content for inside a formatted span / url / code / cell.
fn clean_word() -> impl Strategy<Value = String> {
    "[a-z0-9]{1,6}"
}

// `plain_word` carries inline-special and astral chars as *literal text*, so
// escaping and USV bounds are exercised by the round-trip. The first char is
// alphanumeric and the set excludes block-marker chars (`> # - .`): a
// block marker *leading* an item's content (`- >`, `- #`) makes pulldown build
// an empty nested block, not literal text — a degenerate corpus no editor emits
// (leading-marker escaping is covered by `export::tests::*` unit tests instead).
fn plain_word() -> impl Strategy<Value = String> {
    r"[a-z0-9][a-z0-9*_~\\😀你]{0,5}"
}

fn inline_token() -> impl Strategy<Value = String> {
    prop_oneof![
        plain_word(),
        clean_word().prop_map(|w| format!("**{w}**")),
        clean_word().prop_map(|w| format!("_{w}_")),
        clean_word().prop_map(|w| format!("~~{w}~~")),
        clean_word().prop_map(|w| format!("`{w}`")),
        clean_word().prop_map(|w| format!("<u>{w}</u>")),
        (clean_word(), clean_word()).prop_map(|(t, u)| format!("[{t}](https://ex.com/{u})")),
    ]
}

fn prose() -> impl Strategy<Value = String> {
    // Mix single-space and hard-break (`\`+newline) separators, and let some
    // tokens abut, so delimiter-adjacency and hard breaks are covered.
    prop::collection::vec(inline_token(), 1..5).prop_map(|toks| toks.join(" "))
}

// Hard breaks join clean, non-empty *text* lines (the realistic case — an
// address block, a signature). A mark that *spans* a hard break, or an empty
// line adjacent to one, is a degenerate corpus markdown cannot represent (no
// blank-then-forced-break syntax); those are recorded as documented codec
// limits (see `export::tests::known_hard_break_limits`), not fuzzed here.
fn hard_break_line() -> impl Strategy<Value = String> {
    prop::collection::vec(clean_word(), 1..4).prop_map(|w| w.join(" "))
}

fn hard_break_prose() -> impl Strategy<Value = String> {
    prop::collection::vec(hard_break_line(), 2..4).prop_map(|lines| lines.join("\\\n"))
}

fn block() -> impl Strategy<Value = String> {
    prop_oneof![
        prose(),
        hard_break_prose(),
        (1u8..=6, prose()).prop_map(|(lvl, p)| format!("{} {p}", "#".repeat(lvl as usize))),
        // Bullet list, some items multi-paragraph (nested blank line).
        prop::collection::vec(prose(), 1..4).prop_map(|items| items
            .iter()
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")),
        prop::collection::vec(prose(), 1..4).prop_map(|items| items
            .iter()
            .enumerate()
            .map(|(i, p)| format!("{}. {p}", i + 1))
            .collect::<Vec<_>>()
            .join("\n")),
        // Nested bullet list (two container levels).
        (prose(), prose(), prose())
            .prop_map(|(a, b, c)| format!("- {a}\n  - {b}\n  - {c}")),
        prose().prop_map(|p| format!("> {p}")),
        prop::collection::vec(clean_word(), 1..4)
            .prop_map(|ls| format!("```\n{}\n```", ls.join("\n"))),
        (clean_word(), clean_word()).prop_map(|(a, b)| format!("| {a} | {b} |\n| --- | --- |\n| 1 | 2 |")),
    ]
}

fn document() -> impl Strategy<Value = String> {
    prop::collection::vec(block(), 1..6).prop_map(|blocks| blocks.join("\n\n"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// Property 1: the corpus is a fixed point of export∘import, and every
    /// imported corpus satisfies its invariants.
    #[test]
    fn corpus_round_trip_and_invariants(md in document()) {
        let rt = from_markdown(&md).unwrap();
        prop_assert_eq!(rt.validate(), Ok(()), "invariants for {:?}", md);

        let md2 = to_markdown(&rt);
        let rt2 = from_markdown(&md2).unwrap();
        prop_assert_eq!(&rt, &rt2, "not a fixed point.\n in:  {:?}\n out: {:?}", md, md2);
    }

    /// Property 2a: canonical JSON is a fixed point.
    #[test]
    fn canonical_json_fixed_point(md in document()) {
        let rt = from_markdown(&md).unwrap();
        let json = rt.to_canonical_json();
        let back = RichText::from_canonical_json(&json).unwrap();
        prop_assert_eq!(back.to_canonical_json(), json);
    }

    /// Property 2b: canonical bytes are insensitive to mark *discovery* order
    /// (normalization sorts them). Islands are inherently ordered by slot
    /// position, so only mark order is a free variable.
    #[test]
    fn canonical_json_order_insensitive(md in document()) {
        let rt = from_markdown(&md).unwrap();
        let mut shuffled = rt.clone();
        shuffled.marks.reverse();
        prop_assert_eq!(rt.to_canonical_json(), shuffled.to_canonical_json());
    }

    /// Property 3: an anchor over text that survives a rewrite is carried
    /// forward by diff-import. We prepend context (edit elsewhere), so the
    /// anchored span is untouched and must survive.
    #[test]
    fn diff_import_preserves_surviving_anchor(a in "[a-z]{3,8}", b in "[a-z]{3,8}") {
        let base_md = format!("keep {a} here");
        let mut base = from_markdown(&base_md).unwrap();
        // Anchor exactly over the `a` word.
        let start = 5;
        let end = 5 + a.chars().count();
        prop_assert_eq!(&base.text[start..end], a.as_str());
        base.marks.push(Mark { start, end, kind: MarkKind::Anchor { id: "c1".into() } });
        base.normalize();

        // Rewrite prepends `b` (edit before the anchor); anchored text unchanged.
        let new_md = format!("{b} keep {a} here");
        let (new_rt, _delta) = diff_import(&base, &new_md).unwrap();
        let anchor = new_rt.marks.iter()
            .find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1"));
        prop_assert!(anchor.is_some(), "anchor lost across surviving edit");
        let anchor = anchor.unwrap();
        prop_assert_eq!(&new_rt.text[anchor.start..anchor.end], a.as_str());
    }

    /// Property 4a: char↔UTF-16 round-trips at every char boundary.
    #[test]
    fn usv_round_trip(s in ".{0,64}") {
        let n = s.chars().count();
        for i in 0..=n {
            let u = char_to_utf16(&s, i);
            prop_assert_eq!(utf16_to_char(&s, u), i, "char {} of {:?}", i, s);
        }
    }

    /// Property 4b: a mid-surrogate UTF-16 index rounds down to its owning char.
    #[test]
    fn usv_mid_surrogate_rounds_down(prefix in "[a-z]{0,8}") {
        // 😀 is a surrogate pair; the index just after its first unit must round
        // down to the emoji's char index.
        let s = format!("{prefix}😀x");
        let emoji_char = prefix.chars().count();
        let u_before = char_to_utf16(&s, emoji_char);
        // u_before + 1 lands mid-pair.
        prop_assert_eq!(utf16_to_char(&s, u_before + 1), emoji_char);
    }
}

// ---------------------------------------------------------------------------
// Fixture corpus: the phase-1 codecs run against real fixture markdown bodies.
// ---------------------------------------------------------------------------

fn fixture_body(name: &str) -> String {
    let path = quillmark_fixtures::resource_path(name);
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn fixture_sample_round_trips() {
    // sample.md is a prose-heavy body exercising headings, lists (nested),
    // marks, links, and inline code — the codec's core surface.
    let md = fixture_body("sample.md");
    let rt = from_markdown(&md).expect("import sample.md");
    assert_eq!(rt.validate(), Ok(()), "sample.md invariants");
    let rt2 = from_markdown(&to_markdown(&rt)).unwrap();
    assert_eq!(rt, rt2, "sample.md corpus not a fixed point");
}

#[test]
fn fixture_bodies_import_and_are_valid() {
    // Every prose resource imports to a valid corpus and is a fixed point.
    for name in ["sample.md", "card_yaml_demo.md", "extended_metadata_demo.md"] {
        let md = fixture_body(name);
        let rt = from_markdown(&md).unwrap_or_else(|e| panic!("import {name}: {e}"));
        assert_eq!(rt.validate(), Ok(()), "{name} invariants");
        let rt2 = from_markdown(&to_markdown(&rt)).unwrap();
        assert_eq!(rt, rt2, "{name} corpus not a fixed point");
    }
}
