//! Phase-1 property suite (issue #831 step 1).
//!
//! The four properties the freeze rests on:
//!
//! 1. **Round-trip modulo loss class** — for a content from import,
//!    `import(export(rt)) == rt`. Markdown source is not canonical; the content
//!    is, so round-trip is defined at the content. Our generator emits only
//!    lossless islands, so equality is exact.
//! 2. **Canonical serialization** — byte-deterministic and a fixed point;
//!    insensitive to mark/island discovery order.
//! 3. **Diff-import preserves identity marks** — an anchor over text that
//!    survives a rewrite is carried forward.

use proptest::prelude::*;
use quillmark_content::delta::diff_import;
use quillmark_content::export::to_markdown;
use quillmark_content::import::from_markdown;
use quillmark_content::model::{Line, Mark, MarkKind};
use quillmark_content::{Delta, Island, LineKind, LineOp, Loss, MarkOp, Op, Content};
use serde_json::{json, Value};

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
// degenerate content no editor emits); but the tail now carries `&` and the
// block-marker chars (`# > - . +`), exercising `&`-entity escaping (#848 part 2)
// and a trailing-`#` heading run (#848 part 3) through the round-trip, not just
// the pinned `export::tests::*` unit tests.
fn plain_word() -> impl Strategy<Value = String> {
    r"[a-z0-9][a-z0-9*_~\\&#>.+😀你-]{0,5}"
}

// `special_alt`/`special_url` carry the destination- and markup-terminating
// chars #900 exposed, in *markdown source* form: a raw `]`/`[`/`\` would break
// the markup at the source level (those live only in the direct-content
// `image_and_link_specials_round_trip` property), so alt carries `&` and inline
// delimiters as literal text, and the url is angle-wrapped in source so a space,
// paren, or `&` reaches the content intact — the escape paths `clean_word` missed.
fn special_alt() -> impl Strategy<Value = String> {
    (clean_word(), r"[a-z0-9&*_~#.+]{0,4}").prop_map(|(a, b)| format!("{a}{b}"))
}

fn special_url() -> impl Strategy<Value = String> {
    (clean_word(), r"[a-z0-9 ()&]{1,5}").prop_map(|(a, b)| format!("ex.com/{a}{b}"))
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
        // #900: a link/image whose url and alt carry specials the escaper must
        // neutralize (space/paren/`&` in the angle-wrapped url; `&`/delimiters
        // in the alt), exercised in prose context (marks, lists, hard breaks).
        (clean_word(), special_url()).prop_map(|(t, u)| format!("[{t}](<{u}>)")),
        (special_alt(), special_url()).prop_map(|(a, u)| format!("![{a}](<{u}>)")),
    ]
}

fn prose() -> impl Strategy<Value = String> {
    // Mix single-space and hard-break (`\`+newline) separators, and let some
    // tokens abut, so delimiter-adjacency and hard breaks are covered.
    prop::collection::vec(inline_token(), 1..5).prop_map(|toks| toks.join(" "))
}

