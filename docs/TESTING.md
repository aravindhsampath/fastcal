# fastcal Testing Guide

This document outlines the testing strategy for `fastcal`, including unit tests, integration tests, AI assistant scenarios, and performance benchmarks.

## Test Coverage

### Unit Tests (69/69 passing ✅)

**Date/Time Parsing** (13 tests)
- ISO 8601 format parsing
- Date-only parsing (YYYY-MM-DD)
- Space-separated datetime (YYYY-MM-DD HH:MM)
- Natural language formats (2pm, 2:30pm, 9am)
- Special cases (noon, midnight)
- ICS format output

**ICS Generation/Parsing** (5 tests)
- Simple event generation
- Event with all details (location, description, attendees)
- Text escaping for ICS format
- Basic event parsing
- All-day event parsing
- Property extraction without false matches

**Configuration** (5 tests)
- Minimal config loading
- Default preferences
- Config save/load roundtrip
- File permissions (0600)
- Config updates from discovery

**CalDAV Operations** (7 tests)
- URL parsing (valid/invalid)
- Calendar name extraction from href
- Date range formatting
- Basic auth encoding
- Unique name generation (no collision, single collision, multiple collisions)
- Display name fetching utilities

**Command**: `cargo test`

## Integration Testing (Phase 9)

Integration tests require a live Fastmail account. These tests verify end-to-end functionality with real CalDAV servers.

### Prerequisites

1. **Fastmail Account** (any tier)
2. **App-Specific Password**
   - Create at: https://www.fastmail.com/settings/security/password
   - Name it "fastcal-testing"
3. **Environment Variables**
   ```bash
   export FASTCAL_USERNAME="your-email@fastmail.com"
   export FASTCAL_PASSWORD="your-app-password"
   ```
4. **Test Calendar** (recommended)
   - Create a dedicated "Testing" calendar in Fastmail
   - Prevents pollution of personal calendars

### Integration Test Scenarios

#### 1. Configuration & Discovery

**Test**: Initialize configuration
```bash
./target/release/fastcal config init
```
**Expected**:
- Config file created at `~/.config/fastcal/config.toml`
- All calendars discovered
- Default calendar set

**Test**: Verify connection
```bash
./target/release/fastcal config test
```
**Expected**:
- Connection successful
- Authentication confirmed
- Calendar count displayed

**Test**: List calendars
```bash
./target/release/fastcal calendars list
```
**Expected**:
- All calendars listed with URLs
- Display names shown correctly

#### 2. Create Events (CRUD - Create)

**Test 1**: Create simple event
```bash
./target/release/fastcal events create \
  --summary "Test Event 1" \
  --start "2026-03-20T10:00:00-08:00" \
  --duration 60
```
**Expected**:
- Event created successfully
- Event ID returned in output
- Event visible in Fastmail web UI

**Test 2**: Create event with all details
```bash
./target/release/fastcal events create \
  --summary "Team Meeting" \
  --start "2026-03-20T14:00:00-08:00" \
  --duration 90 \
  --location "Conference Room A" \
  --description "Quarterly planning session"
```
**Expected**:
- All fields populated correctly
- Event syncs to Fastmail

**Test 3**: Create event with natural language time
```bash
./target/release/fastcal events create \
  --summary "Lunch" \
  --start "2026-03-20 12pm" \
  --duration 60
```
**Expected**:
- Time parsed correctly to 12:00
- Event created

**Test 4**: Create all-day event
```bash
./target/release/fastcal events create \
  --summary "Vacation Day" \
  --start "2026-03-25" \
  --duration 1440
```
**Expected**:
- All-day event created (24 hours)

#### 3. Read Events (CRUD - Read)

**Test 1**: List events in date range
```bash
./target/release/fastcal events list \
  --from "2026-03-20" \
  --to "2026-03-25"
```
**Expected**:
- All created test events shown
- Correct date/time formatting

**Test 2**: Get specific event
```bash
EVENT_ID="<id-from-create>"
./target/release/fastcal events get $EVENT_ID
```
**Expected**:
- Full event details returned
- All properties intact

