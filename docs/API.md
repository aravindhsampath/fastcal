# fastcal API Documentation

This document describes the JSON schemas and API patterns for fastcal CLI commands.

## Table of Contents

- [Output Formats](#output-formats)
- [Event Schema](#event-schema)
- [Command Responses](#command-responses)
- [Batch Operations](#batch-operations)
- [Error Handling](#error-handling)
- [Examples](#examples)

## Output Formats

fastcal supports three output formats via the `--format` flag:

- **json** (default): Structured JSON output, ideal for programmatic parsing
- **text**: Human-readable text output with emoji indicators
- **ics**: Raw iCalendar format (not yet implemented)

```bash
# JSON output (default)
fastcal events list

# Human-readable text
fastcal events list --format text

# Specify format globally
fastcal --format json events list
```

## Event Schema

### Event Object

The core event object used throughout the API:

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "href": "https://caldav.fastmail.com/dav/calendars/user/user@fastmail.com/personal/550e8400.ics",
  "calendar": "personal",
  "summary": "Team Meeting",
  "description": "Weekly sync meeting with the team",
  "start": {"datetime": "2026-03-05T18:00:00Z", "timezone": "America/Los_Angeles"},
  "end": {"datetime": "2026-03-05T19:00:00Z", "timezone": "America/Los_Angeles"},
  "duration_minutes": 60,
  "location": "Conference Room A",
  "attendees": [],
  "status": "CONFIRMED",
  "etag": "\"abc123def456\""
}
```

#### Field Descriptions

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | Yes | Unique event identifier (UID in iCalendar) |
| `href` | string | Yes | CalDAV resource URL |
| `calendar` | string | No | Calendar name (e.g., "personal", "work") |
| `summary` | string | Yes | Event title/summary |
| `description` | string | No | Detailed event description |
| `start` | object | Yes | EventDateTime: `{"datetime": "<ISO 8601 UTC>", "timezone": "<IANA tz>"}` |
| `end` | object | No | EventDateTime: `{"datetime": "<ISO 8601 UTC>", "timezone": "<IANA tz>"}` |
| `duration_minutes` | integer | No | Event duration in minutes |
| `location` | string | No | Event location |
| `attendees` | array | No | List of attendee objects (future) |
| `status` | string | No | Event status (CONFIRMED, TENTATIVE, CANCELLED) |
| `etag` | string | No | ETag for optimistic concurrency control |

### DateTime Format

Event `start` and `end` fields are **objects**, not flat strings:

```json
{
  "datetime": "2026-03-05T22:00:00Z",
  "timezone": "America/Los_Angeles"
}
```

- `datetime`: ISO 8601 UTC string
- `timezone`: IANA timezone name (may be `null` for UTC events)

When **creating** or **updating** events via CLI flags (`--start`, `--end`), use flat strings:
- `2026-03-05T14:00:00-08:00` (offset-aware)
- `2026-03-05 2pm` (natural format, treated as UTC)
- `2026-03-05` (date only, defaults to 00:00 UTC)

## Command Responses

### List Events

**Command:**
```bash
fastcal events list --from 2026-03-05 --to 2026-03-10 --format json
```

**Response:**
```json
{
  "events": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "href": "https://caldav.fastmail.com/.../550e8400.ics",
      "calendar": "personal",
      "summary": "Team Meeting",
      "start": {"datetime": "2026-03-05T18:00:00Z", "timezone": "America/Los_Angeles"},
      "end": {"datetime": "2026-03-05T19:00:00Z", "timezone": "America/Los_Angeles"},
      "duration_minutes": 60,
      "location": "Conference Room A"
    }
  ],
  "metadata": {
    "count": 1,
    "calendar": "personal",
    "from": "2026-03-05",
    "to": "2026-03-10"
  }
}
```

### Create Event

**Command:**
```bash
fastcal events create \
  --summary "Doctor Appointment" \
  --start "2026-03-15T14:00:00-08:00" \
  --duration 30 \
  --location "Medical Center" \
  --format json
```

**Response:**
```json
{
  "event": {
    "id": "7c9e6679-7425-40de-944b-e07fc1f90ae7",
    "href": "https://caldav.fastmail.com/.../7c9e6679.ics",
    "calendar": "personal",
    "summary": "Doctor Appointment",
    "start": "2026-03-15T14:00:00-08:00",
    "end": "2026-03-15T14:30:00-08:00",
    "duration_minutes": 30,
    "location": "Medical Center",
    "status": "CONFIRMED"
  },
  "metadata": {
    "created": true,
    "calendar": "personal"
  }
}
```

### Update Event

**Command:**
```bash
fastcal events update 7c9e6679-7425-40de-944b-e07fc1f90ae7 \
  --start "2026-03-15T15:00:00-08:00" \
  --summary "Doctor Appointment (Rescheduled)" \
  --format json
```

**Response:**
```json
{
  "event": {
    "id": "7c9e6679-7425-40de-944b-e07fc1f90ae7",
    "href": "https://caldav.fastmail.com/.../7c9e6679.ics",
    "calendar": "personal",
    "summary": "Doctor Appointment (Rescheduled)",
    "start": "2026-03-15T15:00:00-08:00",
    "end": "2026-03-15T15:30:00-08:00",
    "duration_minutes": 30,
    "location": "Medical Center"
  },
  "metadata": {
    "updated": true,
    "calendar": "personal"
  }
}
```

### Delete Event

**Command:**
```bash
fastcal events delete 7c9e6679-7425-40de-944b-e07fc1f90ae7 --force
```

**Response (text):**
```
Event deleted successfully
```

### Search Events

**Command:**
```bash
fastcal events search "meeting" --format json
```

**Response:**
```json
{
  "events": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "summary": "Team Meeting",
      "start": "2026-03-05T10:00:00-08:00",
      "calendar": "personal"
    },
    {
      "id": "661f9511-f3ac-52e5-b827-557766551111",
      "summary": "Client Meeting",
      "start": "2026-03-06T14:00:00-08:00",
      "calendar": "work"
    }
  ],
  "metadata": {
    "count": 2,
    "query": "meeting"
  }
}
```

### Check Conflicts

**Command:**
```bash
fastcal events conflicts \
  --start "2026-03-05T10:30:00-08:00" \
  --end "2026-03-05T11:30:00-08:00" \
  --format json
```

**Response:**
```json
{
  "has_conflicts": true,
  "conflicts": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "summary": "Team Meeting",
      "start": "2026-03-05T10:00:00-08:00",
      "end": "2026-03-05T11:00:00-08:00",
      "calendar": "personal"
    }
  ],
  "metadata": {
    "count": 1,
    "proposed_start": "2026-03-05T10:30:00-08:00",
    "proposed_end": "2026-03-05T11:30:00-08:00"
  }
}
```

## Batch Operations

### Batch Create

Create multiple events from a JSON file.

**Input File** (`events.json`):
```json
[
  {
    "summary": "Morning Standup",
    "start": "2026-03-05T09:00:00-08:00",
    "duration": 15,
    "description": "Daily team sync"
  },
  {
    "summary": "Project Review",
    "start": "2026-03-05T14:00:00-08:00",
    "end": "2026-03-05T15:30:00-08:00",
    "location": "Room 301"
  }
]
```

**Command:**
```bash
fastcal batch create events.json --format json
```

**Response:**
```json
{
  "results": [
    {
      "success": true,
      "event": {
        "id": "abc-123",
        "summary": "Morning Standup",
        "start": "2026-03-05T09:00:00-08:00"
      }
    },
    {
      "success": true,
      "event": {
        "id": "def-456",
        "summary": "Project Review",
        "start": "2026-03-05T14:00:00-08:00"
      }
    }
  ],
  "metadata": {
    "total": 2,
    "successful": 2,
    "failed": 0
  }
}
```

### Batch Delete

Delete multiple events by their IDs.

**Input File** (`delete.json`):
```json
[
  "abc-123",
  "def-456",
  "ghi-789"
]
```

**Command:**
```bash
fastcal batch delete delete.json --format json
```

**Response:**
```json
{
  "results": [
    {
      "event_id": "abc-123",
      "success": true
    },
    {
      "event_id": "def-456",
      "success": true
    },
    {
      "event_id": "ghi-789",
      "success": false,
      "error": "Event not found"
    }
  ],
  "metadata": {
    "total": 3,
    "successful": 2,
    "failed": 1
  }
}
```

## Error Handling

fastcal uses standard error messages and exit codes for robust error handling.

### Error Response Format

While fastcal uses `anyhow` for error handling and outputs human-readable error messages to stderr, errors follow predictable patterns:

**Exit Codes:**
- `0`: Success
- `1`: General error (invalid input, command failure)
- `2`: Authentication error
- `3`: Network error
- `4`: Resource not found

**Error Output** (stderr):
```
Error: Calendar 'nonexistent' not found in config

