// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Config command implementations

use crate::caldav;
use crate::config::{self, Config};
use anyhow::{Context, Result};
use serde_json::json;
use std::io::{self, Write};

/// Execute config init command
///
/// Prompts for credentials, performs service discovery,
/// and saves configuration to file.
pub async fn init(_ctx: &crate::commands::context::CommandContext) -> Result<()> {
    println!("fastcal configuration initialization");
    println!();

    // Check if config already exists
    if config::loader::exists() {
        print!("Configuration file already exists. Overwrite? [y/N]: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Initialization cancelled");
            return Ok(());
        }
    }

    // Prompt for credentials
    print!("Fastmail username (full email): ");
    io::stdout().flush()?;
    let mut username = String::new();
    io::stdin().read_line(&mut username)?;
    let username = username.trim().to_string();

    print!("Fastmail app password: ");
    io::stdout().flush()?;
    let password = rpassword::read_password()?;
    println!();

    if username.is_empty() || password.is_empty() {
        anyhow::bail!("Username and password are required");
    }

    println!("Performing service discovery...");

    // Create discovery client
    let client = caldav::create_discovery_client(
        username.clone(),
        password.clone(),
        "https://fastmail.com".to_string(),
    )
    .await
    .context("Failed to create discovery client")?;

    // Perform discovery
    let discovery_result = config::discovery::discover(client)
        .await
        .context("Service discovery failed")?;

    println!("✓ Discovery successful");
    println!("  Context URL: {}", discovery_result.context_url);
    if let Some(ref principal) = discovery_result.principal {
        println!("  Principal: {}", principal);
    }
    println!("  Found {} calendar(s)", discovery_result.calendars.len());

    for (name, href) in &discovery_result.calendars {
        println!("    - {}: {}", name, href);
    }

    // Create config
    let mut config = Config::minimal(username, password);
    config::discovery::update_config_with_discovery(&mut config, discovery_result);

    // Pre-fill the timezone from the host system so a fresh setup matches
    // where the user actually is (instead of a baked-in default).
    let detected_tz = crate::timezone::detect_system_tz();
    println!("  Detected timezone: {}", detected_tz);
    config.preferences.default_timezone = detected_tz;

    // Save config
    let config_path = Config::config_path()?;
    config.save().context("Failed to save configuration")?;

    println!();
    println!("✓ Configuration saved to: {}", config_path.display());
    println!();
    println!("You can now use fastcal commands!");

    Ok(())
}

