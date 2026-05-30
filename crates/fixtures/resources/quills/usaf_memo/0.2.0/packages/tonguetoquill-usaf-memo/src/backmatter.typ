// backmatter.typ: Backmatter rendering for USAF memorandum
//
// This module implements the backmatter (closing section) of a USAF memorandum per
// AFH 33-337 Chapter 14 "The Closing Section". It handles:
// - Signature block placement and formatting
// - Attachments listing
// - Courtesy copy (cc:) distribution
// - Distribution lists

#import "primitives.typ": *

#let backmatter(
  signature_block: none,
  signature_blank_lines: 4,
  signing_field: none,
  attachments: none,
  cc: none,
  distribution: none,
  leading_pagebreak: false,
) = {
  // Render backmatter sections without paragraph numbering
  render-signature-block(signature_block, signature-blank-lines: signature_blank_lines, signing-field: signing_field)
  render-backmatter-sections(
    attachments: attachments,
    cc: cc,
    distribution: distribution,
    leading-pagebreak: leading_pagebreak,
  )
}