Available calendars:
  - personal
  - work
  - family
```

### Common Error Scenarios

#### Authentication Failure

```bash
$ fastcal events list
Error: Failed to authenticate with CalDAV server

Caused by:
    HTTP 401 Unauthorized

Suggestion: Check your FASTCAL_PASSWORD environment variable or
the app_password in ~/.config/fastcal/config.toml
```

#### Calendar Not Found

```bash
$ fastcal events list --calendar invalid
Error: Calendar 'invalid' not found in config

Available calendars:
  - personal
  - work
```

#### Invalid Date Format

```bash
$ fastcal events create --summary "Test" --start "invalid-date"
Error: Failed to parse datetime: invalid-date

Expected format: YYYY-MM-DDTHH:MM:SS±HH:MM
Example: 2026-03-05T14:00:00-08:00
```

#### Event Not Found

```bash
$ fastcal events delete nonexistent-id
Error: Event 'nonexistent-id' not found in any calendar

Searched calendars: personal, work, family
```

## Examples

### Example 1: Create a Simple Event

```bash
# Create a 30-minute meeting tomorrow at 2 PM
fastcal events create \
  --summary "Quick Sync" \
  --start "2026-03-06T14:00:00-08:00" \
  --duration 30 \
  --calendar personal
```

### Example 2: Find Available Time Slots

```bash
# Check if 3-4 PM is free
fastcal events conflicts \
  --start "2026-03-05T15:00:00-08:00" \
  --end "2026-03-05T16:00:00-08:00" \
  --format json

