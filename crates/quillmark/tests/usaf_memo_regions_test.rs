//! Region coverage on the flagship `usaf_memo` quill.
//!
//! The memo package's `render-body` rebuilds body paragraphs through a state
//! buffer (AFH 33-337 auto-numbering) — the hardest placement context a
//! shipped quill exercises. Span tracking rides the rebuilt glyphs' own
//! origins, so the main `$body` and each indorsement card's body stay
//! addressable with no recovery step in the plate; the signature widgets bind
//! schema paths explicitly. This renders the real plate end-to-end and pins
//! that coverage, plus the one-shot regions sidecar and the forward
//! `field_at` direction through the rebuild.

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

    let mut session = match engine.open(&quill, &parsed) {
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

    // The main body regions *through* the package's paragraph rebuild — the
    // case that was inert under value-level marker tagging — and the widgets
    // key on their bound schema paths. The seeded indorsement body is empty
    // (`""`), and a blank field has no inked extent to bound, so its `$body`
    // must be absent here, not present-and-empty.
    for expected in [
        "$body",
        "signature_block",
        "$cards.indorsement.0.signature_block",
    ] {
        assert!(
            fields.contains(expected),
            "expected a region keyed {expected:?}; got {fields:?}"
        );
    }
    assert!(
        !fields.contains("$cards.indorsement.0.$body"),
        "an empty card body draws nothing and surfaces no region: {fields:?}"
    );

    // The forward direction survives the rebuild too: a point inside the
    // surfaced `$body` region resolves back to `$body`.
    let body = regions
        .iter()
        .find(|r| r.field == "$body")
        .expect("$body region present");
    let cx = (body.rect[0] + body.rect[2]) / 2.0;
    let cy = (body.rect[1] + body.rect[3]) / 2.0;
    assert_eq!(
        session.field_at(body.page, cx, cy).as_deref(),
        Some("$body"),
        "a click inside the rebuilt body routes to $body"
    );

    // Give the indorsement a real body via apply — the per-field eval windows
    // regenerate with the helper on every committed edit — and its card
    // address surfaces through the same package rebuild.
    let mut edited = quill.compile_data(&parsed).expect("compile seed data");
    edited["$cards"][0]["$body"] =
        serde_json::json!("The indorsement **body**, rebuilt by render-body.");
    session.apply(&edited).expect("apply edited card body");
    let fields: HashSet<String> = session
        .regions()
        .into_iter()
        .map(|r| r.field)
        .collect();
    assert!(
        fields.contains("$cards.indorsement.0.$body"),
        "a non-empty card body regions through the rebuild: {fields:?}"
    );

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
