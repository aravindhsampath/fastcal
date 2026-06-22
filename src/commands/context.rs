// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Command context
//!
//! Provides shared context for command execution including CLI options.

use crate::cli::OutputFormat;
use chrono_tz::Tz;

/// Command execution context
#[derive(Clone)]
pub struct CommandContext {
    /// Custom config file path
    pub config_path: Option<String>,

    /// Output format
    pub format: OutputFormat,

    /// Target calendar
    pub calendar: Option<String>,

    /// Dry-run mode: parse and validate without sending mutations to the server
    pub dry_run: bool,

    /// The single IANA timezone resolved for this invocation (CLI flag >
    /// config > system > UTC). Every local↔UTC parse and every display
    /// conversion uses this — see [`crate::timezone`].
    pub timezone: Tz,
}

impl CommandContext {
    /// Create new context from CLI options
    pub fn new(
        config_path: Option<String>,
        format: OutputFormat,
        calendar: Option<String>,
        dry_run: bool,
        timezone: Tz,
    ) -> Self {
        Self {
            config_path,
            format,
            calendar,
            dry_run,
            timezone,
        }
    }

    /// Load config using the configured path
    pub fn load_config(&self) -> anyhow::Result<crate::config::Config> {
        if let Some(ref path) = self.config_path {
            crate::config::Config::load_from(std::path::Path::new(path))
        } else {
            crate::config::Config::load()
        }
    }
}