// Hard breaks join clean, non-empty *text* lines (the realistic case — an
// address block, a signature). A mark that *spans* a hard break, or an empty
// line adjacent to one, is a degenerate content markdown cannot represent (no
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

    /// Property 1: the content is a fixed point of export∘import, and every
    /// imported content satisfies its invariants.
    #[test]
    fn content_round_trip_and_invariants(md in document()) {
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
        let mut rt = Content {
            text: text.clone(),
            lines: vec![Line { kind: LineKind::Para, containers: vec![], continues: false }],
            marks,
            islands: vec![],
        };
        rt.normalize();
        prop_assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");

        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap();
        prop_assert_eq!(rt2.validate(), Ok(()), "re-import invalid for {:?}", md);
        // The critical #848 invariant: overlap never corrupts the text (no
        // unbalanced/unclosed delimiter leaks a literal `**`/`*` into the content).
        prop_assert_eq!(&rt2.text, &rt.text, "overlap corrupted text: {:?}", md);

        // Distinct delimiters → an exact fixed point via close-and-reopen. Two
        // asterisk-family marks that still partially overlap after normalization
        // are unrepresentable, so only text-safety (asserted above) holds.
        let asterisk_clash = both_asterisk(&rt.marks) && rt.marks.len() == 2
            && partial_overlap(&rt.marks);
        if !asterisk_clash {
            prop_assert_eq!(&rt, &rt2, "distinct-delim overlap not a fixed point: {:?}", md);
        }
        // Whatever the shape, the re-imported content is itself a genuine fixed
        // point (it lives in the `from_markdown` domain the contract covers).
        prop_assert_eq!(&rt2, &from_markdown(&to_markdown(&rt2)).unwrap(),
            "re-imported overlap content not a fixed point: {:?}", md);
    }

    /// Editor marks over *mixed* text stay text-safe — the punctuation, symbol,
    /// emoji, and whitespace coverage the #848 staircase above deliberately
    /// excludes (its content is `[a-z]` only). An `apply_mark_ops` mark whose
    /// edge falls between a word char and a punctuation/symbol/whitespace char is
    /// not representable as a `*`/`**`/`~~` run under CommonMark flanking, so the
    /// codec clips the mark inward (or drops it) rather than leak a literal
    /// delimiter into the text. Formatting may be lost; the text never is. Guards
    /// the corruption `clip_unflankable` fixes: bolding `a.` used to export
    /// `**a.**b`, which re-imports as the literal string `**a.**b`.
    #[test]
    fn editor_marks_over_mixed_text_are_text_safe(
        mid in prop::collection::vec(
            prop::sample::select(vec![
                'a', 'b', '9', '你', '.', ',', '#', '_', '*', '~', '✓', '€', '😀', ' ',
            ]),
            2..10,
        ),
        specs in prop::collection::vec((0usize..64, 0usize..64, 0u8..4), 0..4),
    ) {
        // Word-char edges keep the mark-free baseline a round-trip fixed point
        // (markdown strips leading/trailing paragraph whitespace and reads a
        // leading `#`/`-`/`*` as a block marker); the punctuation/symbol/emoji/
        // space coverage lives in the interior, which is where marks clash with
        // CommonMark flanking anyway.
        let text: String = std::iter::once('a')
            .chain(mid)
            .chain(std::iter::once('a'))
            .collect();
        let n = text.chars().count();
        let mut rt = Content {
            text: text.clone(),
            lines: vec![Line { kind: LineKind::Para, containers: vec![], continues: false }],
            marks: vec![],
            islands: vec![],
        };
        rt.normalize();
        // Stay in the valid domain and only mark when the text length is intact
        // (normalization can reshape whitespace-only content).
        prop_assume!(rt.validate().is_ok());
        prop_assume!(rt.len_usv() == n);
        // Isolate the mark effect: require the *mark-free* text to already be a
        // round-trip fixed point, so a later mismatch is mark-induced, not the
        // orthogonal markdown limits (leading/trailing paragraph whitespace is
        // stripped on import, etc.).
        prop_assume!(from_markdown(&to_markdown(&rt)).unwrap().text == rt.text);

        let ops: Vec<MarkOp> = specs
            .iter()
            .map(|&(a, b, k)| {
                let (s, e) = (a % (n + 1), b % (n + 1));
                MarkOp::Add { start: s.min(e), end: s.max(e), kind: ov_kind(k) }
            })
            .filter(|op| matches!(op, MarkOp::Add { start, end, .. } if start < end))
            .collect();
        rt.apply_mark_ops(&ops).unwrap();
        prop_assert_eq!(rt.validate(), Ok(()), "editor marks left content invalid");

        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap();
        // The guarantee: no clipped/dropped mark leaks a delimiter into the text.
        // (Mark *fidelity* is not promised — an unrepresentable editor mark is
        // degraded away — only that the text survives. The import-domain
        // fixed-point contract is covered by `content_round_trip_and_invariants`.)
        prop_assert_eq!(&rt2.text, &rt.text,
            "editor mark corrupted text.\n text: {:?}\n md:   {:?}\n out:  {:?}",
            rt.text, md, rt2.text);
        // Text stays safe across a second export cycle too — a degraded content
        // never drifts the text further on re-save.
        prop_assert_eq!(&from_markdown(&to_markdown(&rt2)).unwrap().text, &rt.text,
            "text drifted on the second cycle: {:?}", md);
    }

    /// Issue #900: image alt and image/link URLs carry the markup- and
    /// destination-terminating specials — `]`/`[`/`\` in alt, spaces, unbalanced
    /// parens, `&`, `<`/`>`/`\` in a url — that the codec must escape so the
    /// island/link survives export∘import. The `clean_word` alt/url generator
    /// never emitted them (the gap the issue found); the content is built directly
    /// in the shape import produces (alt trimmed, no newline) to hit the escaper
    /// without fighting source-level markdown quirks.
    #[test]
    fn image_and_link_specials_round_trip(
        alt in r"[a-z0-9\]\[\\&<>*_~#().+ -]{0,10}",
        img_url in r"[a-z0-9 ()&<>\\]{0,10}",
        link_url in r"[a-z0-9 ()&<>\\]{0,10}",
    ) {
        // Import trims alt and never yields a leading/trailing-space alt; match
        // that so the hand-built content stays inside the fixed-point domain.
        let alt = alt.trim().to_string();
        let text = "lnk\u{FFFC}".to_string(); // link over "lnk", image slot after
        let mut rt = Content {
            text,
            lines: vec![Line { kind: LineKind::Para, containers: vec![], continues: false }],
            marks: vec![Mark { start: 0, end: 3, kind: MarkKind::Link { url: link_url } }],
            islands: vec![Island {
                // The id import mints for the first island, so re-import compares equal.
                id: "isl-0".into(),
                island_type: "image".into(),
                props: json!({ "alt": alt, "url": img_url }),
                loss: Loss::Lossless,
            }],
        };
        rt.normalize();
        prop_assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");
        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap();
        prop_assert_eq!(&rt, &rt2, "alt/url specials not a fixed point.\n  md: {:?}", md);
    }

    /// Property 2a: canonical JSON is a fixed point.
    #[test]
    fn canonical_json_fixed_point(md in document()) {
        let rt = from_markdown(&md).unwrap();
        let json = rt.to_canonical_json();
        let back = Content::from_canonical_json(&json).unwrap();
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

}

