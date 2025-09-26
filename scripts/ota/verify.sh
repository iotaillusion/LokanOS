#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <bundle_dir> [public_key]" >&2
  exit 1
fi

BUNDLE_DIR=$(realpath "$1")
PUBLIC_KEY=${2:-security/pki/dev/ota/ota_signing_public.pem}
MANIFEST_PATH="$BUNDLE_DIR/manifest.json"
CHECKSUM_PATH="$BUNDLE_DIR/sig/sha256sum"
SIGNATURE_PATH="$BUNDLE_DIR/sig/signature.pem"

if [[ ! -f "$MANIFEST_PATH" ]]; then
  echo "manifest not found at $MANIFEST_PATH" >&2
  exit 1
fi

if [[ ! -f "$CHECKSUM_PATH" ]]; then
  echo "checksum file not found at $CHECKSUM_PATH" >&2
  exit 1
fi

if [[ ! -f "$SIGNATURE_PATH" ]]; then
  echo "signature file not found at $SIGNATURE_PATH" >&2
  exit 1
fi

python3 - "$BUNDLE_DIR" "$MANIFEST_PATH" "$CHECKSUM_PATH" <<'PY'
import hashlib
import json
import pathlib
import sys

bundle = pathlib.Path(sys.argv[1])
manifest_path = pathlib.Path(sys.argv[2])
checksum_path = pathlib.Path(sys.argv[3])

with manifest_path.open("r", encoding="utf-8") as handle:
    manifest = json.load(handle)

components = manifest.get("components", [])
if not components:
    raise SystemExit("manifest must include at least one component")

expected = {}
with checksum_path.open("r", encoding="utf-8") as handle:
    for line_number, line in enumerate(handle, start=1):
        stripped = line.strip()
        if not stripped:
            continue
        parts = stripped.split()
        if len(parts) != 2:
            raise SystemExit(f"invalid sha256 entry on line {line_number}: {line.rstrip()}")
        if parts[1] in expected:
            raise SystemExit(
                f"duplicate checksum entry for {parts[1]} on line {line_number}"
            )
        expected[parts[1]] = parts[0]

for component in components:
    rel_path = component.get("path")
    if not isinstance(rel_path, str):
        raise SystemExit(f"component path missing or invalid: {component}")
    digest_hex = component.get("sha256")
    if not isinstance(digest_hex, str):
        raise SystemExit(f"component sha256 missing or invalid: {component}")

    target = bundle / rel_path
    if not target.is_file():
        raise SystemExit(f"component file missing: {rel_path}")

    digest = hashlib.sha256()
    with target.open("rb") as fh:
        for chunk in iter(lambda: fh.read(8192), b""):
            digest.update(chunk)
    computed = digest.hexdigest()

    if computed != digest_hex.lower():
        raise SystemExit(f"checksum mismatch for {rel_path}: manifest={digest_hex} computed={computed}")

    sha_value = expected.get(rel_path)
    if sha_value is None:
        raise SystemExit(f"checksum entry missing for {rel_path}")
    if sha_value.lower() != computed:
        raise SystemExit(f"checksum file mismatch for {rel_path}: file={sha_value} manifest={computed}")

component_paths = {component["path"] for component in components}

for entry in expected:
    if entry not in component_paths:
        raise SystemExit(f"unexpected checksum entry: {entry}")
PY

TMP_SIG=$(mktemp)
trap 'rm -f "$TMP_SIG"' EXIT

awk '/-----BEGIN/{flag=1;next}/-----END/{flag=0}flag' "$SIGNATURE_PATH" | base64 -d > "$TMP_SIG"

openssl pkeyutl -verify -rawin -pubin -inkey "$PUBLIC_KEY" -sigfile "$TMP_SIG" -in "$CHECKSUM_PATH"
