# VALARM / reminder support — implementation plan

**Status**: planned, not yet started
**Scope**: option (b) — full read + write
**Spans**: `aravindhsampath/fastcal` (most of the work) + `aravindhsampath/chittiv3` (tool surface + preamble)
**Trigger**: user test 2026-04-22 — calman could not set a "5 minutes before"
reminder on an event. Confirmed to be a fastcal gap, not a CalDAV or
Fastmail limitation. See the investigation notes below for diagnosis.

---

## Why this exists

Asked to create an event with a 5-minute reminder, calman said:

> *My current tools do not support setting custom reminders on events.*

That was correct. `fastcal events create --help` has no `--reminder` flag.
`Event` has no `reminders` field. The ICS builder never emits `VALARM`.
The parser acknowledges `VALARM` exists (comments at `src/parsers/ics.rs`
lines 166, 176) but deliberately *skips* properties nested inside it when
reading. So existing VALARMs on events read from Fastmail are silently
dropped, and any event created or edited through fastcal loses its
reminders if it had any.

CalDAV and Fastmail both fully support VALARM per RFC 5545 §3.6.6 — the
spec defines it as a sub-component of VEVENT, and Fastmail's web UI
renders them as notifications. This gap is entirely client-side.

---

## Design decisions (locked in)

### 1. Trigger format: relative minutes-before only (MVP)

The VALARM `TRIGGER` property supports:

- Relative: `TRIGGER:-PT5M` (5 min before start), `TRIGGER:PT10M` (after)
- Absolute: `TRIGGER;VALUE=DATE-TIME:20260423T120000Z`
- Relative-to-end: `TRIGGER;RELATED=END:-PT5M`

MVP supports **only relative, minutes-before, related to start**:
`TRIGGER:-PT<N>M` where N ≥ 0. Covers >95% of real reminder use. LLM
translates natural language ("an hour before") into minutes (60).

Hours/days and post-event reminders deferred — we can always add
`--reminder "PT1H"` or `--reminder-after-minutes` later without breaking
existing API.

### 2. Action: DISPLAY only (MVP)

Three VALARM actions in the spec:

- `DISPLAY` — desktop notification
- `EMAIL` — email the user (requires ATTENDEE fields in the VALARM)
- `AUDIO` — play a sound

MVP emits `ACTION:DISPLAY`. EMAIL adds more fields and isn't what most
users mean by "reminder." Deferred.

### 3. One reminder per event on write, many on read

Real calendars let an event carry multiple VALARMs (e.g. "15 min before"
AND "1 day before"). The write path accepts exactly one
`reminder_minutes` on create/update — that's the common case and it
matches how the LLM thinks about reminders. The read path surfaces a
`Vec<Reminder>` so existing events with multiple alarms aren't silently
flattened.

Setting multiple alarms on one event via fastcal is a future story
(probably `--reminder-minutes 15,60,1440`), not blocked by this change.

### 4. Update semantics: replace when specified, preserve when absent

Three user intents on update:

| Intent              | Flag                     | Behavior                               |
| ------------------- | ------------------------ | -------------------------------------- |
| Add / change        | `--reminder-minutes N`   | Replace all existing VALARMs with one new |
| Leave alone         | (flag omitted)           | Preserve existing VALARMs untouched    |
| Remove all          | `--no-reminders`         | Strip all VALARMs from the event       |

The third case needs a separate flag because "no `--reminder`" already
means "don't touch." Without this, users can't un-set a reminder via
fastcal — acceptable for MVP. Stretch: implement `--no-reminders` if
time permits.

**Implication**: the update path must now *fetch the existing ICS,
mutate in place, PUT back* rather than building a new VEVENT from
scratch. Today's update (need to verify — may already do this) is the
right shape; if not, needs a small refactor.

### 5. JSON output shape

The `Event` struct gains:

```json
"reminders": [
  { "minutes_before": 5, "action": "display", "description": "Car check" }
]
```

Empty list when the event has none. Omitted from JSON via
`skip_serializing_if = "Vec::is_empty"` so untouched shapes stay stable
for downstream consumers that don't care.

Never `null` or missing when reminders exist — keeps type simple.

---

## Implementation layer 1: fastcal

### Files touched

