// config.typ: Configuration constants and defaults for USAF memorandum template
//
// This module defines core configuration values that implement AFH 33-337 Chapter 14
// formatting requirements for official USAF memorandums.

// =============================================================================
// SPACING CONSTANTS
// =============================================================================
// AFH 33-337 specifies precise spacing requirements throughout Chapter 14

#let spacing = (
  line: .5em, // Internal line spacing for readability (`par.leading`; gap between line boxes)
  tab: 0.5in, // Tab stop for multi-column recipient alignment
  margin: 1in, // AFH 33-337 §4: "Use 1-inch margins on the left, right and bottom"
)

// =============================================================================
// TYPOGRAPHY DEFAULTS
// =============================================================================
// AFH 33-337 §5: "Use 12 point Times New Roman font for text"

#let DEFAULT_LETTERHEAD_FONTS = ("Copperplate CC", "NimbusRomNo9L")
#let DEFAULT_BODY_FONTS = ("NimbusRomNo9L",)  // AFH 33-337 §5: Times New Roman required; NimbusRomNo9L is a metric-compatible clone
// Static monospace face for raw/code text. Liberation Mono is metric-compatible with
// Courier New; only the regular weight is bundled (no bold or italic variants).
#let DEFAULT_MONO_FONTS = ("Liberation Mono",)
#let LETTERHEAD_COLOR = rgb("#355e93")  // Faded USAF blue for letterhead

// =============================================================================
// PARAGRAPH CONFIGURATION
// =============================================================================
// AFH 33-337 "The Text of the Official Memorandum" §2:
// "Number and letter each paragraph and subparagraph"
// Hierarchical numbering: 1., a., (1), (a), etc.

#let paragraph-config = (
  counter-prefix: "par-counter-",
  // AFH 33-337 §2: Hierarchical paragraph numbering format
  // Level 0: 1., 2., 3. | Level 1: a., b., c. | Level 2: (1), (2), (3) | Level 3: (a), (b), (c)
  numbering-formats: ("1.", "a.", "(1)", "(a)", n => underline(str(n)), n => underline(str(n))),
)

// DAF (Headquarters) memo body: first-line indent for unnumbered paragraphs; nested
// items start at 1in, then +0.5in per additional nesting depth.
#let daf-paragraph = (
  top-first-line-indent: 0.5in,
  nested-first-level-indent: 1in,
  nested-step: 0.5in,
)

// =============================================================================
// COUNTERS
// =============================================================================

#let counters = (
  indorsement: counter("indorsement"),
)

// =============================================================================
// CLASSIFICATION COLORS
// =============================================================================
// AFH 33-337 §3: "Follow AFI 31-401, Information Security Program Management,
// applicable executive orders and DoD guidance for the necessary markings on
// classified correspondence."
// Color values follow DoD/CAPCO standard classification marking colors:
//   - TOP SECRET: orange  (#FF671F)
//   - SECRET: red         (#C8102E)
//   - CONFIDENTIAL: black (per project style guide)
//   - CUI: black          (DoDM 5200.48; per project style guide)
//   - UNCLASSIFIED: green (#007A33)

#let CLASSIFICATION_COLORS = (
  "TOP SECRET": rgb(255, 103, 31),
  "SECRET": rgb(200, 16, 46),
  "CONFIDENTIAL": rgb(0, 0, 0),
  "CUI": rgb(0, 0, 0),
  "UNCLASSIFIED": rgb(0, 122, 51),
)
