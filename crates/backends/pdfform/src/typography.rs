//! The single source of truth for how the pdfform backend typesets bound
//! **values** on the flatten path.
//!
//! Under Technique A the AcroForm widget uses `/Helv 0 Tf` (the viewer
//! auto-sizes), but the *flatten* path has to draw the value itself, so it must
//! commit to a concrete font and size. These policy constants/functions are the
//! one place that decision lives, so the flattener — and any future surface that
//! composites a value over a page — agree by construction. That is the
//! "preview and flattening agree exactly" invariant: there is exactly one place
//! that answers "what font and size does this value render at."
//!
//! ## On regions presentation enrichment (#752)
//!
//! Surfacing this typography on the region sidecar (resolved font/size/align)
//! was the deferred half of value flattening. It is deliberately **not** done:
//! the unified flatten technique makes *both* canvas backends produce a
//! **complete** raster (the pdfform session pre-flattens values into the page
//! content, then rasterizes), so no consumer ever composites a value from a
//! region — the trigger the enrichment was waiting on ("font-accurate
//! client-side compositing") never occurs. The region carries only geometry +
//! the schema field address for cross-navigation; it intentionally carries no
//! value or typography. Were a font-accurate-compositing consumer to
//! materialize, it reads sizes from *this* module.

/// House text font — PDF base-14 Helvetica, registered as `/Helv`. Used for
/// text and choice values.
pub(crate) const TEXT_FONT: &str = "Helvetica";

/// House symbol font — PDF base-14 ZapfDingbats, registered as `/ZaDb`. Used for
/// the checkbox check glyph.
pub(crate) const CHECK_FONT: &str = "ZapfDingbats";

/// A flattened value never typesets below this point size …
pub(crate) const MIN_SIZE: f32 = 4.0;
/// … nor above this one. The clamp keeps tiny boxes legible and large boxes from
/// rendering absurdly big text.
pub(crate) const MAX_SIZE: f32 = 12.0;

/// Left inset, in points, of value text from the field box's left edge.
pub(crate) const TEXT_INSET: f32 = 2.0;

/// Top inset, in points, between the box's top edge and the first text baseline.
pub(crate) const TEXT_TOP_INSET: f32 = 1.0;

/// Inter-line spacing factor for multiline text: line height = size × this.
pub(crate) const LINE_SPACING: f32 = 1.2;

/// Auto-sized point size for a text/choice value in a box of height `h` points:
/// 65% of the box height, clamped to `[MIN_SIZE, MAX_SIZE]`. An emulation of the
/// AcroForm `0 Tf` auto-size a synthesizing viewer would pick.
pub(crate) fn value_size(h: f32) -> f32 {
    (h * 0.65).clamp(MIN_SIZE, MAX_SIZE)
}

/// Point size for the checkbox check glyph in a box of height `h` points: 75% of
/// the box height (the glyph reads a touch larger than text), clamped.
pub(crate) fn check_size(h: f32) -> f32 {
    (h * 0.75).clamp(MIN_SIZE, MAX_SIZE)
}

/// Approximate advance width of the ZapfDingbats check glyph (`'4'`) as a
/// fraction of its point size, used to horizontally centre it in the box.
pub(crate) const CHECK_GLYPH_WIDTH_FACTOR: f32 = 0.6;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_size_is_65pct_clamped() {
        assert_eq!(value_size(20.0), 12.0); // 13.0 → clamped to MAX
        assert_eq!(value_size(15.0), 9.75); // within range
        assert_eq!(value_size(2.0), 4.0); // 1.3 → clamped to MIN
    }

    #[test]
    fn check_size_is_75pct_clamped() {
        assert_eq!(check_size(20.0), 12.0); // 15.0 → clamped to MAX
        assert_eq!(check_size(14.0), 10.5); // within range
        assert_eq!(check_size(4.0), 4.0); // 3.0 → clamped to MIN
    }
}