- `src/models/event.rs` — add `Reminder` struct + `reminders: Vec<Reminder>` field
- `src/parsers/ics.rs` — parse VALARMs into `Reminder`s; emit VALARMs in `build_event`
- `src/cli.rs` — `events create` + `events update` gain `--reminder-minutes`
- `src/commands/events.rs` — plumb the flag through to the builder
- `src/caldav/event.rs` — no change expected (reads the same hrefs, writes the same ICS)
- `Cargo.toml` — no new dep (RFC 5545 duration strings for -PT5M are trivial to format)

### Concrete steps

1. **Model** (`src/models/event.rs`):
   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct Reminder {
       /// Minutes before the event's start time the reminder fires.
       /// Only relative "before start" reminders supported today; more
       /// exotic triggers (absolute, RELATED=END) round-trip as raw on
       /// read but cannot be created via this field.
       pub minutes_before: u32,
       /// VALARM ACTION. "display" is the only value emitted on create;
       /// on read, exposes whatever the event carries.
       pub action: String,
       #[serde(default, skip_serializing_if = "Option::is_none")]
       pub description: Option<String>,
   }
   ```
   Add `pub reminders: Vec<Reminder>` to `Event` with `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.

2. **Parser** (`src/parsers/ics.rs`):
   - New helper `extract_valarms(ics: &str) -> Vec<Reminder>` that scans `BEGIN:VALARM` → `END:VALARM` blocks inside the active VEVENT. For each, read `ACTION`, `TRIGGER`, `DESCRIPTION`. If `TRIGGER` matches `-PT<N>M` (or `-PT<N>H`, `-P<N>D`), convert to total minutes and emit `Reminder`. Anything else (absolute datetime, RELATED=END, complex durations) → skip with a `log::debug!`.
   - Call from `parse_event`, populate `event.reminders`.
   - Adjust the depth-tracking in `extract_property` so nested VALARM properties (`TRIGGER`, `ACTION`, `DESCRIPTION`) don't leak into VEVENT-level matches. (Already done — the existing depth guard handles this; just verify.)

