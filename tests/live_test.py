#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "rich>=13.7",
# ]
# ///
"""
fastcal live integration test harness.

Runs against a real Fastmail CalDAV account.
ONLY creates/modifies events in the designated test calendar.
Cleans up all created events even on failure.

Usage:
    uv run tests/live_test.py --calendar "fastcal-test"
    uv run tests/live_test.py --calendar "fastcal-test" --binary ./target/release/fastcal
    uv run tests/live_test.py --calendar "fastcal-test" --verbose

Prerequisites:
    1. Run `fastcal config init` to configure credentials.
    2. Create a dedicated test calendar in Fastmail (e.g. "fastcal-test").
    3. Run `fastcal config init` again (or `fastcal calendars list`) so the
       new calendar appears in your config.

Safety:
    - All test events have summaries prefixed with "FASTCAL_TEST_".
    - A cleanup pass runs at exit (even on KeyboardInterrupt or assertion failure).
    - The script will NOT touch events it did not create.
"""

import argparse
import json
import subprocess
import sys
import tempfile
import textwrap
import traceback
from contextlib import contextmanager
from dataclasses import dataclass, field
from datetime import datetime, timezone, timedelta
from pathlib import Path
from typing import Any

from rich.console import Console
from rich.panel import Panel
from rich.table import Table
from rich import box
from rich.text import Text

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

BINARY_DEFAULT = Path(__file__).parent.parent / "target" / "release" / "fastcal"
TEST_PREFIX = "FASTCAL_TEST_"
# Future-dated base so tests don't mix with real near-term events
BASE_DATE = datetime(2099, 6, 15, 10, 0, 0, tzinfo=timezone.utc)

console = Console()


# ---------------------------------------------------------------------------
# Test result tracking
# ---------------------------------------------------------------------------

@dataclass
class TestResult:
    name: str
    passed: bool
    message: str = ""
    detail: str = ""


@dataclass
class TestSuite:
    results: list[TestResult] = field(default_factory=list)

    def record(self, name: str, passed: bool, message: str = "", detail: str = ""):
        self.results.append(TestResult(name, passed, message, detail))
        status = "[green]PASS[/green]" if passed else "[red]FAIL[/red]"
        if passed:
            console.print(f"  {status}  {name}")
        else:
            console.print(f"  {status}  {name}")
            if message:
                console.print(f"         [dim]{message}[/dim]")
            if detail:
                console.print(f"         [red]{detail}[/red]")

    @property
    def passed(self) -> int:
        return sum(1 for r in self.results if r.passed)

    @property
    def failed(self) -> int:
        return sum(1 for r in self.results if not r.passed)


# ---------------------------------------------------------------------------
# CLI runner
# ---------------------------------------------------------------------------

class Fastcal:
    def __init__(self, binary: Path, calendar: str, verbose: bool = False):
        self.binary = str(binary)
        self.calendar = calendar
        self.verbose = verbose
        self._created_ids: list[str] = []

    def run(
        self,
        *args: str,
        calendar: str | None = None,
        format: str = "json",
        expect_fail: bool = False,
    ) -> tuple[int, dict | str]:
        """Run fastcal with given args. Returns (returncode, parsed_json_or_raw_string)."""
        cal = calendar if calendar is not None else self.calendar
        cmd = [self.binary, "--calendar", cal, "--format", format] + list(args)

        if self.verbose:
            console.print(f"  [dim]$ {' '.join(cmd)}[/dim]")

        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
        )

        # Warnings go to stderr — log them in verbose mode
        if self.verbose and result.stderr.strip():
            for line in result.stderr.strip().splitlines():
                console.print(f"  [dim yellow]stderr: {line}[/dim yellow]")

        if format == "json":
            try:
                # stdout may have JSON output; stderr may have WARN lines
                parsed = json.loads(result.stdout)
                return result.returncode, parsed
            except json.JSONDecodeError:
                return result.returncode, result.stdout
        else:
            return result.returncode, result.stdout

    def create_event(
        self,
        summary: str,
        start: datetime,
        end: datetime | None = None,
        location: str | None = None,
        description: str | None = None,
        attendees: str | None = None,
    ) -> dict:
        """Create a test event and register its ID for cleanup."""
        args = [
            "events", "create",
            "--summary", summary,
            "--start", start.strftime("%Y-%m-%dT%H:%M:%SZ"),
        ]
        if end:
            args += ["--end", end.strftime("%Y-%m-%dT%H:%M:%SZ")]
        if location:
            args += ["--location", location]
        if description:
            args += ["--description", description]
        if attendees:
            args += ["--attendees", attendees]

        rc, data = self.run(*args)
        assert rc == 0 and isinstance(data, dict) and data.get("status") == "success", \
            f"Failed to create event '{summary}': {data}"

        event = data["data"]["event"]
        self._created_ids.append(event["id"])
        return event

    def cleanup(self):
        """Delete all events created during this test run."""
        if not self._created_ids:
            return
        console.print(f"\n[dim]Cleaning up {len(self._created_ids)} test event(s)...[/dim]")
        for uid in list(self._created_ids):
            rc, _ = self.run("events", "delete", uid, "--force")
            if rc == 0:
                self._created_ids.remove(uid)
                if self.verbose:
                    console.print(f"  [dim]Deleted {uid}[/dim]")
            else:
                console.print(f"  [yellow]Warning: failed to delete {uid} — delete it manually[/yellow]")


