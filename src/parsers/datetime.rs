// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Date/time parsing
//!
//! Parses various datetime formats into DateTime<Utc>.

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

#[cfg(test)]
use chrono::Timelike;

/// Parse a datetime string supporting multiple formats:
/// - ISO 8601: "2026-03-05T14:30:00Z"
/// - Date only: "2026-03-05" (defaults to 00:00 UTC)
/// - Date with time: "2026-03-05 14:30"
/// - Natural format: "2026-03-05 2pm", "2026-03-05 2:30pm"
pub fn parse_datetime(input: &str) -> Result<DateTime<Utc>> {
    let input = input.trim();

    // Handle relative dates
    match input.to_lowercase().as_str() {
        "today" => {
            let today = Utc::now().date_naive();
            return Ok(today
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(Utc)
                .unwrap());
        }
        "tomorrow" => {
            let tomorrow = Utc::now().date_naive() + chrono::Duration::days(1);
            return Ok(tomorrow
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(Utc)
                .unwrap());
        }
        _ => {}
    }

    // Try ISO 8601 first
    if let Ok(dt) = DateTime::parse_from_rfc3339(input) {
        return Ok(dt.with_timezone(&Utc));
    }

    // Try date only (YYYY-MM-DD)
    if input.len() == 10 && input.contains('-') {
        let date = NaiveDate::parse_from_str(input, "%Y-%m-%d").context("Failed to parse date")?;
        return Ok(date
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(Utc)
            .unwrap());
    }

    // Try "YYYY-MM-DD HH:MM" format
    if let Ok(dt) = NaiveDateTime::parse_from_str(input, "%Y-%m-%d %H:%M") {
        return Ok(dt.and_local_timezone(Utc).unwrap());
    }

    // Try "YYYY-MM-DD HH:MM:SS" format
    if let Ok(dt) = NaiveDateTime::parse_from_str(input, "%Y-%m-%d %H:%M:%S") {
        return Ok(dt.and_local_timezone(Utc).unwrap());
    }

    // Try natural formats like "2026-03-05 2pm", "2026-03-05 2:30pm"
    if let Some((date_part, time_part)) = input.split_once(' ') {
        if let Ok(date) = NaiveDate::parse_from_str(date_part, "%Y-%m-%d") {
            if let Some(dt) = parse_natural_time(date, time_part) {
                return Ok(dt);
            }
        }
    }

    anyhow::bail!("Unsupported datetime format: '{}'. Try ISO 8601 (2026-03-05T14:30:00Z) or YYYY-MM-DD HH:MM", input)
}

/// Parse natural time formats like "2pm", "2:30pm", "14:30"
fn parse_natural_time(date: NaiveDate, time_str: &str) -> Option<DateTime<Utc>> {
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
                return date
                    .and_hms_opt(hour, min, 0)
                    .map(|dt| dt.and_local_timezone(Utc).unwrap());
            }
        } else {
            // "2pm" format
            if let Ok(mut hour) = time_part.parse::<u32>() {
                if is_pm && hour != 12 {
                    hour += 12;
                } else if !is_pm && hour == 12 {
                    hour = 0;
                }
                return date
                    .and_hms_opt(hour, 0, 0)
                    .map(|dt| dt.and_local_timezone(Utc).unwrap());
            }
        }
    }

    // Handle "14:30" format (24-hour)
    if let Some((hour_str, min_str)) = time_str.split_once(':') {
        if let (Ok(hour), Ok(min)) = (hour_str.parse::<u32>(), min_str.parse::<u32>()) {
            return date
                .and_hms_opt(hour, min, 0)
                .map(|dt| dt.and_local_timezone(Utc).unwrap());
        }
    }

    None
}

/// Format DateTime for iCalendar format (YYYYMMDDTHHMMSSexpZ)
pub fn format_for_ics(dt: &DateTime<Utc>) -> String {
    dt.format("%Y%m%dT%H%M%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_iso8601() {
        let dt = parse_datetime("2026-03-05T14:30:00Z").unwrap();
        assert_eq!(dt.hour(), 14);
        assert_eq!(dt.minute(), 30);
    }

    #[test]
    fn test_parse_date_only() {
        let dt = parse_datetime("2026-03-05").unwrap();
        assert_eq!(dt.hour(), 0);
        assert_eq!(dt.minute(), 0);
    }

    #[test]
    fn test_parse_datetime_space() {
        let dt = parse_datetime("2026-03-05 14:30").unwrap();
        assert_eq!(dt.hour(), 14);
        assert_eq!(dt.minute(), 30);
    }

    #[test]
    fn test_parse_natural_pm() {
        let dt = parse_datetime("2026-03-05 2pm").unwrap();
        assert_eq!(dt.hour(), 14);
        assert_eq!(dt.minute(), 0);
    }

    #[test]
    fn test_parse_natural_pm_with_minutes() {
        let dt = parse_datetime("2026-03-05 2:30pm").unwrap();
        assert_eq!(dt.hour(), 14);
        assert_eq!(dt.minute(), 30);
    }

    #[test]
    fn test_parse_natural_am() {
        let dt = parse_datetime("2026-03-05 9am").unwrap();
        assert_eq!(dt.hour(), 9);
    }

    #[test]
    fn test_parse_noon() {
        let dt = parse_datetime("2026-03-05 12pm").unwrap();
        assert_eq!(dt.hour(), 12);
    }

    #[test]
    fn test_parse_midnight() {
        let dt = parse_datetime("2026-03-05 12am").unwrap();
        assert_eq!(dt.hour(), 0);
    }

    #[test]
    fn test_format_for_ics() {
        let dt = parse_datetime("2026-03-05T14:30:00Z").unwrap();
        let formatted = format_for_ics(&dt);
        assert_eq!(formatted, "20260305T143000Z");
    }

    #[test]
    fn test_parse_today() {
        let dt = parse_datetime("today").unwrap();
        let today = Utc::now().date_naive();
        assert_eq!(dt.date_naive(), today);
        assert_eq!(dt.hour(), 0);
        assert_eq!(dt.minute(), 0);
    }

    #[test]
    fn test_parse_tomorrow() {
        let dt = parse_datetime("tomorrow").unwrap();
        let tomorrow = (Utc::now() + chrono::Duration::days(1)).date_naive();
        assert_eq!(dt.date_naive(), tomorrow);
        assert_eq!(dt.hour(), 0);
        assert_eq!(dt.minute(), 0);
    }

    #[test]
    fn test_parse_today_case_insensitive() {
        let dt = parse_datetime("TODAY").unwrap();
        let today = Utc::now().date_naive();
        assert_eq!(dt.date_naive(), today);
    }
}
