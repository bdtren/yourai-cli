#!/usr/bin/env bash
# Publishes the npm packages for a release: the six platform packages first, then `yourai`.
# Usage: scripts/npm_publish.sh v0.0.1 dist/ [--dry-run]
# Needs: node, npm, tar, unzip, and `npm login` (or NODE_AUTH_TOKEN in CI).
set -euo pipefail

version="${1:?version like v0.0.1}"
dist="$(cd "${2:?folder with the release archives}" && pwd)"
dry="${3:-}"
plain="${version#v}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

publish() {
  if [ "$dry" = "--dry-run" ]; then npm publish --access public --dry-run "$1"; else npm publish --access public "$1"; fi
}

set_version() {
  node -e '
    const fs = require("fs"); const [file, v] = process.argv.slice(1);
    const p = JSON.parse(fs.readFileSync(file, "utf8")); p.version = v;
    for (const k of Object.keys(p.optionalDependencies || {})) p.optionalDependencies[k] = v;
    fs.writeFileSync(file, JSON.stringify(p, null, 2) + "\n");' "$1" "$2"
}

# npm platform name -> Rust target
pairs="darwin-arm64:aarch64-apple-darwin darwin-x64:x86_64-apple-darwin
linux-x64:x86_64-unknown-linux-musl linux-arm64:aarch64-unknown-linux-musl
win32-x64:x86_64-pc-windows-msvc win32-arm64:aarch64-pc-windows-msvc"

for pair in $pairs; do
  plat="${pair%%:*}"; target="${pair##*:}"
  pkg="$work/cli-$plat"
  cp -R "$root/npm/cli-$plat" "$pkg"
  rm -f "$pkg/bin/.gitkeep"
  if [[ "$target" == *windows* ]]; then
    unzip -q -o "$dist/yourai-${version}-${target}.zip" yourai.exe -d "$pkg/bin"
  else
    tar -xzf "$dist/yourai-${version}-${target}.tar.gz" -C "$pkg/bin" yourai
    chmod 0755 "$pkg/bin/yourai"
  fi
  set_version "$pkg/package.json" "$plain"
  echo "==> @yourai/cli-$plat@$plain"
  publish "$pkg"
done

main="$work/yourai"
cp -R "$root/npm/yourai" "$main"
set_version "$main/package.json" "$plain"
echo "==> yourai@$plain"
publish "$main"
