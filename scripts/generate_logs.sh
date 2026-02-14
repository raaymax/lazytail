#!/bin/bash

# Script to generate random log lines for testing live reload

LOG_FILE="${1:-live_test.log}"

# Array of log levels
LEVELS=("INFO" "DEBUG" "WARN" "ERROR")

# Array of log messages
MESSAGES=(
    "Processing request from client"
    "Database query executed successfully"
    "Cache hit for key"
    "API endpoint called"
    "Configuration loaded"
    "Connection established"
    "Request timeout detected"
    "Memory usage: 75%"
    "Background job started"
    "File uploaded successfully"
    "User authentication successful"
    "Session created"
    "Transaction committed"
    "Validation passed"
    "HTTP request received"
    "Response sent to client"
    "Service health check passed"
    "Metrics collected"
    "Event published to queue"
    "Worker thread spawned"
)

# Array of error messages
ERROR_MESSAGES=(
    "Failed to connect to database"
    "Invalid authentication token"
    "Null pointer exception"
    "Request validation failed"
    "Resource not found"
    "Connection timeout"
    "Permission denied"
    "Out of memory error"
    "Disk space critical"
    "Service unavailable"
)

echo "Generating logs to $LOG_FILE (Ctrl+C to stop)..."
echo "You can run: cargo run --release -- $LOG_FILE"
echo ""

# Clear the log file
> "$LOG_FILE"

COUNTER=0

while true; do
    # Get current timestamp
    TIMESTAMP=$(date '+%Y-%m-%d %H:%M:%S')

    # Random log level
    LEVEL=${LEVELS[$RANDOM % ${#LEVELS[@]}]}

    # Choose message based on level
    if [ "$LEVEL" = "ERROR" ]; then
        MESSAGE=${ERROR_MESSAGES[$RANDOM % ${#ERROR_MESSAGES[@]}]}
    else
        MESSAGE=${MESSAGES[$RANDOM % ${#MESSAGES[@]}]}
    fi

    # Add some variety - occasionally add details
    if [ $((RANDOM % 3)) -eq 0 ]; then
        DETAIL=$((RANDOM % 1000))
        MESSAGE="$MESSAGE (id: $DETAIL)"
    fi

    # Write log line
    echo "$TIMESTAMP $LEVEL $MESSAGE" >> "$LOG_FILE"

    COUNTER=$((COUNTER + 1))
    echo "[$COUNTER] $TIMESTAMP $LEVEL $MESSAGE"

    # Sleep for 1 second
    sleep 1
done
