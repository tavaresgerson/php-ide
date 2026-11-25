#!/usr/bin/env bash
# Downloads the PHPantom language server pinned in third_party/phpantom.lock.
#
#   fetch-phpantom.sh                 the binary for this machine, checked
#                                     against the lock, into third_party/phpantom/
#   fetch-phpantom.sh --update VER    pins release VER: downloads every
#                                     archive, writes their SHA-256 into the
#                                     lock and the Flatpak manifest
#
# Environment: ARCH (x86_64 or aarch64; default: uname -m), DEST (default
# third_party/phpantom). Development builds, the tests and the packaging
# scripts look for the binary at third_party/phpantom/phpantom_lsp.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOCK="$ROOT/third_party/phpantom.lock"
MANIFEST="$ROOT/packaging/flatpak/io.github.tavaresgerson.PhpIde.yml"
ARCHES=(x86_64 aarch64)

lock_value() {
  sed -n "s/^$1=//p" "$LOCK" | head -n1
}

# Release asset for an architecture. The static musl builds are published
# under the -unknown-linux-gnu names.
asset_name() {
  echo "phpantom_lsp-$1-unknown-linux-gnu.tar.gz"
}

asset_url() {
  echo "https://github.com/$1/releases/download/$2/$(asset_name "$3")"
}

download() {
  curl --fail --location --silent --show-error --retry 3 --output "$2" "$1"
}

sha256_of() {
  sha256sum "$1" | cut -d' ' -f1
}

normalize_arch() {
  case "$1" in
    x86_64 | amd64) echo x86_64 ;;
    aarch64 | arm64) echo aarch64 ;;
    *)
      echo "PHPantom has no Linux build for $1" >&2
      return 1
      ;;
  esac
}

update() {
  local version="$1" repo tmp arch file sum
  repo="$(lock_value repo)"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  declare -A sums
  for arch in "${ARCHES[@]}"; do
    file="$tmp/$(asset_name "$arch")"
    echo "downloading $(asset_url "$repo" "$version" "$arch")"
    download "$(asset_url "$repo" "$version" "$arch")" "$file"
    sums[$arch]="$(sha256_of "$file")"
  done
  sed -i \
    -e "s/^version=.*/version=$version/" \
    -e "s/^sha256_x86_64=.*/sha256_x86_64=${sums[x86_64]}/" \
    -e "s/^sha256_aarch64=.*/sha256_aarch64=${sums[aarch64]}/" \
    "$LOCK"
  for arch in "${ARCHES[@]}"; do
    # The Flatpak sources carry a "# phpantom <arch>" marker on their url
    # and sha256 lines.
    sed -i \
      -e "s|^\( *url: \).*\( # phpantom $arch\)$|\1$(asset_url "$repo" "$version" "$arch")\2|" \
      -e "s|^\( *sha256: \).*\( # phpantom $arch\)$|\1${sums[$arch]}\2|" \
      "$MANIFEST"
  done
  echo "pinned PHPantom $version"
  for arch in "${ARCHES[@]}"; do
    echo "  $arch ${sums[$arch]}"
  done
}

fetch() {
  local repo version arch expected dest stamp tmp file actual
  repo="$(lock_value repo)"
  version="$(lock_value version)"
  arch="$(normalize_arch "${ARCH:-$(uname -m)}")"
  expected="$(lock_value "sha256_$arch")"
  dest="${DEST:-$ROOT/third_party/phpantom}"
  stamp="$dest/.stamp"
  if [ -z "$expected" ]; then
    echo "third_party/phpantom.lock has no SHA-256 for $arch:" >&2
    echo "run scripts/fetch-phpantom.sh --update $version first" >&2
    exit 1
  fi
  if [ -x "$dest/phpantom_lsp" ] && [ "$(cat "$stamp" 2>/dev/null)" = "$version $expected" ]; then
    echo "PHPantom $version already in $dest"
    return
  fi
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  file="$tmp/$(asset_name "$arch")"
  download "$(asset_url "$repo" "$version" "$arch")" "$file"
  actual="$(sha256_of "$file")"
  if [ "$actual" != "$expected" ]; then
    echo "PHPantom $version ($arch): SHA-256 mismatch" >&2
    echo "  expected $expected" >&2
    echo "  got      $actual" >&2
    exit 1
  fi
  tar -xzf "$file" -C "$tmp" phpantom_lsp
  mkdir -p "$dest"
  install -m755 "$tmp/phpantom_lsp" "$dest/phpantom_lsp"
  echo "$version $expected" > "$stamp"
  echo "PHPantom $version ($arch) in $dest"
}

case "${1:-}" in
  --update)
    update "${2:?usage: fetch-phpantom.sh --update VERSION}"
    ;;
  "")
    fetch
    ;;
  *)
    echo "usage: fetch-phpantom.sh [--update VERSION]" >&2
    exit 2
    ;;
esac
