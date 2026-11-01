#!/usr/bin/env bash
# yourai installer: curl -fsSL https://yourai.bdtren.com/cli/install.sh | bash
# Options (pass with: ... | bash -s -- --version v0.0.1 --no-modify-path --dry-run)
# Environment: YOURAI_VERSION, YOURAI_BASE_URL, YOURAI_INSTALL_DIR, YOURAI_GITHUB_REPO
# Installs into your home folder. Never uses sudo.
set -euo pipefail

say()  { printf 'yourai: %s\n' "$*"; }
warn() { printf 'yourai: warning: %s\n' "$*" >&2; }
err()  { printf 'yourai: error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || err "required command not found: $1"; }

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d' ' -f1
  else err "need sha256sum or shasum to verify the download"; fi
}

shell_profile() {
  case "${SHELL:-}" in
    */zsh)  printf '%s' "$HOME/.zshrc" ;;
    */bash) if [ "$(uname -s)" = Darwin ]; then printf '%s' "$HOME/.bash_profile"; else printf '%s' "$HOME/.bashrc"; fi ;;
    *)      printf '%s' "$HOME/.profile" ;;
  esac
}

add_to_path() {
  local dir="$1" line profile
  # shellcheck disable=SC2016
  line="export PATH=\"$dir:\$PATH\""
  profile="$(shell_profile)"
  if [ -f "$profile" ] && grep -Fq "$dir" "$profile"; then return; fi
  printf '\n# added by yourai installer\n%s\n' "$line" >> "$profile"
  say "added $dir to PATH in $profile"
}

# A piped installer cannot change the PATH of the shell that ran it, so also link the
# binary into a writable folder that is already on PATH. Prints the link path on success.
link_on_path() {
  local bin="$1" dir
  for dir in "$HOME/.local/bin" "$HOME/bin" /opt/homebrew/bin /usr/local/bin; do
    case ":$PATH:" in *":$dir:"*) ;; *) continue ;; esac
    [ -d "$dir" ] && [ -w "$dir" ] || continue
    if [ -e "$dir/yourai" ] || [ -L "$dir/yourai" ]; then
      [ "$(readlink "$dir/yourai")" = "$bin" ] || continue
    fi
    ln -sf "$bin" "$dir/yourai" 2>/dev/null || continue
    printf '%s' "$dir/yourai"
    return 0
  done
  return 1
}

