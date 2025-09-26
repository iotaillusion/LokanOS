#!/usr/bin/env bash
set -euo pipefail

marker="/var/tmp/.lokan_rust_bootstrap"
if [[ -f "$marker" ]]; then
  exit 0
fi

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends \
  build-essential \
  ca-certificates \
  curl \
  libsqlite3-dev \
  libssl-dev \
  pkg-config \
  protobuf-compiler
rm -rf /var/lib/apt/lists/*

touch "$marker"
