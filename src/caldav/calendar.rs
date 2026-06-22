// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Calendar operations
//!
//! Functions for listing and retrieving calendar information.

use super::{utils, Client};
use crate::models::Calendar;
use anyhow::{Context, Result};
use libdav::caldav::{FindCalendarHomeSet, FindCalendars};
use std::collections::HashMap;

/// List all calendars for the user
pub async fn list_calendars(
    client: &Client,
    principal_url: &str,
) -> Result<HashMap<String, Calendar>> {
    log::info!("Listing calendars for principal: {}", principal_url);

    // Find calendar home sets (libdav 0.10.5 takes the href as &str)
    let home_set_response = client
        .request(FindCalendarHomeSet::new(principal_url))
        .await
        .context("Failed to find calendar home sets")?;

    if home_set_response.home_sets.is_empty() {
        log::warn!("No calendar home sets found");
        return Ok(HashMap::new());
    }

    let mut all_calendars = HashMap::new();

    // Find calendars in each home set
    for home_set in &home_set_response.home_sets {
        log::debug!("Searching for calendars in: {}", home_set);

        let home_set_href = home_set.to_string();
        let calendar_response = client
            .request(FindCalendars::new(&home_set_href))
            .await
            .context("Failed to find calendars")?;

        // Fetch display names concurrently for all calendars
        let calendar_hrefs: Vec<String> = calendar_response
            .calendars
            .iter()
            .map(|c| c.href.to_string())
            .collect();
        let display_names = utils::fetch_display_names_concurrent(client, &calendar_hrefs).await;

        for cal in calendar_response.calendars {
            log::info!("Found calendar: {}", cal.href);

            let href_string = cal.href.to_string();
            let display_name = display_names.get(&href_string).and_then(|dn| dn.clone());

            let name = display_name
                .clone()
                .unwrap_or_else(|| utils::extract_calendar_name_from_href(&cal.href.to_string()));

            let calendar = Calendar::new(name.clone(), cal.href.to_string())
                .with_display_name(display_name.unwrap_or_else(|| name.clone()));

            // Ensure unique names
            let final_name = utils::ensure_unique_name(name, &all_calendars);

            all_calendars.insert(final_name, calendar);
        }
    }

    log::info!("Found {} calendars", all_calendars.len());
    Ok(all_calendars)
}

/// Get information about a specific calendar
pub async fn get_calendar_info(client: &Client, calendar_href: &str) -> Result<Calendar> {
    log::info!("Getting info for calendar: {}", calendar_href);

    let display_names =
        utils::fetch_display_names_concurrent(client, &[calendar_href.to_string()]).await;

    let display_name = display_names.values().next().and_then(|dn| dn.clone());

    let name = display_name
        .clone()
        .unwrap_or_else(|| utils::extract_calendar_name_from_href(calendar_href));

    let calendar = Calendar::new(name, calendar_href.to_string()).with_display_name(
        display_name.unwrap_or_else(|| utils::extract_calendar_name_from_href(calendar_href)),
    );

    Ok(calendar)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_calendar_name() {
        assert_eq!(
            utils::extract_calendar_name_from_href(
                "https://caldav.fastmail.com/dav/calendars/user/personal/"
            ),
            "personal"
        );
        assert_eq!(
            utils::extract_calendar_name_from_href(
                "https://caldav.fastmail.com/dav/calendars/user/work"
            ),
            "work"
        );
    }
}
