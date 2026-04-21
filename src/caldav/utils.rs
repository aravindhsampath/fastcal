// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Utility functions for CalDAV operations

use super::Client;
use libdav::{dav::GetProperty, names::DISPLAY_NAME};
use std::collections::HashMap;

/// Fetch display names for multiple calendar hrefs concurrently
///
/// This function takes a list of calendar hrefs and fetches their display names
/// in parallel, avoiding the N+1 query problem.
///
/// Returns a HashMap with href strings as keys.
pub async fn fetch_display_names_concurrent(
    client: &Client,
    hrefs: &[String],
) -> HashMap<String, Option<String>> {
    let display_name_futures: Vec<_> = hrefs
        .iter()
        .map(|href| async move {
            let display_name = client
                .request(GetProperty::new(href.as_str(), &DISPLAY_NAME))
                .await
                .ok()
                .and_then(|prop| prop.value);
            (href.clone(), display_name)
        })
        .collect();

    // Execute all display name requests concurrently
    let results = futures_util::future::join_all(display_name_futures).await;

    results.into_iter().collect()
}

/// Retry an async operation up to `max_attempts` times on transient network errors.
///
/// Transient errors (connection failures, timeouts, 503 Service Unavailable) are
/// retried with exponential backoff (100ms, 200ms, 400ms for 3 attempts).
/// Permanent errors (auth failures, not-found, validation) propagate immediately.
pub async fn retry_transient<T, F, Fut>(max_attempts: u32, op: F) -> anyhow::Result<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<T>>,
{
    let mut attempts = 0;
    loop {
        match op().await {
            Ok(val) => return Ok(val),
            Err(e) if attempts + 1 < max_attempts && is_transient_error(&e) => {
                let delay = std::time::Duration::from_millis(100 * (1u64 << attempts));
                log::debug!(
                    "Transient error (attempt {}/{}), retrying in {:?}: {}",
                    attempts + 1,
                    max_attempts,
                    delay,
                    e
                );
                tokio::time::sleep(delay).await;
                attempts += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Returns true if an error is likely transient and worth retrying.
fn is_transient_error(e: &anyhow::Error) -> bool {
    let msg = format!("{:#}", e).to_lowercase();
    msg.contains("connection")
        || msg.contains("timed out")
        || msg.contains("503")
        || msg.contains("service unavailable")
        || msg.contains("temporarily")
        || msg.contains("reset by peer")
}

/// Extract calendar name from href (last path segment)
pub fn extract_calendar_name_from_href(href: &str) -> String {
    href.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("calendar")
        .to_string()
}

/// Generate unique name by appending counter if name already exists
pub fn ensure_unique_name(name: String, existing_names: &HashMap<String, impl Clone>) -> String {
    if !existing_names.contains_key(&name) {
        return name;
    }

    let mut counter = 2;
    loop {
        let candidate = format!("{} ({})", name, counter);
        if !existing_names.contains_key(&candidate) {
            return candidate;
        }
        counter += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_calendar_name_from_href() {
        assert_eq!(
            extract_calendar_name_from_href(
                "https://caldav.fastmail.com/dav/calendars/user/personal/"
            ),
            "personal"
        );
        assert_eq!(
            extract_calendar_name_from_href("https://caldav.fastmail.com/dav/calendars/user/work"),
            "work"
        );
    }

    #[test]
    fn test_ensure_unique_name_no_collision() {
        let existing: HashMap<String, String> = HashMap::new();
        assert_eq!(
            ensure_unique_name("calendar".to_string(), &existing),
            "calendar"
        );
    }

    #[test]
    fn test_ensure_unique_name_with_collision() {
        let mut existing: HashMap<String, String> = HashMap::new();
        existing.insert("calendar".to_string(), "value".to_string());

        assert_eq!(
            ensure_unique_name("calendar".to_string(), &existing),
            "calendar (2)"
        );
    }

    #[test]
    fn test_ensure_unique_name_multiple_collisions() {
        let mut existing: HashMap<String, String> = HashMap::new();
        existing.insert("calendar".to_string(), "value".to_string());
        existing.insert("calendar (2)".to_string(), "value".to_string());

        assert_eq!(
            ensure_unique_name("calendar".to_string(), &existing),
            "calendar (3)"
        );
    }
}
