#!/usr/bin/env bash
# Uploads a release to https://yourai.bdtren.com/cli/ (served by Caddy from /opt/yourai_server/shared/cli).
# Usage: scripts/publish_web.sh v0.0.1 dist/
# latest.txt is written LAST so users never see a version whose files are not uploaded yet.
# Environment: REMOTE (default clideploy@159.65.8.162), REMOTE_DIR (default /opt/yourai_server/shared/cli),
#              SSH_OPTS (extra ssh options, e.g. "-i ~/.ssh/deploy_key").
set -euo pipefail

version="${1:?version like v0.0.1}"
dist="${2:?folder with the release archives and .sha256 files}"
REMOTE="${REMOTE:-clideploy@159.65.8.162}"
REMOTE_DIR="${REMOTE_DIR:-/opt/yourai_server/shared/cli}"
SSH_OPTS="${SSH_OPTS:-}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

case "$version" in v[0-9]*) ;; *) echo "version must look like v0.0.1" >&2; exit 2 ;; esac
count="$(find "$dist" -maxdepth 1 -name "yourai-${version}-*" | wc -l | tr -d ' ')"
[ "$count" -ge 12 ] || { echo "expected 6 archives + 6 .sha256 files in $dist, found $count" >&2; exit 2; }

# Commands are built locally on purpose (SC2029).
# shellcheck disable=SC2086,SC2029
ssh_cmd() { ssh $SSH_OPTS "$REMOTE" "$@"; }
# shellcheck disable=SC2086
rsync_to() { rsync -av --chmod=D755,F644 -e "ssh $SSH_OPTS" "$@"; }

echo "==> preparing $REMOTE:$REMOTE_DIR"
ssh_cmd "mkdir -p '$REMOTE_DIR/releases/$version'"

echo "==> uploading release archives"
rsync_to "$dist"/yourai-"${version}"-* "$REMOTE:$REMOTE_DIR/releases/$version/"

echo "==> uploading installers and catalog"
rsync_to "$root/scripts/install.sh" "$root/scripts/install.ps1" "$REMOTE:$REMOTE_DIR/"
# Each catalog format has its own file so older CLIs keep refreshing the format they understand.
catalog_version="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$root/catalog/catalog.json")"
rsync_to "$root/catalog/catalog.json" "$REMOTE:$REMOTE_DIR/catalog.v${catalog_version}.json"

if compgen -G "$root/dist-engines/*.sha256" > /dev/null; then
  echo "==> uploading engine builds from dist-engines/"
  "$root/scripts/publish_engines.sh" "$root/dist-engines"
fi

echo "==> publishing latest.txt = $version"
ssh_cmd "printf '%s\n' '$version' > '$REMOTE_DIR/latest.txt.tmp' && mv '$REMOTE_DIR/latest.txt.tmp' '$REMOTE_DIR/latest.txt' && chmod 644 '$REMOTE_DIR/latest.txt'"

echo "==> done. Check: curl -fsSL https://yourai.bdtren.com/cli/latest.txt"
