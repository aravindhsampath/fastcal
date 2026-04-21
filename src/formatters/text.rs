// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Text output formatter
//!
//! Provides human-readable text output for terminal display.

use crate::models::Event;
use anyhow::Result;
use chrono::DateTime;

/// Format multiple events as human-readable text
pub fn format_events(events: &[Event]) -> Result<String> {
    if events.is_empty() {
        return Ok("No events found.".to_string());
    }

    let mut output = String::new();

    output.push_str(&format!("Found {} event(s):\n\n", events.len()));

    for (i, event) in events.iter().enumerate() {
        if i > 0 {
            output.push_str("\n---\n\n");
        }
        output.push_str(&format_event_compact(event)?);
    }

    Ok(output)
}

/// Format a single event as human-readable text (detailed)
pub fn format_event(event: &Event) -> Result<String> {
    let mut output = String::new();

    output.push_str(&format!("Event: {}\n", event.summary));
    output.push_str(&format!("ID: {}\n", event.id));

    // Parse and format datetime
    let start_dt = DateTime::parse_from_rfc3339(&event.start.datetime)
        .map(|dt| dt.format("%A, %B %d, %Y at %I:%M %p UTC").to_string())
        .unwrap_or_else(|_| event.start.datetime.clone());

    let end_dt = DateTime::parse_from_rfc3339(&event.end.datetime)
        .map(|dt| dt.format("%I:%M %p UTC").to_string())
        .unwrap_or_else(|_| event.end.datetime.clone());

    output.push_str(&format!("When: {} - {}\n", start_dt, end_dt));

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
fn format_event_compact(event: &Event) -> Result<String> {
    let mut output = String::new();

    // Parse and format datetime
    let start_dt = DateTime::parse_from_rfc3339(&event.start.datetime)
        .map(|dt| dt.format("%a %b %d, %I:%M %p UTC").to_string())
        .unwrap_or_else(|_| event.start.datetime.clone());

    output.push_str(&format!("📅 {}\n", event.summary));
    output.push_str(&format!("   {}\n", start_dt));

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
pub fn format_search_results(query: &str, matches: &[Event]) -> Result<String> {
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
            output.push_str(&format_event_compact(event)?);
        }
    }

    Ok(output)
}

/// Format conflict detection results as text
pub fn format_conflicts(
    proposed_start: &str,
    proposed_end: &str,
    conflicts: &[Event],
) -> Result<String> {
    let mut output = String::new();

    let start_dt = DateTime::parse_from_rfc3339(proposed_start)
        .map(|dt| dt.format("%A, %B %d, %Y at %I:%M %p UTC").to_string())
        .unwrap_or_else(|_| proposed_start.to_string());

    let end_dt = DateTime::parse_from_rfc3339(proposed_end)
        .map(|dt| dt.format("%I:%M %p UTC").to_string())
        .unwrap_or_else(|_| proposed_end.to_string());

    output.push_str(&format!(
        "Checking for conflicts: {} - {}\n\n",
        start_dt, end_dt
    ));

    if conflicts.is_empty() {
        output.push_str("✓ No conflicts found. Time slot is available!\n");
    } else {
        output.push_str(&format!("⚠️  Found {} conflict(s):\n\n", conflicts.len()));
        for (i, event) in conflicts.iter().enumerate() {
            if i > 0 {
                output.push('\n');
            }
            output.push_str(&format_event_compact(event)?);
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
