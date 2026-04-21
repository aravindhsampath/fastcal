// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Integration tests for the fastcal CLI binary
//!
//! These tests exercise the compiled binary end-to-end without a live CalDAV
//! server. They verify argument parsing, error formatting, and output structure.

use assert_cmd::Command;
use predicates::prelude::*;

fn cmd() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("fastcal"))
}

// ── Help and version ──────────────────────────────────────────────────────────

#[test]
fn help_exits_zero() {
    cmd().arg("--help").assert().success();
}

#[test]
fn help_mentions_caldav() {
    cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("CalDAV"));
}

#[test]
fn version_exits_zero() {
    cmd().arg("--version").assert().success();
}

// ── Shell completions ─────────────────────────────────────────────────────────

#[test]
fn completions_bash_exits_zero() {
    cmd()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fastcal"));
}

#[test]
fn completions_zsh_exits_zero() {
    cmd()
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fastcal"));
}

// ── No-config error handling ──────────────────────────────────────────────────

#[test]
fn events_list_no_config_exits_nonzero() {
    // Without a config file the command must fail gracefully
    cmd()
        .env("HOME", "/nonexistent_home_for_test")
        .env("XDG_CONFIG_HOME", "/nonexistent_config_for_test")
        .args(["events", "list"])
        .assert()
        .failure();
}

#[test]
fn events_list_json_error_is_structured() {
    // With --format json, errors must be JSON objects, not plain text
    let output = cmd()
        .env("HOME", "/nonexistent_home_for_test")
        .env("XDG_CONFIG_HOME", "/nonexistent_config_for_test")
        .args(["--format", "json", "events", "list"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "expected failure");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Must be valid JSON with a "status" key
    let parsed: serde_json::Value =
        serde_json::from_str(&stderr).expect("error output must be valid JSON");
    assert_eq!(
        parsed["status"], "error",
        "JSON error must have status=error"
    );
    assert!(
        parsed["error"]["message"].is_string(),
        "JSON error must have error.message"
    );
}

#[test]
fn dry_run_flag_is_accepted() {
    // --dry-run is a global flag; it must not be rejected as unknown
    cmd()
        .env("HOME", "/nonexistent_home_for_test")
        .env("XDG_CONFIG_HOME", "/nonexistent_config_for_test")
        .args(["--dry-run", "events", "list"])
        .assert()
        .failure() // fails due to missing config, not flag rejection
        .stderr(predicate::str::contains("config").or(predicate::str::contains("Error")));
}

// ── Batch file parsing ────────────────────────────────────────────────────────

#[test]
fn batch_create_dry_run_from_json() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    writeln!(
        tmp,
        r#"[{{"summary":"Team Meeting","start":"2026-03-10T14:00:00Z"}}]"#
    )
    .unwrap();

    cmd()
        .env("HOME", "/nonexistent_home_for_test")
        .env("XDG_CONFIG_HOME", "/nonexistent_config_for_test")
        .args(["--dry-run", "batch", "create", tmp.path().to_str().unwrap()])
        .assert()
        .failure() // fails at config load, but we verify flag wiring
        .stderr(predicate::str::contains("config").or(predicate::str::contains("Error")));
}
