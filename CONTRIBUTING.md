# Contributing

## Setup

- Rust stable with `rustfmt` and `clippy`.
- Node.js 22 with npm, then `npm ci --prefix third_party` for intelephense
  (`node_modules` is local and not versioned).
- PHP CLI, Xdebug and PHPUnit for the run/debug/test gates. On
  Debian/Ubuntu: `sudo apt-get install -y php-cli php-xdebug phpunit`.
- `Cargo.lock` is versioned; `cargo build --locked` reproduces the same
  dependency versions.

## Quality gates

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features phpide-ui/experimental -- -D warnings
cargo test --workspace
cargo test -p phpide-ui --features experimental
```

Tests run in parallel. The DBGp tests bind free ports and the UI tests keep
their state (recovery, crash reports, keymap) per process or per thread.
A test skipped because a tool is missing counts as **not verified**, never
as supported. CI (`.github/workflows/ci.yml`) runs the same commands with
PHP, Xdebug, Composer and intelephense installed.

Visual snapshots live in `crates/phpide-ui/tests/snapshots/`; a mismatch
writes the new image to `target/snapshots/` (set `UPDATE_SNAPSHOTS=1` when
the change is intended). Review screenshots of each phase, in the three
themes: `cargo test -p phpide-ui phase8_screenshots -- --ignored`.

`scripts/baseline.sh` runs all of the above plus the release build and the
benchmarks, keeps going when a step fails, and writes
`target/baseline/<timestamp>/summary.md`. Use `--quick` to skip the build and
benchmarks.

## Packages

```bash
bash scripts/package-deb.sh                        # target/package/phpide_<version>_<arch>.deb
APPIMAGETOOL=/path/to/appimagetool bash scripts/package-appimage.sh
flatpak-builder --user --install --force-clean --build-args=--share=network \
  build-dir packaging/flatpak/io.github.tavaresgerson.PhpIde.yml
```

All three install the same files through `scripts/install-files.sh`. The
version comes from `crates/phpide-ui/Cargo.toml`.

## Rules

1. Nothing blocks the UI thread: Git, filesystem, processes and network go
   through background jobs.
2. A phase or task is done only with evidence: an interaction test, a
   screenshot for visual changes and a measurement for performance claims.
3. Small commits. Extract code from `main.rs` without behavior changes
   first, then change the behavior or the visuals.
4. Frozen features stay behind `crate::scope::EXPERIMENTAL`; do not expose
   them in the default build.
5. No placeholder presented as a working feature: hide it until it works.
6. UI text is in English.

## Frozen features: disposable databases without root

Only needed for `phpide-db` work (`--features experimental`). The packages
are extracted under `target/tmp/db`; nothing is installed system-wide.

```bash
mkdir -p target/tmp/db/postgres-pkg && cd target/tmp/db/postgres-pkg
apt-get download postgresql-16
dpkg-deb -x postgresql-16_*.deb root
mkdir -p ../postgres-data ../postgres-socket
root/usr/lib/postgresql/16/bin/initdb -D ../postgres-data --no-locale --auth=trust --username=gate
root/usr/lib/postgresql/16/bin/postgres -D ../postgres-data -p 55432 -k "$PWD/../postgres-socket"
```

```bash
mkdir -p target/tmp/db/mariadb-pkg && cd target/tmp/db/mariadb-pkg
apt-get download mariadb-server mariadb-server-core mariadb-client-core
for package in *.deb; do dpkg-deb -x "$package" root; done
root/usr/bin/mariadb-install-db --basedir="$PWD/root/usr" --datadir="$PWD/../mariadb-data" --auth-root-authentication-method=normal --skip-test-db
root/usr/sbin/mariadbd --no-defaults --basedir="$PWD/root/usr" --datadir="$PWD/../mariadb-data" --socket="$PWD/../mariadb.sock" --port=33306 --bind-address=127.0.0.1
```

```bash
export PHPIDE_TEST_POSTGRES_URL='postgresql://gate@127.0.0.1:55432/postgres'
export PHPIDE_TEST_MYSQL_URL='mysql://root@127.0.0.1:33306/mysql'
cargo test -p phpide-db real_ -- --nocapture --test-threads=1
cargo test -p phpide-ui --features experimental controller_flow_when_fixture -- --nocapture --test-threads=1
```
