// indorsement.typ: Indorsement rendering for USAF memorandum
//
// This module implements indorsements (endorsements) per AFH 33-337 Chapter 14.
// Indorsements are used to forward memorandums with additional commentary.
// They follow the format: "1st Ind", "2d Ind", "3d Ind", etc.
// Each indorsement includes its own body text and signature block.
//
// Note: When using #show: indorsement.with(...), the indorsement wraps the
// entire remainder of the document. This works for a single indorsement at
// the end of a file. For multiple indorsements, use the function call syntax:
// #indorsement(...)[Body text...]

#import "primitives.typ": *
#import "body.typ": *

#let indorsement(
  from: none,
  to: none,
  signature_block: none,
  signature_blank_lines: 4,
  signing_field: none,
  attachments: none,
  cc: none,
  date: none,
  // Format of indorsement: "standard" (same page), "informal" (no header), or "separate_page" (starts on new page)
  format: "standard",
  // Approval action: none (default, no action line displayed), "undecided", "approve", or "disapprove".
  // When set to "undecided", the action line is displayed with neither option circled.
  // When set to "approve" or "disapprove", the action line is displayed with the selected option circled.
  action: none,
  content,
) = {
  // Validate format parameter
  assert(
    format in ("standard", "informal", "separate_page"),
    message: "format must be \"standard\", \"informal\", or \"separate_page\"",
  )

  if format != "informal" {
    assert(from != none, message: "from is required")
    assert(to != none, message: "to is required")
  }


  let actual_date = date
  let ind_from = first-or-value(from)
  let ind_for = to

  // An empty body (e.g. a KIND with only an action selected and no markdown
  // body) collapses to zero rendered layout via render-body's filter. To
  // make the "empty body takes no layout space" guarantee end-to-end, also
  // suppress the spacing the surrounding code reserves *for* the body:
  // the header→body gap (when no action is present) and the action→body
  // trailing gap. Without this, an empty body still leaves an extra
  // blank-line stride above the signature, pushing it off the AFH 33-337
  // "fifth line below the last line of text" anchor.
  // The plate calls indorsement with `[]` when the markdown body is empty
  // or whitespace-only; comparing against `[]` reliably detects that.
  let body_empty = content == []

  let effective_action = if action == none or type(action) != str or action.trim() == "" {
    none
  } else {
    action
  }

  if format != "informal" {
    // Step the counter BEFORE the context block to avoid read-then-update loop
    counters.indorsement.step()

    context {
      let config = query(<usaf-memo-config>).first().value
      let memo-style = config.at("memo_style", default: "usaf")
      let original_subject = config.subject
      let original_date = config.original_date
      let original_from = config.original_from

      // Read the counter value (already stepped above)
      let indorsement_number = counters.indorsement.get().at(0, default: 1)
      let indorsement_label = format-indorsement-number(indorsement_number)

      if format == "separate_page" {
        pagebreak()
        [#indorsement_label to #original_from, #display-date(original_date, memo-style: memo-style), #original_subject]

        blank-line()
        grid(
          columns: (auto, 1fr),
          ind_from, align(right)[#if actual_date != none { display-date(actual_date, memo-style: memo-style) }],
        )

        blank-line()
        grid(
          columns: (auto, auto, 1fr),
          "MEMORANDUM FOR", "  ", ind_for,
        )
      } else {
        blank-line()
        grid(
          columns: (auto, 1fr),
          [#indorsement_label, #ind_from], align(right)[#if actual_date != none { display-date(actual_date, memo-style: memo-style) }],
        )

        blank-line()
        grid(
          columns: (auto, auto, 1fr),
          "MEMORANDUM FOR", "  ", ind_for,
        )
      }
    }
    // Header→content gap. Skipped when there is neither an action line nor
    // body to follow — render-signature-block supplies its own 4-line gap.
    if effective_action != none or not body_empty {
      blank-line()
    }
  }

  // Show action line only when an action decision is set (not `none`)
  if effective_action != none {
    render-action-line(effective_action, trailing-blank-line: not body_empty)
  }

  if not body_empty {
    context {
      let memo-style = query(<usaf-memo-config>).first().value.at("memo_style", default: "usaf")
      render-body(content, memo-style: memo-style)
    }
  }

  render-signature-block(signature_block, signature-blank-lines: signature_blank_lines, signing-field: signing_field)

  render-backmatter-sections(attachments: attachments, cc: cc)
}
