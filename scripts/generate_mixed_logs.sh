#!/usr/bin/env bash
# Generate mixed-format logs to stdout.
# Produces a blend of JSON, logfmt, plain text, and colored plain text.
#
# Usage:
#   ./generate_mixed_logs.sh                     # stream forever (0.3s interval)
#   ./generate_mixed_logs.sh --interval 0.1      # stream faster
#   ./generate_mixed_logs.sh --count 10000       # emit N lines then exit
#   ./generate_mixed_logs.sh --count 0           # alias for infinite stream
#
# Examples:
#   ./generate_mixed_logs.sh | lazytail -n mixed
#   ./generate_mixed_logs.sh --count 50000 > test.log

set -euo pipefail

# ---------------------------------------------------------------------------
# ANSI colors
# ---------------------------------------------------------------------------
R=$'\033[0m'
GRAY=$'\033[90m'
BOLD_RED=$'\033[1;31m'
BOLD_YELLOW=$'\033[1;33m'
BOLD_GREEN=$'\033[1;32m'
BOLD_CYAN=$'\033[1;36m'
BOLD_MAGENTA=$'\033[1;35m'
BLUE=$'\033[34m'
CYAN=$'\033[36m'
GREEN=$'\033[32m'
MAGENTA=$'\033[35m'
WHITE=$'\033[37m'
DIM=$'\033[2m'

# ---------------------------------------------------------------------------
# Data pools
# ---------------------------------------------------------------------------
SERVICES=(
  "api-gateway" "api-users" "api-orders" "api-payments"
  "api-inventory" "worker-email" "worker-notifications"
  "worker-analytics" "cache-redis" "db-postgres"
)

PATHS=(
  "/api/v1/users" "/api/v1/users/{id}" "/api/v1/orders"
  "/api/v1/orders/{id}" "/api/v1/payments" "/api/v1/inventory"
  "/api/v1/health" "/api/v1/metrics" "/api/v2/users" "/api/v2/orders"
)

# Weighted: GET appears 6x for ~55% probability
HTTP_METHODS=("GET" "GET" "GET" "GET" "GET" "GET" "POST" "POST" "PUT" "DELETE" "PATCH")

TRACE_MSGS=(
  "entering function handle_request"
  "entering function process_payment"
  "lock acquired on users_table"
  "socket recv 4096 bytes"
  "resolving dependency database"
  "tick 8421"
)

DEBUG_MSGS=(
  "cache lookup for key user:3821"
  "query executed in 42ms"
  "processing batch item 7/100"
  "loading config from env"
  "connection pool: active=5 idle=10"
  "parsing request body"
  "validating input parameters"
  "serializing response"
)

INFO_MSGS=(
  "request processed"
  "user login successful"
  "order created"
  "payment processed"
  "cache updated"
  "health check passed"
  "service started"
  "batch job completed"
  "new user registered"
  "email sent"
)

WARN_MSGS=(
  "rate limit approaching: 850/1000"
  "slow query: 2341ms"
  "retry attempt 2/3"
  "connection pool near capacity: 87%"
  "deprecated API version used"
  "high memory usage: 91%"
  "queue size growing: 532"
  "certificate expires in 7 days"
)

ERROR_MSGS=(
  "failed to connect to database"
  "authentication failed for user u4521"
  "payment processing failed"
  "timeout waiting for upstream"
  "service unavailable"
  "rate limit exceeded"
  "out of memory"
  "connection refused"
  "invalid request format"
  "disk space critical"
)

FATAL_MSGS=(
  "unrecoverable error, shutting down"
  "data corruption detected in users_table"
  "failed to bind port 8080: already in use"
  "segmentation fault in run_query"
  "kernel panic: stack overflow"
)

# ---------------------------------------------------------------------------
# Helpers — use global _LEVEL, _MSG, _SERVICE to avoid subshell overhead
# ---------------------------------------------------------------------------

_pick_level() {
  local r=$(( RANDOM % 100 ))
  if   (( r <  5 )); then _LEVEL="trace"
  elif (( r < 30 )); then _LEVEL="debug"
  elif (( r < 75 )); then _LEVEL="info"
  elif (( r < 90 )); then _LEVEL="warn"
  elif (( r < 98 )); then _LEVEL="error"
  else                    _LEVEL="fatal"
  fi
}

