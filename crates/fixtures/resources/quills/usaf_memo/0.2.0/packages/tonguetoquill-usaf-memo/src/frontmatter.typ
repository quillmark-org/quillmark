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
  letterhead_emblem_height: 1in, // emblem fit-box height; reduce for shorter emblems
  letterhead_font: DEFAULT_LETTERHEAD_FONTS,
  body_font: DEFAULT_BODY_FONTS,
  font_size: 12pt,
  memo_for_cols: 3,
  classification_level: none,
  dissemination: none,
  cui_controlled_by: none,
  cui_category: none,
  cui_limited_dissemination: none,
  cui_poc: none,
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

  // Build the CUI designation indicator block (DoDM 5200.48, Table 1), shown
  // only for CUI when at least one indicator field is set. Rendered as a
  // bottom-right page-1 float (see placement below).
  let cui_indicator = if (
    classification_level != none
    and type(classification_level) == str
    and classification_level.trim().starts-with("CUI")
  ) {
    let lines = ()
    if cui_controlled_by != none and type(cui_controlled_by) == str and cui_controlled_by.trim() != "" {
      lines.push([Controlled By: #cui_controlled_by.trim()])
    }
    if cui_category != none and type(cui_category) == str and cui_category.trim() != "" {
      lines.push([CUI Category: #cui_category.trim()])
    }
    let ldc = if cui_limited_dissemination != none and type(cui_limited_dissemination) == str { cui_limited_dissemination.trim() } else { "" }
    if ldc != "" {
      lines.push([LDC: #upper(ldc)])
    }
    if cui_poc != none and type(cui_poc) == str and cui_poc.trim() != "" {
      lines.push([POC: #cui_poc.trim()])
    }
    if lines.len() > 0 { lines.join(linebreak()) } else { none }
  } else {
    none
  }

  // Document-wide typography settings (inlined from configure())
  set par(leading: spacing.line, spacing: spacing.line, justify: false)
  set block(above: spacing.line, below: 0em, spacing: 0em)
  set text(font: body_font, size: font_size, fallback: true)
  show raw: set text(font: DEFAULT_MONO_FONTS)  // Static monospace face for inline code and code blocks

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

  // DoDM 5200.48 §3: CUI designation indicator block — page 1 only, bottom-right
  // corner, dropped into the 0.5in page-edge band. Emitted as a bottom float so
  // it (1) reserves flow space, raising page 1's effective bottom margin so body
  // text never overlaps it, and (2) stays pinned to page 1 — as the first flow
  // content it can never be bumped to page 2.
  if cui_indicator != none {
    context {
      // The box shrink-wraps to its widest line; `set align(left)` keeps the
      // text flush-left within it, overriding the `align(right)` the placement
      // below imposes.
      let indicator_box = box({
        set text(font: DEFAULT_BODY_FONTS, size: 10pt)
        set par(leading: 0.4em, spacing: 0pt)
        set align(left)
        cui_indicator
      })
      // Reserve only the part of the block inside the text area (`reserved`):
      // float a box of that height, then `place` the full block inside it pushed
      // down by `overhang` so the surplus overflows into the edge band. (A bare
      // `box(height: reserved, indicator_box)` overflows *upward* into the body
      // instead.) The inner `place` adds no size, so the box stays `reserved`
      // tall and the block's bottom lands 0.5in from the page edge.
      let overhang = spacing.margin - 0.5in
      let reserved = measure(indicator_box).height - overhang
      place(
        bottom + right,
        float: true,
        // Slide the right edge into the page-edge band, 0.5in from the border;
        // the inner place right-aligns the block to that edge.
        dx: spacing.margin - 0.5in,
        // Minimum gap to the body's last line; the actual gap is larger when the
        // next paragraph can't fit above the block and breaks to the next page.
        clearance: spacing.line,
        box(height: reserved, place(bottom + right, dy: overhang, indicator_box)),
      )
    }
  }

  render-letterhead(
    letterhead_title,
    letterhead_caption,
    letterhead_font,
    letterhead-seal: letterhead_seal,
    letterhead-seal-subtitle: letterhead_seal_subtitle,
    letterhead-emblem: letterhead_emblem,
    letterhead-emblem-height: letterhead_emblem_height,
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
  let single-ref = if type(references) == array and references.len() == 1 {
    references.at(0)
  } else {
    none
  }
  render-subject-section(subject, inline-reference: single-ref)
  render-references-section(references)

  // AFH 33-337: "Begin text on second line below subject/references".
  // Emitted here (not inside body.typ) so the v() lands at the same lexical
  // level as the preceding header sections and combines correctly with their
  // block spacing.
  blank-line()
  it
}
