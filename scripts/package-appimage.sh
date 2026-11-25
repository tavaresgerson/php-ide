#!/usr/bin/env bash
# Builds target/package/PHP_IDE-<version>-<arch>.AppImage.
# Needs appimagetool: on PATH, or APPIMAGETOOL=/path/to/appimagetool.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_ID="io.github.tavaresgerson.PhpIde"
VERSION="${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/crates/phpide-ui/Cargo.toml" | head -n1)}"
ARCH="${ARCH:-$(uname -m)}"
TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
if [ -z "$TOOL" ]; then
  echo "appimagetool not found: put it on PATH or set APPIMAGETOOL" >&2
  exit 1
fi
OUT="$ROOT/target/package"
APPDIR="$OUT/appimage/PHP_IDE.AppDir"

rm -rf "$APPDIR"
VERSION="$VERSION" bash "$ROOT/scripts/install-files.sh" "$APPDIR" /usr
cp "$APPDIR/usr/share/applications/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
cp "$APPDIR/usr/share/icons/hicolor/256x256/apps/$APP_ID.png" "$APPDIR/$APP_ID.png"
ln -sf "$APP_ID.png" "$APPDIR/.DirIcon"
cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/phpide" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

mkdir -p "$OUT"
ARCH="$ARCH" "$TOOL" "$APPDIR" "$OUT/PHP_IDE-$VERSION-$ARCH.AppImage"
echo "$OUT/PHP_IDE-$VERSION-$ARCH.AppImage"
