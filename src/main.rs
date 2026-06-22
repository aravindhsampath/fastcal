// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::cli::OutputFormat;
use crate::models::ErrorResponse;
use clap::Parser;
use log::info;

mod caldav;
mod cli;
mod commands;
mod config;
mod formatters;
mod models;
mod parsers;
mod timezone;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI arguments first to get verbose flag
    let cli = cli::Cli::parse();

    // Initialize logging based on verbose flag
    let log_level = if cli.verbose { "debug" } else { "warn" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    info!("Starting fastcal (verbose={})", cli.verbose);

    // Capture the format before cli is consumed by execute()
    let use_json = matches!(cli.format, Some(OutputFormat::Json));

    // Execute command with a 60-second wall-clock timeout
    // Prevents hanging indefinitely if the CalDAV server is unresponsive
    let result = tokio::time::timeout(std::time::Duration::from_secs(60), cli.execute()).await;

    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let code = classify_exit_code(&e);
            if use_json {
                let response = ErrorResponse::new(format!("{:#}", e));
                eprintln!(
                    "{}",
                    serde_json::to_string_pretty(&response).unwrap_or_default()
                );
            } else {
                eprintln!("Error: {:#}", e);
            }
            std::process::exit(code);
        }
        Err(_) => {
            if use_json {
                let response = ErrorResponse::new("operation timed out after 60 seconds");
                eprintln!(
                    "{}",
                    serde_json::to_string_pretty(&response).unwrap_or_default()
                );
            } else {
                eprintln!("Error: operation timed out after 60 seconds");
            }
            std::process::exit(3); // network/timeout
        }
    }

    Ok(())
}

/// Classify an anyhow error into a process exit code.
///
/// - 1: general error
/// - 2: authentication failure (HTTP 401/403, auth errors)
/// - 3: network / connectivity failure (timeouts, connection errors)
/// - 4: resource not found (event or calendar not found)
fn classify_exit_code(e: &anyhow::Error) -> i32 {
    let msg = format!("{:#}", e).to_lowercase();
    if msg.contains("not found") || msg.contains("no vevent") {
        4
    } else if msg.contains("401")
        || msg.contains("403")
        || msg.contains("unauthorized")
        || msg.contains("forbidden")
        || msg.contains("auth failed")
    {
        2
    } else if msg.contains("timed out")
        || msg.contains("connection")
        || msg.contains("dns")
        || msg.contains("network")
    {
        3
    } else {
        1
    }
}
