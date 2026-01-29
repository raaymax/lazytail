#!/bin/bash

# Script to generate random colored log lines for testing

# ANSI color codes
COLOR_RESET="\033[0m"
COLOR_RED="\033[0;31m"
COLOR_GREEN="\033[0;32m"
COLOR_YELLOW="\033[0;33m"
COLOR_BLUE="\033[0;34m"
COLOR_MAGENTA="\033[0;35m"
COLOR_CYAN="\033[0;36m"
COLOR_WHITE="\033[0;37m"
COLOR_GRAY="\033[0;90m"

# Bold colors
COLOR_BOLD_RED="\033[1;31m"
COLOR_BOLD_GREEN="\033[1;32m"
COLOR_BOLD_YELLOW="\033[1;33m"
COLOR_BOLD_CYAN="\033[1;36m"

# Array of log levels with their colors
declare -A LEVEL_COLORS
LEVEL_COLORS["INFO"]="$COLOR_BOLD_GREEN"
LEVEL_COLORS["DEBUG"]="$COLOR_BOLD_CYAN"
LEVEL_COLORS["WARN"]="$COLOR_BOLD_YELLOW"
LEVEL_COLORS["ERROR"]="$COLOR_BOLD_RED"

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

echo "Generating colored logs to STDOUT (Ctrl+C to stop)..."
echo ""


COUNTER=0

while true; do
    # Get current timestamp (gray color)
    TIMESTAMP=$(date '+%Y-%m-%d %H:%M:%S')

    # Random log level
    LEVEL=${LEVELS[$RANDOM % ${#LEVELS[@]}]}
    LEVEL_COLOR=${LEVEL_COLORS[$LEVEL]}

    # Choose message based on level
    if [ "$LEVEL" = "ERROR" ]; then
        MESSAGE=${ERROR_MESSAGES[$RANDOM % ${#ERROR_MESSAGES[@]}]}
    else
        MESSAGE=${MESSAGES[$RANDOM % ${#MESSAGES[@]}]}
    fi

    # Add some variety - occasionally add details
    if [ $((RANDOM % 3)) -eq 0 ]; then
        DETAIL=$((RANDOM % 1000))
        MESSAGE="$MESSAGE ${COLOR_MAGENTA}(id: $DETAIL)${COLOR_RESET}"
    fi

    # Color different parts of the log line
    COLORED_TIMESTAMP="${COLOR_GRAY}${TIMESTAMP}${COLOR_RESET}"
    COLORED_LEVEL="${LEVEL_COLOR}${LEVEL}${COLOR_RESET}"
    COLORED_MESSAGE="${COLOR_WHITE}${MESSAGE}${COLOR_RESET}"

    # Write colored log line
    echo -e "$COLORED_TIMESTAMP $COLORED_LEVEL $COLORED_MESSAGE"

    # Sleep for 1 second
    sleep 0.1
done
