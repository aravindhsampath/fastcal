// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Events command implementations

use crate::caldav;
use crate::models::{Metadata, SuccessResponse};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::json;

/// Execute events list command
///
/// Lists events in a calendar within a date range
pub async fn list(
    ctx: &crate::commands::context::CommandContext,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
) -> Result<()> {
    let config = ctx
        .load_config()
        .context("Failed to load configuration. Run 'fastcal config init' first.")?;

    // Determine which calendar to use (CLI option > config default)
    let calendar_name = ctx
        .calendar
        .clone()
        .unwrap_or_else(|| config.preferences.default_calendar.clone());

    // Get calendar href
    let calendar_href = config
        .calendars
        .get(&calendar_name)
        .cloned()
        .with_context(|| {
            crate::commands::helpers::calendar_not_found_error(&calendar_name, &config)
        })?;

    // Create client
    let client = caldav::create_client(&config, false)
        .await
        .context("Failed to create CalDAV client")?;

    // List events
    let events = caldav::event::list_events(
        &client,
        &calendar_href,
        Some(calendar_name.clone()),
        from,
        to,
    )
    .await
    .context("Failed to list events")?;

    // Output based on format
    use crate::cli::OutputFormat;
    match ctx.format {
        OutputFormat::Text => {
            let output = crate::formatters::format_events(&events, OutputFormat::Text)?;
            println!("{}", output);
        }
        OutputFormat::Json => {
            let mut metadata = Metadata::new()
                .with_count(events.len())
                .with_calendar(calendar_name);

            if let (Some(from_dt), Some(to_dt)) = (from, to) {
                metadata = metadata.with_date_range(
                    from_dt.format("%Y-%m-%d").to_string(),
                    to_dt.format("%Y-%m-%d").to_string(),
                );
            }

            let response = SuccessResponse::with_metadata(
                json!({
                    "events": events
                }),
                metadata,
            );

            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        OutputFormat::Ics => {
            anyhow::bail!("ICS format not yet implemented");
        }
    }

    Ok(())
}

/// Execute events get command
///
/// Gets a specific event by ID
pub async fn get(ctx: &crate::commands::context::CommandContext, event_id: String) -> Result<()> {
    let config = ctx
        .load_config()
        .context("Failed to load configuration. Run 'fastcal config init' first.")?;

    // Create client
    let client = caldav::create_client(&config, false)
        .await
        .context("Failed to create CalDAV client")?;

    let (found_calendar, event) = crate::commands::helpers::find_event_for_operation(
        &client,
        &config,
        ctx.calendar.as_deref(),
        &event_id,
    )
    .await?;

    use crate::cli::OutputFormat;
    match ctx.format {
        OutputFormat::Text => {
            println!("Calendar: {}\n", found_calendar);
            let output = crate::formatters::text::format_event(&event)?;
            println!("{}", output);
        }
        OutputFormat::Json => {
            let response = SuccessResponse::new(json!({
                "event": event,
                "calendar": found_calendar,
            }));
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        OutputFormat::Ics => {
            anyhow::bail!("ICS format not yet implemented");
        }
    }

    Ok(())
}

/// CLI overrides for the create command.
///
/// All fields are optional because `--from-json` can supply the required ones.
pub struct EventCreateOverrides {
    pub summary: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub duration: Option<u32>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub attendees: Option<String>,
}

/// Execute events create command
///
/// Creates a new event in the calendar. Either --summary and --start must be
/// provided, or --from-json must point to a JSON file with those fields.
/// CLI flags override corresponding fields from the JSON file.
pub async fn create(
    ctx: &crate::commands::context::CommandContext,
    overrides: EventCreateOverrides,
    from_json: Option<String>,
) -> Result<()> {
    let EventCreateOverrides {
        summary,
        start,
        end,
        duration,
        location,
        description,
        attendees,
    } = overrides;
    let config = ctx
        .load_config()
        .context("Failed to load configuration. Run 'fastcal config init' first.")?;

    // Resolve fields: load JSON defaults first, then let CLI args override
    let (summary, start, end, duration, location, description, attendees) =
        if let Some(ref path) = from_json {
            let contents = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read JSON file: {}", path))?;
            let json_event: crate::commands::batch::EventInput = serde_json::from_str(&contents)
                .with_context(|| format!("Failed to parse JSON file: {}", path))?;
            (
                summary.unwrap_or(json_event.summary),
                start.unwrap_or(json_event.start),
                end.or(json_event.end),
                duration.or(json_event.duration),
                location.or(json_event.location),
                description.or(json_event.description),
                attendees.or(json_event.attendees),
            )
        } else {
            let summary = summary
                .context("--summary is required (or use --from-json to load event from a file)")?;
            let start = start
                .context("--start is required (or use --from-json to load event from a file)")?;
            (
                summary,
                start,
                end,
                duration,
                location,
                description,
                attendees,
            )
        };

    // Determine which calendar to use
    let calendar_name = ctx
        .calendar
        .clone()
        .unwrap_or_else(|| config.preferences.default_calendar.clone());

    // Get calendar href
    let calendar_href = config
        .calendars
        .get(&calendar_name)
        .cloned()
        .with_context(|| {
            crate::commands::helpers::calendar_not_found_error(&calendar_name, &config)
        })?;

    // Create client
    let client = caldav::create_client(&config, false)
        .await
        .context("Failed to create CalDAV client")?;

    // Delegate to shared helper
    let event_input = crate::commands::batch::EventInput {
        summary,
        start,
        end,
        duration,
        location,
        description,
        attendees,
    };

    // Dry-run: compute what would be created without hitting the server
    if ctx.dry_run {
        let start_dt = crate::parsers::datetime::parse_datetime(&event_input.start)
            .with_context(|| format!("Failed to parse start time: {}", event_input.start))?;
        let end_dt = if let Some(ref end_str) = event_input.end {
            crate::parsers::datetime::parse_datetime(end_str)
                .with_context(|| format!("Failed to parse end time: {}", end_str))?
        } else if let Some(dur_mins) = event_input.duration {
            start_dt + chrono::Duration::minutes(dur_mins as i64)
        } else {
            start_dt + chrono::Duration::hours(1)
        };
        let duration_minutes = end_dt.signed_duration_since(start_dt).num_minutes();

        use crate::cli::OutputFormat;
        match ctx.format {
            OutputFormat::Text => {
                println!("[DRY RUN] Would create event in '{}':", calendar_name);
                println!("  Summary:  {}", event_input.summary);
                println!("  Start:    {} UTC", start_dt.format("%a %b %d, %I:%M %p"));
                println!("  End:      {} UTC", end_dt.format("%a %b %d, %I:%M %p"));
                println!("  Duration: {} minutes", duration_minutes);
                if let Some(ref loc) = event_input.location {
                    println!("  Location: {}", loc);
                }
                if let Some(ref desc) = event_input.description {
                    println!("  Desc:     {}", desc);
                }
                if let Some(ref att) = event_input.attendees {
                    println!("  Attendees:{}", att);
                }
            }
            OutputFormat::Json => {
                let response = SuccessResponse::new(json!({
                    "dry_run": true,
                    "would_create": {
                        "summary": event_input.summary,
                        "start": start_dt.to_rfc3339(),
                        "end": end_dt.to_rfc3339(),
                        "duration_minutes": duration_minutes,
                        "location": event_input.location,
                        "description": event_input.description,
                        "attendees": event_input.attendees,
                        "calendar": calendar_name,
                    }
                }));
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            OutputFormat::Ics => {
                anyhow::bail!("ICS format not yet implemented");
            }
        }
        return Ok(());
    }

    let uid = crate::commands::helpers::create_event_on_server(
        &client,
        &calendar_href,
        &config.server.username,
        &event_input,
    )
    .await?;

    // Re-fetch the created event to return it (build a minimal ICS for local parse)
    let event_href = format!("{}/{}.ics", calendar_href.trim_end_matches('/'), uid);
    use libdav::caldav::GetCalendarResources;
    let resources = client
        .request(GetCalendarResources::new(&calendar_href).with_hrefs(vec![event_href.clone()]))
        .await
        .context("Failed to fetch created event")?;

    let event = resources
        .resources
        .into_iter()
        .find_map(|r| {
            r.content.ok().and_then(|fetched| {
                crate::parsers::ics::parse_event(&fetched.data, r.href, Some(fetched.etag)).ok()
            })
        })
        .with_context(|| format!("Failed to parse created event '{}'", uid))?;

    use crate::cli::OutputFormat;
    match ctx.format {
        OutputFormat::Text => {
            println!("Event created successfully in '{}':\n", calendar_name);
            let output = crate::formatters::text::format_event(&event)?;
            println!("{}", output);
        }
        OutputFormat::Json => {
            let response = SuccessResponse::new(json!({
                "event": event,
                "calendar": calendar_name,
                "message": "Event created successfully"
            }));
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        OutputFormat::Ics => {
            anyhow::bail!("ICS format not yet implemented");
        }
    }

    Ok(())
}

/// Execute events delete command
///
/// Deletes an event from the calendar
pub async fn delete(
    ctx: &crate::commands::context::CommandContext,
    event_id: String,
    force: bool,
) -> Result<()> {
    let config = ctx
        .load_config()
        .context("Failed to load configuration. Run 'fastcal config init' first.")?;

    // Create client
    let client = caldav::create_client(&config, false)
        .await
        .context("Failed to create CalDAV client")?;

    let (calendar_name, event) = crate::commands::helpers::find_event_for_operation(
        &client,
        &config,
        ctx.calendar.as_deref(),
        &event_id,
    )
    .await?;

    // Dry-run: show what would be deleted without hitting the server
    if ctx.dry_run {
        use crate::cli::OutputFormat;
        match ctx.format {
            OutputFormat::Text => {
                println!("[DRY RUN] Would delete event:");
                println!("  ID:       {}", event.id);
                println!("  Summary:  {}", event.summary);
                println!("  Start:    {}", event.start.datetime);
                println!("  Calendar: {}", calendar_name);
            }
            OutputFormat::Json => {
                let response = SuccessResponse::new(json!({
                    "dry_run": true,
                    "would_delete": {
                        "event_id": event.id,
                        "summary": event.summary,
                        "start": event.start.datetime,
                        "calendar": calendar_name,
                    }
                }));
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            OutputFormat::Ics => {
                anyhow::bail!("ICS format not yet implemented");
            }
        }
        return Ok(());
    }

    // Show event details and ask for confirmation (unless --force)
    if !force {
        println!("About to delete event:");
        println!("  ID: {}", event.id);
        println!("  Summary: {}", event.summary);
        println!("  Start: {}", event.start.datetime);
        println!("  Calendar: {}", calendar_name);
        println!();
        print!("Are you sure you want to delete this event? [y/N]: ");
        std::io::Write::flush(&mut std::io::stdout())?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Deletion cancelled");
            return Ok(());
        }
    }

    // Delete the event using libdav Delete (with retry for transient network failures)
    use libdav::dav::Delete;
    crate::caldav::retry_transient(3, || async {
        client
            .request(Delete::new(&event.href).force())
            .await
            .context("Failed to delete event from server")
    })
    .await?;

    use crate::cli::OutputFormat;
    match ctx.format {
        OutputFormat::Text => {
            println!(
                "✓ Deleted: {} (ID: {}) from {}",
                event.summary, event.id, calendar_name
            );
        }
        OutputFormat::Json => {
            let response = SuccessResponse::new(json!({
                "message": "Event deleted successfully",
                "event_id": event.id,
                "summary": event.summary,
                "calendar": calendar_name,
            }));
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        OutputFormat::Ics => {
            anyhow::bail!("ICS format not yet implemented");
        }
    }

    Ok(())
}

/// Execute events update command
///
/// Patch fields for the update command (all optional — only set fields are changed).
pub struct EventUpdatePatch {
    pub summary: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub attendees: Option<String>,
}

/// Updates an existing event in the calendar
pub async fn update(
    ctx: &crate::commands::context::CommandContext,
    event_id: String,
    patch: EventUpdatePatch,
) -> Result<()> {
    let EventUpdatePatch {
        summary,
        start,
        end,
        location,
        description,
        attendees,
    } = patch;
    let config = ctx
        .load_config()
        .context("Failed to load configuration. Run 'fastcal config init' first.")?;

    // Create client
    let client = caldav::create_client(&config, false)
        .await
        .context("Failed to create CalDAV client")?;

    let (calendar_name, mut event) = crate::commands::helpers::find_event_for_operation(
        &client,
        &config,
        ctx.calendar.as_deref(),
        &event_id,
    )
    .await?;

    // Get the etag for optimistic concurrency control
    let etag = event
        .etag
        .clone()
        .context("Cannot update event: etag is missing. Re-fetch the event and try again.")?;

    // Track if anything changed
    let mut changed = false;

    // Update summary if provided
    if let Some(new_summary) = summary {
        event.summary = new_summary;
        changed = true;
    }

    // Update start time if provided
    let start_dt_utc = if let Some(start_str) = start {
        let dt = crate::parsers::datetime::parse_datetime(&start_str)
            .with_context(|| format!("Failed to parse start time: {}", start_str))?;
        event.start.datetime = dt.to_rfc3339();
        changed = true;
        dt
    } else {
        // Parse existing start time
        DateTime::parse_from_rfc3339(&event.start.datetime)
            .with_context(|| "Failed to parse existing start time")?
            .with_timezone(&Utc)
    };

    // Update end time if provided
    let end_dt_utc = if let Some(end_str) = end {
        let dt = crate::parsers::datetime::parse_datetime(&end_str)
            .with_context(|| format!("Failed to parse end time: {}", end_str))?;
        event.end.datetime = dt.to_rfc3339();
        changed = true;
        dt
    } else {
        // Parse existing end time
        DateTime::parse_from_rfc3339(&event.end.datetime)
            .with_context(|| "Failed to parse existing end time")?
            .with_timezone(&Utc)
    };

    // Update location if provided
    if let Some(new_location) = location {
        event.location = if new_location.is_empty() {
            None
        } else {
            Some(new_location)
        };
        changed = true;
    }

    // Update description if provided
    if let Some(new_description) = description {
        event.description = if new_description.is_empty() {
            None
        } else {
            Some(new_description)
        };
        changed = true;
    }

    // Update attendees if provided
    if let Some(attendee_str) = attendees {
        use crate::models::event::Attendee;
        let attendee_list: Vec<Attendee> = if attendee_str.is_empty() {
            Vec::new()
        } else {
            attendee_str
                .split(',')
                .map(|email| Attendee {
                    email: email.trim().to_string(),
                    name: None,
                    status: None,
                })
                .collect()
        };
        event.attendees = if attendee_list.is_empty() {
            None
        } else {
            Some(attendee_list)
        };
        changed = true;
    }

    if !changed {
        anyhow::bail!("No changes specified. Use --summary, --start, --end, --location, --description, or --attendees");
    }

    // Dry-run: show the modified event without PUTting to the server
    if ctx.dry_run {
        event.duration_minutes = Some(end_dt_utc.signed_duration_since(start_dt_utc).num_minutes());
        use crate::cli::OutputFormat;
        match ctx.format {
            OutputFormat::Text => {
                println!("[DRY RUN] Would update event in '{}':", calendar_name);
                let output = crate::formatters::text::format_event(&event)?;
                println!("{}", output);
            }
            OutputFormat::Json => {
                let response = SuccessResponse::new(json!({
                    "dry_run": true,
                    "would_update": event,
                    "calendar": calendar_name,
                }));
                println!("{}", serde_json::to_string_pretty(&response)?);
            }
            OutputFormat::Ics => {
                anyhow::bail!("ICS format not yet implemented");
            }
        }
        return Ok(());
    }

    // Convert attendees to simple email list for ICS builder
    let attendee_emails = event
        .attendees
        .as_ref()
        .map(|att_vec| att_vec.iter().map(|a| a.email.clone()).collect::<Vec<_>>());

    // Build updated ICS event
    let start_ics = crate::parsers::datetime::format_for_ics(&start_dt_utc);
    let end_ics = crate::parsers::datetime::format_for_ics(&end_dt_utc);
    let ics_data = crate::parsers::ics::build_event(&crate::parsers::ics::IcsBuildArgs {
        uid: &event.id,
        summary: &event.summary,
        start: &start_ics,
        end: &end_ics,
        description: event.description.as_deref(),
        location: event.location.as_deref(),
        organizer: Some(&config.server.username),
        attendees: attendee_emails.as_deref(),
    })
    .context("Failed to build updated ICS event")?;

    // Update event using PutResource with etag (with retry for transient network failures)
    use libdav::dav::PutResource;
    crate::caldav::retry_transient(3, || async {
        client
            .request(PutResource::new(&event.href).update(&ics_data, "text/calendar", &etag))
            .await
            .context("Failed to update event on server")
    })
    .await?;

    // Recalculate duration from updated start/end times
    event.duration_minutes = Some(end_dt_utc.signed_duration_since(start_dt_utc).num_minutes());

    use crate::cli::OutputFormat;
    match ctx.format {
        OutputFormat::Text => {
            println!("Event updated successfully in '{}':\n", calendar_name);
            let output = crate::formatters::text::format_event(&event)?;
            println!("{}", output);
        }
        OutputFormat::Json => {
            let response = SuccessResponse::new(json!({
                "event": event,
                "calendar": calendar_name,
                "message": "Event updated successfully"
            }));
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        OutputFormat::Ics => {
            anyhow::bail!("ICS format not yet implemented");
        }
    }

    Ok(())
}

/// Execute events search command
///
/// Searches for events matching a query string
pub async fn search(
    ctx: &crate::commands::context::CommandContext,
    query: String,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
) -> Result<()> {
    let config = ctx
        .load_config()
        .context("Failed to load configuration. Run 'fastcal config init' first.")?;

    // Determine which calendar to use
    let calendar_name = ctx
        .calendar
        .clone()
        .unwrap_or_else(|| config.preferences.default_calendar.clone());

    // Get calendar href
    let calendar_href = config
        .calendars
        .get(&calendar_name)
        .cloned()
        .with_context(|| {
            crate::commands::helpers::calendar_not_found_error(&calendar_name, &config)
        })?;

    // Create client
    let client = caldav::create_client(&config, false)
        .await
        .context("Failed to create CalDAV client")?;

    // List events in date range
    let events = caldav::event::list_events(
        &client,
        &calendar_href,
        Some(calendar_name.clone()),
        from,
        to,
    )
    .await
    .context("Failed to list events")?;

    // Filter events by query (case-insensitive search in summary and description)
    let query_lower = query.to_lowercase();
    let matching_events: Vec<_> = events
        .into_iter()
        .filter(|event| {
            event.summary.to_lowercase().contains(&query_lower)
                || event
                    .description
                    .as_ref()
                    .map(|d| d.to_lowercase().contains(&query_lower))
                    .unwrap_or(false)
        })
        .collect();

    // Output based on format
    use crate::cli::OutputFormat;
    match ctx.format {
        OutputFormat::Text => {
            let output = crate::formatters::text::format_search_results(&query, &matching_events)?;
            println!("{}", output);
        }
        OutputFormat::Json => {
            let mut metadata = Metadata::new()
                .with_count(matching_events.len())
                .with_calendar(calendar_name);

            if let (Some(from_dt), Some(to_dt)) = (from, to) {
                metadata = metadata.with_date_range(
                    from_dt.format("%Y-%m-%d").to_string(),
                    to_dt.format("%Y-%m-%d").to_string(),
                );
            }

            let response = SuccessResponse::with_metadata(
                json!({
                    "query": query,
                    "matches": matching_events,
                }),
                metadata,
            );

            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        OutputFormat::Ics => {
            anyhow::bail!("ICS format not yet implemented");
        }
    }

    Ok(())
}

/// Execute events conflicts command
///
/// Checks for scheduling conflicts in a proposed time range
pub async fn conflicts(
    ctx: &crate::commands::context::CommandContext,
    start: String,
    end: String,
) -> Result<()> {
    let config = ctx
        .load_config()
        .context("Failed to load configuration. Run 'fastcal config init' first.")?;

    // Parse proposed time range
    let proposed_start = crate::parsers::datetime::parse_datetime(&start)
        .with_context(|| format!("Failed to parse start time: {}", start))?;
    let proposed_end = crate::parsers::datetime::parse_datetime(&end)
        .with_context(|| format!("Failed to parse end time: {}", end))?;

    // Validate time range
    if proposed_end <= proposed_start {
        anyhow::bail!("End time must be after start time");
    }

    // Determine which calendar to use
    let calendar_name = ctx
        .calendar
        .clone()
        .unwrap_or_else(|| config.preferences.default_calendar.clone());

    // Get calendar href
    let calendar_href = config
        .calendars
        .get(&calendar_name)
        .cloned()
        .with_context(|| {
            crate::commands::helpers::calendar_not_found_error(&calendar_name, &config)
        })?;

    // Create client
    let client = caldav::create_client(&config, false)
        .await
        .context("Failed to create CalDAV client")?;

    // List events in the proposed time range (with some buffer)
    let buffer = chrono::Duration::hours(1);
    let events = caldav::event::list_events(
        &client,
        &calendar_href,
        Some(calendar_name.clone()),
        Some(proposed_start - buffer),
        Some(proposed_end + buffer),
    )
    .await
    .context("Failed to list events")?;

    // Check for conflicts (overlapping time ranges)
    let conflicting_events: Vec<_> = events
        .into_iter()
        .filter(|event| {
            let event_start = DateTime::parse_from_rfc3339(&event.start.datetime)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc));
            let event_end = DateTime::parse_from_rfc3339(&event.end.datetime)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc));

            match (event_start, event_end) {
                (Some(evt_start), Some(evt_end)) => {
                    events_overlap(evt_start, evt_end, proposed_start, proposed_end)
                }
                _ => false,
            }
        })
        .collect();

    let has_conflicts = !conflicting_events.is_empty();

    // Output based on format
    use crate::cli::OutputFormat;
    match ctx.format {
        OutputFormat::Text => {
            let output = crate::formatters::text::format_conflicts(
                &proposed_start.to_rfc3339(),
                &proposed_end.to_rfc3339(),
                &conflicting_events,
            )?;
            println!("{}", output);
        }
        OutputFormat::Json => {
            let response = SuccessResponse::new(json!({
                "proposed_time": {
                    "start": proposed_start.to_rfc3339(),
                    "end": proposed_end.to_rfc3339(),
                },
                "has_conflicts": has_conflicts,
                "conflicts": conflicting_events,
                "calendar": calendar_name,
            }));

            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        OutputFormat::Ics => {
            anyhow::bail!("ICS format not yet implemented");
        }
    }

    Ok(())
}

/// Returns true if [evt_start, evt_end) overlaps with [proposed_start, proposed_end).
///
/// Two intervals overlap iff one starts before the other ends AND ends after the other starts.
fn events_overlap(
    evt_start: chrono::DateTime<chrono::Utc>,
    evt_end: chrono::DateTime<chrono::Utc>,
    proposed_start: chrono::DateTime<chrono::Utc>,
    proposed_end: chrono::DateTime<chrono::Utc>,
) -> bool {
    evt_start < proposed_end && evt_end > proposed_start
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn dt(h: u32, m: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 10, h, m, 0).unwrap()
    }

    #[test]
    fn test_overlap_exact_same_window() {
        // Identical intervals always overlap
        assert!(events_overlap(dt(10, 0), dt(11, 0), dt(10, 0), dt(11, 0)));
    }

    #[test]
    fn test_overlap_partial_overlap_before() {
        // Event ends inside proposed window
        assert!(events_overlap(dt(9, 0), dt(10, 30), dt(10, 0), dt(11, 0)));
    }

    #[test]
    fn test_overlap_partial_overlap_after() {
        // Event starts inside proposed window
        assert!(events_overlap(dt(10, 30), dt(12, 0), dt(10, 0), dt(11, 0)));
    }

    #[test]
    fn test_overlap_event_contains_proposed() {
        // Event completely contains proposed window
        assert!(events_overlap(dt(9, 0), dt(12, 0), dt(10, 0), dt(11, 0)));
    }

    #[test]
    fn test_overlap_proposed_contains_event() {
        // Proposed window completely contains event
        assert!(events_overlap(dt(10, 15), dt(10, 45), dt(10, 0), dt(11, 0)));
    }

    #[test]
    fn test_no_overlap_event_ends_exactly_at_start() {
        // Event ends exactly when proposed starts — no overlap (half-open intervals)
        assert!(!events_overlap(dt(9, 0), dt(10, 0), dt(10, 0), dt(11, 0)));
    }

    #[test]
    fn test_no_overlap_event_starts_exactly_at_end() {
        // Event starts exactly when proposed ends — no overlap
        assert!(!events_overlap(dt(11, 0), dt(12, 0), dt(10, 0), dt(11, 0)));
    }

    #[test]
    fn test_no_overlap_event_entirely_before() {
        assert!(!events_overlap(dt(8, 0), dt(9, 0), dt(10, 0), dt(11, 0)));
    }

    #[test]
    fn test_no_overlap_event_entirely_after() {
        assert!(!events_overlap(dt(12, 0), dt(13, 0), dt(10, 0), dt(11, 0)));
    }
}
