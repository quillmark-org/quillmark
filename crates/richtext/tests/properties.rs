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
use quillmark_richtext::model::{Line, Mark, MarkKind};
use quillmark_richtext::usv::{char_to_utf16, utf16_to_char};
use quillmark_richtext::{Delta, LineKind, LineOp, MarkOp, Op, RichText};

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
// escaping and USV bounds are exercised by the round-trip. The first char stays
// alphanumeric, so a block marker never *leads* an item's content (`- >`, `- #`
// would make pulldown build an empty nested block, not literal text — a
// degenerate corpus no editor emits); but the tail now carries `&` and the
// block-marker chars (`# > - . +`), exercising `&`-entity escaping (#848 part 2)
// and a trailing-`#` heading run (#848 part 3) through the round-trip, not just
// the pinned `export::tests::*` unit tests.
fn plain_word() -> impl Strategy<Value = String> {
    r"[a-z0-9][a-z0-9*_~\\&#>.+😀你-]{0,5}"
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
        (prose(), prose(), prose()).prop_map(|(a, b, c)| format!("- {a}\n  - {b}\n  - {c}")),
        prose().prop_map(|p| format!("> {p}")),
        prop::collection::vec(clean_word(), 1..4)
            .prop_map(|ls| format!("```\n{}\n```", ls.join("\n"))),
        (clean_word(), clean_word())
            .prop_map(|(a, b)| format!("| {a} | {b} |\n| --- | --- |\n| 1 | 2 |")),
    ]
}

fn document() -> impl Strategy<Value = String> {
    prop::collection::vec(block(), 1..6).prop_map(|blocks| blocks.join("\n\n"))
}

// ---------------------------------------------------------------------------
// Overlapping-mark helpers (issue #848 property): the four formatting kinds an
// editor can freely overlap, and the predicates for the one unrepresentable
// shape (two asterisk-family marks partially overlapping).
// ---------------------------------------------------------------------------

fn ov_kind(i: u8) -> MarkKind {
    match i % 4 {
        0 => MarkKind::Strong,
        1 => MarkKind::Emph,
        2 => MarkKind::Strike,
        _ => MarkKind::Underline,
    }
}

/// Both marks render as a run of the same `*` character (`strong`/`emph`).
fn both_asterisk(marks: &[Mark]) -> bool {
    marks
        .iter()
        .all(|m| matches!(m.kind, MarkKind::Strong | MarkKind::Emph))
}

