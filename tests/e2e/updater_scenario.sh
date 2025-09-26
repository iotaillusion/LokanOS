#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
: "${CARGO_TARGET_DIR:=$ROOT/target}"
LOG_DIR="${CARGO_TARGET_DIR}/e2e-logs"
mkdir -p "${LOG_DIR}"

UPDATER_PORT="${UPDATER_PORT:-${UPDATER_E2E_PORT:-18086}}"
STUB_HEALTH_PORT="${STUB_HEALTH_PORT:-${UPDATER_E2E_HEALTH_PORT:-18087}}"
RUST_LOG="${RUST_LOG:-info}"
ROLLBACK_THRESHOLD="${ROLLBACK_THRESHOLD:-${UPDATER_E2E_ROLLBACK_THRESHOLD:-2}}"

UPDATER_BIN_DEBUG="${CARGO_TARGET_DIR}/debug/updater"
UPDATER_BIN_RELEASE="${CARGO_TARGET_DIR}/release/updater"
UPDATER_BIN="${UPDATER_BIN:-}"

UPDATER_LOG="${LOG_DIR}/updater.log"
STUB_LOG="${LOG_DIR}/stub-health.log"
STATE_FILE="$(mktemp "${LOG_DIR}/stub-health-state.XXXXXX")"
STUB_HEALTH_SCRIPT="${LOG_DIR}/stub_health.py"

log() {
  printf '[updater-e2e] %s\n' "$*"
}

echo "[updater-e2e] using target dir: ${CARGO_TARGET_DIR}"
echo "[updater-e2e] logs: ${LOG_DIR}"

cleanup() {
  local code=$?
  log "cleaning up"
  if [[ -n "${UPDATER_PID:-}" ]] && kill -0 "${UPDATER_PID}" 2>/dev/null; then
    kill "${UPDATER_PID}" 2>/dev/null || true
    wait "${UPDATER_PID}" 2>/dev/null || true
  fi
  if [[ -n "${HEALTH_PID:-}" ]] && kill -0 "${HEALTH_PID}" 2>/dev/null; then
    kill "${HEALTH_PID}" 2>/dev/null || true
    wait "${HEALTH_PID}" 2>/dev/null || true
  fi
  rm -f "${STATE_FILE}" "${STUB_HEALTH_SCRIPT}"
  if [[ $code -ne 0 ]]; then
    echo "--- tail updater.log ---" >&2
    tail -n 200 "${UPDATER_LOG}" >&2 || true
    echo "--- head stub-health.log ---" >&2
    head -n 50 "${STUB_LOG}" >&2 || true
  fi
  exit $code
}
trap 'cleanup' EXIT

# Ensure updater binary exists (prefer release if present)
if [[ -z "${UPDATER_BIN}" ]]; then
  if [[ -x "${UPDATER_BIN_RELEASE}" ]]; then
    UPDATER_BIN="${UPDATER_BIN_RELEASE}"
  else
    log "building updater (debug)…"
    (cd "${ROOT}" && cargo build -q -p updater --bin updater)
    UPDATER_BIN="${UPDATER_BIN_DEBUG}"
  fi
fi

set_health_state() {
  printf '%s' "$1" > "${STATE_FILE}"
}

start_health_server() {
  log "starting stub health endpoint on port ${STUB_HEALTH_PORT}"
  set_health_state ok
  if command -v python3 >/dev/null 2>&1; then
    cat > "${STUB_HEALTH_SCRIPT}" <<'PY'
import http.server
import json
import os
import socketserver
from pathlib import Path

port = int(os.environ.get("STUB_HEALTH_PORT", "18087"))
state_path = Path(os.environ["STATE_FILE"])

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        state = "ok"
        if state_path.exists():
            try:
                state = state_path.read_text(encoding="utf-8").strip() or "ok"
            except OSError:
                state = "ok"
        payload = json.dumps({"status": state}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *_):
        return

with socketserver.TCPServer(("127.0.0.1", port), Handler) as httpd:
    httpd.serve_forever()
PY
    STUB_HEALTH_PORT="${STUB_HEALTH_PORT}" STATE_FILE="${STATE_FILE}" nohup python3 "${STUB_HEALTH_SCRIPT}" > "${STUB_LOG}" 2>&1 &
    HEALTH_PID=$!
  else
    nohup bash -c '
      STATE_FILE="$1"
      PORT="$2"
      while true; do
        if [[ -f "$STATE_FILE" ]]; then
          STATE=$(<"$STATE_FILE")
        else
          STATE="ok"
        fi
        if [[ "$STATE" == "fail" ]]; then
          BODY="{\"status\":\"fail\"}"
        else
          BODY="{\"status\":\"ok\"}"
        fi
        printf "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: %s\r\n\r\n%s" "${#BODY}" "$BODY" | nc -l -s 127.0.0.1 -p "$PORT" -q 1
      done
    ' bash "${STATE_FILE}" "${STUB_HEALTH_PORT}" > "${STUB_LOG}" 2>&1 &
    HEALTH_PID=$!
  fi
}

