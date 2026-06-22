// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! iCalendar (ICS) parsing using calcard
//!
//! Uses the calcard crate for robust RFC 5545 compliant parsing.
//! Handles line folding, property parameters, and all iCalendar edge cases.

use crate::models::{Attendee, Event, EventDateTime, EventStatus, Reminder};
use anyhow::{Context, Result};
use calcard::icalendar::{ICalendar, ICalendarProperty, ICalendarStatus};
use chrono::{DateTime, Duration, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use log::{debug, warn};

/// Parse ICS data into an Event
pub fn parse_event(ics_data: &str, href: String, etag: Option<String>) -> Result<Event> {
    // Parse using calcard - this handles line folding and all RFC 5545 complexity
    let calendar = ICalendar::parse(ics_data)
        .map_err(|e| anyhow::anyhow!("Failed to parse iCalendar: {:?}", e))?;

    // Find the first VEVENT component
    let event_component = calendar
        .components
        .iter()
        .find(|c| {
            matches!(
                c.component_type,
                calcard::icalendar::ICalendarComponentType::VEvent
            )
        })
        .context("No VEVENT component found in iCalendar data")?;

    // Extract UID (required) - calcard provides this as a convenience method
    let id = event_component
        .uid()
        .context("No UID found in event")?
        .to_string();

    // Unfold the original ICS data (join RFC 5545 continuation lines) so the
    // TZID-aware date and VALARM extractors below can scan it directly.
    let unfolded = unfold_ics(ics_data);

    // Content (non-date) properties come straight from calcard's parsed model,
    // which has already unfolded lines and unescaped text. Read a single-value
    // text property off the VEVENT component.
    let text_prop = |prop: &ICalendarProperty| -> Option<String> {
        event_component
            .property(prop)
            .and_then(|entry| entry.values.first())
            .and_then(|value| value.as_text())
            .map(str::to_owned)
    };

    let summary =
        text_prop(&ICalendarProperty::Summary).unwrap_or_else(|| "(No title)".to_string());
    let description = text_prop(&ICalendarProperty::Description);
    let location = text_prop(&ICalendarProperty::Location);

    // Extract start time (required). Uses the TZID-aware extractor so
    // `DTSTART;TZID=Asia/Kolkata:20260423T143000` round-trips correctly
    // instead of being silently flattened as UTC.
    let (dtstart_val, dtstart_tzid) =
        extract_dt_property(&unfolded, "DTSTART").context("No DTSTART found in event")?;
    let (start_dt, start_tz, all_day) = parse_datetime(&dtstart_val, dtstart_tzid.as_deref())?;
    let start = EventDateTime::new(start_dt.clone(), start_tz.clone());

    // Extract end time
    let end = if let Some((dtend_val, dtend_tzid)) = extract_dt_property(&unfolded, "DTEND") {
        let (end_dt, end_tz, _) = parse_datetime(&dtend_val, dtend_tzid.as_deref())?;
        EventDateTime::new(end_dt, end_tz)
    } else if let Some(duration_str) = extract_property(&unfolded, "DURATION") {
        if let Ok(duration_secs) = parse_duration(&duration_str) {
            let start_parsed = DateTime::parse_from_rfc3339(&start_dt)
                .or_else(|_| parse_date_only(&start_dt))
                .context("Failed to parse start datetime")?;
            let end_dt = start_parsed + Duration::seconds(duration_secs);
            EventDateTime::new(end_dt.to_rfc3339(), start_tz.clone())
        } else {
            default_end_time(&start_dt, &start_tz)?
        }
    } else {
        default_end_time(&start_dt, &start_tz)?
    };

    // Calculate duration in minutes
    let duration_minutes = calculate_duration(&start.datetime, &end.datetime).ok();

    // Status via calcard's typed accessor (other statuses map to None, as
    // before — our model only carries the three VEVENT states).
    let status = event_component.status().and_then(|s| match s {
        ICalendarStatus::Confirmed => Some(EventStatus::Confirmed),
        ICalendarStatus::Tentative => Some(EventStatus::Tentative),
        ICalendarStatus::Cancelled => Some(EventStatus::Cancelled),
        _ => None,
    });

    // Extract created/modified timestamps
    // CREATED / LAST-MODIFIED are defined by RFC 5545 to always be UTC
    // (end with 'Z'), so TZID isn't meaningful for them — pass `None`.
    let created = extract_property(&unfolded, "CREATED")
        .and_then(|s| parse_datetime(&s, None).ok())
        .and_then(|(dt, _, _)| DateTime::parse_from_rfc3339(&dt).ok())
        .map(|dt| dt.with_timezone(&Utc));

    let modified = extract_property(&unfolded, "LAST-MODIFIED")
        .and_then(|s| parse_datetime(&s, None).ok())
        .and_then(|(dt, _, _)| DateTime::parse_from_rfc3339(&dt).ok())
        .map(|dt| dt.with_timezone(&Utc));

    // Organizer / attendees via calcard (CAL-ADDRESS URIs); strip mailto:.
    let strip_mailto = |s: &str| s.strip_prefix("mailto:").unwrap_or(s).to_string();
    let organizer = text_prop(&ICalendarProperty::Organizer).map(|s| strip_mailto(&s));

    let attendee_list: Vec<Attendee> = event_component
        .properties(&ICalendarProperty::Attendee)
        .filter_map(|entry| entry.values.first())
        .filter_map(|value| value.as_text())
        .map(|s| Attendee {
            email: strip_mailto(s),
            name: None,
            status: None,
        })
        .collect();
    let attendees = (!attendee_list.is_empty()).then_some(attendee_list);

    // Recurrence rule, preserved verbatim. Present on master events of a
    // series; absent on single events and on server-expanded instances.
    let rrule = extract_property(&unfolded, "RRULE");

    // RECURRENCE-ID: present on an expanded instance or an overridden
    // occurrence. Can carry a TZID, so reuse the TZID-aware extractor.
    let recurrence_id = extract_dt_property(&unfolded, "RECURRENCE-ID")
        .and_then(|(val, tz)| parse_datetime(&val, tz.as_deref()).ok())
        .map(|(iso, _, _)| iso);

    // VALARM sub-components: one `Reminder` per supported VALARM block.
    // Absolute-time / RELATED=END triggers are silently dropped with a
    // debug log — see `extract_valarms` for the supported shapes.
    let reminders = extract_valarms(&unfolded);

    Ok(Event {
        id,
        href,
        calendar: None, // Will be set by caller
        summary,
        description,
        start,
        end,
        duration_minutes,
        location,
        attendees,
        status,
        created,
        modified,
        organizer,
        all_day,
        etag,
        rrule,
        recurrence_id,
        reminders,
    })
}

/// Scan VEVENT for VALARM sub-components and normalize each into a
/// [`Reminder`]. Only supported shape is a relative-before-start trigger
/// expressed as `-PT<N>M`, `-PT<N>H`, or `-P<N>D` (optionally combined:
/// `-PT1H30M`). Absolute triggers (`TRIGGER;VALUE=DATE-TIME:...`) and
/// `RELATED=END` are dropped with a `debug!` — the caller loses them on
/// round-trip, which is acceptable for the MVP.
///
/// Input is expected to be unfolded (see [`unfold_ics`]).
fn extract_valarms(unfolded: &str) -> Vec<Reminder> {
    let mut out = Vec::new();
    let mut in_vevent = false;
    let mut in_valarm = false;
    // Per-VALARM collectors — reset on BEGIN:VALARM.
    let mut action: Option<String> = None;
    let mut trigger_raw: Option<String> = None;
    let mut trigger_has_params = false;
    let mut description: Option<String> = None;

    for line in unfolded.lines() {
        let line = line.trim();

        if line == "BEGIN:VEVENT" {
            in_vevent = true;
            continue;
        }
        if line == "END:VEVENT" {
            in_vevent = false;
            in_valarm = false;
            continue;
        }
        if !in_vevent {
            continue;
        }

        if line == "BEGIN:VALARM" {
            in_valarm = true;
            action = None;
            trigger_raw = None;
            trigger_has_params = false;
            description = None;
            continue;
        }
        if line == "END:VALARM" {
            if let Some(reminder) = finalize_valarm(
                action.take(),
                trigger_raw.take(),
                trigger_has_params,
                description.take(),
            ) {
                out.push(reminder);
            }
            trigger_has_params = false;
            in_valarm = false;
            continue;
        }
        if !in_valarm {
            continue;
        }

        // Inside a VALARM — pick up the three properties we care about.
        // `TRIGGER` may carry parameters (`TRIGGER;VALUE=DATE-TIME:...`
        // or `TRIGGER;RELATED=END:-PT5M`) — record whether any appear so
        // we can reject non-default shapes at finalize time.
        if let Some(rest) = line.strip_prefix("ACTION") {
            if let Some(val) = value_after_colon(rest) {
                action = Some(val.to_ascii_lowercase());
            }
        } else if let Some(rest) = line.strip_prefix("TRIGGER") {
            match rest.chars().next() {
                Some(':') => {
                    trigger_raw = Some(rest[1..].trim().to_owned());
                }
                Some(';') => {
                    trigger_has_params = true;
                    if let Some(val) = value_after_colon(rest) {
                        trigger_raw = Some(val.to_owned());
                    }
                }
                _ => {}
            }
        } else if let Some(rest) = line.strip_prefix("DESCRIPTION") {
            if let Some(val) = value_after_colon(rest) {
                description = Some(unescape_ics_text(val));
            }
        }
    }

    out
}

/// Return the substring after the first `:` of a property-suffix (the
/// portion after the property name). `rest` starts with either `:` or
/// `;PARAM=VAL...:`. Returns `None` if neither is true.
fn value_after_colon(rest: &str) -> Option<&str> {
    match rest.chars().next()? {
        ':' => Some(rest[1..].trim()),
        ';' => {
            let colon = rest.find(':')?;
            Some(rest[colon + 1..].trim())
        }
        _ => None,
    }
}

/// Combine the three collected VALARM properties into a `Reminder`, or
/// `None` if the TRIGGER isn't a supported shape.
///
/// Supported: `-PT<N>M`, `-PT<N>H`, `-P<N>D`, `-PT<H>H<M>M` — anything
/// starting with `-P` and consisting of optional days then optional
/// hours+minutes+seconds. Absolute datetimes (`20260423T...`) and
/// RELATED=END (marked by `trigger_has_params`) are rejected.
fn finalize_valarm(
    action: Option<String>,
    trigger: Option<String>,
    trigger_has_params: bool,
    description: Option<String>,
) -> Option<Reminder> {
    // `RELATED=END`, `VALUE=DATE-TIME`, or any other param → punt for now.
    // The MVP only cares about the default "relative to start" shape.
    if trigger_has_params {
        debug!(
            "valarm: skipping TRIGGER with params (likely RELATED=END or VALUE=DATE-TIME) \
             — not supported in MVP"
        );
        return None;
    }
    let trigger = trigger?;
    let minutes_before = parse_relative_before_start(&trigger)?;
    Some(Reminder {
        minutes_before,
        action: action.unwrap_or_else(|| "display".to_owned()),
        description,
    })
}

/// Parse a "negative duration before event start" trigger like `-PT5M`,
/// `-PT1H`, `-P1D`, or `-PT1H30M` into whole minutes. Returns `None` on:
/// - missing leading `-` (post-event triggers aren't supported on write,
///   but we drop them on read too rather than bolt on signed minutes)
/// - missing leading `P`
/// - any non-zero seconds component (we round down — but log it)
/// - unrecognized grammar
fn parse_relative_before_start(trigger: &str) -> Option<u32> {
    let after_sign = trigger.strip_prefix('-')?;
    let after_p = after_sign.strip_prefix('P')?;

    let (date_part, time_part) = match after_p.find('T') {
        Some(idx) => (&after_p[..idx], Some(&after_p[idx + 1..])),
        None => (after_p, None),
    };

    let mut minutes: u32 = 0;

    if !date_part.is_empty() {
        // Accept days and weeks (rare but spec-valid).
        let mut rest = date_part;
        while !rest.is_empty() {
            let letter_pos = rest.find(|c: char| c.is_ascii_alphabetic())?;
            let value: u32 = rest[..letter_pos].parse().ok()?;
            match rest.as_bytes()[letter_pos] {
                b'D' => minutes = minutes.checked_add(value.checked_mul(1440)?)?,
                b'W' => minutes = minutes.checked_add(value.checked_mul(7 * 1440)?)?,
                _ => return None,
            }
            rest = &rest[letter_pos + 1..];
        }
    }

    if let Some(mut rest) = time_part {
        while !rest.is_empty() {
            let letter_pos = rest.find(|c: char| c.is_ascii_alphabetic())?;
            let value: u32 = rest[..letter_pos].parse().ok()?;
            match rest.as_bytes()[letter_pos] {
                b'H' => minutes = minutes.checked_add(value.checked_mul(60)?)?,
                b'M' => minutes = minutes.checked_add(value)?,
                b'S' => {
                    // Sub-minute precision discarded; log once so surprising
                    // round-trips ("4:59 reminder became 4 min") are traceable.
                    if value != 0 {
                        debug!(
                            "valarm: TRIGGER `{trigger}` has seconds component — \
                             rounding down to whole minutes"
                        );
                    }
                }
                _ => return None,
            }
            rest = &rest[letter_pos + 1..];
        }
    }

    Some(minutes)
}

/// Extract a property value from ICS data, scoped to the VEVENT section
///
/// Only searches within BEGIN:VEVENT..END:VEVENT to avoid matching
/// properties from VTIMEZONE or other components (e.g., DTSTART in
/// VTIMEZONE's DAYLIGHT/STANDARD sub-components).
fn extract_property(ics_data: &str, property_name: &str) -> Option<String> {
    let mut in_vevent = false;
    let mut depth = 0; // Track nested components within VEVENT

    for line in ics_data.lines() {
        let line = line.trim();

        if line == "BEGIN:VEVENT" {
            in_vevent = true;
            depth = 0;
            continue;
        }
        if line == "END:VEVENT" {
            in_vevent = false;
            continue;
        }

        // Track nested components like VALARM inside VEVENT
        if in_vevent && line.starts_with("BEGIN:") {
            depth += 1;
            continue;
        }
        if in_vevent && line.starts_with("END:") {
            depth -= 1;
            continue;
        }

        // Only match properties at the VEVENT level (not inside VALARM etc.)
        if in_vevent && depth == 0 && line.starts_with(property_name) {
            if let Some(rest) = line.strip_prefix(property_name) {
                if rest.starts_with(':') || rest.starts_with(';') {
                    if let Some(colon_pos) = line.find(':') {
                        let value = &line[colon_pos + 1..];
                        return Some(unescape_ics_text(value.trim()));
                    }
                }
            }
        }
    }
    None
}

/// Unescape RFC 5545 text escape sequences
///
/// ICS text fields use: \n → newline, \, → comma, \; → semicolon, \\ → backslash
fn unescape_ics_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.peek() {
                Some('n') | Some('N') => {
                    chars.next();
                    result.push('\n');
                }
                Some(',') => {
                    chars.next();
                    result.push(',');
                }
                Some(';') => {
                    chars.next();
                    result.push(';');
                }
                Some('\\') => {
                    chars.next();
                    result.push('\\');
                }
                _ => result.push(ch),
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Parse an ICS datetime value, honoring a TZID parameter when present.
///
/// Returns `(rfc3339_string, timezone_name, is_all_day)`.
///
/// Three input shapes:
/// - `YYYYMMDD` (8 chars, no 'T'): all-day date → `(YYYY-MM-DD, None, true)`.
/// - `YYYYMMDDTHHMMSSZ`: UTC → round-trip as-is.
/// - `YYYYMMDDTHHMMSS` + `tzid`: local time in `tzid`, convert to UTC.
/// - `YYYYMMDDTHHMMSS` + no tzid: "floating" per RFC 5545, no authoritative
///   zone available server-side. We log and fall back to treating it as
///   UTC. Real-world Fastmail events almost always carry Z or TZID, so
///   this path is defensive.
///
/// DST handling: for ambiguous local times (e.g. 02:30 on a fall-back
/// night that happens twice), we pick the *earlier* instant. For
/// non-existent local times (02:30 on a spring-forward night), we
/// return an error — the ICS is malformed.
fn parse_datetime(dt_str: &str, tzid: Option<&str>) -> Result<(String, Option<String>, bool)> {
    // Date-only (all-day) — TZID (if any) is ignored, matching RFC 5545.
    if dt_str.len() == 8 && !dt_str.contains('T') {
        let year = &dt_str[0..4];
        let month = &dt_str[4..6];
        let day = &dt_str[6..8];
        let iso_date = format!("{}-{}-{}", year, month, day);
        return Ok((iso_date, None, true));
    }

    // Trailing 'Z' is explicit UTC. TZID on a Z-suffixed value is
    // technically a spec violation; we treat Z as authoritative.
    if dt_str.ends_with('Z') {
        let dt = parse_ics_datetime(dt_str)?;
        return Ok((dt.to_rfc3339(), Some("UTC".to_owned()), false));
    }

    if let Some(tz_name) = tzid {
        let tz: Tz = tz_name
            .parse()
            .with_context(|| format!("unknown IANA timezone `{tz_name}`"))?;
        let naive = NaiveDateTime::parse_from_str(dt_str, "%Y%m%dT%H%M%S")
            .context("Failed to parse ICS datetime")?;
        let local = match tz.from_local_datetime(&naive) {
            LocalResult::Single(dt) => dt,
            LocalResult::Ambiguous(earlier, _) => earlier,
            LocalResult::None => {
                anyhow::bail!(
                    "local time {naive} does not exist in timezone `{tz_name}` \
                     (likely a DST spring-forward gap)"
                );
            }
        };
        let utc: DateTime<Utc> = local.with_timezone(&Utc);
        return Ok((utc.to_rfc3339(), Some(tz.name().to_owned()), false));
    }

    // Floating time (no Z, no TZID). RFC 5545 says this should be
    // interpreted in the observer's local timezone — which we don't
    // know server-side. Falling back to UTC keeps behavior consistent
    // with historical fastcal; the warning surfaces the ambiguity.
    let dt = parse_ics_datetime(dt_str)?;
    warn!(
        "floating-time event without TZID (`{dt_str}`) — treating as UTC; \
         this may render with a wall-clock offset."
    );
    Ok((dt.to_rfc3339(), None, false))
}

/// Like [`extract_property`], but for datetime properties that can carry
/// a `TZID=...` parameter (DTSTART, DTEND, RECURRENCE-ID). Returns
/// `(value, tzid_opt)`.
///
/// Examples:
/// - `DTSTART;TZID=Asia/Kolkata:20260423T143000`
///   → `("20260423T143000", Some("Asia/Kolkata"))`
/// - `DTSTART:20260423T143000Z` → `("20260423T143000Z", None)`
/// - `DTSTART;VALUE=DATE:20260423` → `("20260423", None)`
/// - `DTSTART;TZID=Europe/Amsterdam;OTHER=X:20260305T100000`
///   → `("20260305T100000", Some("Europe/Amsterdam"))`
///
/// Scoped to VEVENT components, same as [`extract_property`], so
/// VTIMEZONE's DAYLIGHT/STANDARD DTSTART lines don't leak through.
fn extract_dt_property(ics_data: &str, property_name: &str) -> Option<(String, Option<String>)> {
    let mut in_vevent = false;
    let mut depth = 0;

    for line in ics_data.lines() {
        let line = line.trim();

        if line == "BEGIN:VEVENT" {
            in_vevent = true;
            depth = 0;
            continue;
        }
        if line == "END:VEVENT" {
            in_vevent = false;
            continue;
        }
        if in_vevent && line.starts_with("BEGIN:") {
            depth += 1;
            continue;
        }
        if in_vevent && line.starts_with("END:") {
            depth -= 1;
            continue;
        }

        if !(in_vevent && depth == 0 && line.starts_with(property_name)) {
            continue;
        }
        let Some(rest) = line.strip_prefix(property_name) else {
            continue;
        };
        // Require ':' or ';' immediately after the name — otherwise a
        // property like `DTSTARTFOO:` could match when looking for
        // `DTSTART`.
        let (params_blob, value) = match rest.chars().next() {
            Some(':') => ("", &rest[1..]),
            Some(';') => {
                let after_semi = &rest[1..];
                let colon = after_semi.find(':')?;
                (&after_semi[..colon], &after_semi[colon + 1..])
            }
            _ => continue,
        };
        let tzid = params_blob
            .split(';')
            .find_map(|kv| kv.strip_prefix("TZID=").map(str::to_owned));
        return Some((value.trim().to_owned(), tzid));
    }
    None
}

/// Parse ICS datetime format (YYYYMMDDTHHMMSSexpZ) to DateTime
fn parse_ics_datetime(dt_str: &str) -> Result<DateTime<chrono::FixedOffset>> {
    let parsed = NaiveDateTime::parse_from_str(dt_str.trim_end_matches('Z'), "%Y%m%dT%H%M%S")
        .context("Failed to parse ICS datetime")?;
    let dt = DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc);
    Ok(dt.fixed_offset())
}

/// Parse date-only string (YYYY-MM-DD)
fn parse_date_only(date_str: &str) -> Result<DateTime<chrono::FixedOffset>> {
    let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").context("Failed to parse date")?;
    let dt = date
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(Utc)
        .unwrap();
    Ok(dt.fixed_offset())
}

/// Calculate default end time (start + 1 hour)
fn default_end_time(start_dt: &str, start_tz: &Option<String>) -> Result<EventDateTime> {
    let start_parsed = DateTime::parse_from_rfc3339(start_dt)
        .or_else(|_| parse_date_only(start_dt))
        .context("Failed to parse start datetime")?;
    let end_dt = start_parsed + Duration::hours(1);
    Ok(EventDateTime::new(end_dt.to_rfc3339(), start_tz.clone()))
}

/// Parse ISO 8601 duration string
///
/// Supports: P1D, PT1H, PT30M, PT1H30M, PT1H30M15S, P1DT2H, etc.
fn parse_duration(duration_str: &str) -> Result<i64> {
    if !duration_str.starts_with('P') {
        anyhow::bail!("Invalid duration format: {}", duration_str);
    }

    let mut seconds = 0i64;
    let after_p = &duration_str[1..];

    // Split into date part (before T) and time part (after T)
    let (date_part, time_part) = if let Some(t_idx) = after_p.find('T') {
        (&after_p[..t_idx], Some(&after_p[t_idx + 1..]))
    } else {
        (after_p, None)
    };

    // Parse date part: days (D), weeks (W)
    if let Some(d_idx) = date_part.find('D') {
        let days: i64 = date_part[..d_idx]
            .parse()
            .with_context(|| format!("Invalid day value in duration: {}", duration_str))?;
        seconds += days * 86400;
    }
    if let Some(w_idx) = date_part.find('W') {
        let weeks: i64 = date_part[..w_idx]
            .parse()
            .with_context(|| format!("Invalid week value in duration: {}", duration_str))?;
        seconds += weeks * 7 * 86400;
    }

    // Parse time part: hours (H), minutes (M), seconds (S)
    if let Some(time) = time_part {
        let mut pos = 0;
        while pos < time.len() {
            // Find the next letter (H, M, or S)
            if let Some(letter_offset) = time[pos..].find(|c: char| c.is_ascii_alphabetic()) {
                let letter_pos = pos + letter_offset;
                let value: i64 = time[pos..letter_pos]
                    .parse()
                    .with_context(|| format!("Invalid time value in duration: {}", duration_str))?;
                match time.as_bytes()[letter_pos] {
                    b'H' => seconds += value * 3600,
                    b'M' => seconds += value * 60,
                    b'S' => seconds += value,
                    _ => {}
                }
                pos = letter_pos + 1;
            } else {
                break;
            }
        }
    }

    Ok(seconds)
}

/// Calculate duration in minutes between two datetime strings
fn calculate_duration(start: &str, end: &str) -> Result<i64> {
    let start_dt = DateTime::parse_from_rfc3339(start)
        .or_else(|_| parse_date_only(start))
        .context("Failed to parse start datetime")?;

    let end_dt = DateTime::parse_from_rfc3339(end)
        .or_else(|_| parse_date_only(end))
        .context("Failed to parse end datetime")?;

    let duration = end_dt.signed_duration_since(start_dt);
    Ok(duration.num_minutes())
}

/// Unfold RFC 5545 line continuations so property lines are whole.
///
/// RFC 5545 §3.1: a long content line may be split by inserting CRLF immediately
/// followed by a single whitespace character (SPACE or HTAB). This function joins
/// those continuation lines back, producing one logical line per property.
fn unfold_ics(ics_data: &str) -> String {
    let mut result = String::with_capacity(ics_data.len());
    for line in ics_data.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation: strip the leading whitespace and append to prior line
            result.push_str(line.trim_start_matches([' ', '\t']));
        } else {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(line);
        }
    }
    result
}

