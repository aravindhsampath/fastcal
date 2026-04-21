# fastcal Opus Review

**Reviewer**: Claude Opus 4.6
**Date**: 2026-03-06
**Method**: Full source code audit of all 26 source files (~4,928 LOC), docs, tests, dependency tree. Previous Sonnet review (REVIEW_v1.md) cross-referenced for fixed/remaining issues. Web searches for dependency currency and RFC compliance. `cargo test` 45/45, `cargo clippy` 0 warnings.

---

## Executive Summary

fastcal is a well-architected Rust CLI that wraps libdav to provide AI-friendly CalDAV operations against Fastmail. The previous Sonnet review identified 18 issues; many critical ones (text unescaping, RFC 5545 line folding, `--from-json` wiring, `--format` respect, dead code) have been fixed. The codebase now has 45 passing tests (up from 30), zero clippy warnings, and solid RFC compliance.

What remains are primarily **efficiency problems** (full-calendar scans on every mutation), **architectural gaps** (no retry/timeout handling, no dry-run mode), **missing test coverage at key inflection points**, and **several design decisions that trade unnecessary complexity for no benefit**. Fixing these would elevate the project from "works correctly" to "engineered to win."

**Overall Grade**: Silver-tier. Solid foundation, clean code, good test coverage for parsing. Needs the performance and resilience work below to reach Gold.

---

## Dimension 1: Simplicity yet Elegance

### [1.1] Dependency audit — PASS with notes

The 22 direct dependencies are all justified. Good choices:
- `libdav` for CalDAV — proven, same library as davcli
- `calcard` for ICS parsing — replaced the weaker `ical` crate from the original plan
- `clap` derive API — clean, zero-boilerplate CLI definitions
- `anyhow`/`thiserror` — standard Rust error handling duo

**Minor concern**: `tokio` with `features = ["full"]` pulls in every tokio subsystem (fs, process, signal, sync, time, io, net, macros, rt-multi-thread). fastcal only needs `rt-multi-thread` + `macros` + `net`. Using `features = ["rt-multi-thread", "macros", "net"]` would reduce compile time and binary size.

**Note**: `thiserror = "2.0"` is listed in dependencies but **never used** — there are zero `#[derive(thiserror::Error)]` types in the codebase. All error handling uses `anyhow`. This is dead weight.

```toml
# Remove from Cargo.toml — unused
thiserror = "2.0"
```

**Note**: `futures = "0.3"` is used solely for `futures::future::join_all` in one file (`caldav/utils.rs`). This pulls in `futures-core`, `futures-channel`, `futures-executor`, `futures-io`, `futures-sink`, `futures-task`, `futures-util`. Consider using `futures-util = "0.3"` alone (which provides `join_all`), or even `tokio::join!` / `tokio::task::JoinSet` since you already depend on tokio.

### [1.2] Module boundaries — GOOD

The six-module architecture is clean and well-motivated:

```
caldav/    — libdav wrapper (auth, client, calendar ops, event ops, utils)
commands/  — CLI command handlers (config, calendars, events, batch, context)
config/    — Config loading, saving, discovery
models/    — Data types (Event, Calendar, output wrappers)
parsers/   — ICS and datetime parsing
formatters/ — Text output formatting
```

Each module has a clear single responsibility. The `CommandContext` abstraction is lightweight and useful. The separation of `parsers/ics.rs` (ICS<->Event) from `caldav/event.rs` (network operations) is a good layering decision.

### [1.3] Code duplication — TWO INSTANCES

**Duplication 1: Event creation logic**

`commands/events.rs:create()` (lines 168-311) and `commands/batch.rs:create_single_event()` (lines 146-206) contain nearly identical event creation logic:
- Parse start datetime
- Calculate end datetime (with same `if let Some(end) / else if let Some(duration) / else default` cascade)
- Generate UUID
- Format for ICS
- Parse attendees
- Build ICS
- Call PutResource

This should be extracted into a shared helper function. The batch version is essentially a stripped-down copy of the events version.

```rust
// Suggested: src/commands/helpers.rs or a method on CommandContext
pub async fn create_event_on_server(
    client: &Client,
    config: &Config,
    calendar_href: &str,
    summary: &str,
    start: &str,
    end: Option<&str>,
    duration: Option<u32>,
    location: Option<&str>,
    description: Option<&str>,
    attendees: Option<&str>,
) -> Result<(String, Event)> { ... }
```

**Duplication 2: "Find event then operate" pattern**

`events::get()`, `events::update()`, `events::delete()`, and `batch::delete_single_event()` all contain the same pattern:

```rust
let (calendar_name, event) = if let Some(ref cal) = ctx.calendar {
    let calendar_href = config.calendars.get(cal)...;
    let events = caldav::event::list_events(..., None, None)...;
    let event = events.into_iter().find(|e| e.id == event_id)...;
    (cal.clone(), event)
} else {
    caldav::event::find_event_by_id(...)...
};
```