**Test 3**: List with JSON format
```bash
./target/release/fastcal events list \
  --from "2026-03-20" \
  --to "2026-03-25" \
  --format json
```
**Expected**:
- Valid JSON output
- Parseable by `jq`

#### 4. Update Events (CRUD - Update)

**Test 1**: Update event time
```bash
EVENT_ID="<id-from-create>"
./target/release/fastcal events update $EVENT_ID \
  --start "2026-03-20T15:00:00-08:00"
```
**Expected**:
- Event time changed
- Other properties preserved
- Update syncs to Fastmail

**Test 2**: Update event location
```bash
./target/release/fastcal events update $EVENT_ID \
  --location "Zoom Meeting Room"
```
**Expected**:
- Only location changed
- Summary, time unchanged

**Test 3**: Update multiple properties
```bash
./target/release/fastcal events update $EVENT_ID \
  --summary "Updated Meeting" \
  --location "Building B, Room 101"
```
**Expected**:
- Both fields updated
- Other fields preserved

#### 5. Delete Events (CRUD - Delete)

**Test 1**: Delete with confirmation
```bash
./target/release/fastcal events delete $EVENT_ID
# Should prompt for confirmation
```
**Expected**:
- Confirmation prompt shown
- Event deleted after confirmation

**Test 2**: Delete with force flag
```bash
./target/release/fastcal events delete $EVENT_ID --force
```
**Expected**:
- No confirmation prompt
- Event deleted immediately
- Deletion syncs to Fastmail

#### 6. Search Functionality

**Test 1**: Search by text
```bash
./target/release/fastcal events search "meeting"
```
**Expected**:
- All events with "meeting" in summary/description
- Case-insensitive matching

**Test 2**: Search in date range
```bash
./target/release/fastcal events search "test" \
  --from "2026-03-20" \
  --to "2026-03-25"
```
**Expected**:
- Only events in date range returned

#### 7. Conflict Detection

**Test 1**: Check for conflicts (should find)
```bash
# Assuming event exists at 2026-03-20 10:00-11:00
./target/release/fastcal events conflicts \
  --start "2026-03-20T10:30:00-08:00" \
  --end "2026-03-20T11:30:00-08:00"
```
**Expected**:
- Conflicting event listed
- Overlap detected

**Test 2**: Check for conflicts (no conflicts)
```bash
./target/release/fastcal events conflicts \
  --start "2026-03-20T08:00:00-08:00" \
  --end "2026-03-20T09:00:00-08:00"
```
**Expected**:
- No conflicts reported
- Empty list returned

#### 8. Batch Operations

**Test 1**: Batch create
```bash
cat > /tmp/test_batch.json <<EOF
[
  {
    "summary": "Batch Event 1",
    "start": "2026-03-21T09:00:00-08:00",
    "duration_minutes": 30
  },
  {
    "summary": "Batch Event 2",
    "start": "2026-03-21T10:00:00-08:00",
    "duration_minutes": 30
  },
  {
    "summary": "Batch Event 3",
    "start": "2026-03-21T11:00:00-08:00",
    "duration_minutes": 30
  }
]
EOF

./target/release/fastcal batch create /tmp/test_batch.json
```
**Expected**:
- All 3 events created
- Progress shown during creation
- Success count reported

**Test 2**: Batch delete
```bash
cat > /tmp/test_batch_delete.json <<EOF
{
  "event_ids": ["id1", "id2", "id3"]
}
EOF

./target/release/fastcal batch delete /tmp/test_batch_delete.json
```
**Expected**:
- All events deleted
- Success/failure per event reported

#### 9. Error Handling

**Test 1**: Invalid credentials
```bash
FASTCAL_PASSWORD="invalid" ./target/release/fastcal config test
```
**Expected**:
- Clear error message about authentication
- Suggestion to check credentials

