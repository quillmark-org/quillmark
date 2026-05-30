// frontmatter.typ: Frontmatter show rule for USAF memorandum
//
// This module implements the frontmatter (heading section) of a USAF memorandum
// per AFH 33-337 Chapter 14 "The Heading Section". It handles:
// - Page setup with proper margins
// - Letterhead rendering
// - Date, MEMORANDUM FOR, FROM, SUBJECT, and References placement
// - Classification markings in headers/footers

#import "primitives.typ": *

#let frontmatter(
  subject: none,
  memo_for: none,
  memo_from: none,
  date: none,
  references: none,
  letterhead_title: "DEPARTMENT OF THE AIR FORCE",
  letterhead_caption: "[YOUR SQUADRON/UNIT NAME]",
  letterhead_seal: none,
  letterhead_seal_subtitle: none, // optional line under seal (9pt bold caps); ignored if no seal
  letterhead_emblem: none, // optional image placed opposite the seal (right side)
  letterhead_font: DEFAULT_LETTERHEAD_FONTS,
  body_font: DEFAULT_BODY_FONTS,
  font_size: 12pt,
  memo_for_cols: 3,
  classification_level: none,
  dissemination: none,
  footer_tag_line: none,
  memo_style: "usaf",
  it,
) = {
  assert(subject != none, message: "subject is required")
  assert(memo_for != none, message: "memo_for is required")
  assert(
    memo_style in ("usaf", "daf"),
    message: "memo_style must be \"usaf\" or \"daf\"",
  )

  let actual_date = if date == none { datetime.today() } else { date }

  let classification_marking = if classification_level == none or type(classification_level) != str {
    none
  } else {
    let base = classification_level.trim()
    if base == "" {
      none
    } else {
      let disp = if dissemination == none or type(dissemination) != str {
        ""
      } else {
        dissemination.trim()
      }
      if disp != "" {
        base + "//" + upper(disp)
      } else {
        base
      }
    }
  }
  let classification_color = get-classification-level-color(classification_level)

  // Document-wide typography settings (inlined from configure())
  set par(leading: spacing.line, spacing: spacing.line, justify: false)
  set block(above: spacing.line, below: 0em, spacing: 0em)
  set text(font: body_font, size: font_size, fallback: true)

  set page(
    paper: "us-letter",
    // AFH 33-337 §4: "Use 1-inch margins on the left, right and bottom"
    margin: (
      left: spacing.margin,
      right: spacing.margin,
      top: spacing.margin,
      bottom: spacing.margin,
    ),
    header: {
      // AFH 33-337 "Page numbering" §12: "The first page of a memorandum is never numbered.
      // Number the succeeding pages starting with page 2. Place page numbers 0.5-inch from
      // the top of the page, flush with the right margin."
      context if counter(page).get().first() > 1 {
        place(
          dy: +.5in,
          block(
            width: 100%,
            align(right, text(12pt)[#counter(page).display()]),
          ),
        )
      }

      if classification_marking != none {
        place(
          top + center,
          dy: 0.375in,
          text(12pt, font: DEFAULT_BODY_FONTS, fill: classification_color)[#strong(classification_marking)],
        )
      }
    },
    footer: {
      if classification_marking != none {
        place(
          bottom + center,
          dy: -.375in,
          text(12pt, font: DEFAULT_BODY_FONTS, fill: classification_color)[#strong(classification_marking)],
        )
      }

      if not falsey(footer_tag_line) {
        place(
          bottom + center,
          dy: -0.625in,
          align(center)[
            #text(fill: LETTERHEAD_COLOR, font: "cinzel", size: 15pt)[#footer_tag_line]
          ],
        )
      }
    },
  )

  render-letterhead(
    letterhead_title,
    letterhead_caption,
    letterhead_font,
    letterhead-seal: letterhead_seal,
    letterhead-seal-subtitle: letterhead_seal_subtitle,
    letterhead-emblem: letterhead_emblem,
  )

  // AFH 33-337 "Date": "Place the date 1 inch from the right edge, 1.75 inches from the top"
  // Since we have a 1-inch top margin, we need (1.75in - margin) vertical space
  v(1.75in - spacing.margin)

  // Measure and cache body line stride once for body line-count heuristics.
  context {
    let one-line = measure(par(spacing: 0pt)[x]).height
    let line-stride = measure(par(spacing: 0pt)[x#linebreak()x]).height - one-line
    LINE_STRIDE.update(line-stride)
  }

  [#metadata((
    subject: subject,
    original_date: actual_date,
    original_from: first-or-value(memo_from),
    body_font: body_font,
    font_size: font_size,
    memo_style: memo_style,
  )) <usaf-memo-config>]

  render-date-section(actual_date, memo-style: memo_style)
  render-for-section(memo_for, memo_for_cols)
  if not falsey(memo_from) { render-from-section(memo_from) }
  render-subject-section(subject)
  render-references-section(references)

  // AFH 33-337: "Begin text on second line below subject/references".
  // Emitted here (not inside body.typ) so the v() lands at the same lexical
  // level as the preceding header sections and combines correctly with their
  // block spacing.
  blank-line()
  it
}