main() {
  local base_url="${YOURAI_BASE_URL:-https://yourai.bdtren.com/cli}"
  local install_dir="${YOURAI_INSTALL_DIR:-$HOME/.yourai/bin}"
  local github_repo="${YOURAI_GITHUB_REPO:-bdtren/yourai-cli}"
  local version="${YOURAI_VERSION:-}"
  local modify_path=1 dry_run=0

  while [ $# -gt 0 ]; do
    case "$1" in
      --version)        version="${2:?--version needs a value}"; shift 2 ;;
      --no-modify-path) modify_path=0; shift ;;
      --dry-run)        dry_run=1; shift ;;
      -h|--help)
        say "usage: install.sh [--version vX.Y.Z] [--no-modify-path] [--dry-run]"; return 0 ;;
      *) err "unknown option: $1" ;;
    esac
  done

  need curl; need tar; need uname

  local os arch target
  case "$(uname -s)" in
    Darwin) os="apple-darwin" ;;
    Linux)  os="unknown-linux-musl" ;;
    *) err "unsupported OS: $(uname -s). On Windows use: irm https://yourai.bdtren.com/cli/install.ps1 | iex" ;;
  esac
  case "$(uname -m)" in
    x86_64|amd64)  arch="x86_64" ;;
    arm64|aarch64) arch="aarch64" ;;
    *) err "unsupported CPU architecture: $(uname -m)" ;;
  esac
  # A terminal running under Rosetta reports x86_64 on Apple Silicon.
  if [ "$os" = "apple-darwin" ] && [ "$arch" = "x86_64" ] \
     && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = "1" ]; then
    arch="aarch64"
  fi
  target="${arch}-${os}"

  [ -n "$version" ] || version="$(curl -fsSL "$base_url/latest.txt" | tr -d '[:space:]')"
  [ -n "$version" ] || err "could not determine the latest version"
  case "$version" in v*) ;; *) version="v$version" ;; esac

  local archive="yourai-${version}-${target}.tar.gz"
  local url="$base_url/releases/${version}/${archive}"

  if [ "$dry_run" = 1 ]; then
    say "dry run: would install yourai ${version} for ${target}"
    say "  download: $url"
    say "  verify:   $url.sha256 (and the GitHub release copy when reachable)"
    say "  install:  $install_dir/yourai"
    [ "$modify_path" = 1 ] && say "  PATH:     add $install_dir to your shell profile if missing, and link yourai into a writable folder already on PATH"
    return 0
  fi

  local tmp; tmp="$(mktemp -d)"
  # shellcheck disable=SC2064
  trap "rm -rf '$tmp'" EXIT

  say "installing yourai ${version} for ${target}"
  curl -fSL --progress-bar "$url" -o "$tmp/$archive" || err "download failed: $url"
  curl -fsSL "$url.sha256" -o "$tmp/$archive.sha256" || err "could not download the checksum file"

  local expected actual second
  expected="$(cut -d' ' -f1 < "$tmp/$archive.sha256" | tr 'A-F' 'a-f')"
  actual="$(sha256_of "$tmp/$archive")"
  [ "$expected" = "$actual" ] || err "checksum mismatch. Aborting; nothing was installed."

  # Second channel: the checksum published on GitHub Releases must match too, when reachable.
  if second="$(curl -fsSL --max-time 15 "https://github.com/${github_repo}/releases/download/${version}/${archive}.sha256" 2>/dev/null)"; then
    second="$(printf '%s' "$second" | cut -d' ' -f1 | tr 'A-F' 'a-f')"
    [ "$second" = "$actual" ] || err "checksum differs from the GitHub release. Aborting; nothing was installed."
    say "checksum verified against $base_url and GitHub"
  else
    warn "could not reach GitHub to double-check the checksum; verified against $base_url only"
  fi

  tar -xzf "$tmp/$archive" -C "$tmp"
  [ -f "$tmp/yourai" ] || err "the archive does not contain the yourai binary"
  mkdir -p "$install_dir" "$HOME/.yourai"
  install -m 0755 "$tmp/yourai" "$install_dir/yourai.new"
  mv -f "$install_dir/yourai.new" "$install_dir/yourai"

  local on_path=0 link=""
  case ":$PATH:" in *":$install_dir:"*) on_path=1 ;; esac
  if [ "$modify_path" = 1 ] && [ "$on_path" = 0 ]; then
    add_to_path "$install_dir"
    link="$(link_on_path "$install_dir/yourai" || true)"
  fi

  local link_json=""
  [ -n "$link" ] && link_json=",\"link\":\"$link\""
  printf '{"method":"script","version":"%s","install_dir":"%s"%s}\n' "$version" "$install_dir" "$link_json" \
    > "$HOME/.yourai/install-receipt.json"

  say "installed to $install_dir/yourai"
  if [ "$on_path" = 1 ]; then
    say "next: run  yourai doctor  then  yourai init"
  elif [ -n "$link" ]; then
    say "linked $link, so you can run yourai now"
    say "next: run  yourai doctor  then  yourai init"
  elif [ "$modify_path" = 1 ]; then
    printf '\n'
    say "this terminal does not see yourai yet. Run this once (or open a new terminal):"
    printf '\n    source %s\n\n' "$(shell_profile)"
    say "then: yourai doctor  and  yourai init"
  else
    say "next: run  $install_dir/yourai doctor  (or add $install_dir to your PATH)"
  fi
}

main "$@"
