#!/usr/bin/env bash
# Signs a macOS binary with the Developer ID certificate and notarizes it with Apple.
# Run it on the binary BEFORE scripts/package.sh, because signing changes the checksum.
# Usage: scripts/macos_sign.sh target/aarch64-apple-darwin/release/yourai
#
# On your Mac it uses the certificate in the login keychain and the notarytool profile
# NOTARY_PROFILE (default yourai-notary). In CI it uses these environment variables instead:
#   MACOS_CERT_P12 (base64 .p12), MACOS_CERT_PASSWORD,
#   APPLE_API_KEY_P8 (base64 .p8), APPLE_API_KEY_ID, APPLE_API_ISSUER.
# MACOS_SIGN_IDENTITY overrides the certificate name.
set -euo pipefail

bin="${1:?path to the binary}"
identity="${MACOS_SIGN_IDENTITY:-Developer ID Application: Nguyen Duy (3T8C2BPM64)}"
work="$(mktemp -d)"
keychain=""
cleanup() {
  if [ -n "$keychain" ]; then security delete-keychain "$keychain" 2>/dev/null || true; fi
  rm -rf "$work"
}
trap cleanup EXIT

sign_opts=()
if [ -n "${MACOS_CERT_P12:-}" ]; then
  keychain="$work/sign.keychain-db"
  kc_pass="$(openssl rand -hex 16)"
  printf '%s' "$MACOS_CERT_P12" | base64 --decode > "$work/cert.p12"
  security create-keychain -p "$kc_pass" "$keychain"
  security set-keychain-settings -lut 3600 "$keychain"
  security unlock-keychain -p "$kc_pass" "$keychain"
  security import "$work/cert.p12" -k "$keychain" -P "${MACOS_CERT_PASSWORD:?}" -T /usr/bin/codesign >/dev/null
  security set-key-partition-list -S apple-tool:,apple: -s -k "$kc_pass" "$keychain" >/dev/null
  # codesign looks up the certificate chain through the keychain search list.
  # shellcheck disable=SC2046
  security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
  sign_opts=(--keychain "$keychain")
fi

echo "==> signing $bin"
codesign --force --options runtime --timestamp --sign "$identity" ${sign_opts[@]+"${sign_opts[@]}"} "$bin"
codesign --verify --strict --verbose=2 "$bin"

if [ -n "${APPLE_API_KEY_P8:-}" ]; then
  printf '%s' "$APPLE_API_KEY_P8" | base64 --decode > "$work/key.p8"
  notary=(--key "$work/key.p8" --key-id "${APPLE_API_KEY_ID:?}" --issuer "${APPLE_API_ISSUER:?}")
else
  notary=(--keychain-profile "${NOTARY_PROFILE:-yourai-notary}")
fi

echo "==> notarizing (usually 1 to 5 minutes)"
ditto -c -k --keepParent "$bin" "$work/notarize.zip"
# First submission on a new Apple Developer account can take well over 30 minutes.
xcrun notarytool submit "$work/notarize.zip" "${notary[@]}" --wait --timeout 120m --output-format json > "$work/result.json"
status="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["status"])' "$work/result.json")"
if [ "$status" != "Accepted" ]; then
  id="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["id"])' "$work/result.json")"
  echo "notarization status: $status" >&2
  xcrun notarytool log "$id" "${notary[@]}" >&2 || true
  exit 1
fi
echo "==> notarized: $bin"
