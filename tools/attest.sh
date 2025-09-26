#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
OUTPUT_PATH=${1:-$REPO_ROOT/dist/lokanos.att.json}

mkdir -p "$(dirname "$OUTPUT_PATH")"
TMP_FILE=$(mktemp)

cleanup() {
  rm -f "$TMP_FILE"
}
trap cleanup EXIT

BUILD_SHA=$(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || echo "unknown")
BUILD_TIME=$(date -u +%Y-%m-%dT%H:%M:%SZ)
BUILDER_ID=${BUILDER_ID:-$(whoami 2>/dev/null || echo "unknown")@$(hostname 2>/dev/null || echo "unknown")}

REPO_ROOT="$REPO_ROOT" BUILD_SHA="$BUILD_SHA" BUILD_TIME="$BUILD_TIME" BUILDER_ID="$BUILDER_ID" python3 - "$TMP_FILE" <<'PY'
import json
import os
import sys
import subprocess

output_path = sys.argv[1]
repo_root = os.environ["REPO_ROOT"]

result = subprocess.run(
    ["git", "-C", repo_root, "ls-files"],
    check=True,
    capture_output=True,
    text=True,
)
inputs = [line.strip() for line in result.stdout.splitlines() if line.strip()]

attestation = {
    "_type": "https://in-toto.io/Statement/v0.1",
    "predicateType": "https://slsa.dev/provenance/v1",
    "subject": [
        {"name": "repo", "digest": {"sha1": os.environ["BUILD_SHA"]}},
    ],
    "predicate": {
        "buildType": "https://lokanos.dev/build",
        "builder": {"id": os.environ["BUILDER_ID"]},
        "buildStartedOn": os.environ["BUILD_TIME"],
        "buildFinishedOn": os.environ["BUILD_TIME"],
        "invocation": {
            "configSource": {
                "uri": os.environ["REPO_ROOT"],
                "digest": {"sha1": os.environ["BUILD_SHA"]},
            },
            "parameters": {},
        },
        "materials": [
            {"uri": f"git+file://{os.environ['REPO_ROOT']}", "digest": {"sha1": os.environ["BUILD_SHA"]}},
        ],
        "metadata": {
            "inputs": inputs,
        },
    },
}

with open(output_path, "w", encoding="utf-8") as handle:
    json.dump(attestation, handle, indent=2)
    handle.write("\n")
PY

mv "$TMP_FILE" "$OUTPUT_PATH"
trap - EXIT
rm -f "$TMP_FILE"

echo "Attestation written to $OUTPUT_PATH"