/// The marks intersect but neither contains the other — the shape that forces a
/// close-and-reopen (and, for asterisk delimiters, an ambiguous `***` merge).
fn partial_overlap(marks: &[Mark]) -> bool {
    let (a, b) = (&marks[0], &marks[1]);
    (a.start < b.start && b.start < a.end && a.end < b.end)
        || (b.start < a.start && a.start < b.end && b.end < a.end)
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

    /// Property 1b (issue #848): overlapping formatting marks — the free
    /// (Peritext-style) overlap `apply_mark_ops` produces but markdown import
    /// never does — export to *balanced* markdown that preserves the text.
    ///
    /// The shape is the issue's canonical overlap: two marks over contiguous
    /// word-char text in a staircase (`s1 < s2 < e1 < e2 == n`), the
    /// representable family markdown can carry. (Arbitrary editor marks that end
    /// mid-word before another word char, or reopen before a space, hit
    /// CommonMark flanking rules and are *not* generally representable — an
    /// editor-only degenerate shape outside the `from_markdown` fixed-point
    /// contract, like the hard-break limits.) The export must:
    ///   - preserve the text exactly (the corruption the issue reported);
    ///   - round-trip exactly for marks with *distinct* delimiters;
    ///   - stay text-safe for the `strong`+`emph` asterisk clash, whose overlap
    ///     is unrepresentable and degrades to its nested subset (documented).
    #[test]
    fn overlapping_marks_export_is_text_safe(
        raw in "[a-z]{4,8}",
        x in 0usize..64, y in 0usize..64, z in 0usize..64,
        k1i in 0u8..4, k2i in 0u8..4,
    ) {
        let text = raw;
        let n = text.chars().count();
        // Three distinct interior cut points → the staircase s1<s2<e1<e2==n,
        // built by construction (no rejection) so the whole family is covered.
        let s1 = x % (n - 2);
        let s2 = s1 + 1 + y % (n - s1 - 2);
        let e1 = s2 + 1 + z % (n - s2 - 1);
        let e2 = n;

        let marks = vec![
            Mark { start: s1, end: e1, kind: ov_kind(k1i) },
            Mark { start: s2, end: e2, kind: ov_kind(k2i) },
        ];
        let mut rt = RichText {
            text: text.clone(),
            lines: vec![Line { kind: LineKind::Para, containers: vec![], continues: false }],
            marks,
            islands: vec![],
        };
        rt.normalize();
        prop_assert_eq!(rt.validate(), Ok(()), "hand-built corpus invalid");

        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap();
        prop_assert_eq!(rt2.validate(), Ok(()), "re-import invalid for {:?}", md);
        // The critical #848 invariant: overlap never corrupts the text (no
        // unbalanced/unclosed delimiter leaks a literal `**`/`*` into the corpus).
        prop_assert_eq!(&rt2.text, &rt.text, "overlap corrupted text: {:?}", md);

        // Distinct delimiters → an exact fixed point via close-and-reopen. Two
        // asterisk-family marks that still partially overlap after normalization
        // are unrepresentable, so only text-safety (asserted above) holds.
        let asterisk_clash = both_asterisk(&rt.marks) && rt.marks.len() == 2
            && partial_overlap(&rt.marks);
        if !asterisk_clash {
            prop_assert_eq!(&rt, &rt2, "distinct-delim overlap not a fixed point: {:?}", md);
        }
        // Whatever the shape, the re-imported corpus is itself a genuine fixed
        // point (it lives in the `from_markdown` domain the contract covers).
        prop_assert_eq!(&rt2, &from_markdown(&to_markdown(&rt2)).unwrap(),
            "re-imported overlap corpus not a fixed point: {:?}", md);
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
// Edit-channel invariant properties (issue #847): the three apply channels
// preserve `validate()`. The corpus arriving at a channel is valid; a
// successful apply must leave it valid. For `apply_text_delta` this includes
// cascading island removal when a slot char is deleted (islands.len() stays in
// sync with the slot count) and rejecting a raw slot insert.
// ---------------------------------------------------------------------------

/// `Op::Retain(n)`, or nothing when `n == 0` (no empty retains in the script).
fn retain(n: usize) -> Vec<Op> {
    if n == 0 {
        vec![]
    } else {
        vec![Op::Retain(n)]
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// `apply_text_delta` preserves `validate()` for a text-channel edit
    /// (insert clean text — including `\n` — or delete a range). A deletion can
    /// span an island slot; the cascade must drop the backing island so the
    /// slot/island counts stay in sync (the #847 corruption, now caught here).
    /// Inserted text excludes U+FFFC, `\r`, and bidi controls — a real editor
    /// never types them through this channel, and a raw slot insert is rejected.
    #[test]
    fn apply_text_delta_preserves_validate(
        md in document(),
        ins in "[a-z0-9 \n]{0,10}",
        pos_seed in 0usize..4096,
        del_seed in 0usize..4096,
        is_delete in any::<bool>(),
    ) {
        let mut rt = from_markdown(&md).unwrap();
        prop_assert_eq!(rt.validate(), Ok(()), "import invalid for {:?}", md);
        let len = rt.len_usv();
        let pos = pos_seed % (len + 1);
        let delta = if is_delete {
            let k = del_seed % (len - pos + 1);
            let ops = retain(pos)
                .into_iter()
                .chain(std::iter::once(Op::Delete(k)))
                .chain(retain(len - pos - k))
                .collect();
            Delta { ops }
        } else {
            let ops = retain(pos)
                .into_iter()
                .chain(std::iter::once(Op::Insert(ins.clone())))
                .chain(retain(len - pos))
                .collect();
            Delta { ops }
        };
        if rt.apply_text_delta(&delta).is_ok() {
            prop_assert_eq!(rt.validate(), Ok(()), "text delta broke an invariant");
        }
    }

    /// `apply_mark_ops` preserves `validate()`: an accepted Add over a clamped
    /// range leaves the corpus valid (normalization trims edges / drops
    /// zero-width).
    #[test]
    fn apply_mark_ops_preserves_validate(
        md in document(),
        s_seed in 0usize..4096,
        e_seed in 0usize..4096,
    ) {
        let mut rt = from_markdown(&md).unwrap();
        let len = rt.len_usv();
        let a = s_seed % (len + 1);
        let b = e_seed % (len + 1);
        let op = MarkOp::Add { start: a.min(b), end: a.max(b), kind: MarkKind::Strong };
        if rt.apply_mark_ops(&[op]).is_ok() {
            prop_assert_eq!(rt.validate(), Ok(()), "mark op broke an invariant");
        }
    }

    /// `apply_line_ops` preserves the structural invariants (line/segment sync
    /// and island/slot sync) across an accepted split/join/set-kind. Marks are
    /// cleared first: a split/join splices `\n` without rebasing marks (mark
    /// rebasing rides the text-delta channel, per the `ops` module docs), so
    /// asserting mark-range preservation here would test a guarantee this
    /// channel does not make. Splicing `\n` never adds or removes a slot, so
    /// island sync is a genuine post-condition to check.
    #[test]
    fn apply_line_ops_preserves_validate(
        md in document(),
        pos_seed in 0usize..4096,
        line_seed in 0usize..64,
        which in 0u8..3,
    ) {
        let mut rt = from_markdown(&md).unwrap();
        rt.marks.clear();
        let len = rt.len_usv();
        let nlines = rt.lines.len().max(1);
        let op = match which {
            0 => LineOp::Split { at: pos_seed % (len + 1) },
            1 => LineOp::Join { line: line_seed % nlines },
            _ => LineOp::SetKind { line: line_seed % nlines, kind: LineKind::Heading { level: 2 } },
        };
        if rt.apply_line_ops(&[op]).is_ok() {
            prop_assert_eq!(rt.validate(), Ok(()), "line op broke an invariant");
        }
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
    for name in [
        "sample.md",
        "card_yaml_demo.md",
        "extended_metadata_demo.md",
    ] {
        let md = fixture_body(name);
        let rt = from_markdown(&md).unwrap_or_else(|e| panic!("import {name}: {e}"));
        assert_eq!(rt.validate(), Ok(()), "{name} invariants");
        let rt2 = from_markdown(&to_markdown(&rt)).unwrap();
        assert_eq!(rt, rt2, "{name} corpus not a fixed point");
    }
}
