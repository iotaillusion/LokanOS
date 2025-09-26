#!/usr/bin/env bash
set -euo pipefail

METRICS_URL="${METRICS_URL:-http://127.0.0.1:8006/metrics}"
METRICS_OUTPUT="${METRICS_OUTPUT:-metrics.txt}"
METRICS_RETRIES="${METRICS_RETRIES:-30}"
METRICS_RETRY_WAIT="${METRICS_RETRY_WAIT:-5}"

work_file="$(mktemp)"
trap 'rm -f "$work_file"' EXIT

for attempt in $(seq 1 "$METRICS_RETRIES"); do
  echo "[check-metrics] Attempt ${attempt}/${METRICS_RETRIES} to fetch ${METRICS_URL}" >&2
  if curl --fail --show-error --silent --location "$METRICS_URL" -o "$work_file"; then
    echo "[check-metrics] Metrics fetched successfully" >&2
    mv "$work_file" "$METRICS_OUTPUT"
    break
  fi

  status=$?
  echo "[check-metrics] Fetch attempt ${attempt} failed with status ${status}" >&2

  if [[ "$attempt" -eq "$METRICS_RETRIES" ]]; then
    echo "[check-metrics] Exhausted retries fetching metrics" >&2
    if [[ -s "$work_file" ]]; then
      echo "[check-metrics] Partial response:" >&2
      cat "$work_file" >&2 || true
    fi
    exit "$status"
  fi

  sleep "$METRICS_RETRY_WAIT"
  : >"$work_file"
done

if command -v promtool >/dev/null 2>&1; then
  echo "[check-metrics] Validating exposition format with promtool" >&2
  promtool check metrics "$METRICS_OUTPUT"
else
  echo "[check-metrics] promtool not available; skipping format validation" >&2
fi

required_series=(
  '^process_uptime_seconds '
  '^http_requests_total'
  '^handler_latency_seconds_bucket'
  '^handler_latency_seconds_sum'
  '^handler_latency_seconds_count'
)

for pattern in "${required_series[@]}"; do
  if ! grep -qE "$pattern" "$METRICS_OUTPUT"; then
    echo "[check-metrics] Missing required series matching pattern: ${pattern}" >&2
    echo "[check-metrics] Metrics dump:" >&2
    cat "$METRICS_OUTPUT" >&2 || true
    exit 1
  fi
  echo "[check-metrics] Found series matching pattern: ${pattern}" >&2
fi

echo "[check-metrics] Metrics validation passed" >&2
