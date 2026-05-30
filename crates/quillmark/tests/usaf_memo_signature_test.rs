//! Regression test for the USAF memo signature-field injection.
//!
//! `usaf_memo`'s plate overlays an unsigned AcroForm signature widget directly
//! above the typed-name signature block, which AFH 33-337 places 4.5 inches
//! (324pt) from the left edge of the page. An earlier version of the plate
//! emitted the field *in flow* at the left margin (72pt), so the clickable
//! "Sign Here" box landed ~3.5in to the left of the signature line and pushed
//! the typed name down by the box's height. This test renders the real plate
//! end-to-end and asserts the widget sits at the 4.5in block, not the margin.

#![cfg(feature = "typst")]

use quillmark::{Document, FillBehavior, OutputFormat, Quillmark, RenderError, RenderOptions};
use quillmark_fixtures::quills_path;

const PT_PER_IN: f32 = 72.0;
const SIG_BLOCK_LEFT_IN: f32 = 4.5;

/// Pull the first `/FT /Sig` widget's `/Rect` out of a PDF as `[x0, y0, x1, y1]`
/// in points. Byte-level scan — good enough for a single uncompressed widget
/// dict emitted by the overlay pass.
fn signature_widget_rect(pdf: &[u8]) -> Option<[f32; 4]> {
    let sig_at = find(pdf, b"/FT /Sig")?;
    let rect_at = sig_at + find(&pdf[sig_at..], b"/Rect")?;
    let open = rect_at + find(&pdf[rect_at..], b"[")? + 1;
    let close = open + find(&pdf[open..], b"]")?;
    let body = std::str::from_utf8(&pdf[open..close]).ok()?;
    let nums: Vec<f32> = body
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    match nums.as_slice() {
        [x0, y0, x1, y1] => Some([*x0, *y0, *x1, *y1]),
        _ => None,
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

#[test]
fn usaf_memo_signature_widget_aligns_with_signature_block() {
    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(quills_path("usaf_memo"))
        .expect("usaf_memo should load");

    let markdown = quill
        .source()
        .config()
        .blueprint_filled(FillBehavior::Preview);
    let parsed = Document::from_markdown(&markdown).expect("blueprint should parse");

    let result = quill.render(
        &parsed,
        &RenderOptions {
            output_format: Some(OutputFormat::Pdf),
            ..Default::default()
        },
    );

    // Font-less CI cannot exercise the renderer; skip rather than fail, matching
    // the convention in quiver_test.rs.
    if let Err(RenderError::EngineCreation { diags }) = &result {
        if diags[0].message.contains("No fonts found") {
            eprintln!("skipping — no fonts available");
            return;
        }
    }

    let rendered = result.expect("usaf_memo should render to PDF");
    let pdf = &rendered.artifacts[0].bytes;

    assert!(
        find(pdf, b"/AcroForm").is_some(),
        "rendered memo should carry an AcroForm with the signature widget"
    );

    let [x0, _y0, x1, _y1] =
        signature_widget_rect(pdf).expect("PDF should contain a /FT /Sig widget /Rect");

    let left_in = x0 / PT_PER_IN;
    // The widget must align with the 4.5in signature block, not the 1in left
    // margin. Allow a small tolerance for rounding in the layout pass.
    assert!(
        (left_in - SIG_BLOCK_LEFT_IN).abs() < 0.1,
        "signature widget left edge should sit at the {SIG_BLOCK_LEFT_IN}in \
         signature block, but was at {left_in:.2}in (rect x0={x0}pt). A value \
         near 1.0in means the field regressed to the left margin."
    );
    assert!(
        x1 > x0,
        "widget rect should have positive width, got x0={x0} x1={x1}"
    );
}
