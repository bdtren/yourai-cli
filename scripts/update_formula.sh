#!/usr/bin/env bash
# Renders the Homebrew formula for a release.
# Usage: scripts/update_formula.sh v0.0.1 dist/ path/to/homebrew-yourai/Formula/yourai.rb
set -euo pipefail

version="${1:?version like v0.0.1}"
dist="${2:?folder with the release archives and .sha256 files}"
out="${3:?output path of the formula, e.g. ../homebrew-yourai/Formula/yourai.rb}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
plain="${version#v}"
sha() { cut -d' ' -f1 < "$dist/yourai-${version}-$1.tar.gz.sha256"; }

mac_arm="$(sha aarch64-apple-darwin)"
mac_intel="$(sha x86_64-apple-darwin)"
linux_arm="$(sha aarch64-unknown-linux-musl)"
linux_intel="$(sha x86_64-unknown-linux-musl)"

mkdir -p "$(dirname "$out")"
sed \
  -e "s/__VERSION__/${plain}/g" \
  -e "s/__SHA_MAC_ARM__/${mac_arm}/g" \
  -e "s/__SHA_MAC_INTEL__/${mac_intel}/g" \
  -e "s/__SHA_LINUX_ARM__/${linux_arm}/g" \
  -e "s/__SHA_LINUX_INTEL__/${linux_intel}/g" \
  "$here/../packaging/homebrew/yourai.rb" > "$out"
echo "wrote $out for $version"