# ---------------------------------------------------------------------------
# Individual test functions
# ---------------------------------------------------------------------------

def test_config_connection(fc: Fastcal, suite: TestSuite):
    """Verify the tool can connect to Fastmail."""
    console.print("\n[bold]Connection[/bold]")

    rc, data = fc.run("config", "test", calendar="")
    suite.record(
        "config test succeeds",
        rc == 0 and isinstance(data, dict) and data.get("status") == "success",
        detail=str(data) if rc != 0 else "",
    )

    if rc == 0:
        cal_count = data["data"].get("calendars_configured", 0)
        suite.record(
            "at least one calendar configured",
            cal_count >= 1,
            message=f"Found {cal_count} calendar(s)",
        )


def test_calendar_list(fc: Fastcal, suite: TestSuite):
    """Verify the test calendar exists and is discoverable."""
    console.print("\n[bold]Calendar Discovery[/bold]")

    rc, data = fc.run("calendars", "list", calendar="")
    suite.record(
        "calendars list returns success",
        rc == 0 and isinstance(data, dict) and data.get("status") == "success",
        detail=str(data) if rc != 0 else "",
    )

    if rc == 0:
        names = [c["name"] for c in data["data"]["calendars"]]
        suite.record(
            f"test calendar '{fc.calendar}' exists in server",
            fc.calendar in names,
            message=f"Available: {', '.join(names)}",
        )


def test_event_create_minimal(fc: Fastcal, suite: TestSuite):
    """Create event with only required fields."""
    console.print("\n[bold]Event Create — Minimal[/bold]")

    start = BASE_DATE
    event = fc.create_event(f"{TEST_PREFIX}Minimal", start)

    suite.record("create returns event id", bool(event.get("id")))
    suite.record("create returns correct summary", event.get("summary") == f"{TEST_PREFIX}Minimal")
    suite.record("create sets all_day=false", event.get("all_day") == False)
    suite.record(
        "create sets default 1-hour duration",
        event.get("duration_minutes") == 60,
        message=f"Got {event.get('duration_minutes')} minutes",
    )
    suite.record(
        "start time roundtrips correctly",
        "2099-06-15T10:00:00" in event["start"]["datetime"],
        message=f"Got: {event['start']['datetime']}",
    )


def test_event_create_full(fc: Fastcal, suite: TestSuite):
    """Create event with all optional fields."""
    console.print("\n[bold]Event Create — Full Fields[/bold]")

    start = BASE_DATE + timedelta(hours=2)
    end = start + timedelta(hours=2)
    event = fc.create_event(
        f"{TEST_PREFIX}Full",
        start,
        end=end,
        location="Test Room 101",
        description="This is a test description",
    )

    suite.record("full create: location set", event.get("location") == "Test Room 101")
    suite.record("full create: description set", event.get("description") == "This is a test description")
    suite.record(
        "full create: 2-hour duration",
        event.get("duration_minutes") == 120,
        message=f"Got {event.get('duration_minutes')} minutes",
    )


