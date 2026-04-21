# fastcal Review — v1

**Date**: 2026-03-06
**Reviewer**: Claude Sonnet 4.6
**Method**: Full source code audit + live Fastmail server testing
**Test result**: 36/36 unit tests pass. All live CRUD flows verified and cleaned up.

---

## Executive Summary

The core CalDAV plumbing is solid — authentication, service discovery, event listing, and the ICS parser all work correctly against a live Fastmail server. The architecture is clean and idiomatic Rust. However, there are **four bugs that cause advertised features to silently do nothing**, one RFC compliance violation that could break compatibility with strict servers, a text parsing bug that garbles displayed values, and a handful of code hygiene and performance issues that matter for its intended use as an AI personal assistant tool.

---

## Critical Bugs (Broken Advertised Features)

### 1. `events list --search` is silently ignored

**File**: `src/cli.rs:258`

```rust
EventCommands::List { from, to, search: _ }  // search is thrown away
```

The `--search` flag appears in `--help` but does absolutely nothing. The `list()` function is called without it, and there is no filtering. An AI using `--search "keyword"` to narrow results receives the full unfiltered list with no warning or error. The `events search` subcommand already provides this functionality correctly — the `--search` on `list` is either redundant or should be wired to the same logic.

### 2. `events create --from-json` is silently ignored

**File**: `src/cli.rs:280`

```rust
EventCommands::Create {
    summary, start, ..., from_json: _  // from_json thrown away
}
```

The `--from-json` option is documented in `--help` as a way to create an event from a JSON file, but it is parsed and immediately discarded. `--summary` remains required even when `--from-json` is provided, so the flag cannot even be used without errors. The `events::create()` function does not accept `from_json` at all.

### 3. `events get` always outputs JSON, ignores `--format text`

**File**: `src/commands/events.rs:140-145`

```rust
// ctx.format is never checked — always prints JSON
let response = SuccessResponse::new(json!({ "event": event, ... }));
println!("{}", serde_json::to_string_pretty(&response)?);
```

The `formatters::format_event()` function exists specifically for text output of a single event and is marked `#[allow(dead_code)]` precisely because it is never called here. `--format text` is silently ignored.

### 4. `events create` always outputs JSON, ignores `--format text`

**File**: `src/commands/events.rs:242-248`

Same issue. After creating an event, `ctx.format` is never consulted — always JSON.

---

## Logic and Safety Issues

### 5. `.unwrap()` after explicit `is_err()` check

**File**: `src/caldav/event.rs:169, 189`

```rust
let listed = client.request(list_result).await;
if listed.is_err() {
    log::warn!("...");
    continue;
}
let hrefs = listed.unwrap()  // ← anti-pattern
    .resources
    .into_iter()
    ...

let resources = client.request(...).await;
if resources.is_err() {           // checked
    ...
}
for resource in resources.unwrap().resources {  // ← second unwrap
```

This pattern checks `is_err()` and logs/continues, then calls `.unwrap()` on the next line. While it cannot panic in the current flow, it is a code smell and a maintenance trap — if the error path ever changes, the unwrap becomes a real panic risk. The idiomatic Rust approach is a `match` or `let Ok(x) = ... else { continue; }`.

### 6. Empty `etag` bypasses optimistic concurrency control in `events update`

**File**: `src/commands/events.rs:389`

```rust
let etag = event.etag.clone().unwrap_or_default();  // → empty string ""
```

When `etag` is absent, an empty string is sent as the `If-Match` value. The CalDAV `PutResource::update()` call uses this etag to enforce that no other client modified the event between fetch and write. An empty etag either causes a server rejection or silently bypasses the check — neither is correct. The value should either propagate an error ("cannot update: etag unknown, re-fetch first") or the update should be sent unconditionally with no `If-Match` header rather than a bogus one.

### 7. ICS text values not unescaped on parse

**File**: `src/parsers/ics.rs:133-173` (`extract_property`)

