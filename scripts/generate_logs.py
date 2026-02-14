#!/usr/bin/env python3
"""Generate a large JSONL log file for testing LazyTail."""

import json
import random
import sys
from datetime import datetime, timedelta

# Configuration
DEFAULT_LINES = 1_000_000
OUTPUT_FILE = "large_test.jsonl"

# Log data templates
SERVICES = [
    "api-gateway",
    "api-users",
    "api-orders",
    "api-payments",
    "api-inventory",
    "worker-email",
    "worker-notifications",
    "worker-analytics",
    "cache-redis",
    "db-postgres",
]

LEVELS = ["debug", "info", "warn", "error"]
LEVEL_WEIGHTS = [0.3, 0.5, 0.15, 0.05]  # debug 30%, info 50%, warn 15%, error 5%

PATHS = [
    "/api/v1/users",
    "/api/v1/users/{id}",
    "/api/v1/orders",
    "/api/v1/orders/{id}",
    "/api/v1/payments",
    "/api/v1/inventory",
    "/api/v1/health",
    "/api/v1/metrics",
    "/api/v2/users",
    "/api/v2/orders",
]

HTTP_METHODS = ["GET", "POST", "PUT", "DELETE", "PATCH"]
METHOD_WEIGHTS = [0.6, 0.2, 0.1, 0.05, 0.05]

# Message templates by level
MESSAGES = {
    "debug": [
        "Cache lookup for key: {key}",
        "Query executed in {latency}ms",
        "Processing batch item {idx} of {total}",
        "Loading configuration from {source}",
        "Connection pool stats: active={active}, idle={idle}",
        "Parsing request body",
        "Validating input parameters",
        "Serializing response",
    ],
    "info": [
        "Request processed",
        "User login successful",
        "Order created",
        "Payment processed",
        "Email sent to {email}",
        "Cache updated for key: {key}",
        "Health check passed",
        "Service started",
        "Batch job completed",
        "New user registered",
    ],
    "warn": [
        "Rate limit approaching: {count}/1000",
        "Slow query detected: {latency}ms",
        "Retry attempt {attempt} of 3",
        "Connection pool near capacity: {pct}%",
        "Deprecated API version used",
        "High memory usage: {pct}%",
        "Queue size growing: {count}",
        "Certificate expires in {days} days",
    ],
    "error": [
        "Failed to connect to database",
        "Authentication failed for user {user_id}",
        "Payment processing failed",
        "Timeout waiting for response",
        "Invalid request format",
        "Service unavailable",
        "Rate limit exceeded",
        "Out of memory",
        "Disk space critical",
        "Connection refused",
    ],
}

STATUS_CODES = {
    "debug": [200],
    "info": [200, 201, 204],
    "warn": [400, 401, 403, 404, 429],
    "error": [500, 502, 503, 504, 400, 401, 403],
}


def random_user_id():
    return f"u{random.randint(1000, 9999)}"


def random_order_id():
    return f"ord-{random.randint(10000, 99999)}"


def random_email():
    domains = ["gmail.com", "example.com", "company.org", "mail.io"]
    return f"user{random.randint(1, 1000)}@{random.choice(domains)}"


def random_key():
    prefixes = ["user", "session", "cache", "config", "token"]
    return f"{random.choice(prefixes)}:{random.randint(1, 10000)}"


def generate_log_entry(timestamp):
    level = random.choices(LEVELS, weights=LEVEL_WEIGHTS)[0]
    service = random.choice(SERVICES)

    entry = {
        "timestamp": timestamp.isoformat() + "Z",
        "level": level,
        "service": service,
    }

    # Add message with potential placeholders filled
    msg_template = random.choice(MESSAGES[level])
    msg = msg_template.format(
        key=random_key(),
        latency=random.randint(1, 5000),
        idx=random.randint(1, 100),
        total=random.randint(100, 1000),
        source=random.choice(["env", "file", "consul", "vault"]),
        active=random.randint(1, 50),
        idle=random.randint(0, 20),
        email=random_email(),
        count=random.randint(1, 1000),
        attempt=random.randint(1, 3),
        pct=random.randint(70, 99),
        days=random.randint(1, 30),
        user_id=random_user_id(),
    )
    entry["msg"] = msg

    # Add contextual fields based on service type
    if service.startswith("api-"):
        entry["path"] = random.choice(PATHS)
        entry["method"] = random.choices(HTTP_METHODS, weights=METHOD_WEIGHTS)[0]
        entry["latency"] = random.randint(1, 500) if level != "warn" else random.randint(500, 5000)
        entry["status"] = random.choice(STATUS_CODES[level])

        if "users" in service:
            entry["user_id"] = random_user_id()
        if "orders" in service:
            entry["order_id"] = random_order_id()
            if level == "info":
                entry["amount"] = round(random.uniform(10, 500), 2)

    elif service.startswith("worker-"):
        entry["batch_size"] = random.randint(10, 1000)
        entry["processed"] = random.randint(0, 1000)
        if "email" in service:
            entry["template"] = random.choice(["welcome", "reset", "newsletter", "receipt"])
        if "notifications" in service:
            entry["channel"] = random.choice(["push", "sms", "webhook"])

    elif service.startswith("cache-") or service.startswith("db-"):
        entry["connections"] = random.randint(1, 100)
        entry["queries_per_sec"] = random.randint(100, 10000)
        if level in ["warn", "error"]:
            entry["queue_depth"] = random.randint(100, 1000)

    # Add nested fields occasionally (20% of entries)
    if random.random() < 0.2:
        entry["request"] = {
            "id": f"req-{random.randint(100000, 999999)}",
            "client_ip": f"192.168.{random.randint(1, 255)}.{random.randint(1, 255)}",
            "user_agent": random.choice([
                "Mozilla/5.0",
                "curl/7.68.0",
                "Python/3.9",
                "Go-http-client/1.1",
            ]),
        }

    # Add trace context occasionally (30% of entries)
    if random.random() < 0.3:
        entry["trace_id"] = f"{random.randint(0, 0xFFFFFFFF):08x}{random.randint(0, 0xFFFFFFFF):08x}"
        entry["span_id"] = f"{random.randint(0, 0xFFFFFFFF):08x}"

    return entry


def main():
    num_lines = int(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_LINES
    output_file = sys.argv[2] if len(sys.argv) > 2 else OUTPUT_FILE

    print(f"Generating {num_lines:,} log lines to {output_file}...")

    start_time = datetime(2026, 1, 31, 0, 0, 0)
    time_increment = timedelta(milliseconds=86400000 / num_lines)  # Spread over 24 hours

    with open(output_file, "w") as f:
        for i in range(num_lines):
            timestamp = start_time + (time_increment * i)
            # Add some randomness to timestamp
            timestamp += timedelta(milliseconds=random.randint(-500, 500))

            entry = generate_log_entry(timestamp)
            f.write(json.dumps(entry) + "\n")

            if (i + 1) % 100000 == 0:
                print(f"  {i + 1:,} lines written ({(i + 1) * 100 // num_lines}%)")

    print(f"Done! Generated {output_file}")

    # Print file size
    import os
    size = os.path.getsize(output_file)
    if size > 1_000_000_000:
        print(f"File size: {size / 1_000_000_000:.2f} GB")
    elif size > 1_000_000:
        print(f"File size: {size / 1_000_000:.2f} MB")
    else:
        print(f"File size: {size / 1_000:.2f} KB")


if __name__ == "__main__":
    main()