_pick_msg() {
  case $_LEVEL in
    trace) _MSG="${TRACE_MSGS[RANDOM % ${#TRACE_MSGS[@]}]}" ;;
    debug) _MSG="${DEBUG_MSGS[RANDOM % ${#DEBUG_MSGS[@]}]}" ;;
    info)  _MSG="${INFO_MSGS[RANDOM % ${#INFO_MSGS[@]}]}" ;;
    warn)  _MSG="${WARN_MSGS[RANDOM % ${#WARN_MSGS[@]}]}" ;;
    error) _MSG="${ERROR_MSGS[RANDOM % ${#ERROR_MSGS[@]}]}" ;;
    fatal) _MSG="${FATAL_MSGS[RANDOM % ${#FATAL_MSGS[@]}]}" ;;
  esac
}

_pick_status() {
  case $_LEVEL in
    trace|debug) _STATUS=200 ;;
    info)  local s=(200 201 204); _STATUS="${s[RANDOM % ${#s[@]}]}" ;;
    warn)  local s=(400 401 403 404 429); _STATUS="${s[RANDOM % ${#s[@]}]}" ;;
    error|fatal) local s=(500 502 503 504); _STATUS="${s[RANDOM % ${#s[@]}]}" ;;
  esac
}

_pick_latency() {
  case $_LEVEL in
    warn|error|fatal) _LATENCY=$(( RANDOM % 4500 + 500 )) ;;
    *)                _LATENCY=$(( RANDOM % 499  + 1   )) ;;
  esac
}

_level_color() {
  case $_LEVEL in
    trace) _LC="${DIM}${WHITE}" ;;
    debug) _LC="${BOLD_CYAN}" ;;
    info)  _LC="${BOLD_GREEN}" ;;
    warn)  _LC="${BOLD_YELLOW}" ;;
    error) _LC="${BOLD_RED}" ;;
    fatal) _LC="${BOLD_MAGENTA}" ;;
  esac
}

_trace_id() {
  # ~30% chance; sets _TRACE / _SPAN or clears them
  if (( RANDOM % 10 < 3 )); then
    _TRACE=$(printf '%08x%08x' $(( RANDOM * RANDOM )) $(( RANDOM * RANDOM )))
    _SPAN=$(printf  '%08x'     $(( RANDOM * RANDOM )))
  else
    _TRACE=""
    _SPAN=""
  fi
}

# ---------------------------------------------------------------------------
# Format generators — print one line to stdout
# ---------------------------------------------------------------------------

make_json() {
  local ts=$1
  _pick_level; _pick_msg; _pick_status; _pick_latency; _trace_id
  local service="${SERVICES[RANDOM % ${#SERVICES[@]}]}"
  local line="{\"ts\":\"${ts}\",\"level\":\"${_LEVEL}\",\"service\":\"${service}\",\"msg\":\"${_MSG}\""

  if [[ $service == api-* ]]; then
    local method="${HTTP_METHODS[RANDOM % ${#HTTP_METHODS[@]}]}"
    local path="${PATHS[RANDOM % ${#PATHS[@]}]}"
    line+=",\"method\":\"${method}\",\"path\":\"${path}\",\"status\":${_STATUS},\"latency\":${_LATENCY}"
  elif [[ $service == worker-* ]]; then
    local bs=$(( RANDOM % 490 + 10 ))
    local done=$(( RANDOM % (bs + 1) ))
    line+=",\"batch_size\":${bs},\"processed\":${done}"
  elif [[ $service == cache-* || $service == db-* ]]; then
    local conns=$(( RANDOM % 100 + 1 ))
    local qps=$(( RANDOM % 9950 + 50 ))
    line+=",\"connections\":${conns},\"qps\":${qps}"
  fi

  [[ -n $_TRACE ]] && line+=",\"trace_id\":\"${_TRACE}\",\"span_id\":\"${_SPAN}\""
  printf '%s}\n' "$line"
}