RFC 5545 defines text escape sequences: `\n` = newline, `\,` = comma, `\;` = semicolon, `\\` = backslash. The `extract_property` function returns raw ICS strings without unescaping. In text output, this produces:

```
📍 Høegh-Guldbergs Gade 4\nbuilding 1651\, 120\n8000 Aarhus C\nDenmar
```

Instead of:

```
📍 Høegh-Guldbergs Gade 4
building 1651, 120
8000 Aarhus C
```

The `escape_ics_text()` write-side function exists but there is no matching `unescape_ics_text()` on the read side.

---

## RFC Compliance Violation

### 8. ICS lines not folded at 75 octets (RFC 5545 §3.1)

**File**: `src/parsers/ics.rs:347-410` (`build_event`)

RFC 5545 §3.1 requires that content lines be no longer than 75 octets (excluding CRLF). Lines that exceed this limit must be "folded" by inserting a CRLF followed by a single whitespace character. The `build_event` function writes property lines without folding:

```
SUMMARY:This is a very long summary that exceeds the 75 octet RFC 5545 line folding requirement...
```

This line is 145 octets. Fastmail accepted it, but **strict CalDAV servers may reject the entire PUT request**. Events with long titles, multi-sentence descriptions, or multi-line locations are at risk. There is a `TODO` comment in the code acknowledging this:

```rust
// Build ICS string manually for now
// TODO: Use calcard's builder API when available
```

---

## Performance Issues

### 9. `find_event_by_id` fetches every event in every calendar

**File**: `src/caldav/event.rs:147-204`

To find one event by UID, the function:
1. Lists **all** event hrefs in **all** configured calendars (N calendars × all time)
2. Fetches **all** event bodies in bulk
3. Parses each one, checking if `event.id == target_id`

For a calendar with 200 events across 4 calendars, this is up to 800 HTTP requests + parses for a single lookup. This function is called by `events get`, `events update`, `events delete`, and `batch delete` (once per item). In practice, Fastmail uses the filename `{uid}.ics`, so the much faster approach is a direct fetch of `{calendar_href}/{event_id}.ics` first, falling back to scan only on 404.

### 10. No date range on find/delete/update lookups

`find_event_by_id` calls `list_events` with `from=None, to=None`, loading the **entire calendar history** including events from years past. Every `events update`, `events delete`, and `batch delete` item scans every event ever created in all calendars.

---

## Dead Code (6 functions with `#[allow(dead_code)]`)

