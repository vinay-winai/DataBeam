#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
if [[ -z "${VERSION}" ]]; then
  echo "Failed to read version from Cargo.toml"
  exit 1
fi

DIST_DIR="${ROOT_DIR}/dist"
mkdir -p "${DIST_DIR}"

copy_if_exists() {
  local src="$1"
  local dst="$2"
  if [[ -f "${src}" ]]; then
    cp -f "${src}" "${dst}"
    echo "Created ${dst}"
  fi
}

# macOS arm64
copy_if_exists "${DIST_DIR}/databeam-macos-arm64" "${DIST_DIR}/databeam_v${VERSION}_macos-arm64"

# Windows x64
copy_if_exists "${DIST_DIR}/databeam-windows-x64.exe" "${DIST_DIR}/databeam_v${VERSION}_windows-x64.exe"
copy_if_exists "${DIST_DIR}/databeam-windows-x64.pdb" "${DIST_DIR}/databeam_v${VERSION}_windows-x64.pdb"

if [[ -f "${DIST_DIR}/databeam_v${VERSION}_windows-x64.exe" ]]; then
  (
    cd "${DIST_DIR}"
    zip -q -r "databeam_v${VERSION}_windows-x64.zip" \
      "databeam_v${VERSION}_windows-x64.exe" \
      "databeam_v${VERSION}_windows-x64.pdb"
  )
  echo "Created ${DIST_DIR}/databeam_v${VERSION}_windows-x64.zip"
fi
