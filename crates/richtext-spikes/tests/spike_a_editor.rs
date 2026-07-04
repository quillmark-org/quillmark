//! Spike A — editor binding / mark semantics + annotation rebase. Each test is
//! a claim in `phase-0-finding-a-editor.md`.
//!
//! The JS editors (ProseMirror/Lexical/Quill/CodeMirror) can't run in a Rust
//! test, so this spike encodes the *model-side* contract those editors must
//! agree with — the normalization the canonical serialization commits to — and
//! runs the genuinely novel, project-specific risk end to end: the stale-text
//! writer's cold-parse + corpus-diff annotation rebase, including the paragraph
//! reorder where the move weak spot bites. The editor-behavior half of the
//! finding (edge-expand, adjacent-merge) is documented from the editors' known
//! semantics; this file pins what the *model* does.

use quillmark_richtext_spikes::codec::{export_markdown, import_markdown};
use quillmark_richtext_spikes::diff::*;
use quillmark_richtext_spikes::model::*;
use quillmark_richtext_spikes::usv;

fn anchor_over(rt: &mut RichText, needle: &str, id: &str) -> CharRange {
    let b = rt.text.find(needle).expect("needle present");
    let start = rt.text[..b].chars().count();
    let range = CharRange::new(start, start + needle.chars().count());
    rt.marks.push(Mark {
        range,
        kind: MarkKind::Anchor { id: id.into() },
    });
    range
}

// ── Normalization the serialization commits to ──────────────────────────────

#[test]
fn same_kind_adjacent_and_overlapping_formatting_marks_union() {
    let mut rt = RichText {
        text: "abcdef".into(),
        lines: vec![Line { kind: LineKind::Para, containers: vec![] }],
        marks: vec![
            Mark { range: CharRange::new(0, 3), kind: MarkKind::Strong }, // abc
            Mark { range: CharRange::new(3, 6), kind: MarkKind::Strong }, // def (adjacent)
            Mark { range: CharRange::new(1, 4), kind: MarkKind::Strong }, // overlapping
        ],
        islands: vec![],
    };
    rt.normalize_marks();
    let strong: Vec<_> = rt.marks.iter().filter(|m| m.kind == MarkKind::Strong).collect();
    assert_eq!(strong.len(), 1, "same-kind marks union: {:?}", rt.marks);
    assert_eq!(strong[0].range, CharRange::new(0, 6));
}

#[test]
fn different_kind_marks_freely_overlap() {
    let mut rt = RichText {
        text: "abcdef".into(),
        lines: vec![Line { kind: LineKind::Para, containers: vec![] }],
        marks: vec![
            Mark { range: CharRange::new(0, 4), kind: MarkKind::Strong },
            Mark { range: CharRange::new(2, 6), kind: MarkKind::Emph },
        ],
        islands: vec![],
    };
    rt.normalize_marks();
    assert_eq!(rt.marks.len(), 2, "cross-kind overlap is preserved, not split");
}

#[test]
fn identity_marks_never_merge() {
    // Two comments over the same text are two comments — an anchor is identity,
    // not formatting, so normalization must not union them.
    let mut rt = RichText {
        text: "abcdef".into(),
        lines: vec![Line { kind: LineKind::Para, containers: vec![] }],
        marks: vec![
            Mark { range: CharRange::new(0, 6), kind: MarkKind::Anchor { id: "c1".into() } },
            Mark { range: CharRange::new(0, 6), kind: MarkKind::Anchor { id: "c2".into() } },
        ],
        islands: vec![],
    };
    rt.normalize_marks();
    assert_eq!(rt.marks.len(), 2, "distinct anchors stay distinct");
}

// ── Split / join carry marks (the most common prose edit) ───────────────────

