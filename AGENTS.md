# Development Conventions for AI Agents

## Build & Test

Always verify correctness before committing:
```
make all   # fmt → clippy -D warnings → check → test → build --release
```
All 69 tests must pass (60 unit + 9 integration). Zero clippy warnings are required.

## Branching

Work on a feature branch (not `main`). Commit with descriptive messages after each logical change.

## Code Style

- Idiomatic Rust: use `?`, `Option`/`Result`, iterator chains
- `anyhow::Context` / `with_context` for all error annotations
- `log::info!` for milestones, `log::debug!` for internals, never `log::warn!` for expected conditions

## Architecture

```
caldav/     — libdav wrapper (network only)
commands/   — CLI handlers; use CommandContext for shared state
config/     — load/save ~/.config/fastcal/config.toml
models/     — Event, Calendar, SuccessResponse, ErrorResponse
parsers/    — ICS ↔ Event, datetime string parsing
formatters/ — text + JSON output formatting
```

Key shared helpers in `commands/helpers.rs`:
- `find_event_for_operation` — fast-path uid.ics lookup + fallback scan
- `create_event_on_server` — builds ICS and PUTs to CalDAV server
- `calendar_not_found_error` — lists available calendars in error message

## Output

All commands must respect `--format text|json`. Text output to stdout, errors to stderr. JSON success uses `SuccessResponse`; errors use `ErrorResponse` (emitted in `main.rs`).

## Dry-Run

`--dry-run` is a global flag. Mutating commands (create, update, delete, batch create/delete) must check `ctx.dry_run` before any network mutation and output a `"dry_run": true` preview instead.
