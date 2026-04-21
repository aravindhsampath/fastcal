// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Service discovery module
//!
//! Performs CalDAV service discovery to find calendar endpoints.
//! Based on davcli's discovery patterns.

use super::Config;
use crate::caldav::{utils, Client};
use anyhow::{bail, Context, Result};
use libdav::{
    caldav::{FindCalendarHomeSet, FindCalendars},
    caldav_service_for_url,
    sd::{find_context_url, FindContextUrlResult},
};
use std::collections::HashMap;

/// Discovery result containing all discovered information
#[derive(Debug)]
pub struct DiscoveryResult {
    pub context_url: String,
    pub principal: Option<String>,
    pub calendars: HashMap<String, String>,
}

/// Perform full service discovery
///
/// Based on davcli's discover() function.
/// Returns discovered URLs and calendar information.
pub async fn discover(mut client: Client) -> Result<DiscoveryResult> {
    log::info!("Starting service discovery");

    let base_url = client.base_url().to_string();
    log::debug!("Base URL: {}", base_url);

    // Step 1: Find context URL
    let service = caldav_service_for_url(client.base_url())
        .context("Failed to determine CalDAV service for URL")?;

    let context_url = match find_context_url(&client, service).await {
        FindContextUrlResult::BaseUrl => {
            log::info!("Base URL is a valid context path");
            base_url.clone()
        }
        FindContextUrlResult::Found(uri) => {
            log::info!("Resolved context path: {}", uri);
            // Update client's base URL
            client.webdav_client.base_url = uri.clone();
            uri.to_string()
        }
        FindContextUrlResult::NoneFound => {
            log::warn!("Context path not found; using given URL (this might not work)");
            base_url.clone()
        }
        FindContextUrlResult::Error(err) => {
            bail!("No usable context path found: {}", err);
        }
    };

    // Step 2: Find current user principal
    let principal = match client.find_current_user_principal().await? {
        Some(p) => {
            log::info!("Current user principal: {}", p);
            Some(p.to_string())
        }
        None => {
            log::warn!("Current user principal not found");
            None
        }
    };

    // Step 3: Find calendar home sets
    let mut calendar_home_sets = Vec::new();

    if let Some(ref principal_url) = principal {
        let principal_uri: http::Uri = principal_url
            .parse()
            .context("Failed to parse principal URL")?;

        let home_set_response = client
            .request(FindCalendarHomeSet::new(&principal_uri))
            .await
            .context("Failed to find calendar home sets")?;

        if home_set_response.home_sets.is_empty() {
            log::warn!("No calendar home set found");
        } else {
            for home_set in &home_set_response.home_sets {
                log::info!("Calendar home set: {}", home_set);
                calendar_home_sets.push(home_set.to_string());
            }
        }
    }

    // Step 4: Find calendars
    let mut calendars = HashMap::new();

    for home_set_url in &calendar_home_sets {
        log::debug!("Searching for calendars in: {}", home_set_url);

        let home_set_uri: http::Uri = home_set_url
            .parse()
            .context("Failed to parse home set URL")?;

        let calendar_response = client
            .request(FindCalendars::new(&home_set_uri))
            .await
            .context("Failed to find calendars")?;

        // Fetch display names concurrently for all calendars
        let calendar_hrefs: Vec<String> = calendar_response
            .calendars
            .iter()
            .map(|c| c.href.to_string())
            .collect();
        let display_names = utils::fetch_display_names_concurrent(&client, &calendar_hrefs).await;

        for calendar in calendar_response.calendars {
            log::info!("Found calendar: {}", calendar.href);

            let href_string = calendar.href.to_string();
            let display_name = display_names.get(&href_string).and_then(|dn| dn.clone());

            let name = display_name.clone().unwrap_or_else(|| href_string.clone());

            if display_name.is_some() {
                log::debug!("Calendar '{}' name: {}", calendar.href, name);
            }

            // Ensure unique names
            let unique_name = utils::ensure_unique_name(name, &calendars);

            calendars.insert(unique_name, calendar.href.to_string());
        }
    }

    log::info!("Discovery complete. Found {} calendars", calendars.len());

    Ok(DiscoveryResult {
        context_url,
        principal,
        calendars,
    })
}

/// Update config with discovery results
pub fn update_config_with_discovery(config: &mut Config, result: DiscoveryResult) {
    // Update server config with discovered URLs
    config.server.caldav_url = Some(result.context_url);
    config.server.principal = result.principal;

    // Update calendars
    config.calendars = result.calendars.clone();

    // Set default calendar to the first discovered calendar if not already set
    // or if the current default doesn't exist in the discovered calendars
    if config.calendars.is_empty() {
        log::warn!("No calendars discovered");
    } else if !config
        .calendars
        .contains_key(&config.preferences.default_calendar)
    {
        // Pick the first calendar (alphabetically sorted for determinism)
        let mut calendar_names: Vec<_> = result.calendars.keys().collect();
        calendar_names.sort();
        if let Some(first_calendar) = calendar_names.first() {
            config.preferences.default_calendar = (*first_calendar).clone();
            log::info!("Set default calendar to: {}", first_calendar);
        }
    }

    log::info!("Config updated with discovery results");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_config() {
        let mut config = Config::minimal("user@fastmail.com".to_string(), "password".to_string());

        let mut calendars = HashMap::new();
        calendars.insert(
            "Personal".to_string(),
            "https://example.com/cal1/".to_string(),
        );
        calendars.insert("Work".to_string(), "https://example.com/cal2/".to_string());

        let result = DiscoveryResult {
            context_url: "https://caldav.example.com/".to_string(),
            principal: Some("https://example.com/principal/user/".to_string()),
            calendars,
        };

        update_config_with_discovery(&mut config, result);

        assert_eq!(
            config.server.caldav_url,
            Some("https://caldav.example.com/".to_string())
        );
        assert_eq!(config.calendars.len(), 2);
        assert!(config.calendars.contains_key("Personal"));
        assert!(config.calendars.contains_key("Work"));
    }
}