This ~20 lines of boilerplate is copy-pasted 4 times. Extract it:

```rust
impl CommandContext {
    pub async fn find_event(
        &self,
        client: &Client,
        config: &Config,
        event_id: &str,
    ) -> Result<(String, Event)> { ... }
}
```

### [1.4] Feature completeness vs complexity — EXCELLENT

The feature set (CRUD, search, conflicts, batch) is well-scoped for the use case. ~5k LOC for a full CalDAV CLI with ICS parsing is lean. There's no over-engineering — no plugin system, no unnecessary abstractions, no speculative features.

### [1.5] Configuration design — GOOD

The three-tier precedence (env var > config file > default) is clean and well-implemented. The `get_password()`, `get_username()`, `get_base_url()` methods in `Config` handle this transparently.

**One issue**: `preferences.output_format` is stored in config and settable via `config set`, but is **never consulted** when determining output format. The CLI `--format` flag has a hardcoded default of `"text"`. There's no fallback to the config preference. This is a broken contract — the user can set it but it has no effect.

Fix in `cli.rs`:
```rust
// In Cli::execute(), after loading config:
let effective_format = if self.format_explicitly_set {
    self.format
} else if let Ok(config) = ctx.load_config() {
    match config.preferences.output_format.as_str() {
        "json" => OutputFormat::Json,
        "ics" => OutputFormat::Ics,
        _ => OutputFormat::Text,
    }
} else {
    self.format
};
```

This requires detecting whether `--format` was explicitly passed vs defaulted, which clap supports via `#[arg(default_value_t)]` patterns or by making the field `Option<OutputFormat>`.

### [1.6] Type design — GOOD with one concern

The `Event` struct stores datetimes as `EventDateTime { datetime: String, timezone: Option<String> }` rather than typed `DateTime<Utc>` or `DateTime<FixedOffset>`. This means every consumer that needs to do time math must parse the string again (and does — see `events.rs:472`, `events.rs:487`, `events.rs:729-733`). The string-based design was chosen for JSON serialization flexibility, but it creates repeated parse-format-parse cycles throughout the codebase.

This is a deliberate trade-off (JSON output shows the original timezone-aware string) but it means that:
- Duration calculation parses both start and end strings
- Conflict detection parses start and end strings per event
- Update parses existing start/end to compute new values

An alternative: store `DateTime<Utc>` internally, serialize to RFC 3339 for JSON via serde. This eliminates all the manual parsing. However, this is a significant refactor and the current approach works — just with unnecessary parse round-trips.

---

## Dimension 2: Resource Efficiency

### [2.1] String allocation audit — SEVERAL UNNECESSARY CLONES

**Hot path clones in event listing:**

`src/caldav/calendar.rs:63-76`:
```rust
let href_string = cal.href.to_string();                    // alloc 1
let display_name = display_names.get(&href_string)
    .and_then(|dn| dn.clone());                            // alloc 2

let name = display_name.clone()                            // alloc 3
    .unwrap_or_else(|| utils::extract_calendar_name_from_href(&cal.href.to_string())); // alloc 4

let calendar = Calendar::new(name.clone(), cal.href.to_string()) // alloc 5, 6
    .with_display_name(display_name.unwrap_or_else(|| name.clone())); // alloc 7

let final_name = utils::ensure_unique_name(name, &all_calendars); // name moved here
```

This creates **7 allocations** per calendar where 2-3 would suffice. The pattern of `clone()` followed by `unwrap_or_else(|| ...)` that also clones is a smell. Restructuring to use references where possible and move semantics where ownership is needed would halve these allocations.

**`Cli::execute()` clones:**

`src/cli.rs:220-225`:
```rust
let ctx = CommandContext::new(
    self.config.clone(),    // clones Option<String>
    self.format,
    self.calendar.clone(),  // clones Option<String>
    self.verbose,
);
```

Since `self` is consumed (`self`, not `&self`), these could be moved instead of cloned:
```rust
let ctx = CommandContext::new(self.config, self.format, self.calendar, self.verbose);
```

But `self.command` is matched afterwards... however, since `self.command` doesn't reference `config` or `calendar`, you could destructure first:
```rust
let Cli { config, format, calendar, verbose, command } = self;
let ctx = CommandContext::new(config, format, calendar, verbose);
match command { ... }
```

### [2.2] Network efficiency — CRITICAL ISSUE REMAINS