// ---------------------------------------------------------------------------
// Edit-channel invariant properties (issue #847): the three apply channels
// preserve `validate()`. The content arriving at a channel is valid; a
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
    /// The insert charset includes `\r` and bidi controls (U+202E, U+2069):
    /// `apply_text_delta` strips them, so the apply still leaves a valid content
    /// (issue #899 — before the fix these returned Ok over a broken invariant).
    /// U+FFFC (a raw slot) is excluded — that insert is rejected outright.
    #[test]
    fn apply_text_delta_preserves_validate(
        md in document(),
        ins in "[a-z0-9 \n\r\u{202E}\u{2069}]{0,10}",
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

    /// Property (issue #1039): an anchor's `id` is bit-invariant under a random
    /// splice + rebase. The mark may drop (its anchored text deleted) or move
    /// (its range rebases through `map_pos`), but any *surviving* anchor carries
    /// the exact id it started with — the runtime never rewrites an id. Import
    /// mints no anchor, so the one id seeded here is the only one that can appear;
    /// an astral char in it would expose any byte-level munging.
    #[test]
    fn anchor_id_bit_invariant_under_splice(
        md in document(),
        a_seed in 0usize..4096,
        b_seed in 0usize..4096,
        ins in "[a-z0-9 \n]{0,10}",
        pos_seed in 0usize..4096,
        del_seed in 0usize..4096,
        is_delete in any::<bool>(),
    ) {
        const ID: &str = "anchor-\u{1f4a1}-42";
        let mut rt = from_markdown(&md).unwrap();
        let len = rt.len_usv();
        let a = a_seed % (len + 1);
        let b = b_seed % (len + 1);
        rt.marks.push(Mark {
            start: a.min(b),
            end: a.max(b),
            kind: MarkKind::Anchor { id: ID.into() },
        });
        rt.normalize();
        prop_assert_eq!(rt.validate(), Ok(()), "seeded content invalid for {:?}", md);

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
            for m in &rt.marks {
                if let MarkKind::Anchor { id } = &m.kind {
                    prop_assert_eq!(id.as_str(), ID, "rebase rewrote an anchor id");
                }
            }
            prop_assert_eq!(rt.validate(), Ok(()), "splice broke an invariant");
        }
    }

    /// `apply_mark_ops` preserves `validate()`: an accepted Add over a clamped
    /// range leaves the content valid (normalization trims edges / drops
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

    /// `apply_line_ops` preserves `validate()` across an accepted
    /// split/join/set-kind — line/segment sync, island/slot sync, and (marks
    /// left in place) mark ranges. Split/join splice a `\n` and rebase marks
    /// through that one-char change (`Delta::map_pos` semantics), so the
    /// rebased marks must still normalize to a valid set; keeping the imported
    /// marks exercises that remap in the fuzzer. Splicing `\n` never adds or
    /// removes a slot, so island sync is a genuine post-condition too.
    #[test]
    fn apply_line_ops_preserves_validate(
        md in document(),
        pos_seed in 0usize..4096,
        line_seed in 0usize..64,
        which in 0u8..3,
    ) {
        let mut rt = from_markdown(&md).unwrap();
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
// Fixture content: the phase-1 codecs run against real fixture markdown bodies.
// ---------------------------------------------------------------------------

fn fixture_body(name: &str) -> String {
    let path = quillmark_fixtures::resource_path(name);
    std::fs::read_to_string(path).unwrap()
}

// ---------------------------------------------------------------------------
// Structured-table property (issue #880): an editor-built table island — one
// whose shape markdown import never produces (ragged rows, an empty/short
// header, an `aligns` array out of sync with the columns) — is a fixed point of
// export∘import *after normalization*, and normalization always yields a content
// that `validate()` accepts. Cell content is drawn from import-produced cells
// (so representability and canonical marks are guaranteed by construction);
// the generator's entropy is the table *shape* plus in-cell literal specials
// (`|`, backtick, backslash) and unicode, which the codec must escape.
// ---------------------------------------------------------------------------

/// A cell's markdown content: clean words, formatted spans, and words carrying
/// literal specials that must escape to survive a pipe cell and round-trip.
fn cell_token() -> impl Strategy<Value = String> {
    prop_oneof![
        clean_word(),
        clean_word().prop_map(|w| format!("{w}\\|{w}")), // literal pipe
        clean_word().prop_map(|w| format!("{w}\\`{w}")), // literal backtick
        clean_word().prop_map(|w| format!("{w}\\\\{w}")), // literal backslash
        clean_word().prop_map(|w| format!("{w}你{w}")),  // BMP unicode
        clean_word().prop_map(|w| format!("{w}😀")),     // astral unicode
        clean_word().prop_map(|w| format!("**{w}**")),
        clean_word().prop_map(|w| format!("*{w}*")),
        clean_word().prop_map(|w| format!("~~{w}~~")),
        clean_word().prop_map(|w| format!("`{w}`")),
        (clean_word(), clean_word()).prop_map(|(t, u)| format!("[{t}](https://ex.com/{u})")),
    ]
}

fn cell_content() -> impl Strategy<Value = String> {
    prop::collection::vec(cell_token(), 1..3).prop_map(|toks| toks.join(" "))
}

fn alignment() -> impl Strategy<Value = &'static str> {
    prop_oneof![Just("none"), Just("left"), Just("center"), Just("right")]
}

