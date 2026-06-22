// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Timezone resolution and validation.
//!
//! fastcal keeps instants in UTC internally and on the CalDAV wire, but the
//! human boundary (input parsing and display) happens in exactly one IANA
//! zone resolved per invocation. This module resolves and validates that
//! zone; [`crate::parsers::datetime`] does the local↔UTC conversions.

use anyhow::{Context, Result};
use chrono_tz::Tz;

/// Parse and validate an IANA timezone name (e.g. `Europe/Amsterdam`).
///
/// Rejects fixed-offset strings and typos with an actionable error so a bad
/// `--timezone` flag or config value fails loudly instead of silently doing
/// the wrong thing.
pub fn parse_tz(name: &str) -> Result<Tz> {
    name.parse::<Tz>().map_err(|_| {
        anyhow::anyhow!(
            "unknown timezone '{name}'. Use an IANA name like \
             'Europe/Amsterdam' or 'America/New_York'."
        )
    })
}

/// Resolve the single effective timezone for this invocation, by precedence:
/// 1. `flag` — the global `--timezone` CLI override (travel / one-offs)
/// 2. `config_tz` — `preferences.default_timezone` (the user's home zone)
/// 3. the host system timezone (via `iana-time-zone`)
/// 4. `UTC` (last-resort fallback)
///
/// An explicitly-configured zone (flag or config) that fails to parse is a
/// hard error — we never silently fall back past a value the user set on
/// purpose. A missing or unparseable *system* zone falls through to UTC,
/// since the host zone is incidental (a cloud box is usually UTC).
pub fn resolve(flag: Option<&str>, config_tz: Option<&str>) -> Result<Tz> {
    if let Some(name) = flag {
        return parse_tz(name).context("invalid --timezone");
    }
    if let Some(name) = config_tz {
        return parse_tz(name).context(
            "invalid preferences.default_timezone in config \
             (fix with `fastcal config set preferences.default_timezone <IANA>`)",
        );
    }
    if let Some(tz) = iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| name.parse::<Tz>().ok())
    {
        return Ok(tz);
    }
    Ok(Tz::UTC)
}

/// Best-effort detection of the host system timezone as a canonical IANA
/// name, falling back to `"UTC"`. Used by `config init` to pre-fill
/// `default_timezone` so a fresh setup matches where the user actually is.
pub fn detect_system_tz() -> String {
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| name.parse::<Tz>().ok())
        .map(|tz| tz.name().to_string())
        .unwrap_or_else(|| "UTC".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tz_accepts_iana_name() {
        assert_eq!(parse_tz("Europe/Amsterdam").unwrap(), Tz::Europe__Amsterdam);
    }

    #[test]
    fn parse_tz_rejects_unknown() {
        let err = parse_tz("Totally/Fake").unwrap_err();
        assert!(err.to_string().to_lowercase().contains("unknown"));
    }

    #[test]
    fn parse_tz_rejects_fixed_offset() {
        // Fixed offsets defeat DST handling — only IANA zones are allowed.
        assert!(parse_tz("+02:00").is_err());
    }

    #[test]
    fn resolve_prefers_flag_over_config() {
        let tz = resolve(Some("America/New_York"), Some("Europe/Amsterdam")).unwrap();
        assert_eq!(tz, Tz::America__New_York);
    }

    #[test]
    fn resolve_uses_config_when_no_flag() {
        let tz = resolve(None, Some("Europe/Amsterdam")).unwrap();
        assert_eq!(tz, Tz::Europe__Amsterdam);
    }

    #[test]
    fn resolve_errors_on_invalid_flag() {
        assert!(resolve(Some("Nope/Nope"), Some("Europe/Amsterdam")).is_err());
    }

    #[test]
    fn resolve_errors_on_invalid_config() {
        assert!(resolve(None, Some("Bogus/Zone")).is_err());
    }

    #[test]
    fn resolve_falls_back_without_flag_or_config() {
        // No flag, no config → system or UTC; must succeed and yield *some* zone.
        assert!(resolve(None, None).is_ok());
    }
}
