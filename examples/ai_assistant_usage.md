# AI Assistant Usage Guide for fastcal

This guide demonstrates how AI assistants can effectively use fastcal to manage calendars on behalf of users.

## Table of Contents

- [Quick Start](#quick-start)
- [Common User Requests](#common-user-requests)
- [Workflow Patterns](#workflow-patterns)
- [Best Practices](#best-practices)
- [Error Handling](#error-handling)
- [Example Conversations](#example-conversations)

## Quick Start

### Setup Check

Before using fastcal, verify configuration:

```bash
# Test connection
fastcal config test

# List available calendars
fastcal calendars list --format json
```

### Basic Pattern

1. **Always use `--format json`** for programmatic parsing
2. **Use `jq`** to extract fields reliably
3. **Check exit codes** to detect errors
4. **Search before modify** to find event IDs

## Common User Requests

### 1. "Am I free tomorrow afternoon?"

**AI Process:**
```bash
#!/bin/bash
# Calculate tomorrow's date
TOMORROW=$(date -v+1d +%Y-%m-%d)

# List afternoon events (12 PM - 5 PM)
EVENTS=$(fastcal events list \
  --from "${TOMORROW}T12:00:00-08:00" \
  --to "${TOMORROW}T17:00:00-08:00" \
  --format json)

# Check event count
COUNT=$(echo "$EVENTS" | jq '.metadata.count')

if [ "$COUNT" -eq 0 ]; then
  echo "Yes, you're completely free tomorrow afternoon!"
else
  echo "You have $COUNT event(s) tomorrow afternoon:"
  echo "$EVENTS" | jq -r '.events[] | "  • \(.start | split("T")[1] | split("-")[0]) - \(.summary)"'
fi
```

### 2. "Schedule a meeting with John tomorrow at 2 PM for 1 hour"

**AI Process:**
```bash
#!/bin/bash
# Calculate tomorrow's date and time
TOMORROW=$(date -v+1d +%Y-%m-%d)
START_TIME="${TOMORROW}T14:00:00-08:00"
END_TIME="${TOMORROW}T15:00:00-08:00"

# Step 1: Check for conflicts
CONFLICTS=$(fastcal events conflicts \
  --start "$START_TIME" \
  --end "$END_TIME" \
  --format json)

HAS_CONFLICTS=$(echo "$CONFLICTS" | jq -r '.has_conflicts')

if [ "$HAS_CONFLICTS" = "true" ]; then
  echo "⚠️  You have a conflict at that time."
  exit 1
fi

# Step 2: Create the event
fastcal events create \
  --summary "Meeting with John" \
  --start "$START_TIME" \
  --duration 60 \
  --format json

echo "✓ Meeting scheduled for tomorrow at 2 PM"
```

### 3. "Find my dentist appointment"

**AI Process:**
```bash
#!/bin/bash
# Search for dentist-related events
RESULTS=$(fastcal events search "dentist" --format json)

COUNT=$(echo "$RESULTS" | jq '.metadata.count')

if [ "$COUNT" -eq 0 ]; then
  echo "I couldn't find any dentist appointments in your calendar."
else
  echo "Found $COUNT dentist appointment(s):"
  echo "$RESULTS" | jq -r '.events[] | "  • \(.summary) on \(.start | split("T")[0])"'
fi
```

### 4. "Move my 3 PM meeting to 4 PM"

**AI Process:**
```bash
#!/bin/bash
TODAY=$(date +%Y-%m-%d)

# Step 1: Find the 3 PM meeting
EVENTS=$(fastcal events list \
  --from "${TODAY}T15:00:00-08:00" \
  --to "${TODAY}T15:01:00-08:00" \
  --format json)

EVENT_ID=$(echo "$EVENTS" | jq -r '.events[0].id')

if [ "$EVENT_ID" = "null" ]; then
  echo "I couldn't find a meeting at 3 PM today."
  exit 1
fi

# Step 2: Update the event
fastcal events update "$EVENT_ID" \
  --start "${TODAY}T16:00:00-08:00" \
  --format json

echo "✓ Meeting moved to 4 PM"
```

## Workflow Patterns

### Pattern 1: Safe Event Creation

Always check for conflicts before creating:

```bash
check_and_create_event() {
  local summary="$1"
  local start="$2"
  local duration="$3"

  # Check conflicts
  if fastcal events conflicts --start "$start" --end "$end" --format json | jq -e '.has_conflicts'; then
    echo "Conflict detected."
    return 1
  fi

  # Create event
  fastcal events create \
    --summary "$summary" \
    --start "$start" \
    --duration "$duration" \
    --format json
}
```

### Pattern 2: Search and Modify

Always search to find event ID before updating:

```bash
find_and_update_event() {
  local search_term="$1"
  local new_start="$2"

  # Find event
  local event_id=$(fastcal events search "$search_term" --format json | jq -r '.events[0].id')

  if [ "$event_id" = "null" ]; then
    echo "Event not found: $search_term"
    return 1
  fi

  # Update event
  fastcal events update "$event_id" --start "$new_start" --format json
}
```

## Best Practices

### 1. Always Use JSON Format

```bash
# Good: Parseable output
fastcal events list --format json | jq '.events[].summary'

# Avoid: Text format is for humans
fastcal events list --format text
```

### 2. Handle Errors Gracefully

```bash
# Check exit codes
if fastcal events create --summary "Test" --start "2026-03-10T10:00:00-08:00" --duration 30; then
  echo "Event created successfully"
else
  echo "Failed to create event"
fi
```

### 3. Use jq for Robust Parsing

```bash
# Good: Use jq for field extraction
EVENT_ID=$(fastcal events search "meeting" --format json | jq -r '.events[0].id')
```

## Error Handling

### Common Errors and Solutions

#### Authentication Error

```bash
Error: Failed to authenticate with CalDAV server
```

**Solution:**
```bash
echo "Please check your FASTCAL_PASSWORD environment variable."
```

#### Event Not Found

```bash
Error: Event 'abc-123' not found
```

**Solution:**
```bash
# Search for similar events
fastcal events search "$USER_QUERY" --format json
```

## Example Conversations

### Conversation 1: Schedule a Meeting

**User:** "Can you schedule a meeting with Sarah next Tuesday at 3 PM?"

**AI:**
```
Let me check your calendar for next Tuesday at 3 PM...

[Runs: fastcal events conflicts --start "2026-03-11T15:00:00-08:00" --end "2026-03-11T16:00:00-08:00"]

You're free at that time. I'll schedule the meeting.

[Runs: fastcal events create --summary "Meeting with Sarah" --start "2026-03-11T15:00:00-08:00" --duration 60]

✓ Meeting scheduled for Tuesday, March 11 at 3:00 PM.
```

### Conversation 2: Find and Reschedule

**User:** "When is my dentist appointment?"

**AI:**
```
[Runs: fastcal events search "dentist"]

Your dentist appointment is on March 15 at 2:00 PM.
```

**User:** "Can you move it to the 22nd at the same time?"

**AI:**
```
[Runs: fastcal events update EVENT_ID --start "2026-03-22T14:00:00-08:00"]

✓ Dentist appointment moved to March 22 at 2:00 PM.
```

## Summary

When using fastcal as an AI assistant:

1. **Use JSON format** for all programmatic operations
2. **Check for conflicts** before scheduling
3. **Search for events** before modifying or deleting
4. **Handle errors gracefully** and provide helpful feedback
5. **Use jq** for robust JSON parsing

With these patterns, AI assistants can reliably manage calendars through natural language commands.