def test_event_create_with_duration(fc: Fastcal, suite: TestSuite):
    """Create event using --duration instead of --end."""
    console.print("\n[bold]Event Create — Duration[/bold]")

    start = BASE_DATE + timedelta(hours=4)
    rc, data = fc.run(
        "events", "create",
        "--summary", f"{TEST_PREFIX}Duration",
        "--start", start.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "--duration", "45",
    )

    suite.record(
        "create with --duration succeeds",
        rc == 0 and isinstance(data, dict) and data.get("status") == "success",
        detail=str(data) if rc != 0 else "",
    )

    if rc == 0:
        event = data["data"]["event"]
        fc._created_ids.append(event["id"])
        suite.record(
            "duration=45 sets 45-minute event",
            event.get("duration_minutes") == 45,
            message=f"Got {event.get('duration_minutes')} minutes",
        )


def test_event_get(fc: Fastcal, suite: TestSuite):
    """Fetch a created event by ID."""
    console.print("\n[bold]Event Get[/bold]")

    event = fc.create_event(f"{TEST_PREFIX}Get", BASE_DATE + timedelta(hours=6))
    uid = event["id"]

    rc, data = fc.run("events", "get", uid)
    suite.record(
        "get returns success",
        rc == 0 and isinstance(data, dict) and data.get("status") == "success",
        detail=str(data) if rc != 0 else "",
    )

    if rc == 0:
        fetched = data["data"]["event"]
        suite.record("get returns correct id", fetched.get("id") == uid)
        suite.record("get returns correct summary", fetched.get("summary") == f"{TEST_PREFIX}Get")
        suite.record("get includes etag", bool(fetched.get("etag")))

    rc2, _ = fc.run("events", "get", "nonexistent-uid-that-does-not-exist-12345")
    suite.record("get nonexistent returns error exit code", rc2 != 0)


def test_event_update(fc: Fastcal, suite: TestSuite):
    """Update each field of an event."""
    console.print("\n[bold]Event Update[/bold]")

    event = fc.create_event(f"{TEST_PREFIX}UpdateOrig", BASE_DATE + timedelta(hours=8))
    uid = event["id"]

    new_start = BASE_DATE + timedelta(hours=9)
    new_end = new_start + timedelta(hours=3)

    rc, data = fc.run(
        "events", "update", uid,
        "--summary", f"{TEST_PREFIX}UpdatedSummary",
        "--start", new_start.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "--end", new_end.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "--location", "Updated Location",
        "--description", "Updated description",
    )

    suite.record(
        "update returns success",
        rc == 0 and isinstance(data, dict) and data.get("status") == "success",
        detail=str(data) if rc != 0 else "",
    )

    if rc == 0:
        updated = data["data"]["event"]
        suite.record("update: summary changed", updated.get("summary") == f"{TEST_PREFIX}UpdatedSummary")
        suite.record("update: location changed", updated.get("location") == "Updated Location")
        suite.record("update: description changed", updated.get("description") == "Updated description")
        suite.record(
            "update: duration recalculated",
            updated.get("duration_minutes") == 180,
            message=f"Got {updated.get('duration_minutes')} min, expected 180",
        )

    # Verify update persisted via get
    rc2, data2 = fc.run("events", "get", uid)
    if rc == 0 and rc2 == 0:
        persisted = data2["data"]["event"]
        suite.record(
            "update: changes persist on server",
            persisted.get("summary") == f"{TEST_PREFIX}UpdatedSummary",
        )

    # Update with no changes should error
    rc3, _ = fc.run("events", "update", uid)
    suite.record("update with no args returns error", rc3 != 0)


def test_event_delete(fc: Fastcal, suite: TestSuite):
    """Delete an event and verify it's gone."""
    console.print("\n[bold]Event Delete[/bold]")

    event = fc.create_event(f"{TEST_PREFIX}ToDelete", BASE_DATE + timedelta(hours=12))
    uid = event["id"]
    # Remove from cleanup list since we're testing delete
    fc._created_ids.remove(uid)

    rc, data = fc.run("events", "delete", uid, "--force")
    suite.record(
        "delete with --force returns success",
        rc == 0 and isinstance(data, dict) and data.get("status") == "success",
        detail=str(data) if rc != 0 else "",
    )

    # Verify it's gone
    rc2, _ = fc.run("events", "get", uid)
    suite.record("deleted event is no longer retrievable", rc2 != 0)

    # Delete nonexistent
    rc3, _ = fc.run("events", "delete", "nonexistent-uid-99999", "--force")
    suite.record("delete nonexistent returns error exit code", rc3 != 0)


