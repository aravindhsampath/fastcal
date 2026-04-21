// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! CalDAV client wrapper module
//!
//! Wraps libdav and provides high-level CalDAV operations.

pub mod auth;
pub mod calendar;
pub mod calendar_query_expand;
pub mod client;
pub mod event;
pub mod utils;

// Re-export commonly used types
pub use client::{create_client, create_discovery_client, Client};
pub use utils::retry_transient;
