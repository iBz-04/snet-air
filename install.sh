#!/bin/sh
# Installs the latest snet release for this machine and checks its SHA-256.
set -eu

repository="iBz-04/snet-air"
base="https://github.com/$repository/releases/latest/download"

fail() {
  printf 'snet install: %s\n' "$1" >&2
  exit 1
}

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) archive="snet-macos-arm64.tar.gz" ;;
  Linux-x86_64) archive="snet-linux-x86_64.tar.gz" ;;
  *) fail "no build for $(uname -s) $(uname -m) yet; build from source: https://github.com/$repository" ;;
esac

command -v curl >/dev/null 2>&1 || fail "curl is needed"
if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  fail "sha256sum or shasum is needed to check the download"
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

curl -fsSL "$base/$archive" -o "$work/$archive" || fail "the download failed"
curl -fsSL "$base/SHA256SUMS" -o "$work/SHA256SUMS" || fail "the checksums could not be downloaded"
expected="$(grep " $archive\$" "$work/SHA256SUMS" | cut -d' ' -f1)"
[ -n "$expected" ] || fail "the release lists no checksum for $archive"
[ "$(sha256 "$work/$archive")" = "$expected" ] || fail "the download does not match its checksum"

tar -xzf "$work/$archive" -C "$work"

if [ -w /usr/local/bin ]; then
  target="/usr/local/bin"
else
  target="$HOME/.local/bin"
  mkdir -p "$target"
fi
install -m 0755 "$work/snet" "$target/snet"

printf '✔ snet is installed in %s\n' "$target"
case ":$PATH:" in
  *":$target:"*) ;;
  *) printf '  Add it to your PATH: export PATH="%s:$PATH"\n' "$target" ;;
esac
printf '  Next: snet pull\n'