start_updater() {
  log "starting updater service on port ${UPDATER_PORT}"
  rm -rf "${ROOT}/data/updater"
  : > "${UPDATER_LOG}"
  (
    cd "${ROOT}"
    nohup env \
      RUST_LOG="${RUST_LOG}" \
      UPDATER_PORT="${UPDATER_PORT}" \
      UPDATER_HEALTH_ENDPOINTS="http://127.0.0.1:${STUB_HEALTH_PORT}/health" \
      UPDATER_HEALTH_DEADLINE_SECS=3 \
      UPDATER_HEALTH_QUORUM=1 \
      "${UPDATER_BIN}" --port "${UPDATER_PORT}" > "${UPDATER_LOG}" 2>&1 &
    echo $! > "${LOG_DIR}/updater.pid"
  )
  UPDATER_PID=$(<"${LOG_DIR}/updater.pid")
  rm -f "${LOG_DIR}/updater.pid"
}

wait_for_service() {
  local health_ok=0
  for url in "http://127.0.0.1:${UPDATER_PORT}/v1/health" "http://127.0.0.1:${UPDATER_PORT}/health"; do
    for _ in $(seq 1 90); do
      if curl -fsS "$url" >/dev/null; then
        log "health OK at $url"
        health_ok=1
        break 2
      fi
      sleep 1
    done
  done

  if [[ "$health_ok" != "1" ]]; then
    echo "::error::updater service did not become healthy"
    echo "--- tail updater.log ---"
    tail -n 200 "${UPDATER_LOG}" || true
    echo "--- head stub-health.log ---"
    head -n 50 "${STUB_LOG}" || true
    exit 2
  fi
}

run_post() {
  local url=$1
  local payload=$2
  local outfile=$3
  local code
  set +e
  code=$(curl -sS -o "$outfile" -w "%{http_code}" -H 'Content-Type: application/json' -X POST -d "$payload" "$url")
  local status=$?
  set -e
  if [[ $status -ne 0 ]]; then
    echo "request to $url failed" >&2
    cat "$outfile" >&2 || true
    exit $status
  fi
  printf '%s\n' "$code"
}

assert_slot_response() {
  local outfile=$1
  local expected_slot=$2
  python3 - "$outfile" "$expected_slot" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
expected = sys.argv[2]

contents = path.read_text(encoding='utf-8')
if not contents.strip():
    raise SystemExit(f"empty response body in {path}")

try:
    data = json.loads(contents)
except json.JSONDecodeError as exc:
    raise SystemExit(f"failed to parse JSON response in {path}: {exc}")

slot = data.get('slot')
if slot != expected:
    raise SystemExit(f"expected slot {expected}, got {slot}")
PY
}

assert_error_contains() {
  local outfile=$1
  local needle=$2
  python3 - "$outfile" "$needle" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
needle = sys.argv[2]

contents = path.read_text(encoding='utf-8')
if not contents.strip():
    raise SystemExit(f"empty error response in {path}")

try:
    data = json.loads(contents)
except json.JSONDecodeError as exc:
    raise SystemExit(f"failed to parse error JSON in {path}: {exc}")

message = data.get('error', '')
if needle not in message:
    raise SystemExit(f"expected error containing '{needle}', got '{message}'")
PY
}

verify_status_success() {
  local tmp=$(mktemp)
  curl -sS "http://127.0.0.1:$UPDATER_PORT/v1/update/status" -o "$tmp"
  python3 - "$tmp" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
contents = path.read_text(encoding='utf-8')
if not contents.strip():
    raise SystemExit("status response was empty")

state = json.loads(contents)
if state.get('active') != 'B':
    raise SystemExit(f"expected active slot B, got {state.get('active')}")
if state.get('previous_active') != 'A':
    raise SystemExit(f"expected previous_active A, got {state.get('previous_active')}")
if state.get('staging') is not None:
    raise SystemExit(f"expected no staging slot, got {state.get('staging')}")
if state.get('last_failed') is not None:
    raise SystemExit(f"expected no failed slot, got {state.get('last_failed')}")
slots = state.get('slots', {})
if slots.get('B', {}).get('state') != 'ACTIVE':
    raise SystemExit('slot B should be ACTIVE after successful commit')
if slots.get('A', {}).get('state') != 'INACTIVE':
    raise SystemExit('slot A should be INACTIVE after successful commit')
PY
  rm -f "$tmp"
}

