#!/bin/bash

# Script to generate streaming JSONL log lines for testing live reload

# Array of services
SERVICES=(
    "api-gateway"
    "api-users"
    "api-orders"
    "api-payments"
    "worker-email"
    "worker-notifications"
    "cache-redis"
    "db-postgres"
)

LEVELS=("debug" "info" "warn" "error")

PATHS=(
    "/api/v1/users"
    "/api/v1/users/{id}"
    "/api/v1/orders"
    "/api/v1/orders/{id}"
    "/api/v1/payments"
    "/api/v1/health"
)

METHODS=("GET" "POST" "PUT" "DELETE")

INFO_MESSAGES=(
    "Request processed"
    "User login successful"
    "Order created"
    "Payment processed"
    "Cache updated"
    "Health check passed"
    "Service started"
    "Batch job completed"
)

WARN_MESSAGES=(
    "Rate limit approaching"
    "Slow query detected"
    "Retry attempt 2 of 3"
    "Connection pool near capacity"
    "Deprecated API version used"
    "High memory usage"
)

ERROR_MESSAGES=(
    "Failed to connect to database"
    "Authentication failed"
    "Payment processing failed"
    "Timeout waiting for response"
    "Service unavailable"
    "Rate limit exceeded"
    "Out of memory"
    "Connection refused"
)

DEBUG_MESSAGES=(
    "Cache lookup"
    "Query executed"
    "Processing batch item"
    "Parsing request body"
    "Validating input parameters"
    "Serializing response"
)

echo "Generating JSONL logs to STDOUT (Ctrl+C to stop)..." >&2
echo "" >&2

while true; do
    TIMESTAMP=$(date -u '+%Y-%m-%dT%H:%M:%SZ')

    # Weighted level selection: ~30% debug, ~50% info, ~15% warn, ~5% error
    ROLL=$((RANDOM % 100))
    if [ $ROLL -lt 5 ]; then
        LEVEL="error"
    elif [ $ROLL -lt 20 ]; then
        LEVEL="warn"
    elif [ $ROLL -lt 70 ]; then
        LEVEL="info"
    else
        LEVEL="debug"
    fi

    SERVICE=${SERVICES[$RANDOM % ${#SERVICES[@]}]}

    # Select message based on level
    case "$LEVEL" in
        debug) MSG=${DEBUG_MESSAGES[$RANDOM % ${#DEBUG_MESSAGES[@]}]} ;;
        info)  MSG=${INFO_MESSAGES[$RANDOM % ${#INFO_MESSAGES[@]}]} ;;
        warn)  MSG=${WARN_MESSAGES[$RANDOM % ${#WARN_MESSAGES[@]}]} ;;
        error) MSG=${ERROR_MESSAGES[$RANDOM % ${#ERROR_MESSAGES[@]}]} ;;
    esac

    # Build base JSON
    JSON="{\"timestamp\":\"$TIMESTAMP\",\"level\":\"$LEVEL\",\"service\":\"$SERVICE\",\"msg\":\"$MSG\""

    # Add contextual fields based on service type
    if [[ "$SERVICE" == api-* ]]; then
        PATH_VAL=${PATHS[$RANDOM % ${#PATHS[@]}]}
        METHOD=${METHODS[$RANDOM % ${#METHODS[@]}]}
        LATENCY=$((RANDOM % 500 + 1))

        case "$LEVEL" in
            info)  STATUS=200 ;;
            warn)  STATUS=$((RANDOM % 2 == 0 ? 429 : 404)); LATENCY=$((RANDOM % 4000 + 1000)) ;;
            error) STATUS=$((RANDOM % 2 == 0 ? 500 : 503)) ;;
            *)     STATUS=200 ;;
        esac

        JSON="$JSON,\"path\":\"$PATH_VAL\",\"method\":\"$METHOD\",\"latency\":$LATENCY,\"status\":$STATUS"

        # Add user_id for user-related services
        if [[ "$SERVICE" == *users* ]]; then
            JSON="$JSON,\"user_id\":\"u$((RANDOM % 9000 + 1000))\""
        fi
    elif [[ "$SERVICE" == worker-* ]]; then
        JSON="$JSON,\"batch_size\":$((RANDOM % 500 + 10)),\"processed\":$((RANDOM % 500))"
    elif [[ "$SERVICE" == cache-* || "$SERVICE" == db-* ]]; then
        JSON="$JSON,\"connections\":$((RANDOM % 100 + 1)),\"queries_per_sec\":$((RANDOM % 5000 + 100))"
    fi

    # Add trace_id occasionally (~30%)
    if [ $((RANDOM % 10)) -lt 3 ]; then
        TRACE=$(printf '%08x%08x' $((RANDOM * RANDOM)) $((RANDOM * RANDOM)))
        SPAN=$(printf '%08x' $((RANDOM * RANDOM)))
        JSON="$JSON,\"trace_id\":\"$TRACE\",\"span_id\":\"$SPAN\""
    fi

    echo "$JSON}"

    sleep "${1:-0.5}"
done
