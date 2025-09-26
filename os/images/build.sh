#!/usr/bin/env bash
set -euo pipefail

umask 022

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)
OUTPUT_DIR="$REPO_ROOT/dist/ota"
WORK_DIR="$REPO_ROOT/target/ota-build"

mkdir -p "$OUTPUT_DIR" "$WORK_DIR"

log() {
  printf '[ota-build] %s\n' "$*"
}

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found in PATH" >&2
  exit 1
fi

BUILD_SHA=$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo dev)
VERSION=${OTA_VERSION:-$(git -C "$REPO_ROOT" describe --tags --dirty --always 2>/dev/null || echo "0.0.0-dev")}
TARGET_SLOT=${OTA_TARGET_SLOT:-A}
BUILD_TIME=$(date -u +%Y-%m-%dT%H:%M:%SZ)
EPOCH_DEFAULT=$(git -C "$REPO_ROOT" log -1 --format=%ct 2>/dev/null || date -u +%s)
SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$EPOCH_DEFAULT}

log "building workspace binaries"
(
  cd "$REPO_ROOT"
  BUILD_SHA="$BUILD_SHA" BUILD_TIME="$BUILD_TIME" cargo build --workspace --all-targets --release
)

ROOTFS_STAGE="$WORK_DIR/rootfs"
BOOT_STAGE="$WORK_DIR/boot"
BUNDLE_NAME="lokan-$VERSION"
BUNDLE_DIR="$WORK_DIR/$BUNDLE_NAME"
IMAGES_DIR="$BUNDLE_DIR/images"
SIG_DIR="$BUNDLE_DIR/sig"
ROOTFS_IMG="$IMAGES_DIR/rootfs.img"
BOOT_IMG="$IMAGES_DIR/boot.img"
MANIFEST_PATH="$BUNDLE_DIR/manifest.json"

rm -rf "$ROOTFS_STAGE" "$BOOT_STAGE" "$BUNDLE_DIR"
mkdir -p "$ROOTFS_STAGE/usr/bin" "$BOOT_STAGE" "$IMAGES_DIR" "$SIG_DIR"

mapfile -t RELEASE_BINS < <(
  find "$REPO_ROOT/target/release" -maxdepth 1 -type f -perm -u+x ! -name "*.d" -printf '%f\n' 2>/dev/null | LC_ALL=C sort
)

if [[ ${#RELEASE_BINS[@]} -eq 0 ]]; then
  log "no release binaries found, creating placeholder"
  printf 'lokan os rootfs placeholder\n' > "$ROOTFS_STAGE/README.txt"
  chmod 0644 "$ROOTFS_STAGE/README.txt"
else
  for bin in "${RELEASE_BINS[@]}"; do
    install -m 0755 "$REPO_ROOT/target/release/$bin" "$ROOTFS_STAGE/usr/bin/$bin"
  done
fi

cat > "$BOOT_STAGE/boot.cfg" <<CFG
# LokanOS development boot configuration
version=$VERSION
build_sha=$BUILD_SHA
CFG
chmod 0644 "$BOOT_STAGE/boot.cfg"

find "$ROOTFS_STAGE" -print0 | xargs -0 touch -h -d "@$SOURCE_DATE_EPOCH" >/dev/null 2>&1 || true
find "$BOOT_STAGE" -print0 | xargs -0 touch -h -d "@$SOURCE_DATE_EPOCH" >/dev/null 2>&1 || true

log "creating payload images"

tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
    --pax-option=exthdr.name=%d/PaxHeaders/%f,delete=atime,delete=ctime \
    -C "$ROOTFS_STAGE" -cf "$ROOTFS_IMG" .

tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
    --pax-option=exthdr.name=%d/PaxHeaders/%f,delete=atime,delete=ctime \
    -C "$BOOT_STAGE" -cf "$BOOT_IMG" .

find "$BUNDLE_DIR" -type d -print0 | xargs -0 touch -h -d "@$SOURCE_DATE_EPOCH" >/dev/null 2>&1 || true

SBOM_PATH=${OTA_SBOM_PATH:-$REPO_ROOT/dist/sbom.json}
SBOM_SHA256=""
if [[ -f "$SBOM_PATH" ]]; then
  SBOM_SHA256=$(sha256sum "$SBOM_PATH" | awk '{print $1}')
  install -m 0644 "$SBOM_PATH" "$BUNDLE_DIR/$(basename "$SBOM_PATH")"
  touch -h -d "@$SOURCE_DATE_EPOCH" "$BUNDLE_DIR/$(basename "$SBOM_PATH")" || true
fi

log "writing manifest"
python3 - "$MANIFEST_PATH" "$VERSION" "$BUILD_SHA" "$BUILD_TIME" "$TARGET_SLOT" "$SBOM_SHA256" <<'PY'
import json
import pathlib
import sys

manifest_path = pathlib.Path(sys.argv[1])
version, build_sha, created_at, target_slot, sbom_sha = sys.argv[2:7]

manifest = {
    "version": version,
    "build_sha": build_sha,
    "created_at": created_at,
    "target_slot": target_slot,
    "components": [
        {"name": "rootfs", "path": "images/rootfs.img", "sha256": ""},
        {"name": "boot", "path": "images/boot.img", "sha256": ""},
    ],
}
if sbom_sha:
    manifest["sbom_sha256"] = sbom_sha

manifest_path.parent.mkdir(parents=True, exist_ok=True)
with manifest_path.open("w", encoding="utf-8") as handle:
    json.dump(manifest, handle, indent=2, sort_keys=True)
    handle.write("\n")
PY

touch -h -d "@$SOURCE_DATE_EPOCH" "$MANIFEST_PATH" || true

log "signing bundle"
"$REPO_ROOT/scripts/ota/sign.sh" "$BUNDLE_DIR"

log "assembling distribution artifacts"
rm -rf "$OUTPUT_DIR/$BUNDLE_NAME"
cp -a "$BUNDLE_DIR" "$OUTPUT_DIR/"

tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
    --pax-option=exthdr.name=%d/PaxHeaders/%f,delete=atime,delete=ctime \
    -C "$WORK_DIR" -cf "$OUTPUT_DIR/$BUNDLE_NAME.tar" "$BUNDLE_NAME"

touch -h -d "@$SOURCE_DATE_EPOCH" "$OUTPUT_DIR/$BUNDLE_NAME.tar" || true

log "bundle ready at $OUTPUT_DIR/$BUNDLE_NAME.tar"
