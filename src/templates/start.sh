#!/usr/bin/env sh
# Starts this YourAI project in the background. Requires the `yourai` CLI on PATH.
# Side effects: launches the local AI server and writes logs to .yourai/server.log.
set -eu
cd "$(dirname "$0")/.."
exec yourai start "$@"
