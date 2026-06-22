// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Date/time parsing — the local→UTC input boundary.
//!
//! Every human-supplied time is interpreted in a single resolved IANA zone
//! (see [`crate::timezone`]) and converted to a `DateTime<Utc>` for use
//! internally and on the CalDAV wire. Inputs that already carry an explicit
//! offset or `Z` are respected as written.

use anyhow::{Context, Result};
use chrono::offset::LocalResult;
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;

/// A parsed time input, before it is pinned to a query/create role.
///
/// The same string can mean different instants depending on the caller: a
/// bare `YYYY-MM-DD` is the start of a day for `--from`, the *end* of a day
/// for `--to`, and an all-day marker for `events create`. Classifying once
/// and letting the caller decide keeps that logic in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeSpec {
    /// A calendar date with no time-of-day (date-only / all-day input).
    Date(NaiveDate),
    /// A precise instant, already normalized to UTC.
    Instant(DateTime<Utc>),
}

/// Classify a datetime string in the context of zone `tz`.
///
/// Recognized forms:
/// - `today` / `tomorrow` → the local calendar day in `tz`
/// - ISO 8601 with offset/`Z` (`2026-03-05T14:30:00Z`) → that exact instant
/// - `YYYY-MM-DD` → that local calendar date (date-only)
/// - `YYYY-MM-DD HH:MM[:SS]`, `YYYY-MM-DD 2pm`, `… 2:30pm`, `… 14:30`
///   → that wall-clock time interpreted in `tz`
pub fn classify(input: &str, tz: Tz) -> Result<TimeSpec> {
    let input = input.trim();

    // Relative day keywords resolve against *local* "now" in the zone, so a
    // user just past local midnight gets today's local day, not the UTC day.
    match input.to_lowercase().as_str() {
        "today" => return Ok(TimeSpec::Date(Utc::now().with_timezone(&tz).date_naive())),
        "tomorrow" => {
            let d = Utc::now().with_timezone(&tz).date_naive() + chrono::Duration::days(1);
            return Ok(TimeSpec::Date(d));
        }
        _ => {}
    }

    // Explicit offset / Z — authoritative, converted to UTC, never reinterpreted.
    if let Ok(dt) = DateTime::parse_from_rfc3339(input) {
        return Ok(TimeSpec::Instant(dt.with_timezone(&Utc)));
    }

    // Date only (YYYY-MM-DD).
    if input.len() == 10 && input.as_bytes()[4] == b'-' {
        let date = NaiveDate::parse_from_str(input, "%Y-%m-%d")
            .with_context(|| format!("Failed to parse date: '{input}'"))?;
        return Ok(TimeSpec::Date(date));
    }

    // Naive datetimes — interpreted in `tz`.
    if let Ok(ndt) = NaiveDateTime::parse_from_str(input, "%Y-%m-%d %H:%M") {
        return Ok(TimeSpec::Instant(local_to_utc(ndt, tz)?));
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(input, "%Y-%m-%d %H:%M:%S") {
        return Ok(TimeSpec::Instant(local_to_utc(ndt, tz)?));
    }

    // Natural formats like "2026-03-05 2pm", "2026-03-05 2:30pm", "… 14:30".
    if let Some((date_part, time_part)) = input.split_once(' ') {
        if let Ok(date) = NaiveDate::parse_from_str(date_part, "%Y-%m-%d") {
            if let Some(ndt) = parse_natural_time(date, time_part) {
                return Ok(TimeSpec::Instant(local_to_utc(ndt, tz)?));
            }
        }
    }

    anyhow::bail!(
        "Unsupported datetime format: '{input}'. Try ISO 8601 (2026-03-05T14:30:00Z), \
         YYYY-MM-DD, or YYYY-MM-DD HH:MM"
    )
}

/// Parse a datetime as a single instant in zone `tz`.
///
/// Used by everything that needs a point in time: `--from`, `search` bounds,
/// `conflicts`, and timed `create`/`update`. A date-only input resolves to
/// **local midnight** (the start of that day).
pub fn parse_datetime(input: &str, tz: Tz) -> Result<DateTime<Utc>> {
    match classify(input, tz)? {
        TimeSpec::Instant(dt) => Ok(dt),
        TimeSpec::Date(d) => local_to_utc(midnight(d), tz),
    }
}

/// Parse the **exclusive upper bound** of a date range in zone `tz`.
///
/// A date-only `--to <date>` means *through the end of that local day*, so it
/// resolves to the **next** local midnight. This makes `--from today --to
/// today` cover the whole local day (half-open `[from, to)`) instead of
/// collapsing to a zero-width window. An input that carries an explicit time
/// is used as-is.
pub fn parse_range_end(input: &str, tz: Tz) -> Result<DateTime<Utc>> {
    match classify(input, tz)? {
        TimeSpec::Instant(dt) => Ok(dt),
        TimeSpec::Date(d) => {
            let next = d
                .succ_opt()
                .with_context(|| format!("date '{d}' is out of representable range"))?;
            local_to_utc(midnight(next), tz)
        }
    }
}

/// Convert a naive wall-clock time in `tz` to a UTC instant.
///
/// DST handling mirrors the ICS reader: for a *fold* (ambiguous local time
/// that happens twice) we pick the earlier instant; for a *gap* (a local time
/// that never occurs on a spring-forward day) we error rather than guess.
fn local_to_utc(naive: NaiveDateTime, tz: Tz) -> Result<DateTime<Utc>> {
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Ok(dt.with_timezone(&Utc)),
        LocalResult::Ambiguous(earlier, _) => Ok(earlier.with_timezone(&Utc)),
        LocalResult::None => anyhow::bail!(
            "local time {naive} does not exist in timezone {} (likely a DST spring-forward gap)",
            tz.name()
        ),
    }
}

