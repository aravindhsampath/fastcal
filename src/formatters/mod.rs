// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Output formatters module
//!
//! Provides different output formats for CLI commands.

pub mod text;

use crate::cli::OutputFormat;
use crate::models::Event;
use anyhow::Result;
use chrono_tz::Tz;

/// Format events based on output format. `tz` is the resolved display zone
/// used for the text rendering.
pub fn format_events(events: &[Event], format: OutputFormat, tz: Tz) -> Result<String> {
    match format {
        OutputFormat::Json => Ok(serde_json::to_string_pretty(events)?),
        OutputFormat::Text => text::format_events(events, tz),
    }
}