# If conflicts found, try another time
fastcal events conflicts \
  --start "2026-03-05T16:00:00-08:00" \
  --end "2026-03-05T17:00:00-08:00" \
  --format json
```

### Example 3: Update Event Time

```bash
# Get event ID from search
EVENT_ID=$(fastcal events search "Doctor" --format json | jq -r '.events[0].id')

# Reschedule to next week
fastcal events update "$EVENT_ID" \
  --start "2026-03-12T14:00:00-08:00"
```

### Example 4: Batch Create Weekly Meetings

Create a JSON file with recurring meeting pattern:

```json
{
  "events": [
    {
      "summary": "Monday Standup",
      "start": "2026-03-03T09:00:00-08:00",
      "duration": 15
    },
    {
      "summary": "Monday Standup",
      "start": "2026-03-10T09:00:00-08:00",
      "duration": 15
    },
    {
      "summary": "Monday Standup",
      "start": "2026-03-17T09:00:00-08:00",
      "duration": 15
    },
    {
      "summary": "Monday Standup",
      "start": "2026-03-24T09:00:00-08:00",
      "duration": 15
    }
  ],
  "calendar": "work"
}
```

```bash
fastcal batch create weekly-standups.json
```

### Example 5: AI Assistant Integration

An AI assistant can use fastcal programmatically:

**User:** "Am I free tomorrow afternoon?"

**AI Process:**
```bash
# 1. Get tomorrow's date (March 6, 2026)
# 2. List events in afternoon timeframe
EVENTS=$(fastcal events list \
  --from "2026-03-06T12:00:00-08:00" \
  --to "2026-03-06T17:00:00-08:00" \
  --format json)

# 3. Parse JSON and analyze availability
echo "$EVENTS" | jq '.events[] | "\(.start) - \(.summary)"'
```

**AI Response:** "You have 2 meetings tomorrow afternoon:
- 2:00 PM - Client Call
- 4:00 PM - Team Sync

You're free from 12:00-2:00 PM and 3:00-4:00 PM."

### Example 6: Export Events to JSON

```bash
# Export all events for March 2026
fastcal events list \
  --from "2026-03-01" \
  --to "2026-03-31" \
  --format json > march_events.json

# Process with jq to extract specific fields
cat march_events.json | jq '.events[] | {summary, start, location}'
```

### Example 7: Human-Readable Output

```bash
# View today's events in text format
fastcal events list \
  --from "2026-03-05" \
  --to "2026-03-05" \
  --format text