/// Midnight (00:00:00) on `date`, as a naive datetime.
fn midnight(date: NaiveDate) -> NaiveDateTime {
    date.and_hms_opt(0, 0, 0)
        .expect("00:00:00 is always a valid time")
}

/// Parse natural time formats like "2pm", "2:30pm", "14:30" into a naive
/// datetime on `date`. Zone application happens in the caller.
fn parse_natural_time(date: NaiveDate, time_str: &str) -> Option<NaiveDateTime> {
    let time_str = time_str.trim().to_lowercase();

    // Handle "2pm", "2:30pm" format
    if time_str.ends_with("pm") || time_str.ends_with("am") {
        let is_pm = time_str.ends_with("pm");
        let time_part = time_str
            .trim_end_matches("pm")
            .trim_end_matches("am")
            .trim();

        if let Some((hour_str, min_str)) = time_part.split_once(':') {
            // "2:30pm" format
            if let (Ok(mut hour), Ok(min)) = (hour_str.parse::<u32>(), min_str.parse::<u32>()) {
                if is_pm && hour != 12 {
                    hour += 12;
                } else if !is_pm && hour == 12 {
                    hour = 0;
                }
                return date.and_hms_opt(hour, min, 0);
            }
        } else {
            // "2pm" format
            if let Ok(mut hour) = time_part.parse::<u32>() {
                if is_pm && hour != 12 {
                    hour += 12;
                } else if !is_pm && hour == 12 {
                    hour = 0;
                }
                return date.and_hms_opt(hour, 0, 0);
            }
        }
    }

    // Handle "14:30" format (24-hour)
    if let Some((hour_str, min_str)) = time_str.split_once(':') {
        if let (Ok(hour), Ok(min)) = (hour_str.parse::<u32>(), min_str.parse::<u32>()) {
            return date.and_hms_opt(hour, min, 0);
        }
    }

    None
}

/// Format a UTC instant for iCalendar (`YYYYMMDDTHHMMSSZ`).
pub fn format_for_ics(dt: &DateTime<Utc>) -> String {
    dt.format("%Y%m%dT%H%M%SZ").to_string()
}