**Test 2**: Invalid date format
```bash
./target/release/fastcal events create \
  --summary "Test" \
  --start "not-a-date" \
  --duration 60
```
**Expected**:
- Error message about datetime format
- Example of correct format shown

**Test 3**: Event not found
```bash
./target/release/fastcal events get "nonexistent-id"
```
**Expected**:
- Clear "event not found" message

**Test 4**: Calendar not found
```bash
./target/release/fastcal events list --calendar "NonExistent"
```
**Expected**:
- Clear "calendar not found" message
- List of available calendars

## AI Assistant Testing (Phase 9)

These scenarios test how well AI assistants can use `fastcal` to help users manage calendars.

### Scenario 1: Schedule a Meeting

**User Request**: "Schedule a meeting with John tomorrow at 2pm for 1 hour"

**AI Command**:
```bash
./target/release/fastcal events create \
  --summary "Meeting with John" \
  --start "$(date -v+1d '+%Y-%m-%d') 2pm" \
  --duration 60 \
  --format json
```

**Validation**:
- AI can parse the relative date ("tomorrow")
- Time parsing works ("2pm")
- JSON output is parseable
- Event details are correct

### Scenario 2: Check Availability

**User Request**: "Am I free tomorrow afternoon?"

**AI Commands**:
```bash
# List events tomorrow between 12pm-5pm
./target/release/fastcal events list \
  --from "$(date -v+1d '+%Y-%m-%d') 12:00" \
  --to "$(date -v+1d '+%Y-%m-%d') 17:00" \
  --format json
```

**Validation**:
- AI can construct date range query
- JSON output is parseable
- AI can determine if time slots are free

### Scenario 3: Find Specific Event

**User Request**: "When is my next dentist appointment?"

**AI Command**:
```bash
./target/release/fastcal events search "dentist" --format json
```

**Validation**:
- Search returns relevant results
- AI can extract the next upcoming appointment
- Date/time information is clear

### Scenario 4: Reschedule Event

**User Request**: "Move my 3pm meeting to 4pm"

**AI Commands**:
```bash
# 1. Find the event
EVENT_ID=$(./target/release/fastcal events list \
  --from today --to today --format json | \
  jq -r '.events[] | select(.start | contains("15:00")) | .id' | head -1)

# 2. Update the time
./target/release/fastcal events update $EVENT_ID \
  --start "$(date '+%Y-%m-%d') 4pm"
```

**Validation**:
- AI can find event by time
- Update preserves other properties
- Rescheduling works correctly

### Scenario 5: Cancel Multiple Events

**User Request**: "Cancel all meetings next Monday"

**AI Commands**:
```bash
# 1. Find all events next Monday
NEXT_MONDAY=$(date -v+mon '+%Y-%m-%d')
EVENT_IDS=$(./target/release/fastcal events list \
  --from $NEXT_MONDAY --to $NEXT_MONDAY --format json | \
  jq -r '.events[].id')

# 2. Delete each event
for id in $EVENT_IDS; do
  ./target/release/fastcal events delete $id --force
done
```

**Validation**:
- AI can identify target date
- Batch deletion works
- All events removed

### AI Success Criteria

- ✅ AI can construct correct commands from natural language
- ✅ JSON output is easily parseable by AI
- ✅ Error messages are clear and actionable
- ✅ Multi-step workflows succeed
- ✅ Date/time parsing handles various formats

## Performance Testing (Phase 9)

### Benchmark Common Operations

**Test Environment**:
- Fastmail account with 100+ events
- Various date ranges
- Multiple calendars

**Metrics to Measure**:
- Response time (p50, p95, p99)
- Memory usage
- Network requests count

#### Benchmark 1: List Events (Small Range)

**Command**:
```bash
time ./target/release/fastcal events list \
  --from today --to today --format json
```

**Target**: < 2 seconds
**Measure**: Time to first result

#### Benchmark 2: List Events (Large Range)

**Command**:
```bash
time ./target/release/fastcal events list \
  --from today --to "+365 days" --format json
```

