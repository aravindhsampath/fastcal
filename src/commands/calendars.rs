// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Calendars command implementations

use crate::caldav;
use crate::models::{Metadata, SuccessResponse};
use anyhow::{Context, Result};
use serde_json::json;

/// Execute calendars list command
///
/// Lists all discovered calendars
pub async fn list(ctx: &crate::commands::context::CommandContext) -> Result<()> {
    let config = ctx
        .load_config()
        .context("Failed to load configuration. Run 'fastcal config init' first.")?;

    // Verify we have principal URL
    let principal_url = config
        .server
        .principal
        .as_ref()
        .context("No principal URL found. Run 'fastcal config init' first.")?;

    // Create client
    let client = caldav::create_client(&config, false)
        .await
        .context("Failed to create CalDAV client")?;

    // List calendars
    let calendars = caldav::calendar::list_calendars(&client, principal_url)
        .await
        .context("Failed to list calendars")?;

    // Convert to serializable format
    let calendars_list: Vec<_> = calendars
        .into_iter()
        .map(|(name, cal)| {
            json!({
                "name": name,
                "href": cal.href,
                "display_name": cal.display_name,
                "description": cal.description,
                "color": cal.color,
                "supported_components": cal.supported_components,
                "timezone": cal.timezone,
            })
        })
        .collect();

    use crate::cli::OutputFormat;
    match ctx.format {
        OutputFormat::Text => {
            let pairs: Vec<(String, serde_json::Value)> = calendars_list
                .iter()
                .filter_map(|v| {
                    v.get("name")
                        .and_then(|n| n.as_str())
                        .map(|name| (name.to_string(), v.clone()))
                })
                .collect();
            let output = crate::formatters::text::format_calendars(&pairs)?;
            println!("{}", output);
        }
        OutputFormat::Json => {
            let metadata = Metadata::new().with_count(calendars_list.len());
            let response = SuccessResponse::with_metadata(
                json!({
                    "calendars": calendars_list
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

/// Execute calendars info command
///
/// Shows details about a specific calendar
pub async fn info(
    ctx: &crate::commands::context::CommandContext,
    calendar_name: String,
) -> Result<()> {
    let config = ctx
        .load_config()
        .context("Failed to load configuration. Run 'fastcal config init' first.")?;

    // Find calendar href
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

    // Get calendar info
    let calendar = caldav::calendar::get_calendar_info(&client, &calendar_href)
        .await
        .context("Failed to get calendar info")?;

    use crate::cli::OutputFormat;
    match ctx.format {
        OutputFormat::Text => {
            let cal_json = json!({
                "href": calendar.href,
                "display_name": calendar.display_name,
                "description": calendar.description,
                "color": calendar.color,
            });
            let output = crate::formatters::text::format_calendar_info(&calendar_name, &cal_json)?;
            println!("{}", output);
        }
        OutputFormat::Json => {
            let response = SuccessResponse::new(json!({
                "calendar": {
                    "name": calendar_name,
                    "href": calendar.href,
                    "display_name": calendar.display_name,
                    "description": calendar.description,
                    "color": calendar.color,
                    "supported_components": calendar.supported_components,
                    "timezone": calendar.timezone,
                }
            }));
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        OutputFormat::Ics => {
            anyhow::bail!("ICS format not yet implemented");
        }
    }

    Ok(())
}
