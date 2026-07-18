//! Headless proof that the pdfform canvas raster is COMPLETE.
//!
//! The canvas contract (`prose/canon/PREVIEW.md`) says a backend whose session
//! returns `Some` from `render_rgba` produces a *complete* page raster: every
//! piece of page content — including bound field values — is already visible in
//! the pixels, with no caller-side compositing. pdfform satisfies this by
//! pre-flattening the field values into the page content streams at
//! session-open, then rasterizing that flat PDF via hayro.
//!
//! This test renders the `sample_form` fixture's session and asserts:
//!   1. `render_rgba(0, scale)` returns a raster whose dimensions match
//!      `page_size_pt × scale` (within rounding), and
//!   2. that raster contains NON-WHITE, opaque pixels inside at least one
//!      field's region rect — i.e. the pre-flatten values are baked into the
//!      raster, not left for the caller to draw.
//!
//! The region geometry is in PDF points (bottom-left origin); the raster is
//! top-left origin in device pixels, so the test applies the canonical
//! `y_canvas = (pageHeightPt - y_pdf) × scale` flip to locate a field box.

use quillmark::{Document, Quillmark};

const FILLED: &str = "~~~\n\
$quill: sample_form\n\
$kind: main\n\
full_name: Ada Lovelace\n\
comments:\n\
  - First comment line.\n\
  - Second comment line.\n\
agree: true\n\
favorite_color: green\n\
~~~\n";

#[test]
fn pdfform_canvas_raster_is_complete() {
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form quill");
    let engine = Quillmark::new();
    let doc = Document::parse(FILLED).expect("parse markdown").document;

    let session = engine.open(&quill, &doc).expect("open session");

    // The pdfform backend reports canvas support (it rasterizes via hayro).
    assert!(
        session.page_size_pt(0).is_some(),
        "pdfform session must expose page geometry"
    );

    // 1. Dimensions: render_rgba(0, scale) matches page_size_pt × scale.
    let scale: f32 = 2.0;
    let (width_pt, height_pt) = session.page_size_pt(0).expect("page 0 size");
    let (px_w, px_h, rgba) = session
        .render_rgba(0, scale)
        .expect("pdfform session must rasterize page 0");

    let expect_w = (width_pt * scale).round() as i64;
    let expect_h = (height_pt * scale).round() as i64;
    // hayro rounds independently per axis; allow ±1 px of rounding slack.
    assert!(
        (px_w as i64 - expect_w).abs() <= 1,
        "raster width {px_w} should match page_size_pt × scale ≈ {expect_w}"
    );
    assert!(
        (px_h as i64 - expect_h).abs() <= 1,
        "raster height {px_h} should match page_size_pt × scale ≈ {expect_h}"
    );
    assert_eq!(
        rgba.len(),
        (px_w as usize) * (px_h as usize) * 4,
        "RGBA buffer must be w*h*4 bytes"
    );

    // 2. Field-value ink: locate a bound text field via the session's region
    //    geometry and prove the flat raster has non-white opaque pixels inside
    //    its rect. Geometry is a session-level query — no second byte render.
    let regions = session.regions();

    // Pick the bound text field (schema path `full_name` = "Ada Lovelace") on
    // page 0, keyed on the schema path.
    let region = regions
        .iter()
        .find(|r| r.page == 0 && r.field == "full_name")
        .expect("a region for the bound text field on page 0");

    // PDF points (bottom-left origin) → canvas pixels (top-left origin), the
    // canonical transform from PREVIEW.md: y_canvas = (pageHeightPt - y_pdf) × scale.
    let [x0, y0, x1, y1] = region.rect;
    let left = (x0 * scale).floor().max(0.0) as u32;
    let right = ((x1 * scale).ceil() as u32).min(px_w);
    let top = ((height_pt - y1) * scale).floor().max(0.0) as u32;
    let bottom = (((height_pt - y0) * scale).ceil() as u32).min(px_h);

    assert!(
        left < right && top < bottom,
        "field region rect must map to a non-empty pixel box: \
         x[{left},{right}) y[{top},{bottom}) in {px_w}x{px_h}"
    );

    let mut ink = 0u64; // non-white, opaque
    let mut opaque = 0u64; // any opaque pixel (page background)
    for y in top..bottom {
        for x in left..right {
            let i = ((y as usize) * (px_w as usize) + (x as usize)) * 4;
            let (r, g, b, a) = (rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]);
            if a == 255 {
                opaque += 1;
                if r < 250 || g < 250 || b < 250 {
                    ink += 1;
                }
            }
        }
    }

    assert!(
        opaque > 0,
        "field region box must contain opaque page-background pixels"
    );
    assert!(
        ink > 0,
        "field region box must contain NON-WHITE opaque pixels — \
         proof the pre-flattened value is baked into the raster"
    );
}
