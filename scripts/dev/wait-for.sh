#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<USAGE
Usage: $0 [--timeout SECONDS] [--interval SECONDS] host port

Wait for a TCP host:port to become available.
USAGE
}

TIMEOUT=60
INTERVAL=2

while [[ $# -gt 0 ]]; do
  case "$1" in
    --timeout)
      shift
      [[ $# -gt 0 ]] || { echo "missing value for --timeout" >&2; exit 1; }
      TIMEOUT="$1"
      ;;
    --interval)
      shift
      [[ $# -gt 0 ]] || { echo "missing value for --interval" >&2; exit 1; }
      INTERVAL="$1"
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    --*)
      echo "unknown option: $1" >&2
      usage
      exit 1
      ;;
    *)
      break
      ;;
  esac
  shift
  continue

done

if [[ $# -lt 2 ]]; then
  usage >&2
  exit 1
fi

HOST="$1"
PORT="$2"

if ! [[ "$TIMEOUT" =~ ^[0-9]+(\.[0-9]+)?$ ]]; then
  echo "invalid timeout: $TIMEOUT" >&2
  exit 1
fi

if ! [[ "$INTERVAL" =~ ^[0-9]+(\.[0-9]+)?$ ]]; then
  echo "invalid interval: $INTERVAL" >&2
  exit 1
fi

start_time=$(date +%s)

timeout_seconds=$(printf '%.0f' "$TIMEOUT" 2>/dev/null || true)
if [[ -z "$timeout_seconds" ]]; then
  timeout_seconds=${TIMEOUT%.*}
fi

while true; do
  if (exec 3<>"/dev/tcp/${HOST}/${PORT}") 2>/dev/null; then
    exec 3>&-
    break
  fi

  current_time=$(date +%s)
  elapsed=$((current_time - start_time))
  if (( elapsed >= timeout_seconds )); then
    echo "timeout waiting for ${HOST}:${PORT}" >&2
    exit 1
  fi

  sleep "$INTERVAL"
done

exit 0
