// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Event operations
//!
//! Functions for listing and retrieving calendar events using libdav.
//!
//! Uses ListCalendarResources and GetCalendarResources from libdav.

use super::Client;
use crate::models::Event;
use crate::parsers::datetime::format_for_ics;
use crate::parsers::ics;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use libdav::caldav::{GetCalendarResources, ListCalendarResources};

/// List events in a calendar within a date range
///
/// Uses libdav's ListCalendarResources for metadata listing and
/// GetCalendarResources to fetch full event data.
pub async fn list_events(
    client: &Client,
    calendar_href: &str,
    calendar_name: Option<String>,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
) -> Result<Vec<Event>> {
    log::info!(
        "Listing events in calendar: {} (from={:?}, to={:?})",
        calendar_href,
        from,
        to
    );

    // Step 1: List event hrefs with optional filtering
    // Need to keep datetime strings alive for the duration of the request
    let from_ics_opt = from.map(|dt| format_for_ics(&dt));
    let to_ics_opt = to.map(|dt| format_for_ics(&dt));

    let list_request = if let (Some(ref from_ics), Some(ref to_ics)) = (&from_ics_opt, &to_ics_opt)
    {
        // List with time range filter
        log::debug!("Using time range: {} to {}", from_ics, to_ics);

        ListCalendarResources::new(calendar_href)
            .with_component_and_time_range("VEVENT", Some(from_ics.as_str()), Some(to_ics.as_str()))
            .context("Failed to create time range filter")?
    } else {
        // List all VEVENTs
        ListCalendarResources::new(calendar_href)
            .with_component("VEVENT")
            .context("Failed to create component filter")?
    };

    let listed = client
        .request(list_request)
        .await
        .context("Failed to list calendar resources")?;

    log::info!("Found {} event(s)", listed.resources.len());

    if listed.resources.is_empty() {
        return Ok(Vec::new());
    }

    // Step 2: Fetch full event data for all hrefs
    let hrefs: Vec<String> = listed.resources.into_iter().map(|r| r.href).collect();

    let resources = client
        .request(GetCalendarResources::new(calendar_href).with_hrefs(hrefs))
        .await
        .context("Failed to fetch calendar resources")?;

    // Step 3: Parse ICS data into Event structs
    let mut events = Vec::new();
    for resource in resources.resources {
        match &resource.content {
            Ok(fetched) => {
                match ics::parse_event(
                    &fetched.data,
                    resource.href.clone(),
                    Some(fetched.etag.clone()),
                ) {
                    Ok(mut event) => {
                        event.calendar = calendar_name.clone();
                        events.push(event);
                    }
                    Err(e) => {
                        log::debug!("Skipping unparseable event {}: {}", resource.href, e);
                    }
                }
            }
            Err(status) => {
                log::debug!("Skipping event {}: HTTP {}", resource.href, status);
            }
        }
    }

    log::info!("Successfully parsed {} event(s)", events.len());
    Ok(events)
}

/// Find an event by ID across all calendars
///
/// Fast path: tries `{calendar_href}/{event_id}.ics` directly (O(N) where N=calendars).
/// Fastmail and most CalDAV servers use the UID as the resource filename.
/// Falls back to a full scan only if the fast path misses everywhere.
pub async fn find_event_by_id(
    client: &Client,
    event_id: &str,
    calendars: &std::collections::HashMap<String, String>,
) -> Result<Option<(String, Event)>> {
    log::info!("Searching for event with UID: {}", event_id);

    // Fast path: try direct fetch by convention (uid.ics)
    for (calendar_name, calendar_href) in calendars {
        let event_href = format!("{}/{}.ics", calendar_href.trim_end_matches('/'), event_id);
        log::debug!("Fast-path fetch: {}", event_href);

        let resources = match client
            .request(GetCalendarResources::new(calendar_href).with_hrefs(vec![event_href]))
            .await
        {
            Ok(r) => r,
            Err(_) => continue,
        };

        for resource in resources.resources {
            if let Ok(fetched) = resource.content {
                if let Ok(mut event) =
                    ics::parse_event(&fetched.data, resource.href, Some(fetched.etag))
                {
                    if event.id == event_id {
                        event.calendar = Some(calendar_name.clone());
                        log::debug!("Found via fast path in calendar: {}", calendar_name);
                        return Ok(Some((calendar_name.clone(), event)));
                    }
                }
            }
        }
    }

    // Slow path: full scan (for non-standard filename conventions)
    log::debug!("Fast path missed; falling back to full scan");
    for (calendar_name, calendar_href) in calendars {
        let list_result = ListCalendarResources::new(calendar_href)
            .with_component("VEVENT")
            .context("Failed to create component filter")?;

        let listed = match client.request(list_result).await {
            Ok(l) => l,
            Err(e) => {
                log::warn!("Failed to list events in calendar {}: {}", calendar_name, e);
                continue;
            }
        };

        let hrefs: Vec<String> = listed.resources.into_iter().map(|r| r.href).collect();
        if hrefs.is_empty() {
            continue;
        }

        let resources = match client
            .request(GetCalendarResources::new(calendar_href).with_hrefs(hrefs))
            .await
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!(
                    "Failed to fetch events in calendar {}: {}",
                    calendar_name,
                    e
                );
                continue;
            }
        };

        for resource in resources.resources {
            if let Ok(fetched) = resource.content {
                if let Ok(mut event) =
                    ics::parse_event(&fetched.data, resource.href, Some(fetched.etag))
                {
                    if event.id == event_id {
                        event.calendar = Some(calendar_name.clone());
                        return Ok(Some((calendar_name.clone(), event)));
                    }
                }
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_date_range() {
        let now = Utc::now();
        let future = now + Duration::days(30);

        assert!(future > now);
    }

    #[test]
    fn test_format_for_ics() {
        let dt = Utc::now();
        let formatted = format_for_ics(&dt);

        // Should be YYYYMMDDTHHMMSSz format
        assert!(formatted.ends_with('Z'));
        assert!(formatted.contains('T'));
        assert_eq!(formatted.len(), 16); // YYYYMMDDTHHMMSSz = 16 chars
    }
}
