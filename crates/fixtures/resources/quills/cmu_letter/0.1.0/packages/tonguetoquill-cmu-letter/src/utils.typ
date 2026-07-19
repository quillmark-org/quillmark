// utils.typ: Utility functions for CMU letterhead template
//
// This module provides core utility functions used by the main letterhead template.

#import "config.typ": *

// =============================================================================
// SPACING UTILITIES
// =============================================================================

/// Creates vertical spacing equivalent to multiple blank lines.
///
/// - count (int): Number of blank lines to create
/// - weak (bool): Whether spacing can be compressed at page breaks
/// -> content
#let blank-lines(count, weak: true) = {
  for i in range(0, count) {
    v(1.4em)
  }
}

/// Creates vertical spacing equivalent to one blank line.
/// -> content
#let blank-line(weak: true) = blank-lines(1, weak: weak)

// =============================================================================
// GENERAL UTILITIES
// =============================================================================

/// Checks if a value is "falsey" (none, false, empty array, or empty string).
///
/// - value (any): The value to check
/// -> bool
#let falsey(value) = {
  value == none or value == false or (type(value) == array and value.len() == 0) or (type(value) == str and value == "")
}

/// Ensures the input is a string. If an array, joins elements with separator.
///
/// - value: Any value to normalize to string form
/// - separator: String to use when joining array elements (default: "\n")
/// -> str
#let ensure-string(value, separator: "\n") = {
  if value == none {
    ""
  } else if type(value) == array {
    value.join(separator)
  } else {
    str(value)
  }
}

/// Scales content to fit within a specified box while maintaining aspect ratio.
///
/// - width (length): Maximum width for the content
/// - height (length): Maximum height for the content
/// - alignment (alignment): Content alignment within the box
/// - body (content): Content to scale and fit
/// -> content
#let fit-box(width: 2in, height: 1in, alignment: left + horizon, body) = context {
  let s = measure(body)
  let f = calc.min(width / s.width, height / s.height) * 100%
  box(width: width, height: height, clip: true)[
    #align(alignment)[
      #scale(f, reflow: true)[#body]
    ]
  ]
}

// =============================================================================
// DATE FORMATTING
// =============================================================================

/// Formats a date in CMU civilian format: "Month Day, Year"
/// Guidelines section 3.2: Date format is "Month Day, Year" (e.g., November 29, 2025)
///
/// - date (str|datetime|dictionary): Date to format — a raw string, a native
///   `datetime`, or a Quillmark date value-object.
/// -> str | content
#let display-date(date) = {
  if type(date) == str {
    date
  } else if type(date) == datetime {
    // A bare `datetime` — the `today()` fallback for a blank date — keeps
    // native method sugar (returns `str`).
    date.display("[month repr:long] [day padding:none], [year]")
  } else {
    // A Quillmark date field crosses as a value-object dict whose `display` key
    // is a closure returning region-bearing content; grabbing `.display` off a
    // native datetime is a compile error, so the dict takes the paren form.
    (date.display)("[month repr:long] [day padding:none], [year]")
  }
}
