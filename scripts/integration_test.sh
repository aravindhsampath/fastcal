#!/bin/bash
# Integration test script for fastcal
# Requires: FASTCAL_USERNAME and FASTCAL_PASSWORD environment variables

set -e  # Exit on error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

# Binary path
FASTCAL="./target/release/fastcal"

# Test results
FAILED_TESTS=()

# Helper functions
log_info() {
    echo -e "${BLUE}ℹ ${NC}$1"
}

log_success() {
    echo -e "${GREEN}✓${NC} $1"
    ((TESTS_PASSED++))
}

log_error() {
    echo -e "${RED}✗${NC} $1"
    ((TESTS_FAILED++))
    FAILED_TESTS+=("$1")
}

log_warn() {
    echo -e "${YELLOW}⚠${NC} $1"
}

run_test() {
    local test_name="$1"
    local command="$2"
    local expected_pattern="$3"

    ((TESTS_RUN++))
    log_info "Running: $test_name"

    if output=$(eval "$command" 2>&1); then
        if [[ -z "$expected_pattern" ]] || echo "$output" | grep -q "$expected_pattern"; then
            log_success "$test_name"
            return 0
        else
            log_error "$test_name (pattern not found: $expected_pattern)"
            echo "Output: $output"
            return 1
        fi
    else
        log_error "$test_name (command failed)"
        echo "Output: $output"
        return 1
    fi
}

# Check prerequisites
check_prerequisites() {
    log_info "Checking prerequisites..."

    # Check if binary exists
    if [[ ! -f "$FASTCAL" ]]; then
        log_error "Binary not found at $FASTCAL"
        log_info "Run: cargo build --release"
        exit 1
    fi

    # Check environment variables
    if [[ -z "$FASTCAL_USERNAME" ]]; then
        log_error "FASTCAL_USERNAME not set"
        log_info "Run: export FASTCAL_USERNAME=\"your-email@fastmail.com\""
        exit 1
    fi

    if [[ -z "$FASTCAL_PASSWORD" ]]; then
        log_error "FASTCAL_PASSWORD not set"
        log_info "Run: export FASTCAL_PASSWORD=\"your-app-password\""
        exit 1
    fi

    # Check if jq is installed (for JSON parsing)
    if ! command -v jq &> /dev/null; then
        log_warn "jq not installed - JSON validation tests will be skipped"
        log_info "Install: brew install jq (macOS) or apt-get install jq (Linux)"
    fi

    log_success "Prerequisites check passed"
}

# Test suites
test_configuration() {
    echo -e "\n${BLUE}━━━ Configuration & Discovery Tests ━━━${NC}\n"

    run_test "Config test" \
        "$FASTCAL config test" \
        "Connected"

    run_test "List calendars" \
        "$FASTCAL calendars list" \
        ""
}

test_create_events() {
    echo -e "\n${BLUE}━━━ Create Event Tests ━━━${NC}\n"

    # Test 1: Simple event
    run_test "Create simple event" \
        "$FASTCAL events create --summary 'Test Event 1' --start '2026-03-20T10:00:00-08:00' --duration 60" \
        "Test Event 1"

    # Test 2: Event with all details
    run_test "Create event with details" \
        "$FASTCAL events create --summary 'Team Meeting' --start '2026-03-20T14:00:00-08:00' --duration 90 --location 'Conference Room A' --description 'Quarterly planning'" \
        "Team Meeting"

    # Test 3: Natural language time
    run_test "Create event with natural time" \
        "$FASTCAL events create --summary 'Lunch' --start '2026-03-20 12pm' --duration 60" \
        "Lunch"

    # Test 4: Event for conflict testing
    run_test "Create event for conflict test" \
        "$FASTCAL events create --summary 'Conflict Test Event' --start '2026-03-21T10:00:00-08:00' --duration 60" \
        "Conflict Test Event"
}

