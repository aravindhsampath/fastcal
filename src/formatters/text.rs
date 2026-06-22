// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Text output formatter — the UTC→local display boundary.
//!
//! Event instants are stored in UTC; here they are rendered in the resolved
//! IANA zone with an explicit `CEST (+02:00)` style label. All-day events are
//! shown date-only and never given a time or offset.

use crate::models::Event;
use anyhow::Result;
use chrono::{DateTime, NaiveDate};
use chrono_tz::Tz;

/// chrono format fragment for the zone label, e.g. ` CEST (+02:00)`.
const ZONE_LABEL: &str = "%Z (%:z)";

/// Render a stored RFC3339 datetime string in `tz` using `fmt`. Falls back to
/// the raw string if it can't be parsed.
fn fmt_local(datetime: &str, tz: Tz, fmt: &str) -> String {
    match DateTime::parse_from_rfc3339(datetime) {
        Ok(dt) => dt.with_timezone(&tz).format(fmt).to_string(),
        Err(_) => datetime.to_string(),
    }
}

/// Render an all-day event's date span (date-only, no zone). `start` and
/// `end` are both inclusive (fastcal's human-facing convention).
fn format_all_day(event: &Event) -> String {
    let start = NaiveDate::parse_from_str(&event.start.datetime, "%Y-%m-%d").ok();
    let last = NaiveDate::parse_from_str(&event.end.datetime, "%Y-%m-%d").ok();
    match (start, last) {
        (Some(s), Some(last)) => {
            if last <= s {
                format!("{} (all-day)", s.format("%a %b %d, %Y"))
            } else {
                format!(
                    "{} – {} (all-day)",
                    s.format("%a %b %d, %Y"),
                    last.format("%a %b %d, %Y")
                )
            }
        }
        _ => format!("{} (all-day)", event.start.datetime),
    }
}

/// Format multiple events as human-readable text
pub fn format_events(events: &[Event], tz: Tz) -> Result<String> {
    if events.is_empty() {
        return Ok("No events found.".to_string());
    }

    let mut output = String::new();
    output.push_str(&format!("Found {} event(s):\n\n", events.len()));

    for (i, event) in events.iter().enumerate() {
        if i > 0 {
            output.push_str("\n---\n\n");
        }
        output.push_str(&format_event_compact(event, tz)?);
    }

    Ok(output)
}

/// Format a single event as human-readable text (detailed)
pub fn format_event(event: &Event, tz: Tz) -> Result<String> {
    let mut output = String::new();

    output.push_str(&format!("Event: {}\n", event.summary));
    output.push_str(&format!("ID: {}\n", event.id));

    let when = if event.all_day {
        format_all_day(event)
    } else {
        let start = fmt_local(
            &event.start.datetime,
            tz,
            &format!("%A, %B %d, %Y at %I:%M %p {ZONE_LABEL}"),
        );
        let end = fmt_local(&event.end.datetime, tz, &format!("%I:%M %p {ZONE_LABEL}"));
        format!("{} - {}", start, end)
    };
    output.push_str(&format!("When: {}\n", when));

    if let Some(duration) = event.duration_minutes {
        output.push_str(&format!("Duration: {} minutes\n", duration));
    }

    if let Some(ref location) = event.location {
        output.push_str(&format!("Location: {}\n", location));
    }

    if let Some(ref description) = event.description {
        output.push_str(&format!("Description: {}\n", description));
    }

    if let Some(ref attendees) = event.attendees {
        output.push_str("Attendees:\n");
        for attendee in attendees {
            let status = attendee
                .status
                .as_ref()
                .map(|s| format!(" ({})", s))
                .unwrap_or_default();
            output.push_str(&format!("  - {}{}\n", attendee.email, status));
        }
    }

    if let Some(ref calendar) = event.calendar {
        output.push_str(&format!("Calendar: {}\n", calendar));
    }

    Ok(output)
}

/// Format a single event as compact text (for lists)
fn format_event_compact(event: &Event, tz: Tz) -> Result<String> {
    let mut output = String::new();

    let when = if event.all_day {
        format_all_day(event)
    } else {
        fmt_local(
            &event.start.datetime,
            tz,
            &format!("%a %b %d, %I:%M %p {ZONE_LABEL}"),
        )
    };

    output.push_str(&format!("📅 {}\n", event.summary));
    output.push_str(&format!("   {}\n", when));

    if let Some(ref location) = event.location {
        output.push_str(&format!("   📍 {}\n", location));
    }

    if let Some(duration) = event.duration_minutes {
        output.push_str(&format!("   ⏱️  {} min\n", duration));
    }

    output.push_str(&format!("   ID: {}\n", event.id));

    Ok(output)
}

/// Format search results as text
pub fn format_search_results(query: &str, matches: &[Event], tz: Tz) -> Result<String> {
    let mut output = String::new();

    output.push_str(&format!(
        "Search results for '{}': {} match(es)\n\n",
        query,
        matches.len()
    ));

    if matches.is_empty() {
        output.push_str("No events found matching your query.\n");
    } else {
        for (i, event) in matches.iter().enumerate() {
            if i > 0 {
                output.push('\n');
            }
            output.push_str(&format_event_compact(event, tz)?);
        }
    }

    Ok(output)
}

