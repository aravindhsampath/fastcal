# Driving fastcal from an AI agent

fastcal is a thin, deterministic CalDAV CLI. It does **not** reason about
natural language, free/busy, or recurrence — that's the calling model's job.
This page lists the contract and the quirks the model must know.

## Golden rules

1. **Always pass `--timezone <IANA>`** (e.g. `--timezone Europe/Amsterdam`), or
   rely on the config default. A single invocation uses that one zone for
   **both** interpreting input **and** formatting output.
2. **Resolve relative dates yourself.** fastcal only understands `today`,
   `tomorrow`, and `YYYY-MM-DD`. "this Thursday", "next week", "the 15th", "the
   morning of" must become concrete dates before you call it.
3. **`--format json`** for machine use; **`--dry-run`** to preview a create /
   update / delete without mutating.
4. **Target a calendar** with `--calendar <name>`; otherwise the config
   `default_calendar` is used.

## Date / time input

Accepted (the time part may carry an `am`/`pm` suffix):

| Form | Example | Result |
|---|---|---|
| relative day | `today`, `tomorrow` | local day in the zone |
| date only | `2026-06-25` | all-day / start-of-day depending on role |
| ISO 8601 | `2026-06-25T14:00:00Z`, `…+02:00` | that exact instant, respected as written |
| 24-hour | `2026-06-25 14:30`, `…14:30:00` | 14:30 local |
| 12-hour | `2026-06-25 2:30pm`, `2pm` | 14:30 / 14:00 local |
| bare hour | `2026-06-25 17` | 17:00 (read as **24-hour**) |
| word | `noon`/`midday` → 12:00, `midnight` → 00:00 | |
| half/quarter | `half past 6`→6:30, `quarter past 9`→9:15, `quarter to 9`→8:45 | |
| minutes to/past | `5 to 9`/`5 minutes to 9`→8:55, `10 past 9`→9:10 | |

**Not** accepted — normalize these yourself before calling:
- Bare `half 9` (ambiguous: 9:30 English vs 8:30 Dutch). Use `half past 9`.
- Word-number minutes/hours (`ten to nine`). Use digits.
- Weekday/relative names (`next Tuesday`, `Friday`). Resolve to a date.
- A bare hour with no am/pm is **24-hour** — add `am`/`pm` (or use 24h) when the
  user means evening, e.g. send `5pm`, not `5`, for a 5 o'clock dinner.

Invalid dates/times are rejected deterministically with a precise message
(`invalid date '2026-02-31'`, `invalid time '25:00'`) — never a silent
mis-parse.

## Ranges & queries

- `events list --from <a> --to <b>` is **half-open `[a, b)`**.
- A date-only `--to <d>` covers **through the end of that local day** (it
  expands to the next local midnight). So `--from today --to today` = the whole
  local day.
- No `--from`/`--to` defaults to `[today, today+30d)`.
- When both bounds are set, recurring events are **expanded** to one row per
  occurrence (each carries `recurrence_id`); open-ended queries return the
  unexpanded master with its `rrule`.

## All-day events

- A **date-only** `--start` makes an all-day event.
- `--end` is the **inclusive last day**: `--start 2026-06-25 --end 2026-06-27`
  covers the 25th, 26th, and 27th. Reads return the same inclusive `end` (the
  RFC 5545 exclusive `DTEND` stays on the wire only).
- All-day events have **no `duration_minutes`** and never carry a time/zone.
- Don't mix a date and a time across start/end — that's rejected.

## Creating / updating times

- **Reschedule:** `events update <id> --start <new>` alone **shifts the end to
  preserve duration** (moving a 45-min meeting "to 2 PM" keeps it 45 min). To
  change the duration, pass `--end` (or both `--start` and `--end`).
- `--duration <minutes>` is an alternative to `--end` on create.

## Reminders (multiple supported)

- Repeat the flag: `--reminder-minutes 60 --reminder-minutes 1440` (1 h + 1
  day). 0 = "at start". Units are always minutes (60 = 1 h, 1440 = 1 day).
- On `update`, `--reminder-minutes …` **replaces** all existing reminders;
  `--no-reminders` strips them; omitting both leaves them untouched.
- JSON input (`--from-json`, `batch`) accepts `reminder_minutes` as a single
  number **or** an array: `"reminder_minutes": [60, 1440]`.

## Things fastcal does NOT do (orchestrate these yourself)

- **Free/busy & availability.** There is no "find a free slot" command. List
  each relevant calendar over the window (`--calendar A`, `--calendar B`) and
  compute gaps/overlaps yourself. Example policy: *evening* = 18:00–22:00; a
  "free evening for me" = my calendar free **and** my partner's calendar free;
  a "date night" = both calendars free.
- **Recurring creation.** fastcal reads RRULE but can't write it. Expand
  "every Wednesday" into discrete events and use `batch create` (a JSON array).
  Editing "one occurrence" then means editing that one dated event; "the whole
  series" means editing each matching event.
- **Conflicts across calendars.** `events conflicts --start --end` checks
  overlaps within the **one** targeted calendar only; all-day events (no time)
  don't participate. For cross-calendar checks, list each and compare.
- **RSVP / invitations, agenda summarization, natural-language parsing.**

## JSON output contract

- `start`/`end` are RFC 3339 with the **resolved-zone offset**
  (`2026-06-25T14:00:00+02:00`); `start.timezone` is the resolved IANA name.
- All-day `start`/`end` are bare dates (`2026-06-25`), no `timezone`.
- `created`/`modified` stay UTC. `rrule`/`recurrence_id` present only when
  applicable. `reminders` is an array of `{minutes_before, action}`.
- Errors in `--format json` go to **stderr** as `{"status":"error","error":{"message":…}}`.

## Exit codes

`0` ok · `1` general error · `2` auth (401/403) · `3` network/timeout · `4`
not found (event or calendar).
