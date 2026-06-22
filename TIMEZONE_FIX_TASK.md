# Task: Make fastcal timezone-aware (config + `--timezone`), with a clean local → UTC → local boundary

## Project & conventions
You are working on **fastcal**, a Rust CalDAV CLI at `/home/aravindh/ai/fastcal/`. Follow the project's existing conventions and its `.claude/` + `AGENTS.md` workflow. Maintain its standards: all existing tests pass, **zero** compiler/clippy warnings, `cargo fmt` clean. Build and test on this Linux machine.

## Problem
fastcal is **UTC end-to-end at the human boundary**, and its timezone preference is **dead config**. Consequences for a user in `Europe/Amsterdam`:
- Listed/displayed event times are shown in UTC (a 12:00 local event prints as "10:00 UTC").
- `today`/`tomorrow` resolve to the **UTC** calendar day, not the user's local day — so day boundaries land at 02:00 local (summer), misplacing anything near midnight.
- A naive create time like `--start "14:00"` is interpreted as 14:00 **UTC** (= 16:00 local).
- `--from today --to today` collapses to a **zero-width** window (`[T00:00:00Z, T00:00:00Z]`) that matches no timed events.

## Evidence (verified in source — confirm before changing)
- `src/config/mod.rs:54-55,75` — `default_timezone` is defined with a default, but it is **only consumed by the `config` command** (`src/commands/config.rs:146-147, 200-201` for show/set). No date/parse/display code reads it.
- `src/parsers/datetime.rs:19` — `pub fn parse_datetime(input) -> Result<DateTime<Utc>>`; line 24-25 `"today" => Utc::now().date_naive()`, line 33 `"tomorrow" => Utc::now()...`. Everything is UTC; the configured zone is ignored.
- `src/commands/events.rs:257-258` — display hardcodes `"  Start: {} UTC"` / `"  End: {} UTC"`.
- `src/commands/events.rs:534-548` — create forces `.with_timezone(&Utc)`; `from`/`to` are `DateTime<Utc>`.
- Live observation: `--from today --to today` produced `from=2026-06-22T00:00:00Z, to=2026-06-22T00:00:00Z`; current config has `default_timezone = "America/Los_Angeles"` (wrong — user is `Europe/Amsterdam`).

## Required solution
Keep instants in **UTC internally and on the CalDAV wire** — that part is correct. Add a thin **local-time boundary** at input and output. Think "UTC sandwich": local in → UTC core → local out.

### 1. Resolved timezone — exactly one per invocation
Resolve a single IANA zone (`chrono-tz::Tz`) once at startup, by precedence:
1. `--timezone <IANA>` global CLI flag (e.g. `--timezone America/New_York`)
2. config `preferences.default_timezone` (IANA name)
3. system timezone (detect via the `iana-time-zone` crate)
4. `UTC` (last-resort fallback)

Validate the zone from the flag and from config; on an unknown zone, fail with a clear, actionable error. Use **IANA zones, never fixed offsets**, so DST (CET↔CEST) is handled automatically. Thread the resolved `Tz` through the command context so every parse/format call uses it.

### 2. Input boundary (local → UTC)
- `today` / `tomorrow` / `YYYY-MM-DD` resolve to the local calendar day **in the resolved zone**, then convert to UTC for the query/create.
- Date ranges are **half-open `[from, to)`**. A date-only `--to <date>` means *through the end of that local day*, i.e. `to = (date + 1 day) at 00:00 local`. Therefore `--from today --to today` covers the **entire local day** (this also removes the zero-width-window bug). Worked example, Amsterdam summer: `today` → local `[06-22 00:00 CEST, 06-23 00:00 CEST)` → CalDAV `[06-21T22:00:00Z, 06-22T22:00:00Z)`.
- Naive datetimes (`--start "14:00"`, `"YYYY-MM-DD HH:MM"`) are interpreted in the resolved zone.
- Inputs that already carry an explicit offset or `Z` are respected as written (converted to UTC), **not** reinterpreted in the resolved zone.

### 3. Output boundary (UTC → local)
- Convert event instants to the resolved zone for display, and **label the zone/offset** (e.g. `12:00 CEST` or `12:00 +02:00`). Remove the hardcoded `UTC` strings.
- In JSON output, render datetimes with the resolved-zone offset (unambiguous) — do not emit bare UTC for human-facing fields. Keep machine-stable fields if needed, but the displayed local time must be correct.
- **All-day events stay date-only.** Never convert an all-day event into a timed instant or shift its date by an offset.
- On read, an event with its own TZID keeps its true instant; display it in the resolved zone (showing the original zone too is a nice-to-have, optional).

### 4. Config
- `default_timezone` is an IANA name; `config set preferences.default_timezone Europe/Amsterdam` works and is validated.
- `config init` auto-detects the system timezone and pre-fills `default_timezone` (kills the `America/Los_Angeles` default footgun).

## Reasoning (so choices are clear)
- UTC core is correct; only the human edges were wrong → add a conversion boundary rather than removing UTC.
- **Config-authoritative, not host-tz-at-runtime:** fastcal runs on a server whose host zone is incidental (a cloud box is usually UTC); the user's home zone is stable and belongs in config. The `--timezone` flag covers travel/one-offs; system detection is only a setup-time default + last-resort fallback.
- **IANA over fixed offset** for DST correctness.
- **Half-open intervals** avoid both the zero-width window and double-counting at day boundaries.

## Acceptance criteria
With `default_timezone = "Europe/Amsterdam"` (summer / CEST):
- `events list --from today --to today` returns the full local day; an event at 12:00 local renders as `12:00` (CEST), not `10:00 UTC`; the CalDAV window is `[prevDay 22:00Z, today 22:00Z)`.
- `--timezone America/New_York` overrides config for that single invocation.
- `events create --start "2026-06-25 14:00" --dry-run` shows 14:00 local (→ 12:00Z); a real create stores 12:00Z.
- All-day events are unchanged (still date-only, correct date).
- An input like `2026-06-25T14:00:00Z` keeps its meaning.
- An invalid zone (flag or config) yields a clear error.
- `config init` writes the detected system zone.
- A DST test passes both ways: a date in CET (winter, +01:00) and one in CEST (summer, +02:00) resolve correctly.
- All pre-existing tests pass (update any that asserted UTC display); add unit/integration tests for: today/tomorrow-in-zone, half-open day boundary, DST winter vs summer, naive-create interpretation, `--timezone` override, all-day passthrough, invalid-zone error.
- Build green, clippy zero-warnings, fmt clean on this Linux box.

## Notes / scope
- Don't change CalDAV wire semantics beyond translating the query window and storing correct UTC instants.
- Check `Cargo.toml`; you will likely add `chrono-tz` and `iana-time-zone` (confirm versions; keep dependencies lean).
- Keep the change cohesive with fastcal's existing module layout (`parsers/datetime.rs`, `commands/events.rs`, `config/`, `commands/context.rs`).