3. **Builder** (`src/parsers/ics.rs::build_event`):
   - `IcsBuildArgs` gains `pub reminders: &'a [Reminder]` (or `Option<&'a Reminder>` for MVP's single-reminder). Prefer `&[Reminder]` so we can add multi-support later without another breaking change.
   - After the event body and before `END:VEVENT`, emit one VALARM block per reminder:
     ```rust
     for r in args.reminders {
         ics.push_str("BEGIN:VALARM\r\n");
         ics.push_str(&fold_line(&format!("ACTION:{}", r.action.to_uppercase())));
         ics.push_str(&fold_line(&format!("TRIGGER:-PT{}M", r.minutes_before)));
         let desc = r.description.as_deref().unwrap_or(summary);
         ics.push_str(&fold_line(&format!("DESCRIPTION:{}", escape_ics_text(desc))));
         ics.push_str("END:VALARM\r\n");
     }
     ```
   - Default DESCRIPTION to the event's SUMMARY — it's what Fastmail's UI shows in the notification.

4. **CLI** (`src/cli.rs`):
   - `EventCommands::Create` gains `#[arg(long)] pub reminder_minutes: Option<u32>`.
   - Same for `EventCommands::Update`.
   - Add stretch: `#[arg(long)] pub no_reminders: bool` for "strip all on update."

5. **Command plumbing** (`src/commands/events.rs`):
   - `EventCreateOverrides` gains `reminder_minutes: Option<u32>`.
   - In `create()`, materialize `Vec<Reminder>` from the flag (single-element or empty) and pass to `build_event` via `IcsBuildArgs`.
   - Update flow: fetch existing event (already done today), mutate the returned `Event`'s `reminders` field per the rules in decision #4, rebuild ICS via `build_event`, PUT back.

6. **Tests** (`src/parsers/ics.rs`):
   - `parse_event_exposes_5min_reminder` — ICS with `BEGIN:VALARM ACTION:DISPLAY TRIGGER:-PT5M END:VALARM` → `event.reminders[0].minutes_before == 5`.
   - `parse_event_reports_multiple_reminders` — two VALARMs in one VEVENT → two entries in the vec.
   - `parse_event_skips_unsupported_trigger_shapes` — absolute `TRIGGER;VALUE=DATE-TIME:...` is logged and dropped, no panic.
   - `parse_event_handles_hour_and_day_triggers` — `-PT1H` → 60, `-P1D` → 1440.
   - `build_event_emits_valarm_when_reminder_present` — builder produces a VEVENT containing the exact VALARM block.
   - `build_event_emits_no_valarm_when_reminders_empty` — regression guard: default path unchanged.
   - `build_event_with_multiple_reminders_emits_all` — forward-compat.
   - `valarm_round_trip_preserves_minutes_before` — build → parse → same value back.

### Acceptance for layer 1

- `fastcal events create --summary Test --start 2026-05-01T14:00:00+02:00 --duration 60 --reminder-minutes 5 --dry-run` returns a `would_create` JSON that includes a `reminders` array with one entry.
- Same without `--dry-run` actually creates the event. Verify via Fastmail web UI that the notification fires 5 min before.
- `fastcal events get <id>` on a Fastmail event that was created with a reminder in another client (e.g. Apple Calendar) correctly exposes `reminders`.
- `make all` green, 8+ new tests added.

---

## Implementation layer 2: chittiv3

### Files touched

- `src/tools/calcli/types.rs` — add `Reminder` struct + `reminders` field on `Event`; impl `LocalizeTimes` for it (no-op; reminders are relative durations, no UTC→local conversion needed)
- `src/tools/calcli/events.rs` — `EventsCreateArgs` / `EventsUpdateArgs` gain `reminder_minutes: Option<u32>`; wire through to fastcal argv
- `agents/calman.md` — tiny note: "you can request a reminder with `reminder_minutes: N`"
- Submodule bump in chittiv3 after fastcal merges

### Concrete steps

1. **`types.rs`**:
   ```rust
   #[derive(Debug, Clone, Deserialize, Serialize)]
   pub struct Reminder {
       pub minutes_before: u32,
       pub action: String,
       #[serde(default, skip_serializing_if = "Option::is_none")]
       pub description: Option<String>,
   }
   ```
   Add `pub reminders: Vec<Reminder>` to `Event` with `#[serde(default)]` and `skip_serializing_if = "Vec::is_empty"`.
   `impl LocalizeTimes for Reminder { fn localize(&mut self, _: Tz) {} }` — reminders are relative, nothing to convert. Event's existing `localize` impl doesn't need to touch the reminders vec.

2. **`events.rs`**:
   - `EventsCreateArgs` gains `#[serde(default)] pub reminder_minutes: Option<u32>`.
   - In `build_create_argv`, push `--reminder-minutes <N>` when Some.
   - Same for `EventsUpdateArgs` + `build_update_argv`.
   - Update the `ToolDefinition::parameters` schema to advertise the new field to the LLM.

3. **`agents/calman.md`**:
   - In "The calcli tool surface" section, update the `events_create` row to mention `reminder_minutes: Option<u32>`.
   - Add a short note: *"If the user asks for a reminder, pass `reminder_minutes` (e.g. 5 for 5 min before, 60 for 1 hour before, 1440 for 1 day before). Default to no reminder if not requested — don't add unsolicited."*

4. **Tests**:
   - `events.rs` test `create_argv_includes_reminder_minutes_flag` — confirms `--reminder-minutes 5` appears in the argv.
   - `types.rs` test `event_deserialize_with_reminders` — JSON with a reminders array → populated struct.

5. **Submodule bump**: after fastcal's `feat/valarm-support` is merged to main, `cd tools/fastcal && git fetch && git checkout <new-hash>` in chittiv3, commit the bump alongside the tool-args changes.

### Acceptance for layer 2

- Calman, asked "add a car check at 5pm Thursday with a 5-minute reminder," proposes the event WITH the reminder, and on 👍 calls `events_create` with `reminder_minutes: 5`.
- Calman, asked "what's on my Thursday" for a day containing a reminder-bearing event, mentions the reminder if asked for detail.
- `make all` green.

---

## Order of operations

1. **fastcal** `feat/valarm-support` branch:
   - Land parser read support + tests (commit 1)
   - Land builder emit + tests (commit 2)
   - Land CLI flag + tests (commit 3)
   - `make all` green, push, merge to main

2. **chittiv3** `feat/reminder-tool-field` branch:
   - Bump `tools/fastcal` submodule to the post-merge hash
   - `types.rs` + `events.rs` changes
   - `agents/calman.md` note
   - Tests
   - `make all` green, push, merge to main

3. **Smoke test against live Fastmail**:
   ```bash
   # tab 1: start Chitti
   cargo run --release

   # tab 2: REPL
   cargo run --bin repl
   /channel calendar
   add a test event for tomorrow at 10am for 30 mins with a 5 min reminder in fastcal-test calendar
   # then 👍 when calman proposes
   ```

   Open the Fastmail web UI, verify the event exists in `fastcal-test` and has a "5 minutes before" alarm. Then delete it (via `events_delete` in the REPL) to clean up.

---

## Test matrix

| Case                                              | Layer   | Test |
| ------------------------------------------------- | ------- | ---- |
| Parse `-PT5M` trigger                             | fastcal | ✓   |
| Parse `-PT1H` → 60 minutes                        | fastcal | ✓   |
| Parse `-P1D` → 1440 minutes                       | fastcal | ✓   |
| Parse absolute `TRIGGER;VALUE=DATE-TIME:...` → skip | fastcal | ✓   |
| Parse `RELATED=END:-PT5M` → skip (for now)        | fastcal | ✓   |
| Parse multiple VALARMs → Vec with all             | fastcal | ✓   |
| Build event with no reminder → no VALARM emitted  | fastcal | ✓ (regression) |
| Build event with one reminder → one VALARM        | fastcal | ✓   |
| Build event with multiple reminders → each VALARM | fastcal | ✓   |
| Round-trip: build → parse → same count + minutes  | fastcal | ✓   |
| CLI flag plumbs to reminders                      | fastcal | integration test |
| `events_create` tool argv includes flag           | chittiv3 | ✓  |
| JSON with reminders round-trips via Event struct  | chittiv3 | ✓  |
| Reminder field skip_serializing when empty        | chittiv3 | ✓  |
| Live: create + fetch + read back via Fastmail     | manual | smoke test |

---

## Deferred / out of scope

- **`--no-reminders` flag** on update (to clear existing reminders). Ship if easy, otherwise next PR.
- **Multiple reminders on create/update via CLI** (`--reminder-minutes 5,60`). Shape is already in place via `Vec<Reminder>`; just needs CLI parsing.
- **EMAIL / AUDIO actions.** Require attendee handling (EMAIL) or attachment URI (AUDIO).
- **Absolute-time triggers** (`TRIGGER;VALUE=DATE-TIME:...`).
- **Relative-to-end triggers** (`TRIGGER;RELATED=END:...`).
- **Complex ISO-8601 durations** on write (`-PT1H30M`). Read accepts them already via the hours+minutes+days conversion.
- **Preservation of unknown reminder shapes across edit.** If a user edits an event that has an absolute-time reminder we don't understand, today's proposal replaces all VALARMs (or preserves none). Full fidelity would require passing through raw VALARM blocks untouched. Deferred — flag and warn when dropping.

---

## Open questions for the build step

1. **Update semantics verification**: confirm that `fastcal events update` today rebuilds the full VEVENT ICS vs. a targeted property patch. If it's a full rebuild, preserving unknown reminders isn't possible without more plumbing; I'd accept "on update, all existing VALARMs are replaced by whatever the user passes" as the MVP behavior and warn in the CLI help.
2. **Duration parsing fidelity**: does `-PT90M` parse correctly in the regex I'll use? Write that test first, don't assume.
3. **Fastmail quirks**: does Fastmail add a default VALARM to events created without one (some providers do)? If yes, verify the "no-reminder" path in the live smoke test — our round-trip might show an unexpected reminder.

---

## Estimated effort

- **Layer 1 (fastcal)**: ~300 lines of code + ~150 lines of tests. Half a day.
- **Layer 2 (chittiv3)**: ~80 lines of code + ~40 lines of tests. 1 hour after fastcal is merged.
- **Smoke test**: 15 minutes.
- **Total**: ~1 focused day, split across two branches / two merges.

---

## References

- RFC 5545 §3.6.6 — Alarm Component — https://datatracker.ietf.org/doc/html/rfc5545#section-3.6.6
- RFC 5545 §3.3.10 — Duration value type (`-PT5M`, `-P1D` grammar) — https://datatracker.ietf.org/doc/html/rfc5545#section-3.3.10
- Existing fastcal files:
  - `src/parsers/ics.rs` — parser + builder
  - `src/models/event.rs` — Event struct
  - `src/cli.rs` — CLI args
  - `src/commands/events.rs` — command implementations
- Existing chittiv3 files:
  - `src/tools/calcli/types.rs` — Event/Reminder types for deserialization
  - `src/tools/calcli/events.rs` — `EventsCreateArgs`, `EventsUpdateArgs`
  - `agents/calman.md` — preamble