/// Execute config show command
///
/// Displays current configuration (with password redacted)
pub async fn show(ctx: &crate::commands::context::CommandContext) -> Result<()> {
    let config = ctx
        .load_config()
        .context("Failed to load configuration. Run 'fastcal config init' first.")?;

    // Create redacted copy
    let mut config_display = config.clone();
    if config_display.server.app_password.is_some() {
        config_display.server.app_password = Some("***REDACTED***".to_string());
    }

    let config_path = Config::config_path()?.display().to_string();

    use crate::cli::OutputFormat;
    match ctx.format {
        OutputFormat::Text => {
            println!("Config file: {}\n", config_path);
            println!("Server:");
            println!("  URL:          {}", config_display.server.url);
            println!("  Username:     {}", config_display.server.username);
            println!(
                "  App password: {}",
                config_display
                    .server
                    .app_password
                    .as_deref()
                    .unwrap_or("(not set)")
            );
            if let Some(ref url) = config_display.server.caldav_url {
                println!("  CalDAV URL:   {}", url);
            }
            if let Some(ref p) = config_display.server.principal {
                println!("  Principal:    {}", p);
            }
            println!("\nCalendars:");
            if config_display.calendars.is_empty() {
                println!("  (none configured)");
            } else {
                let mut names: Vec<_> = config_display.calendars.iter().collect();
                names.sort_by_key(|(k, _)| (*k).clone());
                for (name, href) in names {
                    println!("  {}: {}", name, href);
                }
            }
            println!("\nPreferences:");
            println!(
                "  Default calendar: {}",
                config_display.preferences.default_calendar
            );
            println!(
                "  Default timezone: {}",
                config_display.preferences.default_timezone
            );
            println!(
                "  Output format:    {}",
                config_display.preferences.output_format
            );
        }
        OutputFormat::Json => {
            let response = crate::models::SuccessResponse::new(json!({
                "config": {
                    "server": {
                        "url": config_display.server.url,
                        "username": config_display.server.username,
                        "app_password": config_display.server.app_password,
                        "caldav_url": config_display.server.caldav_url,
                        "principal": config_display.server.principal,
                    },
                    "calendars": config_display.calendars,
                    "preferences": {
                        "default_calendar": config_display.preferences.default_calendar,
                        "default_timezone": config_display.preferences.default_timezone,
                        "output_format": config_display.preferences.output_format,
                    }
                },
                "config_file": config_path,
            }));
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
    }

    Ok(())
}

/// Execute config set command
///
/// Sets a configuration value
pub async fn set(
    ctx: &crate::commands::context::CommandContext,
    key: String,
    value: String,
) -> Result<()> {
    let mut config = ctx
        .load_config()
        .context("Failed to load configuration. Run 'fastcal config init' first.")?;

    // Parse the key and update config
    match key.as_str() {
        "preferences.default_calendar" => {
            config.preferences.default_calendar = value.clone();
        }
        "preferences.default_timezone" => {
            // Validate up front so a bad zone can't be persisted and then
            // break every later command at resolution time.
            crate::timezone::parse_tz(&value).context("cannot set default_timezone")?;
            config.preferences.default_timezone = value.clone();
        }
        "preferences.output_format" => {
            if !["json", "text"].contains(&value.as_str()) {
                anyhow::bail!("Invalid output format. Must be: json or text");
            }
            config.preferences.output_format = value.clone();
        }
        _ => {
            anyhow::bail!("Unknown configuration key: {}. Supported keys: preferences.default_calendar, preferences.default_timezone, preferences.output_format", key);
        }
    }

    // Save updated config
    config.save().context("Failed to save configuration")?;

    use crate::cli::OutputFormat;
    match ctx.format {
        OutputFormat::Text => {
            println!("Configuration updated: {} = {}", key, value);
        }
        OutputFormat::Json => {
            let response = crate::models::SuccessResponse::new(json!({
                "message": format!("Configuration updated: {} = {}", key, value)
            }));
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
    }

    Ok(())
}

/// Execute config test command
///
/// Tests connection to Fastmail using discovered endpoints
pub async fn test(ctx: &crate::commands::context::CommandContext) -> Result<()> {
    eprintln!("Testing connection to Fastmail...");

    let config = ctx
        .load_config()
        .context("Failed to load configuration. Run 'fastcal config init' first.")?;

    // Verify we have discovered URLs
    if config.server.caldav_url.is_none() {
        anyhow::bail!("No CalDAV URL discovered. Run 'fastcal config init' first.");
    }

    let principal_url = config
        .server
        .principal
        .as_ref()
        .context("No principal URL found. Run 'fastcal config init' first.")?;

    // Create client using discovered URLs (no re-discovery needed)
    let client = caldav::create_client(&config, false)
        .await
        .context("Failed to create CalDAV client")?;

    use libdav::caldav::FindCalendarHomeSet;

    match client
        .request(FindCalendarHomeSet::new(principal_url))
        .await
    {
        Ok(home_sets) => {
            use crate::cli::OutputFormat;
            match ctx.format {
                OutputFormat::Text => {
                    println!("✓ Connection successful");
                    println!("  Principal:            {}", principal_url);
                    if let Some(ref url) = config.server.caldav_url {
                        println!("  CalDAV URL:           {}", url);
                    }
                    println!("  Calendar home sets:   {}", home_sets.home_sets.len());
                    println!("  Calendars configured: {}", config.calendars.len());
                }
                OutputFormat::Json => {
                    let response = crate::models::SuccessResponse::new(json!({
                        "message": "Connection successful",
                        "principal": principal_url,
                        "caldav_url": config.server.caldav_url,
                        "calendar_home_sets": home_sets.home_sets.len(),
                        "calendars_configured": config.calendars.len(),
                    }));
                    println!("{}", serde_json::to_string_pretty(&response)?);
                }
            }
            Ok(())
        }
        Err(e) => {
            anyhow::bail!(
                "Failed to access calendar home sets: {}. Try running 'fastcal config init' again.",
                e
            );
        }
    }
}