```

Output:
```
📅 Team Meeting
   2026-03-05 10:00:00 PST
   📍 Conference Room A
   ⏱️  60 min
   ID: 550e8400-e29b-41d4-a716-446655440000

📅 Lunch with Client
   2026-03-05 12:30:00 PST
   📍 Downtown Cafe
   ⏱️  90 min
   ID: 661f9511-f3ac-52e5-b827-557766551111
```

## AI Integration Tips

### Best Practices for AI Assistants

1. **Always use JSON format** for programmatic parsing:
   ```bash
   fastcal --format json events list
   ```

2. **Parse with jq** for reliable field extraction:
   ```bash
   EVENT_COUNT=$(fastcal events list --format json | jq '.metadata.count')
   ```

3. **Handle errors gracefully** by checking exit codes:
   ```bash
   if fastcal events create --summary "Test" --start "2026-03-05T10:00:00-08:00"; then
     echo "Event created successfully"
   else
     echo "Failed to create event"
   fi
   ```

4. **Use search before update/delete** to find event IDs:
   ```bash
   # Find event ID
   ID=$(fastcal events search "meeting" --format json | jq -r '.events[0].id')

   # Update the event
   fastcal events update "$ID" --start "2026-03-06T14:00:00-08:00"
   ```

5. **Check for conflicts** before scheduling:
   ```bash
   # Check if time slot is available
   CONFLICTS=$(fastcal events conflicts \
     --start "2026-03-05T14:00:00-08:00" \
     --end "2026-03-05T15:00:00-08:00" \
     --format json)

   if [ "$(echo "$CONFLICTS" | jq '.has_conflicts')" = "true" ]; then
     echo "Time slot is busy"
   else
     echo "Time slot is available"
   fi
   ```

### Common AI Workflows

#### Workflow 1: Schedule a Meeting

```bash
# 1. Check for conflicts
fastcal events conflicts \
  --start "2026-03-05T14:00:00-08:00" \
  --end "2026-03-05T15:00:00-08:00" \
  --format json

# 2. If no conflicts, create event
fastcal events create \
  --summary "Team Sync" \
  --start "2026-03-05T14:00:00-08:00" \
  --duration 60 \
  --location "Zoom" \
  --format json
```

#### Workflow 2: Reschedule an Event

```bash
# 1. Find the event
EVENT=$(fastcal events search "dentist" --format json)
EVENT_ID=$(echo "$EVENT" | jq -r '.events[0].id')

# 2. Update the start time
fastcal events update "$EVENT_ID" \
  --start "2026-03-12T10:00:00-08:00" \
  --format json
```

#### Workflow 3: Check Daily Schedule

```bash
# Get today's events
TODAY=$(date +%Y-%m-%d)
fastcal events list \
  --from "$TODAY" \
  --to "$TODAY" \
  --format json | jq '.events[] | "\(.start) - \(.summary)"'
```

## Appendix

### Datetime Parsing Reference

fastcal accepts ISO 8601 datetime strings with timezone information:

**Valid formats:**
```
2026-03-05T14:00:00-08:00    # Full datetime with UTC offset
2026-03-05T14:00:00Z          # UTC time (Z suffix)
2026-03-05T14:00:00-05:00    # Eastern Time
```

**Timezone abbreviations are not supported** - use UTC offset instead:
```
✗ 2026-03-05T14:00:00 PST     # Not supported
✓ 2026-03-05T14:00:00-08:00   # Correct
```

### Duration vs End Time

You can specify event duration in two ways:

1. **Using --end flag:**
   ```bash
   fastcal events create \
     --summary "Meeting" \
     --start "2026-03-05T14:00:00-08:00" \
     --end "2026-03-05T15:30:00-08:00"
   ```

2. **Using --duration flag (in minutes):**
   ```bash
   fastcal events create \
     --summary "Meeting" \
     --start "2026-03-05T14:00:00-08:00" \
     --duration 90
   ```

If both are provided, `--end` takes precedence.

### Calendar Selection

Specify target calendar in three ways:

1. **Command flag:**
   ```bash
   fastcal events list --calendar work
   ```

2. **Global flag:**
   ```bash
   fastcal --calendar work events list
   ```

3. **Default calendar** (from config):
   ```toml
   [preferences]
   default_calendar = "personal"
   ```

Priority: command flag > global flag > config default