test_read_events() {
    echo -e "\n${BLUE}━━━ Read Event Tests ━━━${NC}\n"

    run_test "List events in range" \
        "$FASTCAL events list --from '2026-03-20' --to '2026-03-25'" \
        "Test Event 1"

    run_test "List events JSON format" \
        "$FASTCAL events list --from '2026-03-20' --to '2026-03-25' --format json" \
        '"events"'

    # Validate JSON if jq is available
    if command -v jq &> /dev/null; then
        run_test "Validate JSON output" \
            "$FASTCAL events list --from '2026-03-20' --to '2026-03-25' --format json | jq -e '.events'" \
            ""
    fi
}

test_search() {
    echo -e "\n${BLUE}━━━ Search Tests ━━━${NC}\n"

    run_test "Search by text" \
        "$FASTCAL events search 'Test'" \
        "Test Event"

    run_test "Search in date range" \
        "$FASTCAL events search 'Meeting' --from '2026-03-20' --to '2026-03-25'" \
        "Team Meeting"
}

test_conflicts() {
    echo -e "\n${BLUE}━━━ Conflict Detection Tests ━━━${NC}\n"

    run_test "Detect conflicts (should find)" \
        "$FASTCAL events conflicts --start '2026-03-21T10:30:00-08:00' --end '2026-03-21T11:30:00-08:00'" \
        "Conflict Test Event"

    run_test "No conflicts (should be empty)" \
        "$FASTCAL events conflicts --start '2026-03-21T08:00:00-08:00' --end '2026-03-21T09:00:00-08:00'" \
        ""
}

test_update_events() {
    echo -e "\n${BLUE}━━━ Update Event Tests ━━━${NC}\n"

    # Get an event ID first
    log_info "Finding event to update..."
    if command -v jq &> /dev/null; then
        EVENT_ID=$($FASTCAL events search "Test Event 1" --format json | jq -r '.events[0].id' 2>/dev/null || echo "")

        if [[ -n "$EVENT_ID" && "$EVENT_ID" != "null" ]]; then
            run_test "Update event time" \
                "$FASTCAL events update '$EVENT_ID' --start '2026-03-20T11:00:00-08:00'" \
                "11:00"

            run_test "Update event location" \
                "$FASTCAL events update '$EVENT_ID' --location 'Room B'" \
                "Room B"
        else
            log_warn "Skipping update tests - could not find event ID"
        fi
    else
        log_warn "Skipping update tests - jq not available"
    fi
}

test_batch_operations() {
    echo -e "\n${BLUE}━━━ Batch Operations Tests ━━━${NC}\n"

    # Create batch test file
    BATCH_FILE="/tmp/fastcal_batch_test.json"
    cat > "$BATCH_FILE" <<EOF
[
  {
    "summary": "Batch Event 1",
    "start": "2026-03-22T09:00:00-08:00",
    "duration_minutes": 30
  },
  {
    "summary": "Batch Event 2",
    "start": "2026-03-22T10:00:00-08:00",
    "duration_minutes": 30
  },
  {
    "summary": "Batch Event 3",
    "start": "2026-03-22T11:00:00-08:00",
    "duration_minutes": 30
  }
]
EOF

    run_test "Batch create events" \
        "$FASTCAL batch create '$BATCH_FILE'" \
        "Created 3"

    rm -f "$BATCH_FILE"
}

test_error_handling() {
    echo -e "\n${BLUE}━━━ Error Handling Tests ━━━${NC}\n"

    # These tests expect failures
    ((TESTS_RUN++))
    if $FASTCAL events create --summary "Test" --start "invalid-date" --duration 60 2>&1 | grep -q "Unsupported datetime format"; then
        log_success "Invalid date format error"
        ((TESTS_PASSED++))
    else
        log_error "Invalid date format error (expected error not shown)"
        ((TESTS_FAILED++))
    fi

    ((TESTS_RUN++))
    if $FASTCAL events get "nonexistent-id-12345" 2>&1 | grep -q "not found\|No event found"; then
        log_success "Event not found error"
        ((TESTS_PASSED++))
    else
        log_error "Event not found error (expected error not shown)"
        ((TESTS_FAILED++))
    fi
}

