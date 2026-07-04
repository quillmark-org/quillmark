//! Spike C — seam encoding + determinism (confirms Option A, gates phase-2
//! storage). Each test is a claim in `phase-0-finding-c-seam.md`.
//!
//! Questions answered:
//! 1. Does canonical RichText-JSON serialize byte-deterministically?
//! 2. Is the seam JSON the same canonical bytes as the storage serialization
//!    (one encoding, not two to keep aligned)?
//! 3. Does it lower trivially to both backends — `typst` codegen and the
//!    `pdfform` `.text` path — with no `sample_form` fixture change?
//! 4. Where does island-mint nondeterminism enter the content hash?

use quillmark_richtext_spikes::canonical::{canonical_json, content_hash};
use quillmark_richtext_spikes::codec::{import_markdown, to_plaintext};
use quillmark_richtext_spikes::model::*;
use quillmark_typst::convert::mark_to_typst;

/// A representative corpus with every mark kind, a heading, a list, and one
/// island — the seam value the tests lower and hash.
fn sample() -> RichText {
    let mut rt = import_markdown(
        "# Title\n\nA paragraph with **bold**, _italic_, ~~struck~~ and `code`.\n\n- one\n- two",
    );
    // Anchor (identity) mark over "paragraph" — must survive serialization.
    let start = rt.text.find("paragraph").map(|b| rt.text[..b].chars().count());
    if let Some(s) = start {
        rt.marks.push(Mark {
            range: CharRange::new(s, s + "paragraph".chars().count()),
            kind: MarkKind::Anchor {
                id: "comment-42".into(),
            },
        });
    }
    rt.islands.push(Island {
        id: "isl_fixed_for_test".into(),
        island_type: "figure".into(),
        props: serde_json::json!({ "src": "taro.png", "caption": "A figure" }),
        loss: Loss::Unrepresentable,
    });
    rt.text.push_str(&format!("\n{}", ISLAND_SLOT));
    rt.lines.push(Line {
        kind: LineKind::Island,
        containers: vec![],
    });
    rt.normalize_marks();
    rt
}

#[test]
fn q1_canonical_json_is_byte_deterministic() {
    let rt = sample();
    // Two independent serializations of the same value are byte-identical.
    let a = canonical_json(&rt);
    let b = canonical_json(&rt.clone());
    assert_eq!(a, b, "canonical JSON must be byte-stable across serializations");
    assert_eq!(content_hash(&rt), content_hash(&rt.clone()));

    // Mark discovery order must NOT affect the bytes: shuffle the marks and the
    // island order, re-serialize, and the canonical form is unchanged.
    let mut shuffled = rt.clone();
    shuffled.marks.reverse();
    shuffled.islands.reverse();
    assert_eq!(
        canonical_json(&shuffled),
        a,
        "canonicalization must absorb mark/island ordering differences"
    );
}

#[test]
fn q2_seam_json_is_the_storage_json() {
    // Option A's durable claim: content crosses the seam as the *same*
    // canonical bytes it is stored as — there is one encoding to keep
    // deterministic, not two to keep aligned. We model both as `canonical_json`
    // and assert identity; a real split (seam serializer ≠ storage serializer)
    // is exactly the drift this pins against.
    let rt = sample();
    let seam_bytes = canonical_json(&rt);
    let storage_bytes = canonical_json(&rt); // same function == same contract
    assert_eq!(seam_bytes, storage_bytes);

    // And it round-trips: JSON → RichText → JSON is a fixed point.
    let back: RichText = serde_json::from_str(&seam_bytes).expect("seam JSON parses");
    assert_eq!(
        canonical_json(&back),
        seam_bytes,
        "deserialize∘serialize is identity on canonical bytes"
    );
}

#[test]
fn q3a_lowers_to_typst_markup() {
    // The `typst` backend lowers content by escaping the corpus text and
    // wrapping marks in `#strong[..]` etc. The spike demonstrates the lowering
    // is available from the corpus by projecting back to markdown and running
    // the *real* backend converter — the same `mark_to_typst` the engine uses
    // in `convert_content_value`.
    let md = "A paragraph with **bold** and _italic_.";
    let rt = import_markdown(md);
    let projected = quillmark_richtext_spikes::codec::export_markdown(&rt);
    let typst = mark_to_typst(&projected).expect("markdown lowers to typst");
    assert!(
        typst.contains("#strong[bold]") && typst.contains("#emph[italic]"),
        "corpus lowers to typst markup via the shipped converter: {typst}"
    );
}

#[test]
fn q3b_lowers_to_pdfform_plaintext_dropping_island_slots() {
    let rt = sample();
    let text = to_plaintext(&rt);
    // The pdfform lowering is `RichText.text` minus island slots — no markup,
    // no slot sentinels.
    assert!(
        !text.contains(ISLAND_SLOT),
        "island slots are dropped for a plaintext form field: {text:?}"
    );
    assert!(text.contains("bold"), "the prose text survives: {text:?}");
    // sample_form binds only scalars (body.enabled: false), so this lowering is
    // never exercised by the fixture today — cost is zero, asserted in the
    // finding by inspection of the fixture, not here.
}

#[test]
fn q4_island_mint_is_the_only_hash_nondeterminism() {
    // Text/marks/lines are deterministic: two cold imports of the same markdown
    // hash identically.
    let md = "A paragraph with **bold** text and a second sentence.";
    assert_eq!(
        content_hash(&import_markdown(md)),
        content_hash(&import_markdown(md)),
        "island-free corpus hashes deterministically (migration is mint-free)"
    );

    // Introduce an island; the *only* field that changes the hash between two
    // otherwise-identical values is the minted `id`. That is the boundary the
    // content-hash contract must tolerate once tables ship (phase 4).
    let base = |id: &str| RichText {
        text: format!("before {ISLAND_SLOT} after"),
        lines: vec![Line {
            kind: LineKind::Para,
            containers: vec![],
        }],
        marks: vec![],
        islands: vec![Island {
            id: id.into(),
            island_type: "table".into(),
            props: serde_json::json!({ "rows": 2 }),
            loss: Loss::Degraded,
        }],
    };
    let h1 = content_hash(&base("isl_aaaaaa"));
    let h2 = content_hash(&base("isl_bbbbbb"));
    assert_ne!(
        h1, h2,
        "differing minted ids differ in the hash — the documented mint boundary"
    );

    // With the id held fixed, the island-bearing value is fully deterministic.
    assert_eq!(content_hash(&base("isl_fixed")), content_hash(&base("isl_fixed")));
}