| Function | File | Status |
|---|---|---|
| `get_event()` | `src/caldav/event.rs:107` | Fetches by href; never called anywhere |
| `format_event()` | `src/formatters/mod.rs` | Single-event formatter; never called (needed to fix bug #3 and #4) |
| `format_event()` | `src/formatters/text.rs:34` | Text impl; never called |
| `format_calendars()` | `src/formatters/text.rs:109` | Calendar formatter; calendars command always outputs JSON |
| `format_date_for_ics()` | `src/parsers/datetime.rs:117` | Date-only ICS format; never called |
| `config_dir()` | `src/config/mod.rs:95` | Returns config directory path; never called |

Suppressing dead code warnings with `#[allow(dead_code)]` means the compiler cannot help catch when these functions fall out of sync with the types they operate on. They should either be used (fixing bugs #3/#4 in the case of `format_event`) or removed.

---

## UI/UX Issues (AI Assistant Context)

### 11. Stale 404 WARN floods stderr on every calendar operation

```
WARN  fastcal::caldav::event] Failed to fetch event .../UID:20220104T065403Z-@synaps-web-686457f6-k5qql.ics: HTTP 404 Not Found
```

There is a stale resource in the "Sathish" calendar — the server includes it in `ListCalendarResources` responses but the actual `.ics` file returns 404. This WARN appears on **every** `events list`, `events search`, `batch delete`, and `events update` call against that calendar. For an AI reading stderr to detect errors, this noise is indistinguishable from a real problem. The 404 case is already handled gracefully (`log::warn` + skip), but there is no mechanism to silence known-stale entries.

### 12. All times displayed without timezone context

Text output shows:

```
Tue May 23, 11:00 PM
```

This is UTC, but it reads as local time. The `preferences.default_timezone` field is configured (defaults to `"America/Los_Angeles"`) but is **never used anywhere** — no time conversion, no display label. An AI inferring meeting times from text output will read UTC as local time.

### 13. `preferences.output_format` in config has no effect

The config stores `preferences.output_format = "json"` and `config set preferences.output_format` accepts new values — but it is never read when executing commands. The CLI `--format` flag always governs, with no fallback to the config preference. This key is stored and validated but is a no-op.

### 14. Batch operations mix progress and JSON on the same stream

Progress messages use `eprintln!` (stderr) and JSON results use `println!` (stdout), which is the correct split. However, `WARN` log messages also go to stderr, so a batch operation produces interleaved progress + warnings on stderr that an AI cannot easily distinguish from real errors without parsing log level prefixes.

### 15. `--calendar` help text is hardcoded to wrong example names

**File**: `src/cli.rs:33`

```rust
/// Target calendar (personal|wife|shared)
```

This example doesn't match any real calendar names discovered from the server. Should be a generic description like "Calendar name (as shown in `calendars list`)".

---

## Code Hygiene

### 16. `Metadata.extra` HashMap is always empty but always serialized

**File**: `src/models/output.rs:48`

```rust
#[serde(flatten)]
pub extra: HashMap<String, serde_json::Value>,
```

This field is `#[serde(flatten)]` so its contents merge into the JSON object. It is allocated on every `Metadata::new()` call and never populated. It adds a heap allocation per response with zero benefit.

### 17. `format_datetime_for_ics` implemented in two modules

`src/parsers/datetime.rs:111` (`format_for_ics`) and `src/caldav/event.rs:207` (`format_datetime_for_ics`) are identical private functions with different names. One should be made public and the other removed.

### 18. Test comment contains placeholder text

**File**: `src/caldav/event.rs:232`

```rust
assert_eq!(formatted.len(), 16); // YYYYMMDDTHHMMSSexpZ = 16 chars
```

`"expZ"` appears to be residual template/autocomplete text. Should read `YYYYMMDDTHHMMSSz`.

---

## Priority Summary

| Priority | Issue | Impact |
|---|---|---|
| Critical | `--search` in `events list` silently ignored | AI gets wrong results with no indication |
| Critical | `--from-json` in `events create` silently ignored | Documented feature completely non-functional |
| Critical | ICS text escape sequences not unescaped | Text output shows raw `\n`, `\,` in locations/descriptions |
| High | `events get` / `events create` ignore `--format` | `--format text` broken for 2 of 5 event subcommands |
| High | `.unwrap()` after `is_err()` in `find_event_by_id` | Code smell, maintenance hazard |
| High | ICS line folding missing (RFC 5545 §3.1) | Interoperability failure risk with strict servers |
| High | `find_event_by_id` fetches all events | Very slow for large calendars; gets worse with batch ops |
| Medium | Empty etag bypasses `If-Match` in update | Optimistic concurrency control ineffective |
| Medium | No date range on find/delete/update | Full calendar history scanned on every mutation |
| Medium | 6 dead code functions suppressed with `#[allow]` | Compiler cannot catch stale code |
| Medium | Stale 404 WARN on every list operation | Pollutes stderr; indistinguishable from real errors for AI |
| Low | UTC times shown without timezone label | Confusing; AI may misinterpret meeting times |
| Low | `preferences.output_format` config key unused | Config option with no effect |
| Low | `--calendar` help text has wrong example names | Minor documentation issue |
| Low | `Metadata.extra` HashMap always empty | Unnecessary heap allocation per response |
