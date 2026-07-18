//! Emit round-trip fuzz target.
//!
//! Property: for any input that `Document::parse` accepts, the
//! emission and re-parse chain must be stable:
//!
//! ```text
//! parse(src)      → doc_a
//! emit(doc_a)     → emit1
//! parse(emit1)    → doc_b
//! assert doc_a == doc_b
//! assert emit(doc_b) == emit1   ← idempotence on the canonical form
//! ```
//!
//! If the first parse fails, the input is discarded (invalid inputs are fine).
//! Any panic in the emitter or the second parse is a bug.
//!
//! ## Running with cargo-fuzz (if installed)
//!
//! ```sh
//! # cargo-fuzz target wiring (add to Cargo.toml [[bin]] if cargo-fuzz installed):
//! # cargo fuzz run emit_roundtrip_fuzz -- -max_total_time=300
//! ```
//!
//! The proptest variant below runs as part of `cargo test -p quillmark-fuzz`.

use proptest::prelude::*;
use quillmark_core::Document;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Arbitrary printable-Unicode input: parse→emit→re-parse must be stable.
    #[test]
    fn fuzz_emit_roundtrip_arbitrary(s in "\\PC{0,500}") {
        let doc_a = match Document::parse(&s) {
            Ok(d) => d.document,
            Err(_) => return Ok(()), // invalid input — discard
        };

        let emit1 = doc_a.to_markdown();

        let doc_b = Document::parse(&emit1).unwrap_or_else(|e| {
            panic!(
                "emit_roundtrip: re-parse of emitted document failed.\nError: {}\nInput: {:.200}\nEmitted:\n{}",
                e, s, emit1
            )
        })
        .document;

        prop_assert_eq!(
            &doc_a,
            &doc_b,
            "emit_roundtrip: doc_a != doc_b after parse→emit→re-parse.\nEmitted:\n{}",
            emit1
        );

        let emit2 = doc_b.to_markdown();
        prop_assert_eq!(
            &emit1,
            &emit2,
            "emit_roundtrip: emit1 != emit2 (not idempotent on canonical form).\nInput: {:.200}",
            s
        );
    }

    /// Inputs that look like a valid card-yaml block with a `$quill:` metadata key.
    #[test]
    fn fuzz_emit_roundtrip_payload_shaped(
        quill in "[a-z][a-z0-9_]{0,20}",
        key in "[a-z][a-z0-9_]{0,15}",
        value in "\\PC{0,100}"
    ) {
        // Build a minimal Quillmark document.
        let src = format!("~~~card-yaml\n$quill: {}\n$kind: main\n{}: \"{}\"\n~~~\n\nBody.\n",
            quill, key, value.replace('\\', "\\\\").replace('"', "\\\""));

        let doc_a = match Document::parse(&src) {
            Ok(d) => d.document,
            Err(_) => return Ok(()),
        };

        let emit1 = doc_a.to_markdown();

        let doc_b = Document::parse(&emit1).unwrap_or_else(|e| {
            panic!(
                "fuzz payload-shaped: re-parse failed.\nError: {}\nSrc:\n{}\nEmitted:\n{}",
                e, src, emit1
            )
        })
        .document;

        prop_assert_eq!(
            &doc_a,
            &doc_b,
            "fuzz payload-shaped: doc_a != doc_b.\nEmitted:\n{}",
            emit1
        );

        let emit2 = doc_b.to_markdown();
        prop_assert_eq!(
            &emit1,
            &emit2,
            "fuzz payload-shaped: emit not idempotent."
        );
    }

    /// Documents with card blocks.
    #[test]
    fn fuzz_emit_roundtrip_with_cards(
        quill in "[a-z][a-z0-9_]{0,20}",
        card_kind in "[a-z][a-z0-9_]{0,15}",
        card_key in "[a-z][a-z0-9_]{0,15}",
        card_value in "[a-zA-Z0-9 ]{0,50}"
    ) {
        let src = format!(
            "~~~card-yaml\n$quill: {}\n$kind: main\ntitle: \"test\"\n~~~\n\nBody here.\n\n~~~card-yaml\n$kind: {}\n{}: \"{}\"\n~~~\n\nCard body.\n",
            quill, card_kind, card_key, card_value
        );

        let doc_a = match Document::parse(&src) {
            Ok(d) => d.document,
            Err(_) => return Ok(()),
        };

        let emit1 = doc_a.to_markdown();

        let doc_b = Document::parse(&emit1).unwrap_or_else(|e| {
            panic!(
                "fuzz with-cards: re-parse failed.\nError: {}\nEmitted:\n{}",
                e, emit1
            )
        })
        .document;

        prop_assert_eq!(&doc_a, &doc_b);

        let emit2 = doc_b.to_markdown();
        prop_assert_eq!(&emit1, &emit2);
    }
}
