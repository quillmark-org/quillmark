//! Region coverage on the flagship `usaf_memo` quill.
//!
//! The memo package's `render-body` rebuilds body paragraphs through a state
//! buffer (AFH 33-337 auto-numbering), which drops value-level auto-tag
//! markers, so the plate brackets the package's *output* with `tagged(..)`
//! and binds the signature widgets to schema paths — keeping `$body` and
//! `signature_block` addressable for cross-navigation across the shipped
//! catalog. This renders the real plate end-to-end and pins that coverage,
//! plus the one-shot regions sidecar.

#![cfg(feature = "typst")]

use std::collections::HashSet;

use quillmark::{OutputFormat, Quillmark, RenderOptions};
use quillmark_fixtures::quills_path;

#[test]
fn usaf_memo_regions_cover_body_signature_and_cards() {
    let engine = Quillmark::new();
    let quill =
        quillmark::quill_from_path(quills_path("usaf_memo")).expect("usaf_memo should load");

    // The seeded document exercises the main memo and one card per declared
    // kind, so the indorsement addresses are present.
    let parsed = quill.seed_document();

    let session = match engine.open(&quill, &parsed) {
        // Font-less CI cannot exercise the renderer; skip rather than fail,
        // matching the convention in quiver_test.rs.
        Err(e) if e.diagnostics()[0].message.contains("No fonts found") => {
            eprintln!("skipping — no fonts available");
            return;
        }
        other => other.expect("usaf_memo should open a session"),
    };

    let regions = session.regions();
    let fields: HashSet<&str> = regions.iter().map(|r| r.field.as_str()).collect();

    // The body regions *through* the package's paragraph rebuild — the case
    // that was inert under value-level auto-tagging alone.
    for expected in [
        "$body",
        "signature_block",
        "$cards.indorsement.0.$body",
        "$cards.indorsement.0.signature_block",
    ] {
        assert!(
            fields.contains(expected),
            "expected a region keyed {expected:?}; got {fields:?}"
        );
    }

    // The one-shot sidecar serves the same geometry without a session in the
    // consumer's hands, and stays empty unless requested.
    let with_regions = engine
        .render(
            &quill,
            &parsed,
            &RenderOptions {
                output_format: Some(OutputFormat::Pdf),
                regions: true,
                ..Default::default()
            },
        )
        .expect("usaf_memo should render to PDF");
    assert_eq!(
        with_regions.regions, regions,
        "one-shot sidecar matches the session query"
    );

    let without_regions = engine
        .render(
            &quill,
            &parsed,
            &RenderOptions {
                output_format: Some(OutputFormat::Pdf),
                ..Default::default()
            },
        )
        .expect("usaf_memo should render to PDF");
    assert!(
        without_regions.regions.is_empty(),
        "the sidecar is opt-in; exports carry no regions by default"
    );
}