/// Format conflict detection results as text. `proposed_start`/`proposed_end`
/// are RFC3339 strings; they are shown in `tz`.
pub fn format_conflicts(
    proposed_start: &str,
    proposed_end: &str,
    conflicts: &[Event],
    tz: Tz,
) -> Result<String> {
    let mut output = String::new();

    let start = fmt_local(
        proposed_start,
        tz,
        &format!("%A, %B %d, %Y at %I:%M %p {ZONE_LABEL}"),
    );
    let end = fmt_local(proposed_end, tz, &format!("%I:%M %p {ZONE_LABEL}"));

    output.push_str(&format!("Checking for conflicts: {} - {}\n\n", start, end));

    if conflicts.is_empty() {
        output.push_str("✓ No conflicts found. Time slot is available!\n");
    } else {
        output.push_str(&format!("⚠️  Found {} conflict(s):\n\n", conflicts.len()));
        for (i, event) in conflicts.iter().enumerate() {
            if i > 0 {
                output.push('\n');
            }
            output.push_str(&format_event_compact(event, tz)?);
        }
    }

    Ok(output)
}

/// Format a list of calendars as human-readable text
pub fn format_calendars(calendars: &[(String, serde_json::Value)]) -> Result<String> {
    if calendars.is_empty() {
        return Ok("No calendars found.".to_string());
    }
    let mut output = format!("Found {} calendar(s):\n\n", calendars.len());
    for (name, cal) in calendars {
        output.push_str(&format!("📆 {}\n", name));
        if let Some(href) = cal.get("href").and_then(|v| v.as_str()) {
            output.push_str(&format!("   href: {}\n", href));
        }
        if let Some(dn) = cal.get("display_name").and_then(|v| v.as_str()) {
            output.push_str(&format!("   display name: {}\n", dn));
        }
        output.push('\n');
    }
    Ok(output.trim_end().to_string())
}

/// Format a single calendar as human-readable text
pub fn format_calendar_info(name: &str, cal: &serde_json::Value) -> Result<String> {
    let mut output = String::new();
    output.push_str(&format!("📆 {}\n", name));
    if let Some(href) = cal.get("href").and_then(|v| v.as_str()) {
        output.push_str(&format!("   href:          {}\n", href));
    }
    if let Some(dn) = cal.get("display_name").and_then(|v| v.as_str()) {
        output.push_str(&format!("   display name:  {}\n", dn));
    }
    if let Some(desc) = cal.get("description").and_then(|v| v.as_str()) {
        output.push_str(&format!("   description:   {}\n", desc));
    }
    if let Some(color) = cal.get("color").and_then(|v| v.as_str()) {
        output.push_str(&format!("   color:         {}\n", color));
    }
    Ok(output.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::EventDateTime;

    fn timed_event() -> Event {
        Event {
            id: "evt-1".into(),
            href: "/evt-1.ics".into(),
            calendar: None,
            summary: "Standup".into(),
            description: None,
            // 12:00 CEST == 10:00 UTC
            start: EventDateTime::new("2026-06-22T10:00:00+00:00".into(), Some("UTC".into())),
            end: EventDateTime::new("2026-06-22T11:00:00+00:00".into(), Some("UTC".into())),
            duration_minutes: Some(60),
            location: None,
            attendees: None,
            status: None,
            created: None,
            modified: None,
            organizer: None,
            all_day: false,
            etag: None,
            rrule: None,
            recurrence_id: None,
            reminders: vec![],
        }
    }

    fn all_day_event() -> Event {
        Event {
            all_day: true,
            // inclusive start == end → single day (06-25)
            start: EventDateTime::new("2026-06-25".into(), None),
            end: EventDateTime::new("2026-06-25".into(), None),
            duration_minutes: None,
            ..timed_event()
        }
    }

    #[test]
    fn timed_event_renders_in_zone_with_label() {
        let ams = Tz::Europe__Amsterdam;
        let out = format_event(&timed_event(), ams).unwrap();
        // 10:00 UTC → 12:00 CEST (+02:00); no bare "UTC".
        assert!(out.contains("12:00 PM"), "got: {out}");
        assert!(out.contains("CEST"), "got: {out}");
        assert!(out.contains("+02:00"), "got: {out}");
        assert!(!out.contains(" UTC"), "should not print UTC: {out}");
    }

    #[test]
    fn timed_event_winter_uses_cet() {
        let ams = Tz::Europe__Amsterdam;
        let mut ev = timed_event();
        ev.start = EventDateTime::new("2026-01-15T10:00:00+00:00".into(), Some("UTC".into()));
        ev.end = EventDateTime::new("2026-01-15T11:00:00+00:00".into(), Some("UTC".into()));
        let out = format_event(&ev, ams).unwrap();
        // 10:00 UTC → 11:00 CET (+01:00) in winter.
        assert!(out.contains("11:00 AM"), "got: {out}");
        assert!(out.contains("CET"), "got: {out}");
        assert!(out.contains("+01:00"), "got: {out}");
    }

    #[test]
    fn all_day_event_is_date_only_no_zone() {
        let ams = Tz::Europe__Amsterdam;
        let out = format_event(&all_day_event(), ams).unwrap();
        assert!(out.contains("all-day"), "got: {out}");
        assert!(out.contains("Jun 25, 2026"), "got: {out}");
        assert!(
            !out.contains("CEST"),
            "all-day must not carry a zone: {out}"
        );
        assert!(
            !out.contains(":00 "),
            "all-day must not carry a time: {out}"
        );
    }
}
