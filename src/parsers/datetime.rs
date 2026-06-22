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

    // A `YYYY-MM-DD` prefix. We validate the date deterministically so a
    // well-formed-but-impossible date ("2026-02-31") gets a precise error
    // here instead of a generic one (or a confusing server rejection later).
    // The char-boundary guard keeps a multibyte input from ever panicking.
    if input.len() >= 10 && input.is_char_boundary(10) && looks_like_date_prefix(&input[..10]) {
        let date = NaiveDate::parse_from_str(&input[..10], "%Y-%m-%d").map_err(|_| {
            anyhow::anyhow!(
                "invalid date '{}': that calendar date does not exist",
                &input[..10]
            )
        })?;
        if input.len() == 10 {
            return Ok(TimeSpec::Date(date));
        }
        // A time-of-day follows the date — the date is known good, so any
        // failure here is specifically a bad *time*.
        let rest = input[10..].trim_start();
        if let Some(naive) = parse_natural_time(date, rest) {
            return Ok(TimeSpec::Instant(local_to_utc(naive, tz)?));
        }
        anyhow::bail!(
            "invalid time '{rest}' for {date}: try HH:MM (14:30), 2:30pm, noon, \
             'half past 2', or 'quarter to 9'"
        );
    }

    anyhow::bail!(
        "Unsupported datetime format: '{input}'. Try ISO 8601 (2026-03-05T14:30:00Z), \
         YYYY-MM-DD, or 'YYYY-MM-DD HH:MM'"
    )
}

/// True if `s` has exactly the 10-byte shape `YYYY-MM-DD` (digits + dashes).
fn looks_like_date_prefix(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5].is_ascii_digit()
        && b[6].is_ascii_digit()
        && b[7] == b'-'
        && b[8].is_ascii_digit()
        && b[9].is_ascii_digit()
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

/// Parse a time-of-day on `date`. Zone application happens in the caller.
///
/// Supported (all optionally with an `am`/`pm` suffix on the hour):
/// - `noon` / `midday` → 12:00, `midnight` → 00:00
/// - `14:30`, `2:30pm`, `9:30:00`
/// - `2pm`, bare hour `17` → 17:00 (a bare hour is read as 24-hour)
/// - `half past 6` → 6:30, `quarter past 9` → 9:15, `quarter to 9` → 8:45
/// - `5 minutes to 9` / `5 to 9` → 8:55, `10 minutes past 9` → 9:10
///
/// A bare `half 9` is intentionally NOT accepted: it's 9:30 in English but
/// 8:30 in Dutch/German, so we require the unambiguous `half past`.
fn parse_natural_time(date: NaiveDate, time_str: &str) -> Option<NaiveDateTime> {
    let s = time_str.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }

    // Word anchors.
    match s.as_str() {
        "noon" | "midday" => return date.and_hms_opt(12, 0, 0),
        "midnight" => return date.and_hms_opt(0, 0, 0),
        _ => {}
    }

    // Pull off an am/pm suffix; it applies to whatever hour we resolve.
    let (body, ampm) = strip_ampm(&s);

    // "<minutes> past <hour>" / "<minutes> to <hour>" (quarter / half / number).
    for (sep, is_to) in [(" past ", false), (" to ", true)] {
        if let Some((mpart, hpart)) = body.split_once(sep) {
            let min = parse_minute_word(mpart)?;
            let hour = apply_ampm(hpart.trim().parse::<u32>().ok()?, ampm);
            return if is_to {
                if min > 60 || hour == 0 {
                    return None; // would cross the day boundary — leave to the caller
                }
                date.and_hms_opt(hour - 1, 60 - min, 0)
            } else {
                date.and_hms_opt(hour, min, 0)
            };
        }
    }

    // "H:MM" or "H:MM:SS".
    let parts: Vec<&str> = body.split(':').collect();
    if parts.len() == 2 || parts.len() == 3 {
        let h = apply_ampm(parts[0].trim().parse::<u32>().ok()?, ampm);
        let m = parts[1].trim().parse::<u32>().ok()?;
        let sec = if parts.len() == 3 {
            parts[2].trim().parse::<u32>().ok()?
        } else {
            0
        };
        return date.and_hms_opt(h, m, sec);
    }

    // Bare hour ("17" → 17:00; "5pm" → 17:00).
    if let Ok(h) = body.trim().parse::<u32>() {
        return date.and_hms_opt(apply_ampm(h, ampm), 0, 0);
    }

    None
}

