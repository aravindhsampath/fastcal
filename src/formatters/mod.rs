// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Output formatters module
//!
//! Provides different output formats for CLI commands.

pub mod text;

use crate::cli::OutputFormat;
use crate::models::Event;
use anyhow::Result;

/// Format events based on output format
pub fn format_events(events: &[Event], format: OutputFormat) -> Result<String> {
    match format {
        OutputFormat::Json => Ok(serde_json::to_string_pretty(events)?),
        OutputFormat::Text => text::format_events(events),
        OutputFormat::Ics => {
            // ICS format would combine all events into a calendar
            anyhow::bail!("ICS format not yet implemented")
        }
    }
}