verify_status_failure() {
  local tmp=$(mktemp)
  curl -sS "http://127.0.0.1:$UPDATER_PORT/v1/update/status" -o "$tmp"
  python3 - "$tmp" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
contents = path.read_text(encoding='utf-8')
if not contents.strip():
    raise SystemExit("status response was empty")

state = json.loads(contents)
if state.get('active') != 'B':
    raise SystemExit(f"expected active slot B, got {state.get('active')}")
if state.get('last_failed') != 'A':
    raise SystemExit(f"expected last_failed A, got {state.get('last_failed')}")
slots = state.get('slots', {})
if slots.get('A', {}).get('state') != 'BAD':
    raise SystemExit('slot A should be marked BAD after failed commits')
if slots.get('B', {}).get('state') != 'ACTIVE':
    raise SystemExit('slot B should remain ACTIVE after rollback scenario')
PY
  rm -f "$tmp"
}

stage_bundle() {
  local bundle=$1
  local expected_slot=$2
  local tmp=$(mktemp)
  log "staging bundle $bundle"
  local code=$(run_post "http://127.0.0.1:$UPDATER_PORT/v1/update/stage" "{\"artifact\":\"$bundle\"}" "$tmp")
  if [[ "$code" != "202" ]]; then
    cat "$tmp" >&2
    echo "unexpected HTTP $code while staging" >&2
    exit 1
  fi
  log "stage response: $(cat "$tmp")"
  assert_slot_response "$tmp" "$expected_slot"
  rm -f "$tmp"
}

commit_expect_success() {
  local expected_slot=$1
  local tmp=$(mktemp)
  log "committing staged slot (expect success)"
  local code=$(run_post "http://127.0.0.1:$UPDATER_PORT/v1/update/commit" '{}' "$tmp")
  if [[ "$code" != "200" ]]; then
    cat "$tmp" >&2
    echo "unexpected HTTP $code during commit" >&2
    exit 1
  fi
  log "commit response: $(cat "$tmp")"
  assert_slot_response "$tmp" "$expected_slot"
  rm -f "$tmp"
}

commit_expect_failure() {
  local tmp=$(mktemp)
  log "committing staged slot (expect failure)"
  local code=$(run_post "http://127.0.0.1:$UPDATER_PORT/v1/update/commit" '{}' "$tmp")
  if [[ "$code" != "503" && "$code" != "500" ]]; then
    cat "$tmp" >&2
    echo "expected HTTP 503 during failed commit, got $code" >&2
    exit 1
  fi
  if [[ "$code" == "503" ]]; then
    assert_error_contains "$tmp" 'health check quorum not satisfied'
  else
    assert_error_contains "$tmp" 'http error'
  fi
  rm -f "$tmp"
}

main() {
  start_health_server
  start_updater
  wait_for_service

  local happy_version="e2e-happy-$(date +%s)"
  local happy_bundle="$ROOT/dist/ota/lokan-$happy_version"
  log "building healthy OTA bundle targeting slot B"
  OTA_VERSION="$happy_version" OTA_TARGET_SLOT=B "$ROOT/os/images/build.sh" >/dev/null
  stage_bundle "$happy_bundle" B
  commit_expect_success B
  verify_status_success

  sleep 1
  local bad_version="e2e-bad-$(date +%s)"
  local bad_bundle="$ROOT/dist/ota/lokan-$bad_version"
  log "building faulty OTA bundle targeting slot A"
  OTA_VERSION="$bad_version" OTA_TARGET_SLOT=A "$ROOT/os/images/build.sh" >/dev/null
  stage_bundle "$bad_bundle" A

  set_health_state fail
  for attempt in $(seq 1 "$ROLLBACK_THRESHOLD"); do
    if [[ "$attempt" -gt 1 ]]; then
      stage_bundle "$bad_bundle" A
    fi
    commit_expect_failure
  done

  verify_status_failure
  log "scenario completed successfully"
}

main
