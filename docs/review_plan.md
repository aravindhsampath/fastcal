# Opus Review Plan - fastcal

**Reviewer**: Claude Opus 4.6
**Date**: 2026-03-06
**Scope**: Full codebase review across 6 competition dimensions

---

## Review Methodology

I've read every source file (26 `.rs` files, ~4,928 lines), all docs (6 markdown files), the Makefile, Cargo.toml, and test infrastructure. I ran `cargo test` (45/45 passing), `cargo clippy -- -D warnings` (zero warnings), and reviewed the previous Sonnet review (REVIEW_v1.md) to understand which issues were fixed and what remains. I performed web searches to verify dependency currency and RFC compliance.

The review evaluates the codebase against 6 dimensions, each broken into specific checklist items.

---

## Dimension 1: Simplicity yet Elegance

**Reasoning**: Does the project achieve maximum functionality with minimum complexity? Are abstractions earned, not speculative? Is the dependency graph lean?

- [x] 1.1 Dependency audit — `thiserror` unused, `futures` overkill, `tokio` features too broad
- [x] 1.2 Abstraction review — Clean six-module architecture, well-drawn boundaries
- [x] 1.3 Code duplication — Event creation duplicated (events.rs + batch.rs), find-then-operate pattern x4
- [x] 1.4 Feature completeness vs complexity — Excellent: lean ~5k LOC for full CRUD+search+batch+conflicts
- [x] 1.5 Configuration design — Good layering, but `preferences.output_format` is a no-op
- [x] 1.6 Type design — Good, but string-based EventDateTime causes repeated parse round-trips

## Dimension 2: Resource Efficiency

**Reasoning**: For a CLI that runs, does work, and exits, resource efficiency means: minimal allocations, no wasted network calls, compact binary.

- [x] 2.1 String allocation audit — 7 allocations per calendar where 2-3 suffice; unnecessary clones in Cli::execute
- [x] 2.2 Network efficiency — CRITICAL: find_event_by_id is O(N*M) full scan
- [x] 2.3 Memory patterns — Adequate, no major issues
- [x] 2.4 Dependency weight — tokio "full" pulls unused fs/process/signal; futures overkill
- [x] 2.5 Async overhead — Justified for concurrent operations

## Dimension 3: Performance Optimizations

**Reasoning**: Where does latency live? Network I/O dominates, but parsing/serialization should be efficient.

- [x] 3.1 `find_event_by_id` full-scan performance — CRITICAL: try direct {uid}.ics fetch first
- [x] 3.2 ICS parsing efficiency — parse-serialize-rescan anti-pattern, works but doubles work
- [x] 3.3 Concurrent operations — join_all used well for display names, missing for batch ops
- [x] 3.4 Early exits — Good: immediate return on match, empty-list short circuits
- [x] 3.5 Batch operation parallelism — Sequential creates/deletes, should be concurrent

## Dimension 4: Software Engineering

**Reasoning**: The core — is this well-engineered Rust? Would another Rust engineer find it readable, maintainable, testable?

- [x] 4.1 Idiomatic Rust — Very good: proper ? usage, builders, ownership
- [x] 4.2 Error handling — Good chains, but no structured JSON errors; config test double-outputs
- [x] 4.3 Dead code — 2 items: CommandContext.verbose, AttendeeStatus re-export
- [x] 4.4 Code smells — 3x too_many_arguments (use EventInput struct); delete ignores format
- [x] 4.5 Data structure fitness — Good: appropriate use of HashMap, Vec, Option
- [x] 4.6 Control flow — Good, but parse_date in cli.rs should be in parsers/datetime.rs
- [x] 4.7 Test coverage — 45 tests, but gaps: no conflict overlap tests, no today/tomorrow tests
- [x] 4.8 Module boundaries — Good pub API surfaces
- [x] 4.9 Consistency — Very good: consistent headers, logging, serde annotations

## Dimension 5: UI/UX (Human + AI)

**Reasoning**: The tool serves two audiences: humans at terminals and AI models calling it as a tool. Both need clear, predictable, context-efficient interfaces.

- [x] 5.1 CLI ergonomics — Good noun-verb structure, accurate help texts
- [x] 5.2 JSON output consistency — Mostly consistent envelope, but metadata varies
- [x] 5.3 Text output quality — Good emojis/formatting, but no timezone indicator
- [x] 5.4 Error messages — Actionable, but missing "available calendars" list on not-found
- [x] 5.5 AI context efficiency — Good: skip_serializing_if keeps output compact
- [x] 5.6 Dry-run support — ABSENT: no --dry-run for any mutating operation
- [x] 5.7 Documentation completeness — Comprehensive but STALE: wrong test counts, wrong batch schemas
- [x] 5.8 `--format` respect — Mostly fixed, but delete and calendars commands still ignore it

## Dimension 6: Observability and Graceful Error Handling

**Reasoning**: When things go wrong (network drops, bad input, server errors), does the tool inform the user clearly and recover gracefully?

- [x] 6.1 Error propagation — Very good: consistent anyhow::Context usage throughout
- [x] 6.2 Network resilience — ABSENT: no timeouts, no retries
- [x] 6.3 Logging quality — Good levels, but 404 warnings on stale resources should be debug
- [x] 6.4 Graceful degradation — Good: batch ops handle per-item failures correctly
- [x] 6.5 Exit codes — Basic: only 0/1, planned differentiated codes not implemented
- [x] 6.6 Stderr vs stdout separation — Correct: data to stdout, progress/logs to stderr
- [x] 6.7 Progress indication — Adequate for batch ops, not needed for single ops

---

## Execution Plan

1. Work through each dimension systematically
2. Mark checklist items as findings emerge
3. Categorize findings by severity: Critical / High / Medium / Low / Nitpick
4. Write final report in `docs/opus_review.md` with reasoning, code references, and fix suggestions
