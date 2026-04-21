#!/bin/bash
# AI Assistant Integration Test Script for fastcal
# Tests real-world AI assistant usage scenarios

set -e

# Colors
BLUE='\033[0;34m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

FASTCAL="./target/release/fastcal"

log_scenario() {
    echo -e "\n${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}Scenario: $1${NC}"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}\n"
}

log_user_request() {
    echo -e "${YELLOW}User:${NC} $1\n"
}

log_ai_action() {
    echo -e "${GREEN}AI:${NC} $1"
}

log_command() {
    echo -e "  ${BLUE}\$${NC} $1"
}

log_result() {
    echo -e "\n${GREEN}✓${NC} $1\n"
}

log_error() {
    echo -e "\n${RED}✗${NC} $1\n"
}

# Helper to get tomorrow's date
get_tomorrow() {
    if [[ "$OSTYPE" == "darwin"* ]]; then
        date -v+1d '+%Y-%m-%d'
    else
        date -d "tomorrow" '+%Y-%m-%d'
    fi
}

# Helper to get next Monday
get_next_monday() {
    if [[ "$OSTYPE" == "darwin"* ]]; then
        date -v+mon '+%Y-%m-%d'
    else
        date -d "next monday" '+%Y-%m-%d'
    fi
}

# Scenario 1: Schedule a Meeting
scenario_schedule_meeting() {
    log_scenario "Schedule a Meeting"
    log_user_request "Schedule a meeting with John tomorrow at 2pm for 1 hour"

    local tomorrow=$(get_tomorrow)
    local start_time="${tomorrow}T14:00:00-08:00"

    log_ai_action "I'll schedule that meeting for you."
    log_command "$FASTCAL events create --summary 'Meeting with John' --start '$start_time' --duration 60 --format json"

    if output=$($FASTCAL events create --summary "Meeting with John" --start "$start_time" --duration 60 --format json 2>&1); then
        if command -v jq &> /dev/null; then
            event_id=$(echo "$output" | jq -r '.id' 2>/dev/null || echo "")
            echo "$output" | jq '.' 2>/dev/null || echo "$output"
            log_result "Meeting scheduled successfully (ID: $event_id)"
            echo "$event_id" > /tmp/fastcal_test_meeting_id
        else
            echo "$output"
            log_result "Meeting scheduled successfully"
        fi
    else
        log_error "Failed to schedule meeting"
        echo "$output"
        return 1
    fi
}

# Scenario 2: Check Availability
scenario_check_availability() {
    log_scenario "Check Availability"
    log_user_request "Am I free tomorrow afternoon?"

    local tomorrow=$(get_tomorrow)

    log_ai_action "Let me check your calendar for tomorrow afternoon (12 PM - 5 PM)."
    log_command "$FASTCAL events list --from '$tomorrow 12:00' --to '$tomorrow 17:00' --format json"

    if output=$($FASTCAL events list --from "$tomorrow 12:00" --to "$tomorrow 17:00" --format json 2>&1); then
        if command -v jq &> /dev/null; then
            event_count=$(echo "$output" | jq '.events | length' 2>/dev/null || echo "0")
            echo "$output" | jq '.' 2>/dev/null || echo "$output"

            if [[ "$event_count" -eq 0 ]]; then
                log_result "You're completely free tomorrow afternoon!"
            else
                log_result "You have $event_count event(s) tomorrow afternoon."
                echo "$output" | jq -r '.events[] | "  - \(.start): \(.summary)"' 2>/dev/null || true
            fi
        else
            echo "$output"
            log_result "Retrieved calendar for tomorrow afternoon"
        fi
    else
        log_error "Failed to check availability"
        echo "$output"
        return 1
    fi
}

