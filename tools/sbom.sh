#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
OUTPUT_PATH=${1:-$REPO_ROOT/dist/lokanos.sbom.json}

mkdir -p "$(dirname "$OUTPUT_PATH")"
TMP_FILE=$(mktemp)

cleanup() {
  rm -f "$TMP_FILE"
}
trap cleanup EXIT

TIMESTAMP=$(date -u +%Y-%m-%dT%H:%M:%SZ)
VERSION=$(git -C "$REPO_ROOT" describe --tags --dirty --always 2>/dev/null || echo "0.0.0-dev")

if command -v syft >/dev/null 2>&1; then
  syft "$REPO_ROOT" -o cyclonedx-json >"$TMP_FILE"
else
  TIMESTAMP="$TIMESTAMP" VERSION="$VERSION" python3 - "$TMP_FILE" <<'PY'
import json
import os
import sys
import uuid

output_path = sys.argv[1]

bom = {
    "bomFormat": "CycloneDX",
    "specVersion": "1.5",
    "serialNumber": f"urn:uuid:{uuid.uuid4()}",
    "version": 1,
    "metadata": {
        "timestamp": os.environ["TIMESTAMP"],
        "tools": [
            {
                "vendor": "LokanOS",
                "name": "sbom.sh",
                "version": "0.1",
            }
        ],
        "component": {
            "type": "application",
            "bom-ref": "lokanos",
            "name": "LokanOS",
            "version": os.environ["VERSION"],
        },
    },
    "components": [],
}

with open(output_path, "w", encoding="utf-8") as handle:
    json.dump(bom, handle, indent=2)
    handle.write("\n")
PY
fi

mv "$TMP_FILE" "$OUTPUT_PATH"
trap - EXIT
rm -f "$TMP_FILE"

echo "SBOM written to $OUTPUT_PATH"
