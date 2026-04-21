// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! JSON output structures
//!
//! Defines consistent response formats for all commands.

use serde::{Deserialize, Serialize};

/// Standard success response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessResponse<T> {
    pub status: String,
    pub data: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
}

impl<T> SuccessResponse<T> {
    pub fn new(data: T) -> Self {
        Self {
            status: "success".to_string(),
            data,
            metadata: None,
        }
    }

    pub fn with_metadata(data: T, metadata: Metadata) -> Self {
        Self {
            status: "success".to_string(),
            data,
            metadata: Some(metadata),
        }
    }
}

/// Metadata for list responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_range: Option<DateRange>,
}

impl Metadata {
    pub fn new() -> Self {
        Self {
            count: None,
            calendar: None,
            date_range: None,
        }
    }

    pub fn with_count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    pub fn with_calendar(mut self, calendar: String) -> Self {
        self.calendar = Some(calendar);
        self
    }

    pub fn with_date_range(mut self, from: String, to: String) -> Self {
        self.date_range = Some(DateRange { from, to });
        self
    }
}

impl Default for Metadata {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateRange {
    pub from: String,
    pub to: String,
}

/// Standard error response wrapper (for --format json mode)
#[derive(Debug, Clone, Serialize)]
pub struct ErrorResponse {
    pub status: String,
    pub error: ErrorDetail,
}

/// Error detail payload
#[derive(Debug, Clone, Serialize)]
pub struct ErrorDetail {
    pub message: String,
}

impl ErrorResponse {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            status: "error".to_string(),
            error: ErrorDetail {
                message: message.into(),
            },
        }
    }
}