make_logfmt() {
  local ts=$1
  _pick_level; _pick_msg; _pick_status; _pick_latency; _trace_id
  local service="${SERVICES[RANDOM % ${#SERVICES[@]}]}"
  local line="ts=${ts} level=${_LEVEL} service=${service} msg=\"${_MSG}\""

  if [[ $service == api-* ]]; then
    local method="${HTTP_METHODS[RANDOM % ${#HTTP_METHODS[@]}]}"
    local path="${PATHS[RANDOM % ${#PATHS[@]}]}"
    line+=" method=${method} path=${path} status=${_STATUS} latency_ms=${_LATENCY}"
  elif [[ $service == worker-* ]]; then
    local bs=$(( RANDOM % 490 + 10 ))
    local done=$(( RANDOM % (bs + 1) ))
    line+=" batch_size=${bs} processed=${done}"
  elif [[ $service == cache-* || $service == db-* ]]; then
    local conns=$(( RANDOM % 100 + 1 ))
    local qps=$(( RANDOM % 9950 + 50 ))
    line+=" connections=${conns} qps=${qps}"
  fi

  [[ -n $_TRACE ]] && line+=" trace_id=${_TRACE} span_id=${_SPAN}"
  printf '%s\n' "$line"
}

make_plain() {
  local ts=$1 colored=$2
  _pick_level; _pick_msg; _pick_status; _pick_latency
  local service="${SERVICES[RANDOM % ${#SERVICES[@]}]}"
  local level_upper
  printf -v level_upper '%-5s' "${_LEVEL^^}"

  local extra=""
  if [[ $service == api-* ]]; then
    local method="${HTTP_METHODS[RANDOM % ${#HTTP_METHODS[@]}]}"
    local path="${PATHS[RANDOM % ${#PATHS[@]}]}"
    if (( colored )); then
      extra=" ${GRAY}[${CYAN}${method}${GRAY}]${R} ${BLUE}${path}${R} ${GRAY}→${R} ${GREEN}${_STATUS}${R} ${GRAY}(${_LATENCY}ms)${R}"
    else
      extra=" [${method}] ${path} → ${_STATUS} (${_LATENCY}ms)"
    fi
  elif [[ $service == worker-* ]]; then
    local bs=$(( RANDOM % 490 + 10 ))
    local done=$(( RANDOM % (bs + 1) ))
    if (( colored )); then
      extra=" ${GRAY}batch=${CYAN}${done}/${bs}${R}"
    else
      extra=" batch=${done}/${bs}"
    fi
  elif [[ $service == cache-* || $service == db-* ]]; then
    local qps=$(( RANDOM % 9950 + 50 ))
    if (( colored )); then
      extra=" ${GRAY}qps=${CYAN}${qps}${R}"
    else
      extra=" qps=${qps}"
    fi
  fi

  if (( colored )); then
    _level_color
    printf '%s\n' "${GRAY}${ts}${R} ${_LC}${level_upper}${R} ${MAGENTA}${service}${R} ${WHITE}${_MSG}${R}${extra}"
  else
    printf '%s\n' "${ts} ${level_upper} ${service} ${_MSG}${extra}"
  fi
}

# Pick format and emit one line: 35% json, 25% logfmt, 25% plain+color, 15% plain
emit_line() {
  local ms; printf -v ms '%03d' $(( RANDOM % 1000 ))
  local ts_iso; ts_iso="$(date -u '+%Y-%m-%dT%H:%M:%S').${ms}Z"
  local ts_human; ts_human="$(date '+%Y-%m-%d %H:%M:%S')"

  local r=$(( RANDOM % 100 ))
  if   (( r < 35 )); then make_json   "$ts_iso"
  elif (( r < 60 )); then make_logfmt "$ts_iso"
  elif (( r < 85 )); then make_plain  "$ts_human" 1
  else                    make_plain  "$ts_human" 0
  fi
}

# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

interval="0.3"
count=0   # 0 = infinite

while [[ $# -gt 0 ]]; do
  case $1 in
    --interval|-i) interval="$2"; shift 2 ;;
    --count|-n)    count="$2";    shift 2 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

i=0
while true; do
  emit_line
  (( count > 0 && ++i >= count )) && break
  [[ $interval != "0" ]] && sleep "$interval"
done
