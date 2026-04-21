// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Data models module
//!
//! Contains data structures for events, calendars, errors, and output formatting.

pub mod calendar;
pub mod event;
pub mod output;

pub use calendar::Calendar;
pub use event::{Attendee, Event, EventDateTime, EventStatus};
pub use output::{ErrorResponse, Metadata, SuccessResponse};
