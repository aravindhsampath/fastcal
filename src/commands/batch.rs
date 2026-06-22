// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Batch command implementations

use crate::caldav;
use crate::models::SuccessResponse;
use anyhow::{Context, Result};
use futures_util::stream::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;

/// Max in-flight requests for a batch operation. Bounds connections/fds so a
/// large batch doesn't open one socket per event (server throttling / EMFILE).
const MAX_BATCH_CONCURRENCY: usize = 8;

/// Event creation input (used by both single-create and batch-create)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventInput {
    pub summary: String,
    pub start: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attendees: Option<String>,
    /// DISPLAY reminder N minutes before event start. `None` ⇒ no
    /// reminder on this event. Exists on the shared batch input so
    /// `fastcal batch create --from-json file.json` can carry
    /// reminders per-event alongside the required fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reminder_minutes: Option<u32>,
}

/// Result of a single batch operation
#[derive(Debug, Clone, Serialize)]
pub struct BatchOperationResult {
    pub index: usize,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Execute batch create command
///
/// Creates multiple events from a JSON file
pub async fn create(
    ctx: &crate::commands::context::CommandContext,
    json_file: String,
) -> Result<()> {
    let config = ctx
        .load_config()
        .context("Failed to load configuration. Run 'fastcal config init' first.")?;

    // Read and parse JSON file
    let file_contents = fs::read_to_string(&json_file)
        .with_context(|| format!("Failed to read file: {}", json_file))?;

    let events: Vec<EventInput> = serde_json::from_str(&file_contents)
        .with_context(|| format!("Failed to parse JSON from file: {}", json_file))?;

    if events.is_empty() {
        anyhow::bail!("No events found in JSON file");
    }

    eprintln!("Processing {} event(s) from {}", events.len(), json_file);

    // Dry-run: show what would be created without touching the server
    if ctx.dry_run {
        let response = crate::models::SuccessResponse::new(json!({
            "dry_run": true,
            "total": events.len(),
            "would_create": events,
        }));
        println!("{}", serde_json::to_string_pretty(&response)?);
        return Ok(());
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

    let total = events.len();
    let tz = ctx.timezone;
    eprintln!(
        "Dispatching {} create request(s) ({} at a time)...",
        total, MAX_BATCH_CONCURRENCY
    );

    // Bounded concurrency: at most MAX_BATCH_CONCURRENCY creates in flight.
    let mut raw_results: Vec<(usize, Result<String>)> =
        futures_util::stream::iter(events.iter().enumerate())
            .map(|(index, event_input)| {
                let client = &client;
                let config = &config;
                let calendar_name = &calendar_name;
                let calendar_href = &calendar_href;
                async move {
                    let result = create_single_event(
                        client,
                        config,
                        calendar_name,
                        calendar_href,
                        event_input,
                        tz,
                    )
                    .await;
                    (index, result)
                }
            })
            .buffer_unordered(MAX_BATCH_CONCURRENCY)
            .collect()
            .await;
    // buffer_unordered yields in completion order; restore input order.
    raw_results.sort_by_key(|(index, _)| *index);

    // Collect results in order
    let mut results = Vec::new();
    let mut success_count = 0usize;
    let mut error_count = 0usize;

    for (index, outcome) in raw_results {
        match outcome {
            Ok(event_id) => {
                eprintln!("  ✓ [{}/{}] Created: {}", index + 1, total, event_id);
                results.push(BatchOperationResult {
                    index,
                    success: true,
                    event_id: Some(event_id),
                    error: None,
                });
                success_count += 1;
            }
            Err(e) => {
                eprintln!("  ✗ [{}/{}] Error: {}", index + 1, total, e);
                results.push(BatchOperationResult {
                    index,
                    success: false,
                    event_id: None,
                    error: Some(e.to_string()),
                });
                error_count += 1;
            }
        }
    }

    eprintln!();
    eprintln!("Batch create complete:");
    eprintln!("  Success: {}", success_count);
    eprintln!("  Errors: {}", error_count);

    let response = SuccessResponse::new(json!({
        "total": events.len(),
        "success": success_count,
        "errors": error_count,
        "results": results,
    }));

    println!("{}", serde_json::to_string_pretty(&response)?);

    Ok(())
}

/// Create a single event (helper for batch create) — delegates to shared helper
async fn create_single_event(
    client: &caldav::Client,
    config: &crate::config::Config,
    _calendar_name: &str,
    calendar_href: &str,
    event_input: &EventInput,
    tz: chrono_tz::Tz,
) -> Result<String> {
    crate::commands::helpers::create_event_on_server(
        client,
        calendar_href,
        &config.server.username,
        event_input,
        tz,
    )
    .await
}

/// Execute batch delete command
///
/// Deletes multiple events from a JSON file
pub async fn delete(
    ctx: &crate::commands::context::CommandContext,
    json_file: String,
) -> Result<()> {
    let config = ctx
        .load_config()
        .context("Failed to load configuration. Run 'fastcal config init' first.")?;

    // Read and parse JSON file
    let file_contents = fs::read_to_string(&json_file)
        .with_context(|| format!("Failed to read file: {}", json_file))?;

    let event_ids: Vec<String> = serde_json::from_str(&file_contents)
        .with_context(|| format!("Failed to parse JSON from file: {}", json_file))?;

    if event_ids.is_empty() {
        anyhow::bail!("No event IDs found in JSON file");
    }

    eprintln!(
        "Processing {} event ID(s) from {}",
        event_ids.len(),
        json_file
    );

    // Dry-run: show which events would be deleted without touching the server
    if ctx.dry_run {
        let response = crate::models::SuccessResponse::new(json!({
            "dry_run": true,
            "total": event_ids.len(),
            "would_delete": event_ids,
        }));
        println!("{}", serde_json::to_string_pretty(&response)?);
        return Ok(());
    }

    // Create client
    let client = caldav::create_client(&config, false)
        .await
        .context("Failed to create CalDAV client")?;

    let total = event_ids.len();
    let calendar_filter = ctx.calendar.clone();
    eprintln!(
        "Dispatching {} delete request(s) ({} at a time)...",
        total, MAX_BATCH_CONCURRENCY
    );

    // Bounded concurrency: at most MAX_BATCH_CONCURRENCY deletes in flight.
    let mut raw_results: Vec<(usize, String, Result<()>)> =
        futures_util::stream::iter(event_ids.iter().enumerate())
            .map(|(index, event_id)| {
                let client = &client;
                let config = &config;
                let filter = calendar_filter.as_deref();
                async move {
                    let result = delete_single_event(client, config, event_id, filter).await;
                    (index, event_id.clone(), result)
                }
            })
            .buffer_unordered(MAX_BATCH_CONCURRENCY)
            .collect()
            .await;
    raw_results.sort_by_key(|(index, _, _)| *index);

    // Collect results in order
    let mut results = Vec::new();
    let mut success_count = 0usize;
    let mut error_count = 0usize;

    for (index, event_id, outcome) in raw_results {
        match outcome {
            Ok(()) => {
                eprintln!("  ✓ [{}/{}] Deleted: {}", index + 1, total, event_id);
                results.push(BatchOperationResult {
                    index,
                    success: true,
                    event_id: Some(event_id),
                    error: None,
                });
                success_count += 1;
            }
            Err(e) => {
                eprintln!("  ✗ [{}/{}] Error: {}", index + 1, total, e);
                results.push(BatchOperationResult {
                    index,
                    success: false,
                    event_id: Some(event_id),
                    error: Some(e.to_string()),
                });
                error_count += 1;
            }
        }
    }

    eprintln!();
    eprintln!("Batch delete complete:");
    eprintln!("  Success: {}", success_count);
    eprintln!("  Errors: {}", error_count);

    let response = SuccessResponse::new(json!({
        "total": event_ids.len(),
        "success": success_count,
        "errors": error_count,
        "results": results,
    }));

    println!("{}", serde_json::to_string_pretty(&response)?);

    Ok(())
}

/// Delete a single event (helper for batch delete)
async fn delete_single_event(
    client: &caldav::Client,
    config: &crate::config::Config,
    event_id: &str,
    calendar_filter: Option<&str>,
) -> Result<()> {
    let (_calendar_name, event) = crate::commands::helpers::find_event_for_operation(
        client,
        config,
        calendar_filter,
        event_id,
    )
    .await?;

    use libdav::dav::Delete;
    crate::caldav::retry_transient(3, || async {
        client
            .request(Delete::new(&event.href).force())
            .await
            .context("Failed to delete event from server")
    })
    .await?;

    Ok(())
}
