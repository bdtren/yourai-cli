#!/usr/bin/env sh
# Stops this YourAI project's server. Requires the `yourai` CLI on PATH. Does not delete anything.
set -eu
cd "$(dirname "$0")/.."
exec yourai stop "$@"