#[test]
fn splitting_a_paragraph_mid_mark_is_one_insert_and_marks_ride_it() {
    // Base: one paragraph, bold over "quick brown". Split between the words by
    // inserting a newline — a single-char insert. Marks rebase by position; the
    // bold survives across the new line boundary (the derived line tree splits,
    // the mark does not).
    let base = "the quick brown fox";
    let rt = import_markdown("the **quick brown** fox");
    assert!(rt.marks.iter().any(|m| m.kind == MarkKind::Strong));
    let strong = rt.marks.iter().find(|m| m.kind == MarkKind::Strong).unwrap().range;

    // Insert '\n' after "quick" (char index 9 in "the quick brown fox").
    let split_at = base.find("quick brown").unwrap() + "quick".len(); // byte 9
    let split_char = base[..split_at].chars().count();
    let ops = vec![Op::Retain(split_char), Op::Insert("\n".into()), Op::Retain(rt.char_len() - split_char)];

    let (new_range, fate) = rebase_range(&ops, strong);
    assert_eq!(fate, RebaseFate::Kept, "a split touches no text, so the mark is kept");
    assert_eq!(new_range.len(), strong.len() + 1, "the mark now spans the inserted boundary");
}

#[test]
fn a_zero_width_anchor_survives_a_split_at_its_point() {
    // Anchor pinned between "quick" and " brown". A split at that exact point:
    // the anchor maps to the boundary and survives (After bias keeps it on the
    // following text).
    let mut rt = import_markdown("the quick brown fox");
    let anchor = anchor_over(&mut rt, "brown", "note");
    let point = CharRange::new(anchor.start, anchor.start); // zero-width at "brown"
    let ops = vec![Op::Retain(anchor.start), Op::Insert("\n".into()), Op::Retain(rt.char_len() - anchor.start)];
    let (mapped, fate) = rebase_range(&ops, point);
    assert_eq!(fate, RebaseFate::Kept);
    assert!(mapped.is_empty(), "an anchor stays zero-width");
}

// ── Stale-text writer: cold parse + corpus diff → delta → rebase ────────────

#[test]
fn llm_rewrite_around_an_annotation_rebases_it() {
    // Base document with a comment anchored on "lethality". An LLM rewrites the
    // surrounding prose but keeps that word. Cold-parse the rewrite, diff, and
    // the anchor rebases onto the same word — no preservation contract on the
    // LLM.
    let base_md = "The unit improved its lethality during the exercise.";
    let mut base = import_markdown(base_md);
    let anchor = anchor_over(&mut base, "lethality", "comment-7");

    let rewrite_md = "During the exercise, the unit measurably improved its lethality.";
    let target = import_markdown(rewrite_md);

    let ops = diff_chars(&base.text, &target.text);
    let (mapped, fate) = rebase_range(&ops, anchor);
    assert!(
        matches!(fate, RebaseFate::Kept | RebaseFate::Shrunk),
        "the anchored word survived the rewrite: {fate:?}"
    );
    let mapped_text: String = target.text.chars().skip(mapped.start).take(mapped.len()).collect();
    assert_eq!(mapped_text, "lethality", "the anchor lands back on its word");
}

/// The canonical "move block2 to the front" delta for a two-paragraph corpus
/// `"{p1}\n{p2}"` → `"{p2}\n{p1}"`. This is what a reorder *is* to any
/// character differ: the moved block deleted from one place and inserted in
/// another. We construct it explicitly so the demonstration does not hinge on
/// which of two similar blocks an LCS tie-break happens to retain.
fn reorder_delta(p1: &str, p2: &str) -> Vec<Op> {
    vec![
        Op::Insert(format!("{p2}\n")),
        Op::Retain(p1.chars().count()),
        Op::Delete(format!("\n{p2}")),
    ]
}

#[test]
fn a_reorder_shows_up_as_delete_plus_insert_in_the_char_diff() {
    // Ground the premise: the char differ genuinely expresses a paragraph swap
    // as deletes + inserts (never a "move" op it doesn't have).
    let base = import_markdown("Alpha states the plan.\n\nBravo states the risk.");
    let target = import_markdown("Bravo states the risk.\n\nAlpha states the plan.");
    let ops = diff_chars(&base.text, &target.text);
    assert!(
        ops.iter().any(|o| matches!(o, Op::Delete(_)))
            && ops.iter().any(|o| matches!(o, Op::Insert(_))),
        "a reorder is delete+insert to the differ: {ops:?}"
    );
}

