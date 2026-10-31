#!/usr/bin/env bash
# Packages a built binary into dist/yourai-<version>-<target>.(tar.gz|zip) plus a .sha256 file.
# Usage: scripts/package.sh v0.0.1 aarch64-apple-darwin [path/to/binary]
set -euo pipefail

version="${1:?version like v0.0.1}"
target="${2:?rust target triple}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exe="yourai"; [[ "$target" == *windows* ]] && exe="yourai.exe"
bin="${3:-$root/target/$target/release/$exe}"
[ -f "$bin" ] || { echo "binary not found: $bin" >&2; exit 1; }

dist="$root/dist"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$dist"
cp "$bin" "$stage/$exe"
cp "$root/LICENSE" "$root/README.md" "$stage/"

name="yourai-${version}-${target}"
if [[ "$target" == *windows* ]]; then
  file="$name.zip"
  rm -f "$dist/$file"
  if command -v zip >/dev/null 2>&1; then
    (cd "$stage" && zip -q -9 "$dist/$file" "$exe" LICENSE README.md)
  else
    (cd "$stage" && 7z a -tzip -bd "$dist/$file" "$exe" LICENSE README.md >/dev/null)
  fi
else
  file="$name.tar.gz"
  chmod 0755 "$stage/$exe"
  tar -C "$stage" -czf "$dist/$file" "$exe" LICENSE README.md
fi

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$dist" && sha256sum "$file" > "$file.sha256")
else
  (cd "$dist" && shasum -a 256 "$file" > "$file.sha256")
fi
echo "packaged $dist/$file"