# Scenario 3: Find Specific Event
scenario_find_event() {
    log_scenario "Find Specific Event"
    log_user_request "When is my next meeting with John?"

    log_ai_action "Searching for meetings with John..."
    log_command "$FASTCAL events search 'John' --format json"

    if output=$($FASTCAL events search "John" --format json 2>&1); then
        if command -v jq &> /dev/null; then
            next_meeting=$(echo "$output" | jq -r '.events[0] | "\(.start) - \(.summary)"' 2>/dev/null || echo "")
            echo "$output" | jq '.' 2>/dev/null || echo "$output"

            if [[ -n "$next_meeting" && "$next_meeting" != "null - null" ]]; then
                log_result "Your next meeting with John is: $next_meeting"
            else
                log_result "No meetings with John found"
            fi
        else
            echo "$output"
            log_result "Search completed"
        fi
    else
        log_error "Failed to search for meetings"
        echo "$output"
        return 1
    fi
}

# Scenario 4: Reschedule Event
scenario_reschedule() {
    log_scenario "Reschedule Event"
    log_user_request "Move my meeting with John to 4pm"

    log_ai_action "Finding your meeting with John and rescheduling it to 4 PM..."

    # First, find the event
    if [[ -f /tmp/fastcal_test_meeting_id ]]; then
        event_id=$(cat /tmp/fastcal_test_meeting_id)
    else
        if command -v jq &> /dev/null; then
            event_id=$($FASTCAL events search "John" --format json | jq -r '.events[0].id' 2>/dev/null || echo "")
        else
            log_error "Cannot reschedule without jq installed"
            return 1
        fi
    fi

    if [[ -z "$event_id" || "$event_id" == "null" ]]; then
        log_error "Could not find meeting with John"
        return 1
    fi

    local tomorrow=$(get_tomorrow)
    local new_time="${tomorrow}T16:00:00-08:00"

    log_command "$FASTCAL events update '$event_id' --start '$new_time'"

    if output=$($FASTCAL events update "$event_id" --start "$new_time" 2>&1); then
        echo "$output"
        log_result "Meeting rescheduled to 4 PM"
    else
        log_error "Failed to reschedule meeting"
        echo "$output"
        return 1
    fi
}

# Scenario 5: Cancel Events
scenario_cancel_events() {
    log_scenario "Cancel All Events Next Monday"
    log_user_request "Cancel all meetings next Monday"

    local next_monday=$(get_next_monday)

    log_ai_action "Finding all events next Monday ($next_monday)..."
    log_command "$FASTCAL events list --from '$next_monday' --to '$next_monday' --format json"

    if output=$($FASTCAL events list --from "$next_monday" --to "$next_monday" --format json 2>&1); then
        if command -v jq &> /dev/null; then
            event_count=$(echo "$output" | jq '.events | length' 2>/dev/null || echo "0")
            echo "$output" | jq '.' 2>/dev/null || echo "$output"

            if [[ "$event_count" -eq 0 ]]; then
                log_result "No events found for next Monday. Nothing to cancel."
            else
                log_ai_action "Found $event_count event(s). Canceling them..."

                event_ids=$(echo "$output" | jq -r '.events[].id' 2>/dev/null || echo "")

                canceled=0
                for id in $event_ids; do
                    if [[ -n "$id" && "$id" != "null" ]]; then
                        log_command "$FASTCAL events delete '$id' --force"
                        if $FASTCAL events delete "$id" --force >/dev/null 2>&1; then
                            ((canceled++))
                        fi
                    fi
                done

                log_result "Canceled $canceled event(s) for next Monday"
            fi
        else
            echo "$output"
            log_result "Listed events for next Monday (jq required for deletion)"
        fi
    else
        log_error "Failed to list events"
        echo "$output"
        return 1
    fi
}

