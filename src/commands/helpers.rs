// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared helpers for command implementations

use crate::caldav;
use crate::commands::batch::EventInput;
use crate::config::Config;
use crate::models::{Event, Reminder};
use anyhow::{Context, Result};

/// Find a single event by ID, optionally scoped to one calendar.
///
/// If `calendar_filter` is Some, only that calendar is searched (fast).
/// Otherwise, all calendars in config are searched via `find_event_by_id`
/// (which itself tries the fast direct-fetch path first).
pub(crate) async fn find_event_for_operation(
    client: &caldav::Client,
    config: &Config,
    calendar_filter: Option<&str>,
    event_id: &str,
) -> Result<(String, Event)> {
    if let Some(cal) = calendar_filter {
        let calendar_href = config
            .calendars
            .get(cal)
            .cloned()
            .with_context(|| calendar_not_found_error(cal, config))?;

        let events =
            caldav::event::list_events(client, &calendar_href, Some(cal.to_string()), None, None)
                .await
                .context("Failed to list events")?;

        let event = events
            .into_iter()
            .find(|e| e.id == event_id)
            .with_context(|| format!("Event '{}' not found in calendar '{}'", event_id, cal))?;

        Ok((cal.to_string(), event))
    } else {
        caldav::event::find_event_by_id(client, event_id, &config.calendars)
            .await
            .context("Failed to search for event")?
            .with_context(|| format!("Event '{}' not found in any calendar", event_id))
    }
}

/// Build ICS data and PUT an event on the server, returning the generated UID.
///
/// Shared by `events::create` and `batch::create`.
pub(crate) async fn create_event_on_server(
    client: &caldav::Client,
    calendar_href: &str,
    organizer_username: &str,
    event_input: &EventInput,
) -> Result<String> {
    // Parse start datetime
    let start_dt = crate::parsers::datetime::parse_datetime(&event_input.start)
        .with_context(|| format!("Failed to parse start time: {}", event_input.start))?;

    // Calculate end datetime
    let end_dt = if let Some(ref end_str) = event_input.end {
        crate::parsers::datetime::parse_datetime(end_str)
            .with_context(|| format!("Failed to parse end time: {}", end_str))?
    } else if let Some(dur_mins) = event_input.duration {
        start_dt + chrono::Duration::minutes(dur_mins as i64)
    } else {
        start_dt + chrono::Duration::hours(1)
    };

    // Generate UID
    let uid = uuid::Uuid::new_v4().to_string();

    // Format datetimes for ICS
    let start_ics = crate::parsers::datetime::format_for_ics(&start_dt);
    let end_ics = crate::parsers::datetime::format_for_ics(&end_dt);

    // Parse attendees
    let attendee_list = event_input.attendees.as_ref().map(|s| {
        s.split(',')
            .map(|email| email.trim().to_string())
            .collect::<Vec<_>>()
    });

    // Materialize a single-element Vec when a reminder was requested;
    // empty otherwise. `build_event` emits exactly one VALARM per entry.
    let reminders: Vec<Reminder> = event_input
        .reminder_minutes
        .map(|m| {
            vec![Reminder {
                minutes_before: m,
                action: "display".to_owned(),
                description: None,
            }]
        })
        .unwrap_or_default();

    // Build ICS event
    let ics_data = crate::parsers::ics::build_event(&crate::parsers::ics::IcsBuildArgs {
        uid: &uid,
        summary: &event_input.summary,
        start: &start_ics,
        end: &end_ics,
        description: event_input.description.as_deref(),
        location: event_input.location.as_deref(),
        organizer: Some(organizer_username),
        attendees: attendee_list.as_deref(),
        reminders: &reminders,
    })
    .context("Failed to build ICS event")?;

    // PUT event on server (with retry for transient network failures)
    let event_href = format!("{}/{}.ics", calendar_href.trim_end_matches('/'), uid);
    use libdav::dav::PutResource;
    crate::caldav::retry_transient(3, || async {
        client
            .request(PutResource::new(&event_href).create(&ics_data, "text/calendar"))
            .await
            .context("Failed to create event on server")
    })
    .await?;

    Ok(uid)
}

/// Build a descriptive "calendar not found" error message listing available calendars.
pub(crate) fn calendar_not_found_error(calendar_name: &str, config: &Config) -> String {
    if config.calendars.is_empty() {
        format!(
            "Calendar '{}' not found in config. No calendars configured — run 'fastcal config init'.",
            calendar_name
        )
    } else {
        let list = {
            let mut names: Vec<_> = config.calendars.keys().collect();
            names.sort();
            names
                .iter()
                .map(|k| format!("  - {}", k))
                .collect::<Vec<_>>()
                .join("\n")
        };
        format!(
            "Calendar '{}' not found in config.\n\nAvailable calendars:\n{}",
            calendar_name, list
        )
    }
}
