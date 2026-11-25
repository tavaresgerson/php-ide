#!/usr/bin/env bash
# Installs PHP IDE into a staging directory laid out like a Linux prefix:
#   install-files.sh <destdir> [prefix]
# The .deb, AppImage and Flatpak builds all use it, so the three install
# the same files. Builds the release binary first when BIN is not set.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:?usage: install-files.sh <destdir> [prefix]}"
PREFIX="${2:-/usr}"
APP_ID="io.github.tavaresgerson.PhpIde"
VERSION="${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/crates/phpide-ui/Cargo.toml" | head -n1)}"
DATE="${RELEASE_DATE:-$(date -u +%Y-%m-%d)}"
BIN="${BIN:-$ROOT/target/release/phpide-ui}"

if [ ! -x "$BIN" ]; then
  cargo build --release --locked -p phpide-ui --manifest-path "$ROOT/Cargo.toml"
fi

P="$DEST$PREFIX"
install -Dm755 "$BIN" "$P/bin/phpide"
install -Dm644 "$ROOT/packaging/linux/$APP_ID.desktop" "$P/share/applications/$APP_ID.desktop"
METAINFO="$(mktemp)"
sed -e "s/@VERSION@/$VERSION/" -e "s/@DATE@/$DATE/" \
  "$ROOT/packaging/linux/$APP_ID.metainfo.xml" > "$METAINFO"
install -Dm644 "$METAINFO" "$P/share/metainfo/$APP_ID.metainfo.xml"
rm -f "$METAINFO"
install -Dm644 "$ROOT/packaging/linux/icons/$APP_ID.svg" \
  "$P/share/icons/hicolor/scalable/apps/$APP_ID.svg"
for dir in "$ROOT"/packaging/linux/icons/*x*/; do
  size="$(basename "$dir")"
  install -Dm644 "$dir/$APP_ID.png" "$P/share/icons/hicolor/$size/apps/$APP_ID.png"
done
install -Dm644 "$ROOT/README.md" "$P/share/doc/phpide/README.md"
for f in LICENSE-MIT LICENSE-APACHE CHANGELOG.md; do
  if [ -f "$ROOT/$f" ]; then install -Dm644 "$ROOT/$f" "$P/share/doc/phpide/$f"; fi
done
echo "installed PHP IDE $VERSION into $DEST$PREFIX"