/// The canonical `{text, marks}` cells for a row of markdown contents, obtained
/// by importing a header-only table — so each cell is representable and its
/// marks are canonical by construction. `contents` must be non-empty.
fn import_row(contents: &[String]) -> Vec<Value> {
    let cols = contents.len();
    let header = contents.join(" | ");
    let delim = vec!["---"; cols].join(" | ");
    let body = vec!["x"; cols].join(" | ");
    let md = format!("| {header} |\n| {delim} |\n| {body} |");
    let rt = from_markdown(&md).unwrap();
    rt.islands[0].props["header"].as_array().unwrap().clone()
}

/// A single-table content with the given (possibly ill-shaped) props. `id` is the
/// first-island id import mints (`isl-0`), so a re-imported table compares equal.
fn table_content(aligns: Vec<&str>, header: Vec<Value>, rows: Vec<Vec<Value>>) -> Content {
    Content {
        text: "\u{FFFC}".into(),
        lines: vec![Line {
            kind: LineKind::Island,
            containers: vec![],
            continues: false,
        }],
        marks: vec![],
        islands: vec![Island {
            id: "isl-0".into(),
            island_type: "table".into(),
            props: json!({ "aligns": aligns, "header": header, "rows": rows }),
            loss: Loss::Lossless,
        }],
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// A structurally ill-shaped table island normalizes to a valid content and
    /// is a fixed point of export∘import.
    #[test]
    fn table_island_normalizes_and_round_trips(
        header in prop::collection::vec(cell_content(), 0..4),
        rows in prop::collection::vec(prop::collection::vec(cell_content(), 0..4), 0..4),
        aligns in prop::collection::vec(alignment(), 0..5),
    ) {
        let header_cells = if header.is_empty() { vec![] } else { import_row(&header) };
        let row_cells: Vec<Vec<Value>> = rows
            .iter()
            .map(|r| if r.is_empty() { vec![] } else { import_row(r) })
            .collect();

        // The widest column count drives normalization. A fully empty table (all
        // widths zero) has no markdown projection — export drops it, so it cannot
        // round-trip; skip it (a contentless table is out of the fixed-point
        // contract, like the documented hard-break limits).
        let cols = header_cells.len()
            .max(aligns.len())
            .max(row_cells.iter().map(Vec::len).max().unwrap_or(0));
        prop_assume!(cols >= 1);

        let mut rt = table_content(aligns.clone(), header_cells, row_cells);
        rt.normalize();
        prop_assert_eq!(rt.validate(), Ok(()), "normalized table invalid");

        // Post-normalize shape: one column count across header, aligns, rows.
        let props = &rt.islands[0].props;
        prop_assert_eq!(props["header"].as_array().unwrap().len(), cols);
        prop_assert_eq!(props["aligns"].as_array().unwrap().len(), cols);
        for row in props["rows"].as_array().unwrap() {
            prop_assert_eq!(row.as_array().unwrap().len(), cols);
        }

        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap();
        prop_assert_eq!(&rt, &rt2, "table not a fixed point.\n  md: {:?}", md);
    }
}

#[test]
fn fixture_bodies_import_and_are_valid() {
    // Every prose resource imports to a valid content and is a fixed point.
    for name in [
        "sample.md",
        "card_yaml_demo.md",
        "extended_metadata_demo.md",
    ] {
        let md = fixture_body(name);
        let rt = from_markdown(&md).unwrap_or_else(|e| panic!("import {name}: {e}"));
        assert_eq!(rt.validate(), Ok(()), "{name} invariants");
        let rt2 = from_markdown(&to_markdown(&rt)).unwrap();
        assert_eq!(rt, rt2, "{name} content not a fixed point");
    }
}
