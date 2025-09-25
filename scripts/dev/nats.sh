#!/usr/bin/env bash
set -euo pipefail

RUNTIME=${CONTAINER_RUNTIME:-docker}
IMAGE=${NATS_IMAGE:-nats:2.10}
PORT=${NATS_PORT:-4222}
HTTP_PORT=${NATS_HTTP_PORT:-8222}
NAME=${NATS_CONTAINER_NAME:-lokan-nats}

if ! command -v "$RUNTIME" >/dev/null 2>&1; then
  echo "error: container runtime '$RUNTIME' not found" >&2
  exit 1
fi

ARGS=(
  run
  --rm
  --name "$NAME"
  -p "${PORT}:4222"
  -p "${HTTP_PORT}:8222"
  "$IMAGE"
  -js
  "--http_port=${HTTP_PORT}"
)

echo "Starting NATS (${IMAGE}) on ports ${PORT}/tcp and ${HTTP_PORT}/tcp via ${RUNTIME}..."
exec "$RUNTIME" "${ARGS[@]}"
