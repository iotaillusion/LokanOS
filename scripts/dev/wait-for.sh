#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 host:port [timeout_seconds]" >&2
  exit 1
fi

target=$1
timeout=${2:-60}

if [[ $target != *:* ]]; then
  echo "error: target must be in host:port format" >&2
  exit 1
fi

host=${target%%:*}
port=${target##*:}

if [[ -z $host || -z $port ]]; then
  echo "error: invalid host or port" >&2
  exit 1
fi

end=$((SECONDS + timeout))

while (( SECONDS <= end )); do
  if bash -c "</dev/tcp/${host}/${port}" >/dev/null 2>&1; then
    exit 0
  fi
  sleep 1
done

echo "timeout waiting for ${target}" >&2
exit 1
