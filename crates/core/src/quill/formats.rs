use std::sync::LazyLock;

use time::format_description::well_known::Rfc3339;
use time::format_description::{self, FormatItem};
use time::{Date, OffsetDateTime, PrimitiveDateTime};

static DATE_FMT: LazyLock<Vec<FormatItem<'static>>> =
    LazyLock::new(|| format_description::parse("[year]-[month]-[day]").expect("valid format"));

// Strict offset-less wall-clock datetime forms — the `type: datetime` grammar.
// T separator required, seconds optional (zero-filled); no offset, no space
// separator, no fractional seconds, no bare date. Most-specific first so a
// with-seconds string does not partially match the minute-only variant. The
// `time` parser consumes the whole input, so a trailing offset / fraction /
// stray character fails rather than silently truncating.
static DATETIME_FMTS: LazyLock<[Vec<FormatItem<'static>>; 2]> = LazyLock::new(|| {
    [
        format_description::parse("[year]-[month]-[day]T[hour]:[minute]:[second]")
            .expect("valid format"),
        format_description::parse("[year]-[month]-[day]T[hour]:[minute]").expect("valid format"),
    ]
});

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

/// Parse a strict calendar date `YYYY-MM-DD` to `(year, month, day)` — the
/// `type: date` grammar. Any time component (a `T`/space separator and beyond)
/// is rejected: a date field holds a date, full stop, and a time-bearing string
/// is a `type: datetime`. The `time` parser enforces zero-padding and calendar
/// validity (`2026-02-30` is rejected). `None` for any other string.
pub fn parse_date(s: &str) -> Option<(i32, u8, u8)> {
    Date::parse(s, &*DATE_FMT)
        .ok()
        .map(|d| (d.year(), u8::from(d.month()), d.day()))
}

/// Parse a strict offset-less wall-clock datetime `YYYY-MM-DDThh:mm[:ss]` to
/// `(year, month, day, hour, minute, second)`, seconds zero-filled when absent
/// — the `type: datetime` grammar. Rejects timezone offsets (`Z`, `±HH:MM`),
/// the space separator, fractional seconds, and a bare date. The engine keeps
/// wall-clock semantics end to end and does no zone math, so an offset is an
/// error at the seam, never a silently dropped component; the whose-wall-clock
/// decision is forced to the consumer boundary where the context lives. `None`
/// for any other string.
pub fn parse_datetime(s: &str) -> Option<(i32, u8, u8, u8, u8, u8)> {
    for fmt in DATETIME_FMTS.iter() {
        if let Ok(dt) = PrimitiveDateTime::parse(s, fmt) {
            return Some((
                dt.year(),
                u8::from(dt.month()),
                dt.day(),
                dt.hour(),
                dt.minute(),
                dt.second(),
            ));
        }
    }
    None
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

    // ── Strict `type: date` grammar ──────────────────────────────────────────

    #[test]
    fn parse_date_accepts_bare_calendar_dates() {
        assert_eq!(parse_date("2026-06-01"), Some((2026, 6, 1)));
        assert_eq!(parse_date("2000-12-31"), Some((2000, 12, 31)));
    }

    #[test]
    fn parse_date_rejects_time_components_and_malformed() {
        for s in [
            "",
            "2026",
            "2026-06",
            "2026-6-1",              // not zero-padded
            "2026-13-01",            // month out of range
            "2026-02-30",            // Feb 30
            "2026-06-01T12:00",      // time component → this is a datetime
            "2026-06-01T12:00:00",   // time component
            "2026-06-01 12:00",      // space-separated time component
            "2026-06-01Z",           // stray offset marker
            "2026-06-01T12:00:00Z",  // offset instant
            "not-a-date",
        ] {
            assert_eq!(parse_date(s), None, "expected rejected date: {s}");
        }
    }

    // ── Strict `type: datetime` grammar ──────────────────────────────────────

    #[test]
    fn parse_datetime_accepts_offsetless_wall_clock() {
        // Seconds present.
        assert_eq!(
            parse_datetime("2026-06-01T14:30:15"),
            Some((2026, 6, 1, 14, 30, 15))
        );
        // Seconds omitted → zero-filled (the one human concession).
        assert_eq!(
            parse_datetime("2026-06-01T14:30"),
            Some((2026, 6, 1, 14, 30, 0))
        );
        assert_eq!(
            parse_datetime("2026-06-01T00:00:00"),
            Some((2026, 6, 1, 0, 0, 0))
        );
    }

    #[test]
    fn parse_datetime_rejects_offsets_space_fraction_and_bare_date() {
        for s in [
            "",
            "2026-06-01",               // bare date → this is a `type: date`
            "2026-06-01T14:30:00Z",     // UTC offset — rejected, never treated as local
            "2026-06-01T14:30:00+00:00", // explicit zero offset — no special case
            "2026-06-01T14:30:00+05:30", // offset
            "2026-06-01T14:30:00-05:00", // offset
            "2026-06-01 14:30:00",      // YAML space separator, not RFC 3339
            "2026-06-01T14:30:00.5",    // fractional seconds
            "2026-06-01T14:30:00.123",  // fractional seconds
            "2026-06-01T14:30:00 ",     // trailing space
            "2026-06-01T14",            // no minute
            "2026-06-01T14:3",          // single-digit minute
            "2026-13-01T14:30:00",      // month out of range
            "2026-02-30T14:30:00",      // Feb 30
        ] {
            assert_eq!(parse_datetime(s), None, "expected rejected datetime: {s}");
        }
    }
}
