#!/usr/bin/env bash
# Builds target/package/phpide_<version>_<arch>.deb with dpkg-deb.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/crates/phpide-ui/Cargo.toml" | head -n1)}"
ARCH="${ARCH:-$(dpkg --print-architecture)}"
OUT="$ROOT/target/package"
STAGE="$OUT/deb/phpide_${VERSION}_${ARCH}"

rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN"
VERSION="$VERSION" bash "$ROOT/scripts/install-files.sh" "$STAGE" /usr

# Libraries the binary links, from dpkg-shlibdeps when it can tell.
LINKED="libc6, libgcc-s1, zlib1g"
if command -v dpkg-shlibdeps >/dev/null; then
  mkdir -p "$STAGE/debian"
  printf 'Source: phpide\n\nPackage: phpide\nArchitecture: any\n' > "$STAGE/debian/control"
  if found="$(cd "$STAGE" && dpkg-shlibdeps -O -e usr/bin/phpide 2>/dev/null)"; then
    LINKED="${found#shlibs:Depends=}"
  fi
  rm -rf "$STAGE/debian"
fi
# Loaded at run time (not linked): keyboard maps and OpenGL/EGL.
DEPENDS="$LINKED, libxkbcommon0, libegl1 | libgl1"
SIZE="$(du -sk "$STAGE/usr" | cut -f1)"

cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: phpide
Version: $VERSION
Architecture: $ARCH
Maintainer: PHP IDE contributors <https://github.com/tavaresgerson/php-ide>
Installed-Size: $SIZE
Depends: $DEPENDS
Recommends: git, php-cli, libwayland-client0 | libx11-6
Suggests: composer, php-xdebug, nodejs, npm
Section: devel
Priority: optional
Homepage: https://github.com/tavaresgerson/php-ide
Description: PHP development environment
 Free and open source IDE for PHP written in Rust: code editor, completion
 through intelephense or phpactor, run configurations, Xdebug debugger,
 PHPUnit with coverage, Git and an integrated terminal.
CONTROL

# Debian wants a copyright file next to the docs.
cat > "$STAGE/usr/share/doc/phpide/copyright" <<COPYRIGHT
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: PHP IDE
Source: https://github.com/tavaresgerson/php-ide

Files: *
License: MIT or Apache-2.0
COPYRIGHT

mkdir -p "$OUT"
dpkg-deb --root-owner-group --build "$STAGE" "$OUT/phpide_${VERSION}_${ARCH}.deb"
echo "$OUT/phpide_${VERSION}_${ARCH}.deb"