/// Parameters for building a single ICS event.
pub struct IcsBuildArgs<'a> {
    /// The event UID
    pub uid: &'a str,
    /// Event title
    pub summary: &'a str,
    /// Start time in ICS format (YYYYMMDDTHHMMSSz or YYYYMMDD)
    pub start: &'a str,
    /// End time in ICS format (YYYYMMDDTHHMMSSz or YYYYMMDD)
    pub end: &'a str,
    pub description: Option<&'a str>,
    pub location: Option<&'a str>,
    pub organizer: Option<&'a str>,
    pub attendees: Option<&'a [String]>,
    /// VALARM sub-components to emit. Empty ⇒ no VALARM block written.
    /// MVP only writes `ACTION:DISPLAY` + `TRIGGER:-PT<N>M` even if the
    /// source `Reminder` has a different action — we don't produce
    /// VALARM shapes we can't also parse back.
    pub reminders: &'a [Reminder],
}

/// Build an ICS calendar containing a single event
///
/// Creates a well-formed iCalendar object with VCALENDAR and VEVENT components.
pub fn build_event(args: &IcsBuildArgs<'_>) -> Result<String> {
    let IcsBuildArgs {
        uid,
        summary,
        start,
        end,
        description,
        location,
        organizer,
        attendees,
        reminders,
    } = args;
    let mut ics = String::new();

    ics.push_str("BEGIN:VCALENDAR\r\n");
    ics.push_str("VERSION:2.0\r\n");
    ics.push_str("PRODID:-//fastcal//fastcal 0.1.0//EN\r\n");
    ics.push_str("BEGIN:VEVENT\r\n");

    ics.push_str(&fold_line(&format!("UID:{}", uid)));
    ics.push_str(&fold_line(&format!(
        "DTSTAMP:{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    )));
    // An 8-digit date value (no `T`) is an all-day boundary and must carry
    // `;VALUE=DATE`; a `…T…Z` value is a timed instant. Detecting from the
    // value shape keeps callers from threading a separate all-day flag.
    ics.push_str(&fold_line(&dt_line("DTSTART", start)));
    ics.push_str(&fold_line(&dt_line("DTEND", end)));
    ics.push_str(&fold_line(&format!("SUMMARY:{}", escape_ics_text(summary))));

    if let Some(desc) = description {
        ics.push_str(&fold_line(&format!(
            "DESCRIPTION:{}",
            escape_ics_text(desc)
        )));
    }

    if let Some(loc) = location {
        ics.push_str(&fold_line(&format!("LOCATION:{}", escape_ics_text(loc))));
    }

    if let Some(org) = organizer {
        let org_formatted = if !org.starts_with("mailto:") {
            format!("mailto:{}", org)
        } else {
            org.to_string()
        };
        ics.push_str(&fold_line(&format!("ORGANIZER:{}", org_formatted)));
    }

    if let Some(attendee_list) = *attendees {
        for attendee in attendee_list {
            let att_formatted = if !attendee.starts_with("mailto:") {
                format!("mailto:{}", attendee)
            } else {
                attendee.to_string()
            };
            ics.push_str(&fold_line(&format!("ATTENDEE:{}", att_formatted)));
        }
    }

    ics.push_str("STATUS:CONFIRMED\r\n");

    // One VALARM block per reminder. We always emit DISPLAY + a
    // relative-minutes trigger so everything we write can round-trip
    // through our own parser. DESCRIPTION defaults to SUMMARY since
    // Fastmail's notification UI wants some text.
    for r in *reminders {
        ics.push_str("BEGIN:VALARM\r\n");
        ics.push_str(&fold_line("ACTION:DISPLAY"));
        ics.push_str(&fold_line(&format!("TRIGGER:-PT{}M", r.minutes_before)));
        let desc = r.description.as_deref().unwrap_or(summary);
        ics.push_str(&fold_line(&format!(
            "DESCRIPTION:{}",
            escape_ics_text(desc)
        )));
        ics.push_str("END:VALARM\r\n");
    }

    ics.push_str("END:VEVENT\r\n");
    ics.push_str("END:VCALENDAR\r\n");

    // Validate by parsing it back
    ICalendar::parse(&ics).map_err(|e| anyhow::anyhow!("Generated invalid ICS: {:?}", e))?;

    Ok(ics)
}

/// Build a `DTSTART`/`DTEND` content line, tagging date-only values (no `T`,
/// e.g. `20260625`) with `;VALUE=DATE` for all-day events and leaving timed
/// `…T…Z` values bare.
fn dt_line(prop: &str, value: &str) -> String {
    if value.contains('T') {
        format!("{prop}:{value}")
    } else {
        format!("{prop};VALUE=DATE:{value}")
    }
}

/// Fold a content line to comply with RFC 5545 §3.1 (max 75 octets per line).
///
/// Lines exceeding 75 bytes are split with CRLF + a single leading space.
/// UTF-8 character boundaries are respected.
fn fold_line(line: &str) -> String {
    const MAX_FIRST: usize = 75;
    const MAX_CONT: usize = 74; // 74 content bytes + 1 leading space = 75

    if line.len() <= MAX_FIRST {
        return format!("{}\r\n", line);
    }

    let mut result = String::new();
    let mut pos = 0; // byte offset into `line`
    let mut first = true;

    while pos < line.len() {
        let limit = if first { MAX_FIRST } else { MAX_CONT };

        if !first {
            result.push(' ');
        }

        let remaining = &line[pos..];
        if remaining.len() <= limit {
            result.push_str(remaining);
            result.push_str("\r\n");
            break;
        }

        // Walk back from pos+limit to find a valid UTF-8 char boundary
        let mut split = pos + limit;
        while split > pos && !line.is_char_boundary(split) {
            split -= 1;
        }

        result.push_str(&line[pos..split]);
        result.push_str("\r\n");
        pos = split;
        first = false;
    }

    result
}

/// Escape text for ICS format
/// Handles: commas, semicolons, newlines, backslashes
fn escape_ics_text(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace(',', "\\,")
        .replace(';', "\\;")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_simple_event() {
        let ics = build_event(&IcsBuildArgs {
            uid: "test-event-123",
            summary: "Test Meeting",
            start: "20260305T140000Z",
            end: "20260305T150000Z",
            description: None,
            location: None,
            organizer: None,
            attendees: None,
            reminders: &[],
        })
        .unwrap();

        assert!(ics.contains("UID:test-event-123"));
        assert!(ics.contains("SUMMARY:Test Meeting"));
        assert!(ics.contains("DTSTART:20260305T140000Z"));
    }

    #[test]
    fn test_build_event_with_details() {
        let attendees = vec![
            "attendee1@example.com".to_string(),
            "attendee2@example.com".to_string(),
        ];
        let ics = build_event(&IcsBuildArgs {
            uid: "detailed-event",
            summary: "Team Sync",
            start: "20260305T140000Z",
            end: "20260305T150000Z",
            description: Some("Discuss project updates"),
            location: Some("Conference Room A"),
            organizer: Some("organizer@example.com"),
            attendees: Some(&attendees),
            reminders: &[],
        })
        .unwrap();

        assert!(ics.contains("DESCRIPTION:Discuss project updates"));
        assert!(ics.contains("LOCATION:Conference Room A"));
        assert!(ics.contains("ORGANIZER:mailto:organizer@example.com"));
        assert!(ics.contains("ATTENDEE:mailto:attendee1@example.com"));
    }

    #[test]
    fn test_escape_ics_text() {
        assert_eq!(escape_ics_text("Hello, World"), "Hello\\, World");
        assert_eq!(escape_ics_text("Line1\nLine2"), "Line1\\nLine2");
    }

    #[test]
    fn build_all_day_event_emits_value_date() {
        // Date-only ICS values (no `T`) must be tagged `;VALUE=DATE`.
        let ics = build_event(&IcsBuildArgs {
            uid: "all-day-build",
            summary: "Conference",
            start: "20260625",
            end: "20260628",
            description: None,
            location: None,
            organizer: None,
            attendees: None,
            reminders: &[],
        })
        .unwrap();
        assert!(ics.contains("DTSTART;VALUE=DATE:20260625"), "got: {ics}");
        assert!(ics.contains("DTEND;VALUE=DATE:20260628"), "got: {ics}");
    }

    #[test]
    fn all_day_event_round_trips() {
        // Build → parse keeps it all-day with the original dates.
        let ics = build_event(&IcsBuildArgs {
            uid: "rt-all-day",
            summary: "Holiday",
            start: "20260625",
            end: "20260626",
            description: None,
            location: None,
            organizer: None,
            attendees: None,
            reminders: &[],
        })
        .unwrap();
        let ev = parse_event(&ics, "/rt.ics".to_owned(), None).unwrap();
        assert!(ev.all_day);
        assert_eq!(ev.start.datetime, "2026-06-25");
        assert_eq!(ev.end.datetime, "2026-06-26");
    }

    #[test]
    fn timed_event_still_emits_bare_dtstart() {
        // Regression guard: timed values keep the un-parameterized form.
        let ics = build_event(&IcsBuildArgs {
            uid: "timed",
            summary: "Call",
            start: "20260625T120000Z",
            end: "20260625T130000Z",
            description: None,
            location: None,
            organizer: None,
            attendees: None,
            reminders: &[],
        })
        .unwrap();
        assert!(ics.contains("DTSTART:20260625T120000Z"), "got: {ics}");
        assert!(!ics.contains("VALUE=DATE"), "got: {ics}");
    }

    #[test]
    fn test_parse_basic_event() {
        let ics = r#"BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//Test//Test//EN
BEGIN:VEVENT
UID:test-123
SUMMARY:Test Event
DTSTART:20260305T140000Z
DTEND:20260305T150000Z
END:VEVENT
END:VCALENDAR"#;

        let event = parse_event(ics, "/test.ics".to_string(), None).unwrap();
        assert_eq!(event.id, "test-123");
        assert_eq!(event.summary, "Test Event");
        assert!(!event.all_day);
    }

    #[test]
    fn parse_event_extracts_content_via_calcard() {
        // Content properties (summary/location/status/organizer/attendees)
        // now come from calcard's parsed model; mailto: + ATTENDEE params
        // must still yield bare emails.
        let ics = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:meeting-1
DTSTART:20260501T140000Z
DTEND:20260501T150000Z
SUMMARY:Sync
LOCATION:Room A
STATUS:CONFIRMED
ORGANIZER:mailto:boss@example.com
ATTENDEE;CN=Bob:mailto:bob@example.com
ATTENDEE:mailto:carol@example.com
END:VEVENT
END:VCALENDAR"#;
        let ev = parse_event(ics, "/m.ics".to_owned(), None).unwrap();
        assert_eq!(ev.summary, "Sync");
        assert_eq!(ev.location.as_deref(), Some("Room A"));
        assert_eq!(ev.status, Some(EventStatus::Confirmed));
        assert_eq!(ev.organizer.as_deref(), Some("boss@example.com"));
        let emails: Vec<_> = ev.attendees.unwrap().into_iter().map(|a| a.email).collect();
        assert_eq!(emails, vec!["bob@example.com", "carol@example.com"]);
    }

    #[test]
    fn test_parse_all_day_event() {
        let ics = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:all-day-test
SUMMARY:All Day Event
DTSTART:20260305
DTEND:20260306
END:VEVENT
END:VCALENDAR"#;

        let event = parse_event(ics, "/test.ics".to_string(), None).unwrap();
        assert_eq!(event.id, "all-day-test");
        assert!(event.all_day);
    }

    #[test]
    fn test_parse_duration_minutes_only() {
        assert_eq!(parse_duration("PT30M").unwrap(), 1800);
    }

    #[test]
    fn test_parse_duration_hours_and_minutes() {
        assert_eq!(parse_duration("PT1H20M").unwrap(), 4800);
    }

    #[test]
    fn test_parse_duration_hours_only() {
        assert_eq!(parse_duration("PT1H").unwrap(), 3600);
    }

    #[test]
    fn test_parse_duration_days() {
        assert_eq!(parse_duration("P1D").unwrap(), 86400);
    }

    #[test]
    fn test_parse_duration_days_and_time() {
        assert_eq!(parse_duration("P1DT2H30M").unwrap(), 86400 + 7200 + 1800);
    }

    #[test]
    fn test_extract_property_no_false_matches() {
        let ics = r#"BEGIN:VCALENDAR
BEGIN:VEVENT
DESCRIPTION-ALT:This is not a description
DESCRIPTION:This is the real description
END:VEVENT
END:VCALENDAR"#;

        assert_eq!(
            extract_property(ics, "DESCRIPTION"),
            Some("This is the real description".to_string())
        );
    }

    #[test]
    fn test_unescape_ics_text_newline() {
        assert_eq!(unescape_ics_text("line1\\nline2"), "line1\nline2");
        assert_eq!(unescape_ics_text("line1\\Nline2"), "line1\nline2");
    }

    #[test]
    fn test_unescape_ics_text_comma_and_semicolon() {
        assert_eq!(unescape_ics_text("a\\,b"), "a,b");
        assert_eq!(unescape_ics_text("a\\;b"), "a;b");
    }

    #[test]
    fn test_unescape_ics_text_backslash() {
        assert_eq!(unescape_ics_text("a\\\\b"), "a\\b");
    }

    #[test]
    fn test_unescape_ics_text_unknown_escape_passthrough() {
        // Unknown escape sequences keep the backslash
        assert_eq!(unescape_ics_text("a\\xb"), "a\\xb");
    }

    #[test]
    fn test_unescape_is_inverse_of_escape() {
        let original = "Hello, World; line1\nline2 with a \\ backslash";
        let escaped = escape_ics_text(original);
        let unescaped = unescape_ics_text(&escaped);
        assert_eq!(unescaped, original);
    }

    #[test]
    fn test_fold_line_short() {
        // Lines <= 75 bytes are returned as-is with CRLF
        let line = "SUMMARY:Short title";
        let folded = fold_line(line);
        assert_eq!(folded, "SUMMARY:Short title\r\n");
    }

    #[test]
    fn test_fold_line_exactly_75() {
        let line = "X".repeat(75);
        let folded = fold_line(&line);
        assert_eq!(folded, format!("{}\r\n", line));
    }

    #[test]
    fn test_fold_line_long() {
        // Lines > 75 bytes get split; continuation lines start with a space
        let line = "DESCRIPTION:".to_string() + &"A".repeat(100);
        let folded = fold_line(&line);
        let lines: Vec<&str> = folded.split("\r\n").filter(|s| !s.is_empty()).collect();
        assert!(
            lines.len() > 1,
            "expected folding to produce multiple lines"
        );
        // First line must be <= 75 bytes
        assert!(lines[0].len() <= 75);
        // Continuation lines must start with a space
        for cont in &lines[1..] {
            assert!(
                cont.starts_with(' '),
                "continuation line must start with space"
            );
            assert!(cont.len() <= 75);
        }
    }

    #[test]
    fn test_fold_line_utf8_boundary() {
        // emoji is 4 bytes; ensure we don't split inside it
        let prefix = "SUMMARY:";
        let emoji_block = "🎉".repeat(20); // 80 bytes of emoji
        let line = format!("{}{}", prefix, emoji_block);
        let folded = fold_line(&line);
        // Must be valid UTF-8 (no panics during decode)
        for segment in folded.split("\r\n").filter(|s| !s.is_empty()) {
            assert!(std::str::from_utf8(segment.as_bytes()).is_ok());
            assert!(segment.len() <= 75);
        }
    }

    #[test]
    fn test_unfold_ics_joins_continuation_lines() {
        // A SUMMARY split across two folded lines
        let folded = "BEGIN:VCALENDAR\r\nSUMMARY:This is a very long su\r\n mmary that wraps\r\nEND:VCALENDAR";
        let unfolded = unfold_ics(folded);
        assert!(unfolded.contains("SUMMARY:This is a very long summary that wraps"));
    }

    #[test]
    fn test_unfold_ics_tab_continuation() {
        let folded = "DESCRIPTION:line1\r\n\tcontinued";
        let unfolded = unfold_ics(folded);
        assert!(unfolded.contains("DESCRIPTION:line1continued"));
    }

    #[test]
    fn test_parse_event_with_folded_summary() {
        // Folded SUMMARY must be reassembled before extraction
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:fold-test\r\nSUMMARY:Folded Su\r\n mmary\r\nDTSTART:20260305T140000Z\r\nDTEND:20260305T150000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let event = parse_event(ics, "/fold.ics".to_string(), None).unwrap();
        assert_eq!(event.summary, "Folded Summary");
    }

    #[test]
    fn test_extract_property_ignores_vtimezone() {
        // DTSTART appears in VTIMEZONE (DST transition) and in VEVENT
        // extract_property must return the VEVENT one
        let ics = r#"BEGIN:VCALENDAR
BEGIN:VTIMEZONE
TZID:Europe/Amsterdam
BEGIN:DAYLIGHT
DTSTART:19810329T020000
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU
END:DAYLIGHT
END:VTIMEZONE
BEGIN:VEVENT
UID:test-123
DTSTART;TZID=Europe/Amsterdam:20260305T100000
DTEND;TZID=Europe/Amsterdam:20260305T110000
SUMMARY:Real Event
END:VEVENT
END:VCALENDAR"#;

        assert_eq!(
            extract_property(ics, "DTSTART"),
            Some("20260305T100000".to_string())
        );
        assert_eq!(
            extract_property(ics, "SUMMARY"),
            Some("Real Event".to_string())
        );
    }

    // -------- TZID-aware DTSTART/DTEND parsing (fix/timezones) --------

    /// Helper: minimal VCALENDAR wrapping a single VEVENT with a DTSTART
    /// line of the caller's choosing. No DTEND — events_parse computes
    /// one from duration or defaults. Keeps each test case readable.
    fn ics_with_dtstart(dtstart_line: &str) -> String {
        format!(
            "BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VEVENT\nUID:t\nSUMMARY:X\n\
             {dtstart_line}\nEND:VEVENT\nEND:VCALENDAR"
        )
    }

    #[test]
    fn extract_dt_property_reads_tzid_from_params() {
        let ics = ics_with_dtstart("DTSTART;TZID=Asia/Kolkata:20260423T143000");
        let got = extract_dt_property(&ics, "DTSTART").unwrap();
        assert_eq!(got.0, "20260423T143000");
        assert_eq!(got.1.as_deref(), Some("Asia/Kolkata"));
    }

    #[test]
    fn extract_dt_property_with_no_params_has_no_tzid() {
        let ics = ics_with_dtstart("DTSTART:20260423T143000Z");
        let got = extract_dt_property(&ics, "DTSTART").unwrap();
        assert_eq!(got.0, "20260423T143000Z");
        assert!(got.1.is_none());
    }

    #[test]
    fn extract_dt_property_survives_multiple_params() {
        // Real-world calendars sometimes add VALUE=DATE-TIME alongside TZID.
        let ics = ics_with_dtstart("DTSTART;VALUE=DATE-TIME;TZID=Europe/Amsterdam:20260305T100000");
        let got = extract_dt_property(&ics, "DTSTART").unwrap();
        assert_eq!(got.0, "20260305T100000");
        assert_eq!(got.1.as_deref(), Some("Europe/Amsterdam"));
    }

    #[test]
    fn extract_dt_property_all_day_date() {
        let ics = ics_with_dtstart("DTSTART;VALUE=DATE:20260423");
        let got = extract_dt_property(&ics, "DTSTART").unwrap();
        assert_eq!(got.0, "20260423");
        assert!(got.1.is_none());
    }

    #[test]
    fn extract_dt_property_does_not_false_match_prefix() {
        // Guard: DTSTARTFOO must not match DTSTART.
        let ics = ics_with_dtstart("DTSTARTFOO:notreal\nDTSTART:20260423T143000Z");
        let got = extract_dt_property(&ics, "DTSTART").unwrap();
        assert_eq!(got.0, "20260423T143000Z");
    }

    #[test]
    fn parse_datetime_tzid_kolkata_converts_to_utc() {
        // 14:30 IST (+05:30) == 09:00 UTC. This is the exact case that
        // made calman display 20:00 instead of 14:30 before the fix.
        let (iso, tz, all_day) = parse_datetime("20260423T143000", Some("Asia/Kolkata")).unwrap();
        assert_eq!(iso, "2026-04-23T09:00:00+00:00");
        assert_eq!(tz.as_deref(), Some("Asia/Kolkata"));
        assert!(!all_day);
    }

    #[test]
    fn parse_datetime_tzid_amsterdam_cest_converts_to_utc() {
        // April 23 is during Central European Summer Time (UTC+02:00).
        // 11:00 CEST == 09:00 UTC.
        let (iso, tz, _) = parse_datetime("20260423T110000", Some("Europe/Amsterdam")).unwrap();
        assert_eq!(iso, "2026-04-23T09:00:00+00:00");
        assert_eq!(tz.as_deref(), Some("Europe/Amsterdam"));
    }

    #[test]
    fn parse_datetime_tzid_amsterdam_cet_converts_to_utc() {
        // January is Central European Time (UTC+01:00). 11:00 CET == 10:00 UTC.
        // Verifies chrono_tz gives us the right offset for the date, not
        // a fixed "Europe/Amsterdam is always +02:00" assumption.
        let (iso, _, _) = parse_datetime("20260115T110000", Some("Europe/Amsterdam")).unwrap();
        assert_eq!(iso, "2026-01-15T10:00:00+00:00");
    }

    #[test]
    fn parse_datetime_utc_z_suffix_unchanged() {
        // Regression guard: the explicit-UTC path must keep working.
        let (iso, tz, _) = parse_datetime("20260423T143000Z", None).unwrap();
        assert_eq!(iso, "2026-04-23T14:30:00+00:00");
        assert_eq!(tz.as_deref(), Some("UTC"));
    }

    #[test]
    fn parse_datetime_all_day_date_unchanged() {
        let (iso, tz, all_day) = parse_datetime("20260423", None).unwrap();
        assert_eq!(iso, "2026-04-23");
        assert!(tz.is_none());
        assert!(all_day);
    }

    #[test]
    fn parse_datetime_floating_without_tzid_falls_back_to_utc() {
        // Floating time with no TZID: we log a warning and keep the
        // historical "treat as UTC" behavior. The warning is not
        // asserted here — only the compatible shape.
        let (iso, tz, _) = parse_datetime("20260423T143000", None).unwrap();
        assert_eq!(iso, "2026-04-23T14:30:00+00:00");
        assert!(tz.is_none());
    }

    #[test]
    fn parse_datetime_unknown_tzid_errors() {
        // Unknown tz name should surface as an error rather than
        // silently defaulting somewhere wrong.
        let err = parse_datetime("20260423T143000", Some("Totally/Fake")).unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("unknown"),
            "expected 'unknown' in error: {err}"
        );
    }

    #[test]
    fn parse_event_end_to_end_tzid_kolkata() {
        // Integration: the exact ICS shape a Fastmail event with an
        // India-time TZID would take. The user's "Bevestiging" case.
        let ics = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:test-kolkata
DTSTART;TZID=Asia/Kolkata:20260423T143000
DTEND;TZID=Asia/Kolkata:20260423T150000
SUMMARY:Dentist
END:VEVENT
END:VCALENDAR"#;
        let event = parse_event(ics, "/h.ics".to_owned(), None).unwrap();
        assert_eq!(event.start.datetime, "2026-04-23T09:00:00+00:00");
        assert_eq!(event.end.datetime, "2026-04-23T09:30:00+00:00");
        assert_eq!(event.start.timezone.as_deref(), Some("Asia/Kolkata"));
        assert_eq!(event.duration_minutes, Some(30));
    }

    // -------- Recurrence: RRULE + RECURRENCE-ID exposure -------------

    #[test]
    fn parse_event_exposes_rrule_on_master() {
        // Weekly Monday gym: the master event carries an RRULE, no
        // RECURRENCE-ID. Non-recurring events have both as None.
        let ics = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:gym-master
DTSTART;TZID=Europe/Berlin:20260202T080000
DTEND;TZID=Europe/Berlin:20260202T090000
RRULE:FREQ=WEEKLY;BYDAY=MO
SUMMARY:Gym
END:VEVENT
END:VCALENDAR"#;
        let event = parse_event(ics, "/g.ics".to_owned(), None).unwrap();
        assert_eq!(event.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=MO"));
        assert!(event.recurrence_id.is_none(), "master has no RECURRENCE-ID");
    }

    #[test]
    fn parse_event_exposes_recurrence_id_on_expanded_instance() {
        // Server-side `<C:expand>` emits one VEVENT per occurrence, each
        // carrying a RECURRENCE-ID pointing back at its slot in the
        // master's series. The instance's DTSTART is the instance time;
        // the RECURRENCE-ID agrees for a non-overridden instance.
        let ics = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:gym-master
RECURRENCE-ID;TZID=Europe/Berlin:20260427T080000
DTSTART;TZID=Europe/Berlin:20260427T080000
DTEND;TZID=Europe/Berlin:20260427T090000
SUMMARY:Gym
END:VEVENT
END:VCALENDAR"#;
        let event = parse_event(ics, "/g.ics".to_owned(), None).unwrap();
        // Berlin is CEST (+02:00) on Apr 27 → 08:00 local = 06:00 UTC.
        assert_eq!(event.start.datetime, "2026-04-27T06:00:00+00:00");
        assert_eq!(
            event.recurrence_id.as_deref(),
            Some("2026-04-27T06:00:00+00:00")
        );
        assert!(event.rrule.is_none(), "expanded instance has no RRULE");
    }

    // -------- VALARM / reminder parse + build + round-trip ----------

    /// Minimal VCALENDAR wrapping a single VEVENT with caller-supplied
    /// VALARM-block lines.
    fn ics_with_valarms(valarm_body: &str) -> String {
        format!(
            "BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VEVENT\n\
             UID:test-reminder\nSUMMARY:Test\n\
             DTSTART:20260501T140000Z\nDTEND:20260501T150000Z\n\
             {valarm_body}\n\
             END:VEVENT\nEND:VCALENDAR"
        )
    }

    #[test]
    fn parse_event_exposes_5min_reminder() {
        let ics = ics_with_valarms(
            "BEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT5M\nDESCRIPTION:ping\nEND:VALARM",
        );
        let ev = parse_event(&ics, "/h.ics".to_owned(), None).unwrap();
        assert_eq!(ev.reminders.len(), 1);
        assert_eq!(ev.reminders[0].minutes_before, 5);
        assert_eq!(ev.reminders[0].action, "display");
        assert_eq!(ev.reminders[0].description.as_deref(), Some("ping"));
    }

    #[test]
    fn parse_event_reports_multiple_reminders() {
        let ics = ics_with_valarms(
            "BEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT15M\nEND:VALARM\n\
             BEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-P1D\nEND:VALARM",
        );
        let ev = parse_event(&ics, "/h.ics".to_owned(), None).unwrap();
        assert_eq!(ev.reminders.len(), 2);
        assert_eq!(ev.reminders[0].minutes_before, 15);
        assert_eq!(ev.reminders[1].minutes_before, 1440);
    }

    #[test]
    fn parse_event_handles_hour_and_day_triggers() {
        // -PT1H → 60, -P1D → 1440, -PT1H30M → 90
        let ics = ics_with_valarms(
            "BEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT1H\nEND:VALARM\n\
             BEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-P1D\nEND:VALARM\n\
             BEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT1H30M\nEND:VALARM",
        );
        let ev = parse_event(&ics, "/h.ics".to_owned(), None).unwrap();
        let mins: Vec<u32> = ev.reminders.iter().map(|r| r.minutes_before).collect();
        assert_eq!(mins, vec![60, 1440, 90]);
    }

    #[test]
    fn parse_event_skips_absolute_trigger() {
        // TRIGGER;VALUE=DATE-TIME — not supported, dropped.
        let ics = ics_with_valarms(
            "BEGIN:VALARM\nACTION:DISPLAY\n\
             TRIGGER;VALUE=DATE-TIME:20260501T135500Z\n\
             END:VALARM",
        );
        let ev = parse_event(&ics, "/h.ics".to_owned(), None).unwrap();
        assert!(ev.reminders.is_empty());
    }

    #[test]
    fn parse_event_skips_related_end_trigger() {
        // TRIGGER;RELATED=END:-PT5M — not supported in MVP, dropped.
        let ics =
            ics_with_valarms("BEGIN:VALARM\nACTION:DISPLAY\nTRIGGER;RELATED=END:-PT5M\nEND:VALARM");
        let ev = parse_event(&ics, "/h.ics".to_owned(), None).unwrap();
        assert!(ev.reminders.is_empty());
    }

    #[test]
    fn parse_event_without_valarm_has_empty_reminders() {
        let ics = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:plain
DTSTART:20260501T140000Z
DTEND:20260501T150000Z
SUMMARY:Plain
END:VEVENT
END:VCALENDAR"#;
        let ev = parse_event(ics, "/p.ics".to_owned(), None).unwrap();
        assert!(ev.reminders.is_empty());
    }

    #[test]
    fn parse_relative_before_start_grammar() {
        assert_eq!(parse_relative_before_start("-PT5M"), Some(5));
        assert_eq!(parse_relative_before_start("-PT0M"), Some(0));
        assert_eq!(parse_relative_before_start("-PT1H"), Some(60));
        assert_eq!(parse_relative_before_start("-P1D"), Some(1440));
        assert_eq!(parse_relative_before_start("-PT1H30M"), Some(90));
        assert_eq!(parse_relative_before_start("-P1DT2H"), Some(1440 + 120));
        assert_eq!(parse_relative_before_start("-P1W"), Some(7 * 1440));
        // Positive (after-start) triggers not supported here.
        assert_eq!(parse_relative_before_start("PT5M"), None);
        // Absolute datetime not a duration.
        assert_eq!(parse_relative_before_start("-20260501T140000Z"), None);
        // Gibberish.
        assert_eq!(parse_relative_before_start("-Pxyz"), None);
    }

    #[test]
    fn build_event_emits_no_valarm_when_reminders_empty() {
        // Regression guard: the default path must stay byte-stable for
        // events without reminders. A VALARM block appearing here would
        // silently change every event the calman agent touches.
        let ics = build_event(&IcsBuildArgs {
            uid: "no-alarm",
            summary: "Plain",
            start: "20260501T140000Z",
            end: "20260501T150000Z",
            description: None,
            location: None,
            organizer: None,
            attendees: None,
            reminders: &[],
        })
        .unwrap();
        assert!(!ics.contains("BEGIN:VALARM"));
        assert!(!ics.contains("TRIGGER"));
    }

    #[test]
    fn build_event_emits_valarm_when_reminder_present() {
        let reminders = vec![Reminder {
            minutes_before: 5,
            action: "display".to_owned(),
            description: None,
        }];
        let ics = build_event(&IcsBuildArgs {
            uid: "with-alarm",
            summary: "Car check",
            start: "20260501T170000Z",
            end: "20260501T173000Z",
            description: None,
            location: None,
            organizer: None,
            attendees: None,
            reminders: &reminders,
        })
        .unwrap();
        assert!(ics.contains("BEGIN:VALARM\r\n"));
        assert!(ics.contains("ACTION:DISPLAY\r\n"));
        assert!(ics.contains("TRIGGER:-PT5M\r\n"));
        // DESCRIPTION defaults to the event's SUMMARY when caller omits it.
        assert!(ics.contains("DESCRIPTION:Car check\r\n"));
        assert!(ics.contains("END:VALARM\r\n"));
    }

    #[test]
    fn build_event_with_multiple_reminders_emits_all() {
        let reminders = vec![
            Reminder {
                minutes_before: 15,
                action: "display".to_owned(),
                description: None,
            },
            Reminder {
                minutes_before: 1440,
                action: "display".to_owned(),
                description: None,
            },
        ];
        let ics = build_event(&IcsBuildArgs {
            uid: "multi-alarm",
            summary: "Big day",
            start: "20260501T100000Z",
            end: "20260501T110000Z",
            description: None,
            location: None,
            organizer: None,
            attendees: None,
            reminders: &reminders,
        })
        .unwrap();
        assert_eq!(ics.matches("BEGIN:VALARM").count(), 2);
        assert!(ics.contains("TRIGGER:-PT15M"));
        assert!(ics.contains("TRIGGER:-PT1440M"));
    }

    #[test]
    fn valarm_round_trip_preserves_minutes_before() {
        let original = vec![Reminder {
            minutes_before: 30,
            action: "display".to_owned(),
            description: Some("pickup".to_owned()),
        }];
        let ics = build_event(&IcsBuildArgs {
            uid: "rt",
            summary: "Pickup",
            start: "20260501T170000Z",
            end: "20260501T180000Z",
            description: None,
            location: None,
            organizer: None,
            attendees: None,
            reminders: &original,
        })
        .unwrap();
        let parsed = parse_event(&ics, "/rt.ics".to_owned(), None).unwrap();
        assert_eq!(parsed.reminders.len(), 1);
        assert_eq!(parsed.reminders[0].minutes_before, 30);
        assert_eq!(parsed.reminders[0].action, "display");
        assert_eq!(parsed.reminders[0].description.as_deref(), Some("pickup"));
    }

    #[test]
    fn parse_event_non_recurring_has_neither_field() {
        // Regression guard: one-off events keep both fields as None so
        // downstream consumers (serde skip_serializing_if) emit neither.
        let ics = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:one-off
DTSTART:20260501T140000Z
DTEND:20260501T150000Z
SUMMARY:Dentist
END:VEVENT
END:VCALENDAR"#;
        let event = parse_event(ics, "/o.ics".to_owned(), None).unwrap();
        assert!(event.rrule.is_none());
        assert!(event.recurrence_id.is_none());
    }
}