/// Format a date for an all-day iCalendar value (`YYYYMMDD`, used with
/// `;VALUE=DATE`).
pub fn format_date_for_ics(date: &NaiveDate) -> String {
    date.format("%Y%m%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    const UTC: Tz = Tz::UTC;
    const AMS: Tz = Tz::Europe__Amsterdam;

    // ── Instant parsing (UTC-equivalent) ──────────────────────────────────

    #[test]
    fn test_parse_iso8601() {
        let dt = parse_datetime("2026-03-05T14:30:00Z", UTC).unwrap();
        assert_eq!(dt.hour(), 14);
        assert_eq!(dt.minute(), 30);
    }

    #[test]
    fn test_parse_iso8601_offset_respected_regardless_of_zone() {
        // An explicit offset is authoritative even when the resolved zone differs.
        let dt = parse_datetime("2026-06-25T14:00:00Z", AMS).unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-06-25T14:00:00+00:00");
    }

    #[test]
    fn test_parse_date_only_is_local_midnight() {
        // In UTC the local midnight is 00:00Z.
        let dt = parse_datetime("2026-03-05", UTC).unwrap();
        assert_eq!(dt.hour(), 0);
        assert_eq!(dt.minute(), 0);
    }

    #[test]
    fn test_parse_datetime_space() {
        let dt = parse_datetime("2026-03-05 14:30", UTC).unwrap();
        assert_eq!(dt.hour(), 14);
        assert_eq!(dt.minute(), 30);
    }

    #[test]
    fn test_parse_natural_pm() {
        let dt = parse_datetime("2026-03-05 2pm", UTC).unwrap();
        assert_eq!(dt.hour(), 14);
    }

    #[test]
    fn test_parse_natural_pm_with_minutes() {
        let dt = parse_datetime("2026-03-05 2:30pm", UTC).unwrap();
        assert_eq!(dt.hour(), 14);
        assert_eq!(dt.minute(), 30);
    }

    #[test]
    fn test_parse_noon_and_midnight() {
        assert_eq!(parse_datetime("2026-03-05 12pm", UTC).unwrap().hour(), 12);
        assert_eq!(parse_datetime("2026-03-05 12am", UTC).unwrap().hour(), 0);
    }

    #[test]
    fn test_format_for_ics() {
        let dt = parse_datetime("2026-03-05T14:30:00Z", UTC).unwrap();
        assert_eq!(format_for_ics(&dt), "20260305T143000Z");
    }

    #[test]
    fn test_format_date_for_ics() {
        let d = NaiveDate::from_ymd_opt(2026, 6, 25).unwrap();
        assert_eq!(format_date_for_ics(&d), "20260625");
    }

    // ── Zone-aware naive interpretation ───────────────────────────────────

    #[test]
    fn naive_time_interpreted_in_zone_summer() {
        // 14:00 CEST (+02:00) == 12:00 UTC.
        let dt = parse_datetime("2026-06-25 14:00", AMS).unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-06-25T12:00:00+00:00");
    }

    #[test]
    fn naive_time_interpreted_in_zone_winter() {
        // 14:00 CET (+01:00) == 13:00 UTC — DST handled automatically.
        let dt = parse_datetime("2026-01-15 14:00", AMS).unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-01-15T13:00:00+00:00");
    }

    #[test]
    fn dst_gap_is_rejected() {
        // 02:30 on 2026-03-29 never occurs in Amsterdam (spring forward).
        assert!(parse_datetime("2026-03-29 02:30", AMS).is_err());
    }

    #[test]
    fn dst_fold_picks_earlier_instant() {
        // 02:30 on 2026-10-25 happens twice in Amsterdam; we take the earlier
        // (CEST, +02:00) instant → 00:30 UTC.
        let dt = parse_datetime("2026-10-25 02:30", AMS).unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-10-25T00:30:00+00:00");
    }

    // ── Date-only role: start vs end of range ─────────────────────────────

    #[test]
    fn date_only_from_is_local_start_of_day() {
        // 2026-06-22 in Amsterdam summer → 00:00 CEST == prev day 22:00Z.
        let dt = parse_datetime("2026-06-22", AMS).unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-06-21T22:00:00+00:00");
    }

    #[test]
    fn date_only_to_is_next_local_midnight() {
        // Exclusive end: through end of 2026-06-22 local → 2026-06-23 00:00 CEST.
        let dt = parse_range_end("2026-06-22", AMS).unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-06-22T22:00:00+00:00");
    }

    #[test]
    fn today_to_today_covers_full_local_day() {
        // The classic zero-width-window bug: with half-open ranges the window
        // is a full 24h local day, never empty.
        let from = parse_datetime("today", AMS).unwrap();
        let to = parse_range_end("today", AMS).unwrap();
        assert_eq!((to - from).num_hours(), 24);
    }

    #[test]
    fn range_end_with_explicit_time_is_used_verbatim() {
        let dt = parse_range_end("2026-06-22 17:00", AMS).unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-06-22T15:00:00+00:00");
    }

    // ── today / tomorrow ──────────────────────────────────────────────────

    #[test]
    fn classify_today_is_local_date() {
        let spec = classify("today", UTC).unwrap();
        assert_eq!(spec, TimeSpec::Date(Utc::now().date_naive()));
    }

    #[test]
    fn classify_tomorrow_is_local_date_plus_one() {
        let spec = classify("TOMORROW", UTC).unwrap();
        let expected = Utc::now().date_naive() + chrono::Duration::days(1);
        assert_eq!(spec, TimeSpec::Date(expected));
    }

    #[test]
    fn classify_date_only_returns_date_variant() {
        let spec = classify("2026-06-25", AMS).unwrap();
        assert_eq!(
            spec,
            TimeSpec::Date(NaiveDate::from_ymd_opt(2026, 6, 25).unwrap())
        );
    }

    #[test]
    fn classify_timed_returns_instant_variant() {
        let spec = classify("2026-06-25 09:00", AMS).unwrap();
        assert!(matches!(spec, TimeSpec::Instant(_)));
    }

    #[test]
    fn unsupported_format_errors() {
        assert!(parse_datetime("next thursday-ish", UTC).is_err());
    }
}
