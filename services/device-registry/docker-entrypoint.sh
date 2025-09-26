#!/bin/busybox sh
set -eu

POSTGRES_HOST="${DEVICE_REGISTRY_POSTGRES_HOST:-postgres}"
POSTGRES_PORT="${DEVICE_REGISTRY_POSTGRES_PORT:-5432}"
POSTGRES_USER="${DEVICE_REGISTRY_POSTGRES_USER:-postgres}"
POSTGRES_PASSWORD="${DEVICE_REGISTRY_POSTGRES_PASSWORD:-postgres}"
POSTGRES_DB="${DEVICE_REGISTRY_POSTGRES_DB:-device_registry}"

DEFAULT_SQLITE_URL="${DEVICE_REGISTRY_DEFAULT_SQLITE_URL:-sqlite:///var/lib/device-registry/dev.db}"
DEFAULT_POSTGRES_URL="${DEVICE_REGISTRY_DEFAULT_POSTGRES_URL:-postgres://${POSTGRES_USER}:${POSTGRES_PASSWORD}@${POSTGRES_HOST}:${POSTGRES_PORT}/${POSTGRES_DB}}"

backend="${DEVICE_REGISTRY_BACKEND:-auto}"
db_url="${DEVICE_REGISTRY_DATABASE_URL:-}"

probe_postgres() {
  /bin/busybox nc -z -w1 "$POSTGRES_HOST" "$POSTGRES_PORT" >/dev/null 2>&1
}

if [ "$backend" = "auto" ]; then
  if [ -n "$db_url" ]; then
    case "$db_url" in
      postgres://*|postgresql://*)
        backend="postgres"
        ;;
      sqlite://*)
        backend="sqlite"
        ;;
      *)
        if probe_postgres; then
          backend="postgres"
          db_url="$DEFAULT_POSTGRES_URL"
        else
          backend="sqlite"
          db_url="$DEFAULT_SQLITE_URL"
        fi
        ;;
    esac
  else
    if probe_postgres; then
      backend="postgres"
      db_url="$DEFAULT_POSTGRES_URL"
    else
      backend="sqlite"
      db_url="$DEFAULT_SQLITE_URL"
    fi
  fi
fi

case "$backend" in
  postgres)
    if [ -z "$db_url" ] || [ "${db_url%%://*}" = "sqlite" ]; then
      db_url="$DEFAULT_POSTGRES_URL"
    fi
    export DEVICE_REGISTRY_DATABASE_URL="$db_url"
    tries="${DEVICE_REGISTRY_POSTGRES_RETRIES:-60}"
    while [ "$tries" -gt 0 ]; do
      if probe_postgres; then
        break
      fi
      tries=$((tries - 1))
      /bin/busybox sleep 1
    done
    if [ "$tries" -eq 0 ]; then
      echo "device-registry: timed out waiting for postgres at ${POSTGRES_HOST}:${POSTGRES_PORT}, continuing" >&2
    fi
    echo "device-registry: starting postgres backend -> $db_url" >&2
    exec /usr/local/bin/device-registry-postgres "$@"
    ;;
  sqlite)
    if [ -z "$db_url" ] || [ "${db_url%%://*}" = "postgres" ] || [ "${db_url%%://*}" = "postgresql" ]; then
      db_url="$DEFAULT_SQLITE_URL"
    fi
    export DEVICE_REGISTRY_DATABASE_URL="$db_url"
    echo "device-registry: starting sqlite backend -> $db_url" >&2
    exec /usr/local/bin/device-registry-sqlite "$@"
    ;;
  *)
    echo "device-registry: unknown backend '$backend', defaulting to sqlite" >&2
    export DEVICE_REGISTRY_DATABASE_URL="$DEFAULT_SQLITE_URL"
    exec /usr/local/bin/device-registry-sqlite "$@"
    ;;
esac

