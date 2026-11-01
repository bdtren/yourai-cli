#!/usr/bin/env bash
# Builds one engine server from source and packages it for ~/.yourai/engines.
# Used by .github/workflows/engines.yml for the targets upstream does not ship
# (or ships only for very new macOS), and works the same on a developer machine.
#
# Usage: scripts/build_engine.sh <sd-server|whisper-server> <key> [out_dir]
#   key: macos-aarch64-metal, macos-x86_64-cpu, linux-aarch64-cpu, windows-aarch64-cpu, ...
# Output: <out_dir>/<engine>-<version>-<key>.(tar.gz|zip) and a matching .sha256 file.
set -euo pipefail

engine="${1:?engine: sd-server or whisper-server}"
key="${2:?key like macos-aarch64-metal}"
out="${3:-dist-engines}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="${WORK_DIR:-$root/target/engine-build}"
mkdir -p "$work" "$out"
out="$(cd "$out" && pwd)"

# Pinned sources. Bump together with catalog/catalog.json.
SD_REPO=https://github.com/leejet/stable-diffusion.cpp
SD_REF=master-1031-c6dfa9d
WHISPER_REPO=https://github.com/ggml-org/whisper.cpp
WHISPER_REF=v1.9.5

os="${key%%-*}"
rest="${key#*-}"
arch="${rest%%-*}"
backend="${rest#*-}"

flags=(-DCMAKE_BUILD_TYPE=Release -DGGML_NATIVE=OFF -DBUILD_SHARED_LIBS=OFF)
case "$os" in
  macos)
    flags+=(-DCMAKE_OSX_DEPLOYMENT_TARGET=13.3)
    [ "$arch" = x86_64 ] && flags+=(-DCMAKE_OSX_ARCHITECTURES=x86_64)
    [ "$arch" = aarch64 ] && flags+=(-DCMAKE_OSX_ARCHITECTURES=arm64)
    ;;
esac

case "$engine" in
  sd-server)
    repo="$SD_REPO"; ref="$SD_REF"; version="$SD_REF"; target=sd-server
    flags+=(-DSD_BUILD_SHARED_LIBS=OFF -DSD_WEBM=ON -DSD_WEBP=ON)
    case "$backend" in
      metal)  flags+=(-DSD_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON) ;;
      vulkan) flags+=(-DSD_VULKAN=ON) ;;
      cuda)   flags+=(-DSD_CUDA=ON) ;;
      cpu)    [ "$os" = macos ] && flags+=(-DGGML_METAL=OFF) ;;
      *) echo "unknown backend $backend" >&2; exit 2 ;;
    esac
    ;;
  whisper-server)
    repo="$WHISPER_REPO"; ref="$WHISPER_REF"; version="$WHISPER_REF"; target=whisper-server
    flags+=(-DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_EXAMPLES=ON -DWHISPER_BUILD_SERVER=ON)
    case "$backend" in
      metal)  flags+=(-DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON) ;;
      vulkan) flags+=(-DGGML_VULKAN=ON) ;;
      cuda)   flags+=(-DGGML_CUDA=ON) ;;
      cpu)    [ "$os" = macos ] && flags+=(-DGGML_METAL=OFF) ;;
      *) echo "unknown backend $backend" >&2; exit 2 ;;
    esac
    ;;
  *) echo "engine must be sd-server or whisper-server" >&2; exit 2 ;;
esac

src="$work/$engine-$ref"
if [ ! -d "$src/.git" ]; then
  git clone -q --depth 1 --branch "$ref" --recurse-submodules --shallow-submodules "$repo" "$src"
fi
build="$src/build-$key"
cmake -S "$src" -B "$build" "${flags[@]}"
cmake --build "$build" --config Release -j "${JOBS:-8}" --target "$target"

exe_name="$target"
[ "$os" = windows ] && exe_name="$target.exe"
exe="$(find "$build" -type f -name "$exe_name" -perm -u+x 2>/dev/null | head -1)"
[ -n "$exe" ] || exe="$(find "$build" -type f -name "$exe_name" | head -1)"
[ -n "$exe" ] || { echo "built $exe_name not found" >&2; exit 1; }

stage="$work/stage-$engine-$key"
rm -rf "$stage" && mkdir -p "$stage"
cp "$exe" "$stage/"
cp "$src/LICENSE" "$stage/LICENSE.txt" 2>/dev/null || true

name="$engine-$version-$key"
if [ "$os" = windows ]; then
  archive="$out/$name.zip"
  rm -f "$archive"
  if command -v zip >/dev/null 2>&1; then
    (cd "$stage" && zip -q -9 "$archive" ./*)
  else
    (cd "$stage" && 7z a -tzip -bd "$archive" ./* >/dev/null)
  fi
else
  archive="$out/$name.tar.gz"
  tar -C "$stage" -czf "$archive" .
fi

if command -v sha256sum >/dev/null 2>&1; then sum="$(sha256sum "$archive" | cut -d' ' -f1)"
else sum="$(shasum -a 256 "$archive" | cut -d' ' -f1)"; fi
printf '%s  %s\n' "$sum" "$(basename "$archive")" > "$archive.sha256"
echo "$(basename "$archive") $sum"