**`find_event_by_id` is an O(N*M) full scan** (from REVIEW_v1 issue #9, NOT FIXED)

`src/caldav/event.rs:107-168`: To find one event by UID:
1. For each calendar: list ALL event hrefs (no date filter)
2. For each calendar: fetch ALL event bodies
3. Parse each event, compare UID

For a user with 3 calendars and 100 events each, this is ~6 HTTP requests and ~300 ICS parses to find one event.

**This function is called by**: `events get`, `events update`, `events delete`, `batch delete` (once per item).

**Fix**: Fastmail (and most CalDAV servers) use `{UID}.ics` as the filename. Try a direct GET of `{calendar_href}/{event_id}.ics` first, fall back to full scan only on 404:

```rust
pub async fn find_event_by_id(
    client: &Client,
    event_id: &str,
    calendars: &HashMap<String, String>,
) -> Result<Option<(String, Event)>> {
    // Fast path: try direct fetch by convention (uid.ics)
    for (calendar_name, calendar_href) in calendars {
        let event_href = format!("{}/{}.ics",
            calendar_href.trim_end_matches('/'), event_id);
        match client.request(GetCalendarResources::new(calendar_href)
            .with_hrefs(vec![event_href.clone()])).await
        {
            Ok(resources) => {
                for resource in resources.resources {
                    if let Ok(fetched) = resource.content {
                        if let Ok(mut event) = ics::parse_event(
                            &fetched.data, resource.href, Some(fetched.etag))
                        {
                            if event.id == event_id {
                                event.calendar = Some(calendar_name.clone());
                                return Ok(Some((calendar_name.clone(), event)));
                            }
                        }
                    }
                }
            }
            Err(_) => continue, // Try next calendar
        }
    }

    // Slow path: full scan (for non-standard servers)
    // ... existing logic ...
    Ok(None)
}
```

This reduces the common case from O(N*M) to O(N) where N = number of calendars (typically 2-4), with a single small HTTP request per calendar.

### [2.3] Memory patterns — MINOR

`src/caldav/event.rs:68`:
```rust
let hrefs: Vec<String> = listed.resources.into_iter().map(|r| r.href).collect();
```
This collects into a Vec just to pass to `with_hrefs()`. This is fine — the Vec is necessary because `with_hrefs` takes a `Vec<String>`.

### [2.4] Dependency weight — MODERATE

`tokio = { version = "1", features = ["full"] }` enables:
- `fs` — not used (config uses `std::fs`)
- `process` — not used
- `signal` — not used
- `sync` — not used
- `time` — not used (no timeouts!)

Tighten to:
```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net"] }
```

The `futures` crate pulls in 7 sub-crates for one function call. Replace with:
```toml
futures-util = "0.3"
```
And change the import in `caldav/utils.rs` from `futures::future::join_all` to `futures_util::future::join_all`.

### [2.5] Async overhead — JUSTIFIED

The `#[tokio::main]` with multi-threaded runtime is justified because `join_all` for concurrent display name fetching benefits from it. Single-threaded would work too (`current_thread`) and would be lighter for a CLI tool, but the difference is negligible.

---

## Dimension 3: Performance Optimizations

### [3.1] `find_event_by_id` — CRITICAL (see 2.2 above)

This is the single biggest performance issue. A batch delete of 10 events does 10 full-calendar scans sequentially.

### [3.2] ICS parsing efficiency — MODERATE CONCERN

`src/parsers/ics.rs:14-126` (`parse_event`):

The function:
1. Parses ICS with `calcard::ICalendar::parse()` — correct
2. Extracts UID via `event_component.uid()` — good, uses calcard API
3. **Re-serializes** the entire calendar back to a string via `calendar.write_to()` — wasteful
4. Manually scans the re-serialized string line-by-line with `extract_property()` for SUMMARY, DESCRIPTION, LOCATION, DTSTART, DTEND, etc.

Step 3 is bizarre — we parse the ICS into a structured calcard representation, then serialize it back to text just to do manual string scanning. This is because calcard's component API may not expose all properties conveniently, but it's still an anti-pattern. Ideally:

**Option A**: Use calcard's property access API directly (if it exposes property iteration)
**Option B**: Skip calcard for property extraction entirely — just scan the raw `ics_data` input string directly without the parse-serialize round-trip

The current approach works correctly (the re-serialization normalizes line folding, which makes `extract_property`'s line-by-line scan reliable), but it's doing double the work. For a small number of events this doesn't matter; for batch operations with hundreds of events it adds up.

Recommended: At minimum, operate on the original `ics_data` instead of re-serializing. The calcard parse already handles line unfolding, so `extract_property` could work on the original text if it handles folded lines. Actually — calcard's `write_to` produces **unfolded** lines, which is why `extract_property` works. The original ICS data may have folded lines that `extract_property` can't handle (it's line-by-line).

So the current approach has a **correctness reason**: calcard unfolding. But the right fix is to make `extract_property` handle folded lines, then skip the re-serialize. This is a medium-priority optimization.

### [3.3] Concurrent operations — GOOD

`caldav/utils.rs:20-36` uses `futures::future::join_all` for concurrent display name fetching. This correctly avoids the N+1 problem identified in the v1 review.

However, `batch::create()` processes events **sequentially** (line 88: `for (index, event_input) in events.iter().enumerate()`). For 10 events, this means 10 sequential HTTP round-trips. Concurrent batch creation with `join_all` or `tokio::task::JoinSet` would be significantly faster:

```rust
let results: Vec<_> = futures_util::future::join_all(
    events.iter().enumerate().map(|(index, event_input)| {
        let client = &client;
        let config = &config;
        async move {
            (index, create_single_event(client, config, &calendar_name, &calendar_href, event_input).await)
        }
    })
).await;
```

Same applies to `batch::delete()`.

### [3.4] Early exits — GOOD

`find_event_by_id` returns immediately upon finding the matching event. `list_events` returns early on empty listings. Search filters use iterator chains that are lazy until collect.

### [3.5] Batch operation parallelism — see 3.3

---

## Dimension 4: Software Engineering

### [4.1] Idiomatic Rust — VERY GOOD

The codebase reads like clean, idiomatic Rust:
- Consistent use of `?` for error propagation
- Builder patterns (`Calendar::new().with_display_name()`)
- Proper use of `Option`/`Result`
- Clean match exhaustiveness
- Good use of `impl` blocks

The previous review's `.unwrap()` after `is_err()` anti-pattern has been fixed — `find_event_by_id` now uses proper `match` arms.

### [4.2] Error handling — GOOD with gaps

`anyhow::Context` is used consistently to add context to errors:
```rust
.context("Failed to parse start time")?
.with_context(|| format!("Calendar '{}' not found in config", calendar_name))?
```

This creates excellent error chains. However:

**Gap 1: No structured error output for JSON mode**

When a command fails in `--format json` mode, the error is output as plain text to stderr via anyhow's Display impl, not as structured JSON. An AI parsing JSON output gets a bare string error instead of:

```json
{
  "status": "error",
  "error": {
    "code": "CALENDAR_NOT_FOUND",
    "message": "Calendar 'nonexistent' not found in config"
  }
}
```

The `DEVELOPMENT_PLAN.md` specifies error codes (`AUTH_FAILED`, `CALENDAR_NOT_FOUND`, etc.) but they were never implemented. The `SuccessResponse` type exists but there's no `ErrorResponse` type.

**Gap 2: `config test` prints JSON AND returns error**

`src/commands/config.rs:234-246`:
```rust
Err(e) => {
    let output = json!({ "status": "error", ... });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Err(e).context("Connection test failed")
}
```

This prints a JSON error to stdout AND returns an error (which anyhow prints to stderr). The AI gets the error twice in different formats.

### [4.3] Dead code — TWO ITEMS REMAIN

**Item 1**: `#[allow(dead_code)]` on `CommandContext.verbose` (`src/commands/context.rs:24`):
```rust
#[allow(dead_code)]
pub verbose: bool,
```
The `verbose` field is set but never read within commands. It's handled in `main.rs` for log level. The field should either be used or removed from CommandContext.

**Item 2**: `#[allow(unused_imports)]` on `AttendeeStatus` re-export (`src/models/mod.rs:17-18`):
```rust
#[allow(unused_imports)]
pub use event::AttendeeStatus;
```
This re-exports a type that nothing outside the module uses. Remove the allow and the re-export, or actually use it somewhere.

### [4.4] Code smells

**Smell 1: `#[allow(clippy::too_many_arguments)]` appears 3 times**

- `commands/events.rs:167` — `create()` takes 8 parameters
- `commands/events.rs:400` — `update()` takes 8 parameters
- `parsers/ics.rs:380` — `build_event()` takes 8 parameters

This is a strong signal that these parameters should be grouped into a struct:

```rust
pub struct EventInput {
    pub summary: String,
    pub start: String,
    pub end: Option<String>,
    pub duration: Option<u32>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub attendees: Option<String>,
}
```

This would also help with the duplication issue (1.3) — `BatchEventInput` in `batch.rs` is essentially this struct already.

**Smell 2: `delete` always outputs JSON regardless of `--format`**

`src/commands/events.rs:385-393`:
```rust
let response = SuccessResponse::new(json!({...}));
println!("{}", serde_json::to_string_pretty(&response)?);
```

There's no `match ctx.format` — delete always outputs JSON. This is inconsistent with `list`, `get`, `create`, `search`, and `conflicts` which all respect `--format`.

**Smell 3: `calendars list` always outputs JSON**

`src/commands/calendars.rs:37-61`: The `calendars list` and `calendars info` commands always output JSON via `SuccessResponse`, ignoring `--format text`. There's no text formatter for calendar listings.

### [4.5] Data structure fitness — GOOD

- `HashMap<String, String>` for calendars (name -> href) is appropriate
- `Vec<Event>` for event listings is correct
- `Option<Vec<Attendee>>` for attendees (None when absent, Some when present) is clean
- `EventDateTime` as a separate struct is good for timezone metadata

### [4.6] Control flow — GOOD

Match arms are exhaustive. The `ICS` format variant consistently returns `anyhow::bail!("ICS format not yet implemented")` which is honest — better than silently doing nothing.

**One concern**: `parse_date` in `cli.rs` duplicates `parse_datetime` in `parsers/datetime.rs`:

```rust
// cli.rs:331-355
fn parse_date(date_str: &str) -> anyhow::Result<DateTime<Utc>> {
    match date_str.to_lowercase().as_str() {
        "today" => { ... }
        "tomorrow" => { ... }
        _ => {}
    }
    crate::parsers::datetime::parse_datetime(date_str)
}
```

This adds "today"/"tomorrow" handling on top of the datetime parser. This logic should be in `parsers/datetime.rs` so all callers get it, not just the `--from`/`--to` date parsing in `events list` and `events search`.

### [4.7] Test coverage — GOOD but GAPS at critical points

**45 tests** covering:
- Datetime parsing (8 tests) — excellent coverage
- ICS building/parsing (13 tests including fold, escape, VTIMEZONE) — excellent
- Config (5 tests including file permissions) — good
- CalDAV utilities (7 tests) — good
- URL parsing (2 tests) — adequate
- Date range and format (2 tests) — adequate

**Missing test coverage:**

1. **No tests for `parse_date` in `cli.rs`** — the "today"/"tomorrow" logic is untested
2. **No tests for conflict detection logic** — the overlap algorithm in `events::conflicts()` (lines 725-743) is untested. This is a critical inflection point: `evt_start < proposed_end && evt_end > proposed_start`
3. **No tests for attendee parsing** — the `split(',').map(|email| email.trim())` pattern in events create/update
4. **No tests for `parse_duration` edge cases** — what about `P0D`, `PT0S`, negative durations, `P1Y1M` (years/months, which are unsupported)?
5. **No tests for `CommandContext.load_config`** — custom path loading
6. **No integration tests** — the `tests/` directory has only an empty `common/mod.rs` and a Python live test script. No Rust integration tests exist.
7. **No tests for the `from_json` path in event creation** — the JSON file loading + CLI override logic

### [4.8] Module boundaries — GOOD

Public API surfaces are appropriate. Internal functions are `pub(crate)` or module-private. The re-exports in mod.rs files are clean.

### [4.9] Consistency — VERY GOOD

- Consistent copyright headers and SPDX identifiers on every file
- Consistent use of `log::info!`/`log::debug!`/`log::warn!`
- Consistent `anyhow::Context` usage
- Consistent `serde(skip_serializing_if = "Option::is_none")` on optional fields
- `cargo fmt` compliant

---

## Dimension 5: UI/UX

### [5.1] CLI ergonomics — GOOD

The command structure (`fastcal <noun> <verb>`) is intuitive:
```
fastcal events list
fastcal events create --summary "Meeting" --start "2026-03-10 2pm"
fastcal calendars list
fastcal config init
```

Clap's derive API provides `--help` automatically with all options documented.

**Issue**: The `--calendar` help text was fixed from the v1 review (now says "Target calendar name (as configured in config.toml)") — good.

**Missing**: No shell completion generation. Clap supports this via `clap_complete`:
```rust
// In a `completions` subcommand:
clap_complete::generate(shell, &mut Cli::command(), "fastcal", &mut std::io::stdout());
```

This would be a nice-to-have for human users.

### [5.2] JSON output consistency — INCONSISTENT

Different commands use different JSON envelopes:

**`events list`** — uses `SuccessResponse` with metadata:
```json
{"status": "success", "data": {"events": [...]}, "metadata": {"count": 5}}
```

**`events get`** — uses `SuccessResponse` without metadata:
```json
{"status": "success", "data": {"event": {...}, "calendar": "..."}}
```

**`events delete`** — uses `SuccessResponse` directly (no format switch):
```json
{"status": "success", "data": {"message": "...", "event_id": "..."}}
```

**`config show`** — uses raw `json!()` without `SuccessResponse`:
```json
{"status": "success", "data": {"config": {...}}}
```

**`config test` (error)** — uses raw `json!()` AND returns anyhow error:
```json
{"status": "error", "error": {"code": "...", "message": "..."}}
```

The envelope is mostly consistent (`"status"` + `"data"`) but `metadata` appears inconsistently, and error responses don't go through a structured type. An AI agent parsing this needs to handle multiple schemas.

**Recommendation**: Every command should output through `SuccessResponse` for success and a new `ErrorResponse` for errors. Metadata should always be present (even if empty/null).

### [5.3] Text output quality — GOOD with one gap

Text output uses emojis effectively for scanability. The `format_event_compact` function produces clean, readable output.

**Gap: Timezone display**

All times are displayed as UTC without any timezone indicator:
```
📅 Team Meeting
   Tue Mar 10, 02:00 PM    ← is this UTC? PST? User's local time?
```

The `preferences.default_timezone` config field exists but is **never used for display conversion**. Times stored as UTC are shown as-is. For a user in `America/Los_Angeles`, a 2 PM UTC meeting is actually 6 AM Pacific — this is confusing.

**Fix**: Either convert to the configured timezone for display, or append "UTC" to make it unambiguous:
```
📅 Team Meeting
   Tue Mar 10, 02:00 PM UTC
```

### [5.4] Error messages — GOOD

Error messages include context and are generally actionable:
```
Error: Calendar 'nonexistent' not found in config
Error: Failed to load configuration. Run 'fastcal config init' first.
Error: --summary is required (or use --from-json to load event from a file)
```

**Missing**: When a calendar is not found, the error doesn't list available calendars. The DEVELOPMENT_PLAN specifies this but it wasn't implemented:
```
Error: Calendar 'nonexistent' not found in config

Available calendars:
  - Personal
  - Work
```

### [5.5] AI context efficiency — GOOD

JSON output is compact and structured. The `skip_serializing_if = "Option::is_none"` annotations on Event fields avoid bloating output with null fields. Metadata includes count and date range for quick assessment.

**Suggestion**: Add a `--compact` or `--minimal` JSON mode that omits href, etag, and other internal fields that AI agents rarely need:
```json
{"id": "abc", "summary": "Meeting", "start": "2026-03-10T14:00:00Z", "end": "...", "duration_minutes": 60}
```

vs the current full output with href, calendar, status, created, modified, organizer, all_day, etc.

### [5.6] Dry-run support — ABSENT

There is no `--dry-run` flag for mutating operations (create, update, delete, batch). For an AI agent, this is important — the agent should be able to:

```bash
# Preview what would be created
fastcal events create --summary "Meeting" --start "2026-03-10 2pm" --dry-run

# Output:
{
  "dry_run": true,
  "would_create": {
    "summary": "Meeting",
    "start": "2026-03-10T14:00:00Z",
    "end": "2026-03-10T15:00:00Z",
    "calendar": "Personal"
  }
}
```

This lets the AI verify intent and show the user what would happen before executing. It's especially valuable for batch operations where a mistake could create/delete many events.

**Implementation**: Add `--dry-run` as a global flag, check it before the `client.request(PutResource...)` / `client.request(Delete...)` calls, and output the parsed/computed event data without sending it to the server.

### [5.7] Documentation completeness — GOOD with staleness

Docs are comprehensive:
- `README.md` — installation, quick start, command reference
- `docs/API.md` — JSON schemas, examples, AI integration tips
- `docs/FASTMAIL_SETUP.md` — step-by-step setup guide
- `docs/TESTING.md` — test strategy and execution checklist
- `examples/ai_assistant_usage.md` — AI workflow examples

**Staleness issues:**

1. `README.md:22` says "30/30 tests passing" — it's now 45/45
2. `README.md:232` says `cargo test  # 30/30 tests passing` — stale
3. `README.md:5` badge shows "tests-30%2F30" — stale
4. `README.md:22` says "Phase 7 Complete" — it's now Phase 9+ complete
5. `docs/API.md` batch create example shows a `{"events": [...], "calendar": "..."}` format, but the actual batch create expects a flat JSON array `[{...}, {...}]`
6. `docs/API.md` batch delete example shows `{"event_ids": [...]}`, but the actual code expects a flat JSON array of strings `["id1", "id2"]`
7. `docs/TESTING.md:8` says "30/30 passing" — stale
8. `docs/API.md:51` shows `"start": "2026-03-05T10:00:00-08:00"` (flat string) but actual Event schema has `"start": {"datetime": "...", "timezone": "..."}` (nested object)

These documentation-reality mismatches would cause an AI agent to construct incorrect batch input files or parse events incorrectly.

### [5.8] `--format` respect — MOSTLY FIXED, TWO GAPS REMAIN

The v1 review found `events get` and `events create` ignored `--format`. These were fixed. However:

1. **`events delete`** — always outputs JSON (line 385-392), no format switch
2. **`calendars list`** and **`calendars info`** — always output JSON, no text format

---

## Dimension 6: Observability and Graceful Error Handling

### [6.1] Error propagation — VERY GOOD

Every error path uses `anyhow::Context` or `with_context`. Error chains are descriptive:
```
Error: Failed to create event on server

Caused by:
    HTTP 409 Conflict
```

### [6.2] Network resilience — ABSENT

**No retry logic**: If a network request fails (WiFi blip, DNS timeout, 503 Service Unavailable), the operation fails immediately with no retry. For a CLI tool that talks to a remote server, at least one retry with a short backoff is standard practice.

**No timeout configuration**: There are no HTTP timeouts configured on the Hyper client. If the server hangs, fastcal hangs indefinitely. The `HyperClient::builder` supports `pool_idle_timeout` and the tower layer could add a timeout:

```rust
use tower::timeout::Timeout;
use std::time::Duration;

let auth_client = AddAuthorization::basic(raw_client, &username, &password);
let timeout_client = Timeout::new(auth_client, Duration::from_secs(30));
```

However, this would change the `Client` type alias, which has ripple effects. A simpler approach is to use `tokio::time::timeout` around individual operations:

```rust
let events = tokio::time::timeout(
    Duration::from_secs(30),
    caldav::event::list_events(&client, &calendar_href, ...)
).await
.context("Request timed out after 30 seconds")??;
```

**No connection reuse awareness**: The hyper client does connection pooling by default, but there's no configuration of pool size or idle timeout. For a CLI that makes a few requests and exits, this is fine.

### [6.3] Logging quality — GOOD

Verbose mode (`-v`) produces useful debug output:
```
[INFO  fastcal::caldav::client] Creating CalDAV client for: https://caldav.fastmail.com/...
[DEBUG fastcal::caldav::event] Using time range: 20260305T000000Z to 20260310T000000Z
[INFO  fastcal::caldav::event] Found 5 event(s)
```

Log levels are used appropriately:
- `info` for operation milestones
- `debug` for implementation details
- `warn` for recoverable issues (404 on stale resources)

**Concern**: Without `-v`, the default log level is `warn`. This means stale-resource 404 warnings appear in normal operation, which can confuse AI agents reading stderr (from v1 review issue #11). Consider using `log::debug!` instead of `log::warn!` for 404s on individual event fetches, since this is expected behavior for stale resources.

`src/caldav/event.rs:90-91`:
```rust
// Change from:
log::warn!("Failed to parse event {}: {}", resource.href, e);
// To:
log::debug!("Skipping unparseable event {}: {}", resource.href, e);
```

### [6.4] Graceful degradation — GOOD

Batch operations handle per-item failures gracefully — a failed create/delete doesn't abort the entire batch. The success/failure counts are reported at the end. This is correct behavior.

`list_events` handles individual event parse failures with `log::warn` and `continue` — correct.

`list_calendars` handles missing display names with a fallback to href-extracted names — correct.

### [6.5] Exit codes — BASIC

The tool uses only two exit codes:
- `0` for success
- `1` for any error (via anyhow)

The `DEVELOPMENT_PLAN.md` specifies differentiated exit codes (2 for auth, 3 for network, 4 for not found) but they were never implemented. For AI agent integration, differentiated exit codes help the agent decide whether to retry (network error) or adjust input (not found).

### [6.6] Stderr vs stdout separation — GOOD

All data output goes to `stdout` via `println!`. Progress messages in batch operations go to `stderr` via `eprintln!`. Logging goes to `stderr` via `env_logger`. This is correct for piping.

### [6.7] Progress indication — ADEQUATE

Batch operations show progress:
```
Creating event 1/10: Morning Standup
  ✓ Success
Creating event 2/10: Team Meeting
  ✗ Error: Failed to parse start time
```

Non-batch operations have no progress indication, which is fine since they complete in 1-2 seconds.

---

## Priority-Ordered Findings

### Critical (must fix for Gold)

| # | Finding | Dimension | Location |
|---|---------|-----------|----------|
| C1 | `find_event_by_id` scans all events in all calendars — O(N*M) | Perf (3.1) | `caldav/event.rs:107-168` |
| C2 | No HTTP timeout — tool hangs indefinitely if server is unresponsive | Observability (6.2) | `caldav/client.rs` |
| C3 | Documentation has incorrect batch input schemas (JSON structure mismatch) | UI/UX (5.7) | `docs/API.md` |
| C4 | Event start/end shown as EventDateTime object in JSON, but docs show flat strings | UI/UX (5.7) | `docs/API.md:40-53` |

### High (strong differentiators)

| # | Finding | Dimension | Location |
|---|---------|-----------|----------|
| H1 | Event creation logic duplicated between events.rs and batch.rs | Simplicity (1.3) | `commands/events.rs:168+`, `commands/batch.rs:146+` |
| H2 | "Find event then operate" pattern copy-pasted 4 times | Simplicity (1.3) | `commands/events.rs` (3x), `commands/batch.rs` (1x) |
| H3 | `events delete` always outputs JSON, ignores `--format` | UI/UX (5.8) | `commands/events.rs:385-392` |
| H4 | `calendars list/info` always output JSON, no text format | UI/UX (5.8) | `commands/calendars.rs` |
| H5 | `preferences.output_format` config value is stored but never used | Simplicity (1.5) | `config/mod.rs`, `cli.rs` |
| H6 | No `--dry-run` for mutating operations | UI/UX (5.6) | Global |
| H7 | No structured error output in JSON mode | Engineering (4.2) | Global |
| H8 | ICS parsing does parse-serialize-rescan cycle instead of direct property access | Perf (3.2) | `parsers/ics.rs:14-126` |
| H9 | Batch create/delete are sequential, not concurrent | Perf (3.3) | `commands/batch.rs` |
| H10 | `thiserror` dependency unused — dead weight | Simplicity (1.1) | `Cargo.toml` |

### Medium (quality improvements)

| # | Finding | Dimension | Location |
|---|---------|-----------|----------|
| M1 | `tokio` features = ["full"] pulls unused subsystems | Efficiency (2.4) | `Cargo.toml` |
| M2 | `futures` crate used for one function; `futures-util` suffices | Efficiency (2.4) | `Cargo.toml` |
| M3 | Times displayed without timezone indicator (UTC shown as local) | UI/UX (5.3) | `formatters/text.rs` |
| M4 | Missing tests for conflict detection overlap algorithm | Engineering (4.7) | `commands/events.rs:725-743` |
| M5 | Missing tests for "today"/"tomorrow" parsing in cli.rs | Engineering (4.7) | `cli.rs:331-355` |
| M6 | `parse_date` in cli.rs should be part of `parsers/datetime.rs` | Engineering (4.6) | `cli.rs:329-355` |
| M7 | Unnecessary `.clone()` calls in Cli::execute and calendar listing | Efficiency (2.1) | `cli.rs:220-225`, `caldav/calendar.rs:63-76` |
| M8 | `#[allow(dead_code)]` on CommandContext.verbose | Engineering (4.3) | `commands/context.rs:24` |
| M9 | `#[allow(unused_imports)]` on AttendeeStatus re-export | Engineering (4.3) | `models/mod.rs:17-18` |
| M10 | No retry logic for transient network failures | Observability (6.2) | `caldav/client.rs` |
| M11 | `config test` prints JSON error AND returns anyhow error (double output) | Engineering (4.2) | `commands/config.rs:234-246` |
| M12 | `#[allow(clippy::too_many_arguments)]` x3 — use EventInput struct | Engineering (4.4) | Multiple |
| M13 | Error messages don't list available calendars on "calendar not found" | UI/UX (5.4) | `commands/events.rs` |
| M14 | No integration tests (only unit tests + external Python script) | Engineering (4.7) | `tests/` |

### Low (polish)

| # | Finding | Dimension | Location |
|---|---------|-----------|----------|
| L1 | README stale: says 30 tests, Phase 7 — now 45 tests, Phase 9+ | UI/UX (5.7) | `README.md` |
| L2 | TESTING.md stale: says 30 tests | UI/UX (5.7) | `docs/TESTING.md` |
| L3 | Exit codes not differentiated (always 0 or 1) | Observability (6.5) | Global |
| L4 | No shell completion generation | UI/UX (5.1) | `cli.rs` |
| L5 | 404 warnings on stale resources use `warn` level (should be `debug`) | Observability (6.3) | `caldav/event.rs:90-91` |
| L6 | `parsers/mod.rs` has stale TODO comment about duration parsing (already implemented in ics.rs) | Engineering (4.9) | `parsers/mod.rs:11` |
| L7 | `AGENTS.md` contains Gemini CLI mandates, not project-relevant | Engineering | Root |

---

## Summary by Dimension

| Dimension | Grade | Key Strength | Key Gap |
|-----------|-------|-------------|---------|
| 1. Simplicity & Elegance | B+ | Clean module boundaries, lean feature set | Event creation duplication, unused deps |
| 2. Resource Efficiency | B- | Good async usage | Full-scan lookups, loose tokio features |
| 3. Performance | C+ | Concurrent display name fetching | `find_event_by_id` is O(N*M), sequential batches |
| 4. Software Engineering | A- | Idiomatic Rust, good error chains, 45 tests | Missing tests at critical inflection points |
| 5. UI/UX | B | Good CLI design, comprehensive docs | Stale docs, no dry-run, inconsistent format respect |
| 6. Observability | B- | Good logging, stderr/stdout separation | No timeouts, no retries, basic exit codes |

**Overall: B+ (Silver)**

The codebase is well-written, clean, and functional. The architecture is sound and the Rust is idiomatic. The main gaps preventing Gold are:
1. The O(N*M) `find_event_by_id` performance issue
2. No network resilience (timeouts/retries)
3. Documentation-code mismatches
4. Missing dry-run for AI safety
5. Inconsistent `--format` respect across all commands

Fixing C1-C4 and H1-H7 would bring this solidly to Gold tier.