# Scenario 6: Complex Multi-Step Query
scenario_complex_query() {
    log_scenario "Complex Multi-Step Query"
    log_user_request "Do I have any free time between 2pm and 5pm tomorrow? If so, schedule a 30-minute focus time block."

    local tomorrow=$(get_tomorrow)

    log_ai_action "Step 1: Checking your calendar from 2 PM to 5 PM tomorrow..."
    log_command "$FASTCAL events list --from '$tomorrow 14:00' --to '$tomorrow 17:00' --format json"

    if output=$($FASTCAL events list --from "$tomorrow 14:00" --to "$tomorrow 17:00" --format json 2>&1); then
        if command -v jq &> /dev/null; then
            echo "$output" | jq '.' 2>/dev/null || echo "$output"

            # Check for conflicts at different times
            times=("14:00" "15:00" "16:00")
            found_slot=false

            for time in "${times[@]}"; do
                start_time="${tomorrow}T${time}:00-08:00"
                hour=$(echo "$time" | cut -d: -f1)
                end_hour=$((hour + 1))
                end_time="${tomorrow}T$(printf '%02d' $end_hour):00:00-08:00"

                log_ai_action "Checking for conflicts at $time..."
                if $FASTCAL events conflicts --start "$start_time" --end "$end_time" --format json 2>&1 | jq -e '.events | length == 0' >/dev/null 2>&1; then
                    log_ai_action "Found free slot at $time! Scheduling focus time..."
                    log_command "$FASTCAL events create --summary 'Focus Time' --start '$start_time' --duration 30"

                    if $FASTCAL events create --summary "Focus Time" --start "$start_time" --duration 30 >/dev/null 2>&1; then
                        log_result "Scheduled 30-minute focus time at $time"
                        found_slot=true
                        break
                    fi
                fi
            done

            if [[ "$found_slot" == false ]]; then
                log_result "No free slots found between 2 PM and 5 PM tomorrow"
            fi
        else
            echo "$output"
            log_error "jq required for this scenario"
            return 1
        fi
    else
        log_error "Failed to check calendar"
        echo "$output"
        return 1
    fi
}

# Cleanup function
cleanup_test_events() {
    log_scenario "Cleanup Test Events"

    if command -v jq &> /dev/null; then
        log_ai_action "Removing test events..."

        # Delete meeting with John
        if [[ -f /tmp/fastcal_test_meeting_id ]]; then
            event_id=$(cat /tmp/fastcal_test_meeting_id)
            if [[ -n "$event_id" && "$event_id" != "null" ]]; then
                $FASTCAL events delete "$event_id" --force >/dev/null 2>&1 || true
            fi
            rm -f /tmp/fastcal_test_meeting_id
        fi

        # Delete focus time
        focus_id=$($FASTCAL events search "Focus Time" --format json | jq -r '.events[0].id' 2>/dev/null || echo "")
        if [[ -n "$focus_id" && "$focus_id" != "null" ]]; then
            $FASTCAL events delete "$focus_id" --force >/dev/null 2>&1 || true
        fi

        log_result "Cleanup completed"
    fi
}

# Main execution
main() {
    echo -e "${BLUE}╔════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║  AI Assistant Integration Test        ║${NC}"
    echo -e "${BLUE}╚════════════════════════════════════════╝${NC}"

    # Check prerequisites
    if [[ ! -f "$FASTCAL" ]]; then
        log_error "Binary not found. Run: cargo build --release"
        exit 1
    fi

    if [[ -z "$FASTCAL_USERNAME" ]] || [[ -z "$FASTCAL_PASSWORD" ]]; then
        log_error "Environment variables not set"
        echo "Run:"
        echo "  export FASTCAL_USERNAME=\"your-email@fastmail.com\""
        echo "  export FASTCAL_PASSWORD=\"your-app-password\""
        exit 1
    fi

    # Run scenarios
    scenario_schedule_meeting
    sleep 1
    scenario_check_availability
    sleep 1
    scenario_find_event
    sleep 1
    scenario_reschedule
    sleep 1
    scenario_complex_query
    sleep 1
    scenario_cancel_events
    sleep 1

    # Cleanup
    cleanup_test_events

    echo -e "\n${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${GREEN}All AI scenarios completed successfully!${NC}"
    echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}\n"
}

main
