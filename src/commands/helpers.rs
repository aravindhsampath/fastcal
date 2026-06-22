// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared helpers for command implementations

use crate::caldav;
use crate::commands::batch::EventInput;
use crate::config::Config;
use crate::models::{Event, EventDateTime, Reminder};
use crate::parsers::datetime::{classify, format_date_for_ics, format_for_ics, TimeSpec};
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;

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

        // Fast uid.ics path scoped to this one calendar — avoids downloading
        // and parsing the whole calendar just to find one event by id.
        caldav::event::find_event_in_calendar(client, cal, &calendar_href, event_id)
            .await
            .context("Failed to search for event")?
            .with_context(|| format!("Event '{}' not found in calendar '{}'", event_id, cal))
    } else {
        caldav::event::find_event_by_id(client, event_id, &config.calendars)
            .await
            .context("Failed to search for event")?
            .with_context(|| format!("Event '{}' not found in any calendar", event_id))
    }
}

/// The start/end of an event resolved into both ICS wire values and the
/// pieces needed to render a preview, computed once so a dry-run and the real
/// create never disagree.
pub(crate) struct ResolvedEventTimes {
    /// Whether this is an all-day (date-only) event.
    pub all_day: bool,
    /// ICS `DTSTART` value: `YYYYMMDDTHHMMSSZ` (timed) or `YYYYMMDD` (all-day).
    pub start_ics: String,
    /// ICS `DTEND` value. For all-day this is the RFC 5545 *exclusive* end
    /// (the morning after the last day).
    pub end_ics: String,
    /// Timed events: the UTC instants. `None` for all-day.
    pub start_utc: Option<DateTime<Utc>>,
    pub end_utc: Option<DateTime<Utc>>,
    /// All-day events: inclusive first/last day. `None` for timed.
    pub start_date: Option<NaiveDate>,
    pub end_date_inclusive: Option<NaiveDate>,
}

/// Resolve an [`EventInput`]'s start/end in zone `tz`.
///
/// A date-only `start` makes an **all-day** event; a date-only `end` is the
/// inclusive last day (stored as the exclusive RFC 5545 `DTEND`). A timed
/// `start` makes a timed event, defaulting the end to `duration` minutes or
/// one hour. Mixing a date with a time across start/end is rejected.
pub(crate) fn resolve_event_times(event_input: &EventInput, tz: Tz) -> Result<ResolvedEventTimes> {
    let start_spec = classify(&event_input.start, tz)
        .with_context(|| format!("Failed to parse start time: {}", event_input.start))?;

    match start_spec {
        TimeSpec::Date(start_date) => {
            let end_inclusive = match &event_input.end {
                Some(end_str) => {
                    match classify(end_str, tz)
                        .with_context(|| format!("Failed to parse end time: {end_str}"))?
                    {
                        TimeSpec::Date(d) => d,
                        TimeSpec::Instant(_) => anyhow::bail!(
                            "all-day event has a date-only start ('{}') but a timed end ('{}'); \
                             give both as dates (YYYY-MM-DD) or both with times",
                            event_input.start,
                            end_str
                        ),
                    }
                }
                None => start_date, // single day
            };
            if end_inclusive < start_date {
                anyhow::bail!("end date {end_inclusive} is before start date {start_date}");
            }
            let end_exclusive = end_inclusive
                .succ_opt()
                .with_context(|| format!("end date {end_inclusive} is out of range"))?;
            Ok(ResolvedEventTimes {
                all_day: true,
                start_ics: format_date_for_ics(&start_date),
                end_ics: format_date_for_ics(&end_exclusive),
                start_utc: None,
                end_utc: None,
                start_date: Some(start_date),
                end_date_inclusive: Some(end_inclusive),
            })
        }
        TimeSpec::Instant(start_dt) => {
            let end_dt = if let Some(end_str) = &event_input.end {
                match classify(end_str, tz)
                    .with_context(|| format!("Failed to parse end time: {end_str}"))?
                {
                    TimeSpec::Instant(d) => d,
                    TimeSpec::Date(_) => anyhow::bail!(
                        "event has a timed start ('{}') but a date-only end ('{}'); \
                         give both with times or both as dates",
                        event_input.start,
                        end_str
                    ),
                }
            } else if let Some(dur_mins) = event_input.duration {
                start_dt + chrono::Duration::minutes(dur_mins as i64)
            } else {
                start_dt + chrono::Duration::hours(1)
            };
            if end_dt < start_dt {
                anyhow::bail!("end time is before start time");
            }
            Ok(ResolvedEventTimes {
                all_day: false,
                start_ics: format_for_ics(&start_dt),
                end_ics: format_for_ics(&end_dt),
                start_utc: Some(start_dt),
                end_utc: Some(end_dt),
                start_date: None,
                end_date_inclusive: None,
            })
        }
    }
}

