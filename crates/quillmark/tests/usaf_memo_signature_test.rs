//! Regression test for the USAF memo signature-field injection.
//!
//! `usaf_memo`'s plate threads an unsigned AcroForm signature widget into the
//! signature block via the package's `signing_field` parameter. AFH 33-337
//! places the signature block 4.5 inches (324pt) from the left edge of the
//! page, and the package overlays the widget there (offset up into the four
//! blank lines above the typed name) so it consumes no flow. This test renders
//! the real plate end-to-end and asserts every signature widget sits at the
//! 4.5in block rather than regressing to the 1in left margin.

#![cfg(feature = "typst")]

use quillmark::{OutputFormat, Quillmark, RenderOptions};
use quillmark_fixtures::quills_path;

const PT_PER_IN: f32 = 72.0;
const SIG_BLOCK_LEFT_IN: f32 = 4.5;

/// Pull every `/FT /Sig` widget's `/Rect` out of a PDF as `[x0, y0, x1, y1]` in
/// points. Byte-level scan — good enough for the uncompressed widget dicts the
/// overlay pass appends.
fn signature_widget_rects(pdf: &[u8]) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let mut cursor = 0;
    while let Some(off) = find(&pdf[cursor..], b"/FT /Sig") {
        let sig_at = cursor + off;
        if let Some(rect) = rect_after(pdf, sig_at) {
            rects.push(rect);
        }
        cursor = sig_at + b"/FT /Sig".len();
    }
    rects
}

/// Parse the first `/Rect [..]` array at or after `from`.
fn rect_after(pdf: &[u8], from: usize) -> Option<[f32; 4]> {
    let rect_at = from + find(&pdf[from..], b"/Rect")?;
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
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[test]
fn usaf_memo_signature_widget_aligns_with_signature_block() {
    let engine = Quillmark::new();
    let quill =
        quillmark::quill_from_path(quills_path("usaf_memo")).expect("usaf_memo should load");

    // The seeded document exercises the main memo *and* a representative
    // indorsement card (one instance per declared kind), so both the
    // `Signature` and `Ind_0_Signature` widgets are emitted.
    let parsed = quill.seed_document();

    let result = engine.render(
        &quill,
        &parsed,
        &RenderOptions {
            output_format: Some(OutputFormat::Pdf),
            ..Default::default()
        },
    );

    let rendered = result.expect("usaf_memo should render to PDF");
    let pdf = &rendered.artifacts[0].bytes;

    assert!(
        find(pdf, b"/AcroForm").is_some(),
        "rendered memo should carry an AcroForm with the signature widget(s)"
    );

    let rects = signature_widget_rects(pdf);
    assert!(
        !rects.is_empty(),
        "PDF should contain at least one /FT /Sig widget"
    );

    for [x0, _y0, x1, _y1] in &rects {
        let left_in = x0 / PT_PER_IN;
        // The widget must align with the 4.5in signature block, not the 1in
        // left margin. Allow a small tolerance for rounding and for the
        // long-name left-shift the package applies only when a line overflows.
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
}