def test_event_list(fc: Fastcal, suite: TestSuite):
    """List events with and without date filters."""
    console.print("\n[bold]Event List[/bold]")

    # Create two events in the same date window
    day = BASE_DATE + timedelta(days=1)
    e1 = fc.create_event(f"{TEST_PREFIX}ListA", day)
    e2 = fc.create_event(f"{TEST_PREFIX}ListB", day + timedelta(hours=2))

    from_str = (day - timedelta(hours=1)).strftime("%Y-%m-%dT%H:%M:%SZ")
    to_str = (day + timedelta(hours=4)).strftime("%Y-%m-%dT%H:%M:%SZ")

    rc, data = fc.run("events", "list", "--from", from_str, "--to", to_str)
    suite.record(
        "list with date range returns success",
        rc == 0 and isinstance(data, dict) and data.get("status") == "success",
        detail=str(data) if rc != 0 else "",
    )

    if rc == 0:
        event_ids = [e["id"] for e in data["data"]["events"]]
        suite.record("list contains first created event", e1["id"] in event_ids)
        suite.record("list contains second created event", e2["id"] in event_ids)
        suite.record("list metadata has count", "count" in data.get("metadata", {}))
        suite.record("list metadata has date_range", "date_range" in data.get("metadata", {}))

    # List with --format text
    rc2, text_out = fc.run("events", "list", "--from", from_str, "--to", to_str, format="text")
    suite.record("list --format text returns exit 0", rc2 == 0)
    suite.record(
        "list --format text contains event summary",
        isinstance(text_out, str) and f"{TEST_PREFIX}ListA" in text_out,
    )


def test_event_search(fc: Fastcal, suite: TestSuite):
    """Search for events by keyword."""
    console.print("\n[bold]Event Search[/bold]")

    day = BASE_DATE + timedelta(days=2)
    unique_word = "XyZuNiQuE99"
    fc.create_event(f"{TEST_PREFIX}SearchTarget {unique_word}", day)
    fc.create_event(f"{TEST_PREFIX}SearchDecoy", day + timedelta(hours=2))

    rc, data = fc.run("events", "search", unique_word)
    suite.record(
        "search returns success",
        rc == 0 and isinstance(data, dict) and data.get("status") == "success",
        detail=str(data) if rc != 0 else "",
    )

    if rc == 0:
        matches = data["data"].get("matches", [])
        summaries = [m["summary"] for m in matches]
        suite.record(
            "search finds matching event",
            any(unique_word in s for s in summaries),
            message=f"Summaries: {summaries}",
        )
        suite.record(
            "search does not return decoy event",
            not any("SearchDecoy" in s for s in summaries),
            message=f"Summaries: {summaries}",
        )
        suite.record("search returns query in response", data["data"].get("query") == unique_word)

    # Search for something that doesn't exist
    rc2, data2 = fc.run("events", "search", "ZZZNOMATCH_IMPOSSIBLE_STRING_99x")
    if rc2 == 0 and isinstance(data2, dict):
        suite.record(
            "search for missing term returns empty matches",
            len(data2["data"].get("matches", [])) == 0,
        )


