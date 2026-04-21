// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Calendar data model
//!
//! Represents a CalDAV calendar.

use serde::{Deserialize, Serialize};

/// Calendar information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calendar {
    /// Calendar name/identifier
    pub name: String,

    /// Full href/URL to the calendar resource
    pub href: String,

    /// Display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Calendar description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Calendar color (hex format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    /// Supported components (VEVENT, VTODO, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_components: Option<Vec<String>>,

    /// Calendar timezone
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

impl Calendar {
    /// Create a basic calendar from name and href
    pub fn new(name: String, href: String) -> Self {
        Self {
            name,
            href,
            display_name: None,
            description: None,
            color: None,
            supported_components: None,
            timezone: None,
        }
    }

    /// Create with display name
    pub fn with_display_name(mut self, display_name: String) -> Self {
        self.display_name = Some(display_name);
        self
    }
}
