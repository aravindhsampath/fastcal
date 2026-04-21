// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! CalDAV client initialization
//!
//! Based on davcli's caldav_client() pattern.
//! Creates and configures the libdav CalDavClient.

use super::auth::AddAuthorization;
use crate::config::Config;
use anyhow::{Context, Result};
use http::Uri;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::{connect::HttpConnector, Client as HyperClient};
use hyper_util::rt::TokioExecutor;
use libdav::{dav::WebDavClient, CalDavClient};

/// Type alias for our configured CalDAV client
pub type Client =
    CalDavClient<AddAuthorization<HyperClient<HttpsConnector<HttpConnector>, String>>>;

/// The WebDAV client type before wrapping in CalDavClient.
type WebDav = WebDavClient<AddAuthorization<HyperClient<HttpsConnector<HttpConnector>, String>>>;

/// Build an authenticated WebDAV client for the given URL and credentials.
fn build_webdav_client(base_url: Uri, username: &str, password: &str) -> Result<WebDav> {
    let https = HttpsConnectorBuilder::new()
        .with_native_roots()
        .context("Failed to load native TLS roots")?
        .https_or_http()
        .enable_http1()
        .build();

    let raw_client = HyperClient::builder(TokioExecutor::new()).build(https);
    let auth_client = AddAuthorization::basic(raw_client, username, password);

    Ok(WebDavClient::new(base_url, auth_client))
}

/// Create a new CalDAV client from configuration
///
/// Optionally bootstraps via service discovery.
pub async fn create_client(config: &Config, enable_discovery: bool) -> Result<Client> {
    let username = config.get_username()?;
    let password = config.get_password()?;
    let base_url: Uri = config
        .get_base_url()?
        .parse()
        .context("Failed to parse base URL")?;

    log::debug!("Creating CalDAV client for: {}", base_url);

    let webdav = build_webdav_client(base_url, &username, &password)?;

    let client = if enable_discovery {
        log::info!("Bootstrapping CalDAV client via service discovery");
        CalDavClient::bootstrap_via_service_discovery(webdav)
            .await
            .context("Service discovery failed")?
    } else {
        CalDavClient::new(webdav)
    };

    log::debug!("CalDAV client created successfully");
    Ok(client)
}

/// Create a client specifically for discovery (initial setup)
pub async fn create_discovery_client(
    username: String,
    password: String,
    server_url: String,
) -> Result<Client> {
    log::info!("Creating discovery client for: {}", server_url);

    let base_url: Uri = server_url.parse().context("Failed to parse server URL")?;
    let webdav = build_webdav_client(base_url, &username, &password)?;

    let client = CalDavClient::bootstrap_via_service_discovery(webdav)
        .await
        .context("Service discovery failed")?;

    log::info!("Discovery client created successfully");
    Ok(client)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_url() {
        let url = "https://fastmail.com";
        let parsed: Result<Uri, _> = url.parse();
        assert!(parsed.is_ok());
    }

    #[test]
    fn test_parse_invalid_url() {
        let url = "not a url";
        let parsed: Result<Uri, _> = url.parse();
        assert!(parsed.is_err());
    }
}
