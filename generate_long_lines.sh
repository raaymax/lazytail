#!/bin/bash

# Generate log file with long lines to test expandable log entries
# Usage: ./generate_long_lines.sh [output_file] [num_lines]

OUTPUT_FILE="${1:-long_lines_test.log}"
NUM_LINES="${2:-50}"

echo "Generating $NUM_LINES lines to $OUTPUT_FILE..."

> "$OUTPUT_FILE"

for i in $(seq 1 $NUM_LINES); do
    timestamp=$(date -d "+$i seconds" '+%Y-%m-%d %H:%M:%S' 2>/dev/null || date '+%Y-%m-%d %H:%M:%S')

    case $((i % 5)) in
        0)
            # Long JSON log entry
            echo "$timestamp [INFO] {\"event\":\"user_action\",\"user_id\":\"user_$(printf '%05d' $i)\",\"action\":\"page_view\",\"page\":\"/products/category/electronics/smartphones/brand/model-$i\",\"session_id\":\"sess_$(cat /dev/urandom | tr -dc 'a-f0-9' | head -c 32)\",\"metadata\":{\"browser\":\"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36\",\"ip\":\"192.168.$((i % 256)).$((i % 256))\",\"country\":\"US\",\"region\":\"California\",\"city\":\"San Francisco\",\"referrer\":\"https://www.google.com/search?q=best+smartphones+2024&source=web\",\"utm_source\":\"google\",\"utm_medium\":\"cpc\",\"utm_campaign\":\"electronics_q4_2024\"},\"performance\":{\"page_load_ms\":$((RANDOM % 3000 + 500)),\"ttfb_ms\":$((RANDOM % 500 + 50)),\"dom_ready_ms\":$((RANDOM % 1000 + 200))}}" >> "$OUTPUT_FILE"
            ;;
        1)
            # Long error with stack trace
            echo "$timestamp [ERROR] Exception occurred in RequestHandler.processRequest(): NullPointerException - Cannot invoke method getValue() on null object reference at com.example.app.handlers.RequestHandler.processRequest(RequestHandler.java:142) at com.example.app.filters.AuthenticationFilter.doFilter(AuthenticationFilter.java:89) at com.example.app.filters.LoggingFilter.doFilter(LoggingFilter.java:45) at org.springframework.web.servlet.FrameworkServlet.service(FrameworkServlet.java:897) at javax.servlet.http.HttpServlet.service(HttpServlet.java:750) at org.apache.catalina.core.ApplicationFilterChain.internalDoFilter(ApplicationFilterChain.java:231) Request ID: req_$(cat /dev/urandom | tr -dc 'a-f0-9' | head -c 16) User: user_$i Endpoint: /api/v2/users/$i/preferences/notifications/email" >> "$OUTPUT_FILE"
            ;;
        2)
            # Long SQL query log
            echo "$timestamp [DEBUG] Executing SQL query: SELECT u.id, u.username, u.email, u.created_at, u.updated_at, p.first_name, p.last_name, p.avatar_url, p.bio, p.location, p.website, COUNT(DISTINCT o.id) as order_count, SUM(o.total_amount) as total_spent, MAX(o.created_at) as last_order_date FROM users u LEFT JOIN profiles p ON u.id = p.user_id LEFT JOIN orders o ON u.id = o.user_id WHERE u.status = 'active' AND u.created_at >= '2024-01-01' AND (u.email LIKE '%@gmail.com' OR u.email LIKE '%@yahoo.com') GROUP BY u.id, u.username, u.email, u.created_at, u.updated_at, p.first_name, p.last_name, p.avatar_url, p.bio, p.location, p.website HAVING COUNT(DISTINCT o.id) > 5 ORDER BY total_spent DESC LIMIT 100 OFFSET $((i * 100)) -- Query ID: qry_$i Execution time: $((RANDOM % 500 + 10))ms Rows affected: $((RANDOM % 1000))" >> "$OUTPUT_FILE"
            ;;
        3)
            # Long HTTP request/response log
            echo "$timestamp [INFO] HTTP Request completed: method=POST path=/api/v3/webhooks/stripe/payment-intent-succeeded headers={\"Content-Type\":\"application/json\",\"X-Stripe-Signature\":\"t=$(date +%s),v1=$(cat /dev/urandom | tr -dc 'a-f0-9' | head -c 64)\",\"User-Agent\":\"Stripe/1.0 (+https://stripe.com/docs/webhooks)\",\"X-Request-ID\":\"req_$(cat /dev/urandom | tr -dc 'a-zA-Z0-9' | head -c 24)\"} body_size=2847 response_status=200 response_time_ms=$((RANDOM % 200 + 20)) client_ip=54.187.$((RANDOM % 256)).$((RANDOM % 256)) request_id=internal_$(cat /dev/urandom | tr -dc 'a-f0-9' | head -c 16)" >> "$OUTPUT_FILE"
            ;;
        4)
            # Long configuration/environment dump
            echo "$timestamp [WARN] Configuration validation warning: The following environment variables are using default values which may not be suitable for production: DATABASE_URL=postgresql://localhost:5432/myapp_dev (default), REDIS_URL=redis://localhost:6379/0 (default), AWS_REGION=us-east-1 (default), LOG_LEVEL=debug (recommended: info for production), RATE_LIMIT_REQUESTS_PER_MINUTE=1000 (default), MAX_UPLOAD_SIZE_MB=100 (default), SESSION_TIMEOUT_MINUTES=60 (default), CORS_ALLOWED_ORIGINS=* (SECURITY WARNING: should be restricted in production), JWT_SECRET_KEY=development-secret-key-change-in-production (CRITICAL: must be changed), SMTP_HOST=localhost (default), FEATURE_FLAGS={\"new_dashboard\":true,\"beta_features\":false,\"maintenance_mode\":false}" >> "$OUTPUT_FILE"
            ;;
    esac
done

# Add a few extra-long lines
echo "" >> "$OUTPUT_FILE"
echo "$(date '+%Y-%m-%d %H:%M:%S') [INFO] ===== EXTRA LONG TEST LINES BELOW =====" >> "$OUTPUT_FILE"

# Really long repeated pattern
echo "$(date '+%Y-%m-%d %H:%M:%S') [DEBUG] Buffer content: $(printf 'ABCDEFGHIJ%.0s' {1..50})" >> "$OUTPUT_FILE"

# Long base64-like data
echo "$(date '+%Y-%m-%d %H:%M:%S') [INFO] Encoded payload: $(cat /dev/urandom | base64 | tr -d '\n' | head -c 500)" >> "$OUTPUT_FILE"

# Long comma-separated values
echo "$(date '+%Y-%m-%d %H:%M:%S') [DEBUG] Processing IDs: $(seq 1 200 | tr '\n' ',' | sed 's/,$//')" >> "$OUTPUT_FILE"

echo "Done! Generated $OUTPUT_FILE"
echo "Test with: cargo run --release -- $OUTPUT_FILE"
echo "Then press Space on a long line to expand it"