/// Split a trailing `am`/`pm` (or `a.m.`/`p.m.`) off `s`. Returns the
/// remainder and `Some(true)` for pm, `Some(false)` for am, `None` if absent.
fn strip_ampm(s: &str) -> (String, Option<bool>) {
    let s = s.trim();
    for (suffix, is_pm) in [("p.m.", true), ("pm", true), ("a.m.", false), ("am", false)] {
        if let Some(rest) = s.strip_suffix(suffix) {
            return (rest.trim().to_string(), Some(is_pm));
        }
    }
    (s.to_string(), None)
}

/// Apply an am/pm flag to a 12-hour `hour`. With no flag the hour is taken
/// as-is (24-hour).
fn apply_ampm(hour: u32, ampm: Option<bool>) -> u32 {
    match ampm {
        Some(true) if hour != 12 => hour + 12, // pm
        Some(false) if hour == 12 => 0,        // 12am → 00
        _ => hour,
    }
}

/// Parse the minutes part of a "past/to" phrase: `quarter`→15, `half`→30,
/// or leading digits (`5`, `5 minutes`, `20 mins`).
fn parse_minute_word(w: &str) -> Option<u32> {
    match w.trim() {
        "quarter" => Some(15),
        "half" => Some(30),
        other => {
            let digits: String = other.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u32>().ok().filter(|&n| n <= 59)
        }
    }
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

    // ── Natural-language times (L2) ───────────────────────────────────────

    fn hm(input: &str) -> (u32, u32) {
        // Parse on a fixed UTC day and read back the wall-clock h:m.
        let dt = parse_datetime(&format!("2026-06-25 {input}"), UTC).unwrap();
        (dt.hour(), dt.minute())
    }

    #[test]
    fn natural_noon_and_midnight() {
        assert_eq!(hm("noon"), (12, 0));
        assert_eq!(hm("midday"), (12, 0));
        assert_eq!(hm("midnight"), (0, 0));
    }

    #[test]
    fn natural_half_and_quarter() {
        assert_eq!(hm("half past 6"), (6, 30));
        assert_eq!(hm("quarter past 9"), (9, 15));
        assert_eq!(hm("quarter to 9"), (8, 45));
        assert_eq!(hm("half past 6 pm"), (18, 30));
        assert_eq!(hm("quarter to 9 pm"), (20, 45));
    }

    #[test]
    fn natural_minutes_to_and_past() {
        assert_eq!(hm("5 minutes to 9"), (8, 55));
        assert_eq!(hm("5 to 9"), (8, 55));
        assert_eq!(hm("10 minutes past 9"), (9, 10));
        assert_eq!(hm("20 mins past 7"), (7, 20));
    }

    #[test]
    fn bare_hour_is_24h() {
        assert_eq!(hm("17"), (17, 0));
        assert_eq!(hm("12"), (12, 0)); // "lunch at 12" → noon, not midnight
        assert_eq!(hm("9"), (9, 0));
        assert_eq!(hm("9pm"), (21, 0));
    }

    #[test]
    fn natural_seconds_and_existing_forms_still_work() {
        assert_eq!(hm("14:30"), (14, 30));
        assert_eq!(hm("2:30pm"), (14, 30));
        assert_eq!(hm("9:05:30"), (9, 5));
    }

    #[test]
    fn half_bare_is_rejected_to_avoid_dutch_ambiguity() {
        // "half 9" must NOT silently become 09:30 (English) or 08:30 (Dutch).
        assert!(parse_datetime("2026-06-25 half 9", UTC).is_err());
    }

    // ── Deterministic invalid date/time errors (M2) ───────────────────────

    #[test]
    fn invalid_calendar_date_is_caught_with_clear_message() {
        let err = parse_datetime("2026-02-31 10:00", UTC)
            .unwrap_err()
            .to_string();
        assert!(err.to_lowercase().contains("invalid date"), "got: {err}");
    }

    #[test]
    fn invalid_time_on_valid_date_is_distinguished() {
        let err = parse_datetime("2026-06-25 25:00", UTC)
            .unwrap_err()
            .to_string();
        assert!(err.to_lowercase().contains("invalid time"), "got: {err}");
    }

    #[test]
    fn non_date_input_still_generic_error() {
        let err = parse_datetime("next thursday-ish", UTC)
            .unwrap_err()
            .to_string();
        assert!(err.to_lowercase().contains("unsupported"), "got: {err}");
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