#[test]
fn paragraph_reorder_detaches_a_naive_anchor() {
    // THE weak spot. A comment anchored in the second paragraph; the paragraphs
    // are swapped. A naive position rebase collapses the anchor to the
    // deletion point — detached from the text it annotated.
    let (p1, p2) = ("Alpha states the plan.", "Bravo states the risk.");
    let mut base = import_markdown(&format!("{p1}\n\n{p2}"));
    let anchor = anchor_over(&mut base, "risk", "comment-9");
    let target = import_markdown(&format!("{p2}\n\n{p1}"));
    let ops = reorder_delta(p1, p2);

    let (naive, fate) = rebase_range(&ops, CharRange::new(anchor.start, anchor.start));
    assert_eq!(fate, RebaseFate::AnchorDetached, "naive rebase detaches on a move");
    let naive_word: String = target.text.chars().skip(naive.start).take(4).collect();
    assert_ne!(
        naive_word, "risk",
        "naive rebase does NOT land on the moved word: {naive_word:?}"
    );
}

#[test]
fn move_detection_re_homes_the_annotation_onto_the_moved_text() {
    // Same reorder, but with move detection: the anchor follows the moved block
    // onto "risk" in its new location. This confines the weak spot.
    let (p1, p2) = ("Alpha states the plan.", "Bravo states the risk.");
    let mut base = import_markdown(&format!("{p1}\n\n{p2}"));
    let anchor = anchor_over(&mut base, "risk", "comment-9");
    let target = import_markdown(&format!("{p2}\n\n{p1}"));
    let ops = reorder_delta(p1, p2);

    let mv = detect_move(&ops).expect("the swap is detected as a move");
    assert!(
        mv.len >= p2.chars().count() - 1,
        "the moved block is the whole paragraph: {mv:?}"
    );

    let rehomed = rebase_anchor_move_aware(&ops, anchor.start);
    let rehomed_word: String = target.text.chars().skip(rehomed).take(4).collect();
    assert_eq!(rehomed_word, "risk", "move-aware rebase lands on the moved word");
}

// ── Corpus round-trips through the markdown projection ───────────────────────

#[test]
fn corpus_is_stable_through_the_markdown_projection() {
    // The honest round-trip: the corpus is canonical, markdown is a projection.
    // import→export→import is a fixed point for the round-trippable subset.
    for md in [
        "A simple paragraph.",
        "# A heading\n\nAnd a paragraph with **bold** and _italic_ words.",
        "First para.\n\nSecond para with ~~strike~~ and `code`.",
        "- one\n- two\n- three",
    ] {
        let once = import_markdown(md);
        let twice = import_markdown(&export_markdown(&once));
        assert_eq!(
            once.text, twice.text,
            "corpus text is stable through projection for {md:?}"
        );
        assert_eq!(
            once.marks, twice.marks,
            "marks are stable through projection for {md:?}"
        );
    }
}

// ── The USV cross-binding tax ────────────────────────────────────────────────

#[test]
fn usv_offsets_convert_correctly_across_astral_characters() {
    // "a😀b" — the emoji is 1 USV, but 4 UTF-8 bytes and 2 UTF-16 units. A mark
    // after it must be at USV 2, byte 5, UTF-16 3. Getting this wrong (naive
    // .len()) corrupts every downstream offset — the property suite owns it.
    let s = "a😀b";
    assert_eq!(s.chars().count(), 3);
    // USV 2 (the 'b') ↔ byte 5 ↔ UTF-16 3.
    assert_eq!(usv::usv_to_byte(s, 2), 5);
    assert_eq!(usv::byte_to_usv(s, 5), 2);
    assert_eq!(usv::usv_to_utf16(s, 2), 3);
    assert_eq!(usv::utf16_to_usv(s, 3), 2);
    // A UTF-16 index landing mid-surrogate (inside the emoji) rounds down to the
    // char that owns it — the emoji at USV 1.
    assert_eq!(usv::utf16_to_usv(s, 2), 1);
}
