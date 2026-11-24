#!/usr/bin/env bash
# Regenerates crates/phpide-icons/assets/*.svg from the Tabler Icons npm
# package. The semantic name → Tabler name mapping lives in
# crates/phpide-icons/assets/manifest.json; add an entry there (and the
# variant in src/lib.rs) to bring in a new icon.
#
# Usage: scripts/update-icons.sh [version]   (default: the manifest version)
set -euo pipefail
cd "$(dirname "$0")/.."
ASSETS=crates/phpide-icons/assets
VERSION=${1:-$(node -p "require('./$ASSETS/manifest.json').version")}
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
(cd "$TMP" && npm pack --silent "@tabler/icons@$VERSION" >/dev/null && tar xzf tabler-icons-*.tgz)
node - "$TMP/package/icons/outline" "$ASSETS" "$VERSION" <<'JS'
const fs = require('fs');
const [dir, assets, version] = process.argv.slice(2);
const manifestPath = `${assets}/manifest.json`;
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
for (const icon of manifest.icons) {
  const source = `${dir}/${icon.tabler}.svg`;
  if (!fs.existsSync(source)) { console.error(`missing in Tabler ${version}: ${icon.tabler}`); process.exit(1); }
  fs.copyFileSync(source, `${assets}/${icon.name}.svg`);
}
manifest.version = version;
fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
console.log(`updated ${manifest.icons.length} icons to Tabler ${version}`);
JS
cargo test -q -p phpide-icons