test_delete_events() {
    echo -e "\n${BLUE}━━━ Delete Event Tests ━━━${NC}\n"

    if command -v jq &> /dev/null; then
        # Get IDs of test events
        log_info "Finding test events to delete..."
        EVENT_IDS=$($FASTCAL events search "Test Event" --format json | jq -r '.events[].id' 2>/dev/null || echo "")

        if [[ -n "$EVENT_IDS" ]]; then
            for id in $EVENT_IDS; do
                if [[ -n "$id" && "$id" != "null" ]]; then
                    run_test "Delete event $id" \
                        "$FASTCAL events delete '$id' --force" \
                        ""
                fi
            done
        else
            log_warn "No test events found to delete"
        fi

        # Delete batch events
        BATCH_IDS=$($FASTCAL events search "Batch Event" --format json | jq -r '.events[].id' 2>/dev/null || echo "")
        if [[ -n "$BATCH_IDS" ]]; then
            for id in $BATCH_IDS; do
                if [[ -n "$id" && "$id" != "null" ]]; then
                    run_test "Delete batch event $id" \
                        "$FASTCAL events delete '$id' --force" \
                        ""
                fi
            done
        fi

        # Delete other test events
        for name in "Team Meeting" "Lunch" "Conflict Test Event"; do
            id=$($FASTCAL events search "$name" --format json | jq -r '.events[0].id' 2>/dev/null || echo "")
            if [[ -n "$id" && "$id" != "null" ]]; then
                run_test "Delete '$name'" \
                    "$FASTCAL events delete '$id' --force" \
                    ""
            fi
        done
    else
        log_warn "Skipping delete tests - jq not available"
    fi
}

# Performance benchmarks
benchmark_operations() {
    echo -e "\n${BLUE}━━━ Performance Benchmarks ━━━${NC}\n"

    log_info "Benchmarking list events (small range)..."
    time_output=$(TIMEFORMAT='%R'; { time $FASTCAL events list --from today --to today --format json >/dev/null 2>&1; } 2>&1)
    echo "  Time: ${time_output}s"

    log_info "Benchmarking search..."
    time_output=$(TIMEFORMAT='%R'; { time $FASTCAL events search "test" --format json >/dev/null 2>&1; } 2>&1)
    echo "  Time: ${time_output}s"

    log_info "Benchmarking calendar list..."
    time_output=$(TIMEFORMAT='%R'; { time $FASTCAL calendars list >/dev/null 2>&1; } 2>&1)
    echo "  Time: ${time_output}s"

    log_info "Benchmarking event creation..."
    time_output=$(TIMEFORMAT='%R'; { time $FASTCAL events create --summary "Perf Test" --start "2026-03-25T10:00:00-08:00" --duration 60 >/dev/null 2>&1; } 2>&1)
    echo "  Time: ${time_output}s"

    # Clean up perf test event
    if command -v jq &> /dev/null; then
        id=$($FASTCAL events search "Perf Test" --format json | jq -r '.events[0].id' 2>/dev/null)
        if [[ -n "$id" && "$id" != "null" ]]; then
            $FASTCAL events delete "$id" --force >/dev/null 2>&1
        fi
    fi
}

# Main execution
main() {
    echo -e "${BLUE}╔════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║  fastcal Integration Test Suite       ║${NC}"
    echo -e "${BLUE}╚════════════════════════════════════════╝${NC}\n"

    check_prerequisites

    # Run test suites
    test_configuration
    test_create_events
    test_read_events
    test_search
    test_conflicts
    test_update_events
    test_batch_operations
    test_error_handling
    test_delete_events

    # Run benchmarks
    benchmark_operations

    # Summary
    echo -e "\n${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}Test Summary${NC}"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "Total tests run: ${TESTS_RUN}"
    echo -e "${GREEN}Passed: ${TESTS_PASSED}${NC}"
    echo -e "${RED}Failed: ${TESTS_FAILED}${NC}"

    if [[ ${TESTS_FAILED} -gt 0 ]]; then
        echo -e "\n${RED}Failed tests:${NC}"
        for test in "${FAILED_TESTS[@]}"; do
            echo -e "  ${RED}✗${NC} $test"
        done
        exit 1
    else
        echo -e "\n${GREEN}All tests passed! ✓${NC}"
        exit 0
    fi
}

# Run main
main
