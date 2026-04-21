// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Event data model
//!
//! Represents a calendar event with all properties.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Calendar event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Event ID (UID from iCalendar)
    pub id: String,

    /// Full href/URL to the event resource
    pub href: String,

    /// Calendar this event belongs to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar: Option<String>,

    /// Event summary/title
    pub summary: String,

    /// Event description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Event start time
    pub start: EventDateTime,

    /// Event end time
    pub end: EventDateTime,

    /// Duration in minutes (calculated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_minutes: Option<i64>,

    /// Event location
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,

    /// Event attendees
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attendees: Option<Vec<Attendee>>,

    /// Event status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<EventStatus>,

    /// Event creation timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<DateTime<Utc>>,

    /// Event last modified timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<DateTime<Utc>>,

    /// Organizer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organizer: Option<String>,

    /// Is this an all-day event?
    #[serde(default)]
    pub all_day: bool,

    /// ETag for optimistic concurrency control
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
}

/// Event date/time with timezone
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDateTime {
    /// ISO 8601 datetime string
    pub datetime: String,

    /// Timezone name (IANA)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

impl EventDateTime {
    pub fn new(datetime: String, timezone: Option<String>) -> Self {
        Self { datetime, timezone }
    }
}

/// Event attendee
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attendee {
    /// Email address
    pub email: String,

    /// Display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Attendance status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<AttendeeStatus>,
}

/// Attendee status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttendeeStatus {
    Accepted,
    Declined,
    Tentative,
    NeedsAction,
}

impl std::fmt::Display for AttendeeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttendeeStatus::Accepted => write!(f, "accepted"),
            AttendeeStatus::Declined => write!(f, "declined"),
            AttendeeStatus::Tentative => write!(f, "tentative"),
            AttendeeStatus::NeedsAction => write!(f, "needs-action"),
        }
    }
}

/// Event status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventStatus {
    Confirmed,
    Tentative,
    Cancelled,
}

impl std::fmt::Display for EventStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventStatus::Confirmed => write!(f, "confirmed"),
            EventStatus::Tentative => write!(f, "tentative"),
            EventStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}
