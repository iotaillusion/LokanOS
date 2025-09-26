#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <bundle_dir> [private_key] [public_key]" >&2
  exit 1
fi

BUNDLE_DIR=$(realpath "$1")
PRIVATE_KEY=${2:-security/pki/dev/ota/ota_signing_private.pem}
PUBLIC_KEY=${3:-${PRIVATE_KEY%_private.pem}_public.pem}
MANIFEST_PATH="$BUNDLE_DIR/manifest.json"
SIG_DIR="$BUNDLE_DIR/sig"
CHECKSUM_PATH="$SIG_DIR/sha256sum"
SIGNATURE_PATH="$SIG_DIR/signature.pem"

if [[ ! -f "$MANIFEST_PATH" ]]; then
  echo "manifest not found at $MANIFEST_PATH" >&2
  exit 1
fi

if [[ ! -d "$SIG_DIR" ]]; then
  mkdir -p "$SIG_DIR"
fi

python3 - "$BUNDLE_DIR" "$MANIFEST_PATH" "$CHECKSUM_PATH" <<'PY'
import json
import hashlib
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

lines = []
for component in components:
    rel_path = component.get("path")
    if not isinstance(rel_path, str):
        raise SystemExit(f"component path missing or invalid: {component}")
    target = bundle / rel_path
    if not target.is_file():
        raise SystemExit(f"component file missing: {rel_path}")

    digest = hashlib.sha256()
    with target.open("rb") as fh:
        for chunk in iter(lambda: fh.read(8192), b""):
            digest.update(chunk)

    hexdigest = digest.hexdigest()
    component["sha256"] = hexdigest
    lines.append(f"{hexdigest}  {rel_path}\n")

with manifest_path.open("w", encoding="utf-8") as handle:
    json.dump(manifest, handle, indent=2)
    handle.write("\n")

with checksum_path.open("w", encoding="utf-8") as handle:
    handle.writelines(lines)
PY

TMP_SIG=$(mktemp)
trap 'rm -f "$TMP_SIG"' EXIT

openssl pkeyutl -sign -rawin -inkey "$PRIVATE_KEY" -in "$CHECKSUM_PATH" -out "$TMP_SIG"

{
  echo "-----BEGIN ED25519 SIGNATURE-----"
  base64 -w 64 "$TMP_SIG"
  echo "-----END ED25519 SIGNATURE-----"
} > "$SIGNATURE_PATH"

# Verify immediately to ensure bundle integrity
"$(dirname "$0")/verify.sh" "$BUNDLE_DIR" "$PUBLIC_KEY"