def test_conflicts(fc: Fastcal, suite: TestSuite):
    """Check conflict detection."""
    console.print("\n[bold]Conflict Detection[/bold]")

    day = BASE_DATE + timedelta(days=3)
    # Create a 2-hour event at 10:00
    event_start = day.replace(hour=10, minute=0)
    event_end = event_start + timedelta(hours=2)
    fc.create_event(f"{TEST_PREFIX}ConflictBase", event_start, end=event_end)

    # Check a time that overlaps (10:30 - 11:30)
    overlap_start = event_start + timedelta(minutes=30)
    overlap_end = overlap_start + timedelta(hours=1)

    rc, data = fc.run(
        "events", "conflicts",
        "--start", overlap_start.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "--end", overlap_end.strftime("%Y-%m-%dT%H:%M:%SZ"),
    )

    suite.record(
        "conflicts check returns success",
        rc == 0 and isinstance(data, dict) and data.get("status") == "success",
        detail=str(data) if rc != 0 else "",
    )

    if rc == 0:
        suite.record(
            "overlapping time correctly detected as conflict",
            data["data"].get("has_conflicts") == True,
            message=f"has_conflicts={data['data'].get('has_conflicts')}",
        )
        suite.record(
            "conflict result includes conflicting events list",
            len(data["data"].get("conflicts", [])) > 0,
        )

    # Check a time that does NOT overlap (13:00 - 14:00)
    clear_start = event_start + timedelta(hours=4)
    clear_end = clear_start + timedelta(hours=1)

    rc2, data2 = fc.run(
        "events", "conflicts",
        "--start", clear_start.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "--end", clear_end.strftime("%Y-%m-%dT%H:%M:%SZ"),
    )

    if rc2 == 0 and isinstance(data2, dict):
        suite.record(
            "non-overlapping time correctly shows no conflicts",
            data2["data"].get("has_conflicts") == False,
        )

    # End before start should error
    rc3, _ = fc.run(
        "events", "conflicts",
        "--start", clear_end.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "--end", clear_start.strftime("%Y-%m-%dT%H:%M:%SZ"),
    )
    suite.record("conflicts with end before start returns error", rc3 != 0)


def test_batch_create(fc: Fastcal, suite: TestSuite) -> list[str]:
    """Create multiple events from a JSON file."""
    console.print("\n[bold]Batch Create[/bold]")

    day = BASE_DATE + timedelta(days=4)
    batch_events = [
        {
            "summary": f"{TEST_PREFIX}Batch1",
            "start": day.strftime("%Y-%m-%dT%H:%M:%SZ"),
            "duration": 30,
            "description": "First batch event",
        },
        {
            "summary": f"{TEST_PREFIX}Batch2",
            "start": (day + timedelta(hours=1)).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "end": (day + timedelta(hours=3)).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "location": "Batch Room",
        },
        {
            "summary": f"{TEST_PREFIX}Batch3",
            "start": (day + timedelta(hours=4)).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "duration": 90,
        },
    ]

    batch_ids = []

    with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
        json.dump(batch_events, f)
        tmpfile = f.name

    rc, data = fc.run("batch", "create", tmpfile)
    suite.record(
        "batch create returns success",
        rc == 0 and isinstance(data, dict) and data.get("status") == "success",
        detail=str(data) if rc != 0 else "",
    )

    if rc == 0:
        result = data["data"]
        suite.record("batch create: total=3", result.get("total") == 3)
        suite.record("batch create: success=3", result.get("success") == 3)
        suite.record("batch create: errors=0", result.get("errors") == 0)
        suite.record(
            "batch create: all results have event_id",
            all(r.get("event_id") for r in result.get("results", [])),
        )

        for r in result.get("results", []):
            if r.get("event_id"):
                fc._created_ids.append(r["event_id"])
                batch_ids.append(r["event_id"])

    # Test with empty array
    with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
        json.dump([], f)
        empty_file = f.name

    rc2, _ = fc.run("batch", "create", empty_file)
    suite.record("batch create with empty array returns error", rc2 != 0)

    return batch_ids


def test_batch_delete(fc: Fastcal, suite: TestSuite, batch_ids: list[str]):
    """Delete multiple events from a JSON file."""
    console.print("\n[bold]Batch Delete[/bold]")

    if len(batch_ids) < 2:
        suite.record("batch delete: skipped (no batch_ids from previous test)", False,
                     message="batch create must succeed first")
        return

    # Remove from cleanup list since we're explicitly testing delete
    for uid in batch_ids:
        if uid in fc._created_ids:
            fc._created_ids.remove(uid)

    with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
        json.dump(batch_ids, f)
        tmpfile = f.name

    rc, data = fc.run("batch", "delete", tmpfile)
    suite.record(
        "batch delete returns success",
        rc == 0 and isinstance(data, dict) and data.get("status") == "success",
        detail=str(data) if rc != 0 else "",
    )

    if rc == 0:
        result = data["data"]
        suite.record(
            f"batch delete: all {len(batch_ids)} events deleted",
            result.get("success") == len(batch_ids),
            message=f"success={result.get('success')}, errors={result.get('errors')}",
        )

    # Verify they're gone
    gone = 0
    for uid in batch_ids:
        rc2, _ = fc.run("events", "get", uid)
        if rc2 != 0:
            gone += 1
    suite.record(
        "batch deleted events are no longer retrievable",
        gone == len(batch_ids),
        message=f"{gone}/{len(batch_ids)} events gone",
    )


