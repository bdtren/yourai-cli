#!/usr/bin/env bash
# Renders the Homebrew cask for a release.
# Usage: scripts/update_cask.sh v0.0.1 dist/ path/to/homebrew-yourai/Casks/yourai.rb
set -euo pipefail

version="${1:?version like v0.0.1}"
dist="${2:?folder with the release archives and .sha256 files}"
out="${3:?output path of the cask, e.g. ../homebrew-yourai/Casks/yourai.rb}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
plain="${version#v}"
arm="$(cut -d' ' -f1 < "$dist/yourai-${version}-aarch64-apple-darwin.tar.gz.sha256")"
intel="$(cut -d' ' -f1 < "$dist/yourai-${version}-x86_64-apple-darwin.tar.gz.sha256")"

mkdir -p "$(dirname "$out")"
sed -e "s/__VERSION__/${plain}/" -e "s/__SHA256_ARM__/${arm}/" -e "s/__SHA256_INTEL__/${intel}/" \
  "$here/../packaging/homebrew/yourai.rb" > "$out"
echo "wrote $out for $version"
