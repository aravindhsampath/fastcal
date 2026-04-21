#!/bin/bash
# Example: Create various types of events with fastcal

set -e  # Exit on error

echo "=== fastcal Event Creation Examples ==="
echo

# Example 1: Simple meeting with duration
echo "1. Creating a simple 30-minute meeting..."
fastcal events create \
  --summary "Team Standup" \
  --start "2026-03-10T09:00:00-08:00" \
  --duration 30 \
  --description "Daily team sync" \
  --location "Zoom" \
  --format text
echo

# Example 2: Event with explicit end time
echo "2. Creating an all-afternoon workshop..."
fastcal events create \
  --summary "React Workshop" \
  --start "2026-03-11T13:00:00-08:00" \
  --end "2026-03-11T17:00:00-08:00" \
  --location "Training Room" \
  --description "Hands-on React development training" \
  --format text
echo

# Example 3: Quick meeting (minimal fields)
echo "3. Creating a quick meeting with minimal details..."
fastcal events create \
  --summary "Coffee Chat" \
  --start "2026-03-12T10:00:00-08:00" \
  --duration 15 \
  --format text
echo

# Example 4: Event in different calendar
echo "4. Creating a personal event..."
fastcal events create \
  --summary "Doctor Appointment" \
  --start "2026-03-13T14:00:00-08:00" \
  --duration 60 \
  --location "Medical Center" \
  --calendar personal \
  --format text
echo

# Example 5: Using JSON output for programmatic access
echo "5. Creating event and capturing the ID..."
RESULT=$(fastcal events create \
  --summary "Important Meeting" \
  --start "2026-03-14T15:00:00-08:00" \
  --duration 90 \
  --format json)

EVENT_ID=$(echo "$RESULT" | jq -r '.event.id')
echo "Created event with ID: $EVENT_ID"
echo

# Example 6: Check for conflicts before creating
echo "6. Checking for conflicts before scheduling..."
CONFLICTS=$(fastcal events conflicts \
  --start "2026-03-10T09:00:00-08:00" \
  --end "2026-03-10T10:00:00-08:00" \
  --format json)

HAS_CONFLICTS=$(echo "$CONFLICTS" | jq -r '.has_conflicts')
if [ "$HAS_CONFLICTS" = "true" ]; then
  echo "⚠️  Time slot has conflicts!"
  echo "$CONFLICTS" | jq '.conflicts[] | "  - \(.summary) at \(.start)"'
else
  echo "✓ Time slot is available"
  # Safe to create the event
  fastcal events create \
    --summary "New Meeting" \
    --start "2026-03-10T09:00:00-08:00" \
    --duration 60 \
    --format text
fi
echo

echo "=== All examples completed ==="
