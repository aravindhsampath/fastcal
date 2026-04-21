// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Configuration management module
//!
//! Handles loading, saving, and managing configuration for fastcal.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub mod discovery;
pub mod loader;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub calendars: HashMap<String, String>,
    #[serde(default)]
    pub preferences: Preferences,
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Base URL for discovery (e.g., "https://fastmail.com")
    pub url: String,

    /// Username (full email address)
    pub username: String,

    /// App password (optional in config, can use env var)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_password: Option<String>,

    /// Discovered CalDAV URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caldav_url: Option<String>,

    /// Discovered principal URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal: Option<String>,
}

/// User preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default = "default_calendar")]
    pub default_calendar: String,

    #[serde(default = "default_timezone")]
    pub default_timezone: String,

    #[serde(default = "default_output_format")]
    pub output_format: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            default_calendar: default_calendar(),
            default_timezone: default_timezone(),
            output_format: default_output_format(),
        }
    }
}

fn default_calendar() -> String {
    "personal".to_string()
}

fn default_timezone() -> String {
    "America/Los_Angeles".to_string()
}

fn default_output_format() -> String {
    "json".to_string()
}

impl Config {
    /// Get the config file path
    pub fn config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .context("Could not determine config directory")?
            .join("fastcal");

        Ok(config_dir.join("config.toml"))
    }

    /// Load configuration from file
    pub fn load() -> Result<Self> {
        loader::load()
    }

    /// Load from custom path
    pub fn load_from(path: &std::path::Path) -> Result<Self> {
        loader::load_from(path)
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        loader::save(self)
    }

    /// Get the app password, checking env var first, then config
    pub fn get_password(&self) -> Result<String> {
        // Check environment variable first (FASTCAL_PASSWORD)
        if let Ok(password) = std::env::var("FASTCAL_PASSWORD") {
            return Ok(password);
        }

        // Fall back to config file
        self.server
            .app_password
            .clone()
            .context("No password found in config or FASTCAL_PASSWORD environment variable")
    }

    /// Get the username, checking env var first, then config
    pub fn get_username(&self) -> Result<String> {
        // Check environment variable first (FASTCAL_USERNAME)
        if let Ok(username) = std::env::var("FASTCAL_USERNAME") {
            return Ok(username);
        }

        Ok(self.server.username.clone())
    }

    /// Get the base URL, checking env var first, then config
    pub fn get_base_url(&self) -> Result<String> {
        // Check environment variable first (FASTCAL_BASE_URL)
        if let Ok(url) = std::env::var("FASTCAL_BASE_URL") {
            return Ok(url);
        }

        // Use caldav_url if discovered, otherwise fall back to server url
        Ok(self
            .server
            .caldav_url
            .clone()
            .unwrap_or_else(|| self.server.url.clone()))
    }

    /// Create a minimal config for initial setup
    pub fn minimal(username: String, password: String) -> Self {
        Self {
            server: ServerConfig {
                url: "https://fastmail.com".to_string(),
                username,
                app_password: Some(password),
                caldav_url: None,
                principal: None,
            },
            calendars: HashMap::new(),
            preferences: Preferences::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_preferences() {
        let prefs = Preferences::default();
        assert_eq!(prefs.default_calendar, "personal");
        assert_eq!(prefs.output_format, "json");
    }

    #[test]
    fn test_minimal_config() {
        let config = Config::minimal("user@fastmail.com".to_string(), "password123".to_string());

        assert_eq!(config.server.username, "user@fastmail.com");
        assert_eq!(config.server.app_password, Some("password123".to_string()));
        assert_eq!(config.server.url, "https://fastmail.com");
    }
}
