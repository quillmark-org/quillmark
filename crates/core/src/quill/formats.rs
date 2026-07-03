use std::sync::LazyLock;

use time::format_description::well_known::Rfc3339;
use time::format_description::{self, FormatItem};
use time::{Date, OffsetDateTime, PrimitiveDateTime};

static DATE_FMT: LazyLock<Vec<FormatItem<'static>>> =
    LazyLock::new(|| format_description::parse("[year]-[month]-[day]").expect("valid format"));

// Local (no-offset) forms tried in order — most-specific first so that a
// string like "…T12:00:00.5" does not partially match the plain HMS variant.
static LOCAL_FMTS: LazyLock<[Vec<FormatItem<'static>>; 6]> = LazyLock::new(|| {
    [
        format_description::parse("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond]")
            .expect("valid format"),
        format_description::parse("[year]-[month]-[day]T[hour]:[minute]:[second]")
            .expect("valid format"),
        format_description::parse("[year]-[month]-[day]T[hour]:[minute]").expect("valid format"),
        format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second].[subsecond]")
            .expect("valid format"),
        format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second]")
            .expect("valid format"),
        format_description::parse("[year]-[month]-[day] [hour]:[minute]").expect("valid format"),
    ]
});

/// Returns `true` when `s` is a valid datetime string.
///
/// Accepted forms:
/// - Bare date:        `YYYY-MM-DD`
/// - Local datetime:   `YYYY-MM-DD[T ]hh:mm[:ss[.s+]]`
/// - UTC / offset:     RFC 3339 — `YYYY-MM-DDThh:mm:ss[.s+](Z|±HH:MM)`
///
/// Parsing is delegated to the `time` crate, which enforces zero-padded
/// components and calendar validity (Feb 30 is rejected). Timezone offsets
/// require the T separator and seconds (RFC 3339 form).
pub(crate) fn is_valid_datetime(s: &str) -> bool {
    parse_date_ymd(s).is_some()
}

/// Parse a datetime string to its calendar date `(year, month, day)`,
/// discarding any time-of-day and timezone offset. Accepts exactly the forms
/// `is_valid_datetime` accepts — the two share this parser, so a value the
/// coercion layer validates is one the Typst backend can emit as a
/// `datetime(year:, month:, day:)` literal, by construction. `None` for any
/// string that is not one of the accepted forms.
///
/// Date-only by design: the backend today emits `datetime(year:, month:,
/// day:)`, matching the template's former `_parse-date`. Carrying time and
/// offset through is tracked separately (see the datetime follow-up issue).
pub fn parse_date_ymd(s: &str) -> Option<(i32, u8, u8)> {
    let ymd = |d: Date| (d.year(), u8::from(d.month()), d.day());
    if let Ok(dt) = OffsetDateTime::parse(s, &Rfc3339) {
        return Some(ymd(dt.date()));
    }
    for fmt in LOCAL_FMTS.iter() {
        if let Ok(dt) = PrimitiveDateTime::parse(s, fmt) {
            return Some(ymd(dt.date()));
        }
    }
    Date::parse(s, &*DATE_FMT).ok().map(ymd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_datetimes() {
        for s in [
            // bare date
            "2026-06-01",
            // local datetime — T separator
            "2026-06-01T12:00:00",
            "2026-06-01T12:00",
            "2026-06-01T12:00:00.5",
            "2026-06-01T12:00:00.123",
            // local datetime — space separator
            "2026-06-01 12:00:00",
            "2026-06-01 12:00",
            "2026-06-01 12:00:00.5",
            // RFC 3339 — UTC
            "2026-06-01T12:00:00Z",
            // RFC 3339 — offsets
            "2026-06-01T12:00:00+05:30",
            "2026-06-01T12:00:00-05:00",
            "2026-06-01T12:00:00+00:00",
            // RFC 3339 — fractional seconds
            "2026-06-01T12:00:00.5Z",
            "2026-06-01T12:00:00.123+05:30",
        ] {
            assert!(is_valid_datetime(s), "expected valid: {s}");
        }
    }

    #[test]
    fn invalid_datetimes() {
        for s in [
            "",
            "2026",
            "2026-06",
            "not-a-date",
            "13-04-2026",
            "2026-06-01T",
            "2026-06-01T12",
            "2026-06-01T12:",
            "2026-06-01T12:0",      // single-digit minute
            "2026-06-01T12:00:00 ", // trailing space
            "2026-06-01T12:00:00+",
            "2026-06-01T12:00:00X",
            "2026-13-01", // month out of range
            "2026-02-30", // calendar-invalid date (Feb 30)
            "2026-6-1",   // single-digit month/day (not zero-padded)
        ] {
            assert!(!is_valid_datetime(s), "expected invalid: {s}");
        }
    }
}
