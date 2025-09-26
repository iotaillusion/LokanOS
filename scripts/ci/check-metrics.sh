#!/usr/bin/env bash
set -euo pipefail

curl -fsS http://127.0.0.1:8006/metrics -o metrics.txt
if command -v promtool >/dev/null 2>&1; then
  promtool check metrics metrics.txt
fi

grep -q '^process_uptime_seconds ' metrics.txt
grep -q '^http_requests_total' metrics.txt
grep -q '^handler_latency_seconds_bucket' metrics.txt
grep -q '^handler_latency_seconds_sum' metrics.txt
grep -q '^handler_latency_seconds_count' metrics.txt