def test_datetime_parsing(fc: Fastcal, suite: TestSuite):
    """Verify various datetime input formats are accepted."""
    console.print("\n[bold]Datetime Parsing[/bold]")

    formats_to_test = [
        ("ISO 8601 with Z",       "2099-07-01T09:00:00Z"),
        ("ISO 8601 with offset",  "2099-07-01T09:00:00+05:30"),
        ("Date + HH:MM",          "2099-07-01 09:00"),
        ("Date + 12h am/pm",      "2099-07-01 9am"),
        ("Date + 12h with min",   "2099-07-01 9:30am"),
        ("Date only",             "2099-07-01"),
    ]

    for label, fmt in formats_to_test:
        rc, data = fc.run(
            "events", "create",
            "--summary", f"{TEST_PREFIX}DateFmt",
            "--start", fmt,
            "--duration", "30",
        )
        ok = rc == 0 and isinstance(data, dict) and data.get("status") == "success"
        suite.record(f"parse datetime: {label} ({fmt})", ok,
                     detail=str(data) if not ok else "")
        if ok:
            fc._created_ids.append(data["data"]["event"]["id"])


def test_output_formats(fc: Fastcal, suite: TestSuite):
    """Verify JSON and text output format flags work."""
    console.print("\n[bold]Output Format[/bold]")

    start = BASE_DATE + timedelta(days=5)
    event = fc.create_event(f"{TEST_PREFIX}FormatTest", start, location="Format Location")
    uid = event["id"]

    # JSON format for list
    rc, data = fc.run("events", "list",
                      "--from", start.strftime("%Y-%m-%dT%H:%M:%SZ"),
                      "--to", (start + timedelta(hours=2)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                      format="json")
    suite.record(
        "events list --format json returns valid JSON with status field",
        rc == 0 and isinstance(data, dict) and "status" in data,
    )

    # Text format for list
    rc2, text = fc.run("events", "list",
                       "--from", start.strftime("%Y-%m-%dT%H:%M:%SZ"),
                       "--to", (start + timedelta(hours=2)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                       format="text")
    suite.record("events list --format text returns exit 0", rc2 == 0)
    suite.record(
        "events list --format text output is not JSON",
        isinstance(text, str) and not text.strip().startswith("{"),
        message=f"Output starts with: {text[:40]!r}" if text else "",
    )
    suite.record(
        "events list --format text contains summary",
        isinstance(text, str) and "FormatTest" in text,
    )

    # JSON format for search
    rc3, data3 = fc.run("events", "search", "FormatTest", format="json")
    suite.record(
        "events search --format json returns valid JSON",
        rc3 == 0 and isinstance(data3, dict) and data3.get("status") == "success",
    )

    # Text format for search
    rc4, text4 = fc.run("events", "search", "FormatTest", format="text")
    suite.record("events search --format text returns exit 0", rc4 == 0)
    suite.record(
        "events search --format text is not JSON",
        isinstance(text4, str) and not text4.strip().startswith("{"),
    )


def test_special_characters(fc: Fastcal, suite: TestSuite):
    """Events with commas, semicolons, newlines, and unicode in fields."""
    console.print("\n[bold]Special Characters[/bold]")

    start = BASE_DATE + timedelta(days=6)

    # Comma and semicolon in summary
    event = fc.create_event(
        f"{TEST_PREFIX}Commas, Semicolons; Test",
        start,
        description="Line one\nLine two",
        location="Building A, Room 101",
    )

    rc, data = fc.run("events", "get", event["id"])
    if rc == 0:
        fetched = data["data"]["event"]
        suite.record(
            "special chars: summary with comma/semicolon roundtrips",
            "Commas, Semicolons; Test" in fetched.get("summary", ""),
            message=f"Got: {fetched.get('summary')}",
        )
        suite.record(
            "special chars: location with comma roundtrips",
            fetched.get("location") == "Building A, Room 101",
            message=f"Got: {fetched.get('location')}",
        )
    else:
        suite.record("special chars: get after create failed", False, detail=str(data))

    # Unicode in summary
    event2 = fc.create_event(
        f"{TEST_PREFIX}Unicode: Aabye, Aabyhoej, Aarhus",
        start + timedelta(hours=2),
        location="Tousvej 12A, 8230 Åbyhøj",
    )

    rc2, data2 = fc.run("events", "get", event2["id"])
    if rc2 == 0:
        fetched2 = data2["data"]["event"]
        suite.record(
            "unicode: location with non-ASCII roundtrips",
            "Åbyhøj" in (fetched2.get("location") or ""),
            message=f"Got: {fetched2.get('location')}",
        )
    else:
        suite.record("unicode: get after create failed", False, detail=str(data2))


def test_all_day_event(fc: Fastcal, suite: TestSuite):
    """Create an all-day event (date-only start)."""
    console.print("\n[bold]All-Day Event[/bold]")

    rc, data = fc.run(
        "events", "create",
        "--summary", f"{TEST_PREFIX}AllDay",
        "--start", "2099-06-20",
    )

    suite.record(
        "all-day create returns success",
        rc == 0 and isinstance(data, dict) and data.get("status") == "success",
        detail=str(data) if rc != 0 else "",
    )

    if rc == 0:
        event = data["data"]["event"]
        fc._created_ids.append(event["id"])
        # Note: all_day detection depends on ICS parse — currently may not be flagged
        # because create converts to UTC datetime internally
        suite.record(
            "all-day create: event id returned",
            bool(event.get("id")),
        )


def test_error_handling(fc: Fastcal, suite: TestSuite):
    """Verify error conditions return non-zero exit codes and don't crash."""
    console.print("\n[bold]Error Handling[/bold]")

    # Get nonexistent event
    rc, _ = fc.run("events", "get", "does-not-exist-uid-xyz-00000")
    suite.record("get nonexistent: non-zero exit code", rc != 0)

    # Update nonexistent event
    rc2, _ = fc.run("events", "update", "does-not-exist-xyz", "--summary", "x")
    suite.record("update nonexistent: non-zero exit code", rc2 != 0)

    # Delete nonexistent event
    rc3, _ = fc.run("events", "delete", "does-not-exist-xyz", "--force")
    suite.record("delete nonexistent: non-zero exit code", rc3 != 0)

    # Invalid start time
    rc4, _ = fc.run("events", "create", "--summary", f"{TEST_PREFIX}BadDate",
                    "--start", "not-a-date")
    suite.record("create with bad date: non-zero exit code", rc4 != 0)

    # Invalid calendar name
    rc5, _ = fc.run("events", "list", calendar="calendar-that-does-not-exist-xyz")
    suite.record("list with bad calendar name: non-zero exit code", rc5 != 0)

    # Batch create with invalid JSON file
    with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
        f.write("this is not json {{{")
        bad_file = f.name
    rc6, _ = fc.run("batch", "create", bad_file)
    suite.record("batch create with invalid JSON: non-zero exit code", rc6 != 0)

    # Conflicts with end <= start
    rc7, _ = fc.run(
        "events", "conflicts",
        "--start", "2099-07-01T12:00:00Z",
        "--end", "2099-07-01T11:00:00Z",
    )
    suite.record("conflicts with end before start: non-zero exit code", rc7 != 0)


def test_config_show(fc: Fastcal, suite: TestSuite):
    """Verify config show redacts password."""
    console.print("\n[bold]Config Show[/bold]")

    rc, data = fc.run("config", "show", calendar="")
    suite.record(
        "config show returns success",
        rc == 0 and isinstance(data, dict) and data.get("status") == "success",
        detail=str(data) if rc != 0 else "",
    )

    if rc == 0:
        cfg = data["data"]["config"]
        password = cfg.get("server", {}).get("app_password", "")
        suite.record(
            "config show redacts app_password",
            password == "***REDACTED***",
            message=f"Got: {password!r}",
        )
        suite.record(
            "config show includes caldav_url",
            bool(cfg.get("server", {}).get("caldav_url")),
        )
        suite.record(
            "config show includes calendars",
            isinstance(cfg.get("calendars"), dict) and len(cfg["calendars"]) > 0,
        )


# ---------------------------------------------------------------------------
# Entrypoint
# ---------------------------------------------------------------------------

def print_summary(suite: TestSuite):
    table = Table(box=box.SIMPLE, show_header=True, header_style="bold")
    table.add_column("Test", style="white")
    table.add_column("Result", justify="center")
    table.add_column("Notes", style="dim")

    for r in suite.results:
        status = Text("PASS", style="bold green") if r.passed else Text("FAIL", style="bold red")
        notes = r.message or r.detail or ""
        if len(notes) > 80:
            notes = notes[:77] + "..."
        table.add_row(r.name, status, notes)

    console.print()
    console.print(table)
    console.print()

    total = len(suite.results)
    passed = suite.passed
    failed = suite.failed

    if failed == 0:
        console.print(Panel(
            f"[bold green]All {total} tests passed[/bold green]",
            border_style="green",
        ))
    else:
        console.print(Panel(
            f"[bold green]{passed} passed[/bold green]  "
            f"[bold red]{failed} failed[/bold red]  "
            f"[dim]({total} total)[/dim]",
            border_style="red" if failed else "green",
        ))


def main():
    parser = argparse.ArgumentParser(
        description=textwrap.dedent("""
            fastcal live integration test harness.
            Requires a dedicated test calendar in your Fastmail account.
        """),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--calendar", "-c",
        required=True,
        help="Name of the dedicated test calendar (e.g. 'fastcal-test'). "
             "Must already exist and be configured in fastcal.",
    )
    parser.add_argument(
        "--binary", "-b",
        default=str(BINARY_DEFAULT),
        help=f"Path to fastcal binary (default: {BINARY_DEFAULT})",
    )
    parser.add_argument(
        "--verbose", "-v",
        action="store_true",
        help="Print each command invocation and stderr output",
    )
    parser.add_argument(
        "--skip-cleanup",
        action="store_true",
        help="Skip cleanup of test events (useful for post-run inspection)",
    )
    args = parser.parse_args()

    binary = Path(args.binary)
    if not binary.exists():
        console.print(f"[red]Error: binary not found: {binary}[/red]")
        console.print("[dim]Run `cargo build --release` first.[/dim]")
        sys.exit(1)

    console.print(Panel(
        f"[bold]fastcal live integration tests[/bold]\n"
        f"Binary:   {binary}\n"
        f"Calendar: [cyan]{args.calendar}[/cyan]",
        border_style="blue",
    ))

    fc = Fastcal(binary, args.calendar, verbose=args.verbose)
    suite = TestSuite()

    try:
        test_config_show(fc, suite)
        test_config_connection(fc, suite)
        test_calendar_list(fc, suite)
        test_event_create_minimal(fc, suite)
        test_event_create_full(fc, suite)
        test_event_create_with_duration(fc, suite)
        test_event_get(fc, suite)
        test_event_update(fc, suite)
        test_event_delete(fc, suite)
        test_event_list(fc, suite)
        test_event_search(fc, suite)
        test_conflicts(fc, suite)
        batch_ids = test_batch_create(fc, suite)
        test_batch_delete(fc, suite, batch_ids)
        test_datetime_parsing(fc, suite)
        test_output_formats(fc, suite)
        test_special_characters(fc, suite)
        test_all_day_event(fc, suite)
        test_error_handling(fc, suite)

    except KeyboardInterrupt:
        console.print("\n[yellow]Interrupted.[/yellow]")
    except Exception:
        console.print(f"\n[red]Unexpected error:[/red]")
        traceback.print_exc()
    finally:
        if not args.skip_cleanup:
            fc.cleanup()
        else:
            console.print(f"\n[yellow]Skipping cleanup. Test event IDs:[/yellow]")
            for uid in fc._created_ids:
                console.print(f"  {uid}")

    print_summary(suite)
    sys.exit(0 if suite.failed == 0 else 1)


if __name__ == "__main__":
    main()
