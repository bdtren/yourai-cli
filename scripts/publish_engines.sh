#!/usr/bin/env bash
# Uploads engine archives built by scripts/build_engine.sh (or the "engines" workflow)
# to https://yourai.bdtren.com/cli/engines/<engine>/<version>/.
# Usage: scripts/publish_engines.sh [dist-engines]
#   To fetch a CI build first: gh release download engines-<N> -R bdtren/yourai-cli -D dist-engines
# Environment: REMOTE, REMOTE_DIR, SSH_OPTS (same as publish_web.sh).
set -euo pipefail

dist="${1:-dist-engines}"
REMOTE="${REMOTE:-clideploy@159.65.8.162}"
REMOTE_DIR="${REMOTE_DIR:-/opt/yourai_server/shared/cli}"
SSH_OPTS="${SSH_OPTS:-}"

# shellcheck disable=SC2086,SC2029
ssh_cmd() { ssh $SSH_OPTS "$REMOTE" "$@"; }
# shellcheck disable=SC2086
rsync_to() { rsync -av --chmod=D755,F644 -e "ssh $SSH_OPTS" "$@"; }

found=0
for f in "$dist"/*.tar.gz "$dist"/*.zip; do
  [ -f "$f" ] || continue
  [ -f "$f.sha256" ] || { echo "missing $f.sha256" >&2; exit 2; }
  base="$(basename "$f")"
  # <engine>-<version>-<os>-<arch>-<backend>.<ext>
  case "$base" in
    sd-server-*) engine=sd-server ;;
    whisper-server-*) engine=whisper-server ;;
    *) echo "skipping $base" >&2; continue ;;
  esac
  rest="${base#"$engine"-}"
  version="$(printf '%s' "$rest" | sed -E 's/-(macos|linux|windows)-.*$//')"
  dest="$REMOTE_DIR/engines/$engine/$version"
  ssh_cmd "mkdir -p '$dest'"
  rsync_to "$f" "$f.sha256" "$REMOTE:$dest/"
  found=$((found + 1))
done
[ "$found" -gt 0 ] || { echo "no engine archives in $dist" >&2; exit 2; }
echo "==> uploaded $found engine archives"