/// Rewrite an event's displayable datetimes into the resolved zone (e.g.
/// `…+02:00`) and stamp the zone name, for JSON output. All-day events stay
/// date-only and untouched; audit timestamps (`created`/`modified`) keep
/// their UTC form.
pub(crate) fn localize_event_times(event: &mut Event, tz: Tz) {
    if event.all_day {
        return;
    }
    localize_field(&mut event.start, tz);
    localize_field(&mut event.end, tz);
    if let Some(rid) = &event.recurrence_id {
        if let Ok(dt) = DateTime::parse_from_rfc3339(rid) {
            event.recurrence_id = Some(dt.with_timezone(&tz).to_rfc3339());
        }
    }
}

fn localize_field(field: &mut EventDateTime, tz: Tz) {
    if let Ok(dt) = DateTime::parse_from_rfc3339(&field.datetime) {
        field.datetime = dt.with_timezone(&tz).to_rfc3339();
        field.timezone = Some(tz.name().to_string());
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
    tz: Tz,
) -> Result<String> {
    // Resolve start/end (handles timed vs all-day) into ICS wire values.
    let times = resolve_event_times(event_input, tz)?;
    let start_ics = times.start_ics;
    let end_ics = times.end_ics;

    // Generate UID
    let uid = uuid::Uuid::new_v4().to_string();

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::batch::EventInput;

    fn input(start: &str, end: Option<&str>) -> EventInput {
        EventInput {
            summary: "X".into(),
            start: start.into(),
            end: end.map(|s| s.to_owned()),
            duration: None,
            location: None,
            description: None,
            attendees: None,
            reminder_minutes: None,
        }
    }

    #[test]
    fn timed_create_interprets_naive_in_zone() {
        // 14:00 CEST → 12:00Z, default +1h end.
        let t =
            resolve_event_times(&input("2026-06-25 14:00", None), Tz::Europe__Amsterdam).unwrap();
        assert!(!t.all_day);
        assert_eq!(t.start_ics, "20260625T120000Z");
        assert_eq!(t.end_ics, "20260625T130000Z");
    }

    #[test]
    fn all_day_single_day_uses_exclusive_next_day_dtend() {
        let t = resolve_event_times(&input("2026-06-25", None), Tz::UTC).unwrap();
        assert!(t.all_day);
        assert_eq!(t.start_ics, "20260625");
        assert_eq!(t.end_ics, "20260626");
    }

    #[test]
    fn all_day_inclusive_end_becomes_exclusive() {
        // 25th..27th inclusive → DTEND 28th.
        let t = resolve_event_times(&input("2026-06-25", Some("2026-06-27")), Tz::UTC).unwrap();
        assert_eq!(t.start_ics, "20260625");
        assert_eq!(t.end_ics, "20260628");
        assert_eq!(t.end_date_inclusive.unwrap().to_string(), "2026-06-27");
    }

    #[test]
    fn mixing_date_start_and_timed_end_errors() {
        assert!(
            resolve_event_times(&input("2026-06-25", Some("2026-06-27 14:00")), Tz::UTC).is_err()
        );
    }

    #[test]
    fn mixing_timed_start_and_date_end_errors() {
        assert!(
            resolve_event_times(&input("2026-06-25 09:00", Some("2026-06-27")), Tz::UTC).is_err()
        );
    }

    #[test]
    fn duration_minutes_used_when_no_end() {
        let mut i = input("2026-06-25 09:00", None);
        i.duration = Some(90);
        let t = resolve_event_times(&i, Tz::UTC).unwrap();
        assert_eq!(t.start_ics, "20260625T090000Z");
        assert_eq!(t.end_ics, "20260625T103000Z");
    }

    #[test]
    fn localize_event_times_rewrites_timed_into_zone() {
        let mut ev = Event {
            id: "e".into(),
            href: "/e.ics".into(),
            calendar: None,
            summary: "S".into(),
            description: None,
            start: EventDateTime::new("2026-06-25T12:00:00+00:00".into(), Some("UTC".into())),
            end: EventDateTime::new("2026-06-25T13:00:00+00:00".into(), Some("UTC".into())),
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
        };
        localize_event_times(&mut ev, Tz::Europe__Amsterdam);
        assert_eq!(ev.start.datetime, "2026-06-25T14:00:00+02:00");
        assert_eq!(ev.start.timezone.as_deref(), Some("Europe/Amsterdam"));
    }

    #[test]
    fn localize_event_times_leaves_all_day_untouched() {
        let mut ev = Event {
            id: "e".into(),
            href: "/e.ics".into(),
            calendar: None,
            summary: "S".into(),
            description: None,
            start: EventDateTime::new("2026-06-25".into(), None),
            end: EventDateTime::new("2026-06-26".into(), None),
            duration_minutes: None,
            location: None,
            attendees: None,
            status: None,
            created: None,
            modified: None,
            organizer: None,
            all_day: true,
            etag: None,
            rrule: None,
            recurrence_id: None,
            reminders: vec![],
        };
        localize_event_times(&mut ev, Tz::Europe__Amsterdam);
        assert_eq!(ev.start.datetime, "2026-06-25");
        assert!(ev.start.timezone.is_none());
    }
}