**Target**: < 5 seconds for 100 events
**Measure**: Total execution time

#### Benchmark 3: Search Across All Events

**Command**:
```bash
time ./target/release/fastcal events search "meeting" --format json
```

**Target**: < 3 seconds for 100 events
**Measure**: Search completion time

#### Benchmark 4: Create Event

**Command**:
```bash
time ./target/release/fastcal events create \
  --summary "Performance Test" \
  --start "2026-03-25T10:00:00-08:00" \
  --duration 60
```

**Target**: < 2 seconds
**Measure**: Event creation time

#### Benchmark 5: Update Event

**Command**:
```bash
time ./target/release/fastcal events update $EVENT_ID \
  --summary "Updated Title"
```

**Target**: < 2 seconds
**Measure**: Update completion time

#### Benchmark 6: Batch Create (10 events)

**Command**:
```bash
time ./target/release/fastcal batch create /tmp/batch_10_events.json
```

**Target**: < 10 seconds (1s per event)
**Measure**: Total batch time

#### Benchmark 7: Calendar Discovery

**Command**:
```bash
time ./target/release/fastcal calendars list
```

**Target**: < 2 seconds
**Measure**: Discovery + display name fetching (concurrent)

### Performance Success Criteria

- ✅ All operations complete in < 2s (simple) or < 5s (complex)
- ✅ Concurrent HTTP requests working (calendar discovery)
- ✅ Memory usage remains reasonable (< 50MB for typical operations)
- ✅ No N+1 query problems
- ✅ Responsive even with 100+ events

## Test Execution Checklist

### Pre-Testing Setup
- [ ] Fastmail account configured
- [ ] App password created
- [ ] Environment variables set
- [ ] Test calendar created (optional but recommended)
- [ ] Build release binary: `cargo build --release`

### Unit Tests
- [x] Run `cargo test` (69/69 passing)
- [x] Run `cargo clippy` (zero warnings)
- [ ] Run `cargo fmt --check` (code formatted)

### Integration Tests
- [ ] Configuration & Discovery (3 tests)
- [ ] Create Events (4 tests)
- [ ] Read Events (3 tests)
- [ ] Update Events (3 tests)
- [ ] Delete Events (2 tests)
- [ ] Search Functionality (2 tests)
- [ ] Conflict Detection (2 tests)
- [ ] Batch Operations (2 tests)
- [ ] Error Handling (4 tests)

### AI Assistant Scenarios
- [ ] Schedule a meeting (Scenario 1)
- [ ] Check availability (Scenario 2)
- [ ] Find specific event (Scenario 3)
- [ ] Reschedule event (Scenario 4)
- [ ] Cancel multiple events (Scenario 5)

### Performance Benchmarks
- [ ] List events (small range)
- [ ] List events (large range)
- [ ] Search across all events
- [ ] Create event
- [ ] Update event
- [ ] Batch create
- [ ] Calendar discovery

### Post-Testing Cleanup
- [ ] Delete test events
- [ ] Review test results
- [ ] Document any issues found
- [ ] Update DEVELOPMENT_PLAN.md with results

## Continuous Integration

For automated testing in CI/CD:

```yaml
# .github/workflows/test.yml (example)
name: Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - run: cargo test
      - run: cargo clippy -- -D warnings
      - run: cargo fmt --check
```

**Note**: Integration tests requiring Fastmail credentials should be run manually or with encrypted secrets in CI.

## Reporting Issues

When tests fail, capture:
1. **Command used**
2. **Expected result**
3. **Actual result** (full error message)
4. **Environment**: OS, Rust version, fastcal version
5. **Logs**: Run with `-v` flag for verbose output

Example:
```bash
./target/release/fastcal -v events create \
  --summary "Test" \
  --start "2026-03-20T10:00:00-08:00" \
  --duration 60 2>&1 | tee test_failure.log
```

## Next Steps After Testing

1. Fix any identified bugs
2. Optimize slow operations
3. Update documentation based on findings
4. Prepare for Phase 10 (Release Preparation)
