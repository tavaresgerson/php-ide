#!/usr/bin/env bash
# Phase 0 baseline: runs every gate and benchmark, keeps going when a step
# fails, and writes a report to target/baseline/<timestamp>/summary.md.
#
# Usage: scripts/baseline.sh [--quick]
#   --quick  skip the release build and the benchmarks

set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1

QUICK=0
[ "${1:-}" = "--quick" ] && QUICK=1

STAMP="$(date +%Y%m%d-%H%M%S)"
OUT="$ROOT/target/baseline/$STAMP"
mkdir -p "$OUT"
SUMMARY="$OUT/summary.md"

step_results=()

# run <id> <description> <command...>
run() {
    local id="$1" desc="$2"
    shift 2
    local log="$OUT/$id.log"
    printf '==> %s\n' "$desc"
    local t0 t1 status
    t0=$(date +%s)
    "$@" >"$log" 2>&1
    status=$?
    t1=$(date +%s)
    local verdict="ok"
    [ "$status" -ne 0 ] && verdict="FAILED (exit $status)"
    printf '    %s in %ss — log: %s\n' "$verdict" "$((t1 - t0))" "${log#"$ROOT"/}"
    step_results+=("| \`$id\` | $desc | $verdict | $((t1 - t0)) s |")
}

# Sums "test result:" lines of a cargo test log.
test_totals() {
    awk '/^test result:/ {
        for (i = 1; i <= NF; i++) {
            if ($(i+1) ~ /^passed/)  p += $i
            if ($(i+1) ~ /^failed/)  f += $i
            if ($(i+1) ~ /^ignored/) g += $i
        }
    } END { printf "%d passed, %d failed, %d ignored", p, f, g }' "$1"
}

tool_version() {
    local name="$1"
    shift
    if command -v "$name" >/dev/null 2>&1; then
        "$@" 2>&1 | head -n 1
    else
        echo "not found"
    fi
}

# ---------------------------------------------------------------- environment
{
    echo "# Baseline $STAMP"
    echo
    echo "- Commit: \`$(git rev-parse --short HEAD 2>/dev/null || echo '?')\` $(git status --porcelain 2>/dev/null | grep -q . && echo '(working tree has changes)')"
    echo "- Machine: $(uname -srm)"
    echo "- rustc: $(rustc --version 2>&1)"
    echo "- cargo: $(cargo --version 2>&1)"
    echo
    echo "## External tools"
    echo
    echo "| Tool | Version |"
    echo "|---|---|"
    echo "| php | $(tool_version php php -v) |"
    echo "| xdebug | $(php -m 2>/dev/null | grep -qi xdebug && echo loaded || echo 'not loaded') |"
    echo "| phpunit | $(tool_version phpunit phpunit --version) |"
    echo "| composer | $(tool_version composer composer --version) |"
    echo "| intelephense (third_party) | $([ -x third_party/node_modules/.bin/intelephense ] && echo installed || echo 'not installed') |"
    echo "| intelephense (PATH) | $(command -v intelephense >/dev/null && echo found || echo 'not found') |"
    echo "| phpactor | $(tool_version phpactor phpactor --version) |"
    echo "| git | $(tool_version git git --version) |"
    echo "| node | $(tool_version node node --version) |"
} >"$SUMMARY"

# ---------------------------------------------------------------- gates
run fmt "cargo fmt --check" cargo fmt --all -- --check
run clippy "clippy (default features)" cargo clippy --workspace --all-targets -- -D warnings
run clippy-experimental "clippy (--features experimental)" \
    cargo clippy --workspace --all-targets --features phpide-ui/experimental -- -D warnings
run test "cargo test --workspace" cargo test --workspace -- --test-threads=1 --nocapture
run test-experimental "cargo test phpide-ui --features experimental" \
    cargo test -p phpide-ui --features experimental -- --test-threads=1 --nocapture
run gate-composer "gate: Composer + intelephense" \
    cargo test -p phpide-php --test composer_flow -- --test-threads=1 --nocapture
run gate-run-debug "gate: run/debug/PHPUnit" \
    cargo test -p phpide-run --test gate_run_debug -- --test-threads=1 --nocapture

if [ "$QUICK" -eq 0 ]; then
    run build-release "cargo build --release" cargo build --release -p phpide-ui
    run bench-editor "bench: editor pipeline (2k/10k/50k lines)" \
        cargo bench -p phpide-editor --bench editor_pipeline
    run bench-doc "bench: document operations" cargo bench -p phpide-core --bench doc_ops
fi

# ---------------------------------------------------------------- report
{
    echo
    echo "## Steps"
    echo
    echo "| Step | Description | Result | Time |"
    echo "|---|---|---|---|"
    printf '%s\n' "${step_results[@]}"
    echo
    echo "## Tests"
    echo
    for id in test test-experimental gate-composer gate-run-debug; do
        [ -f "$OUT/$id.log" ] && echo "- \`$id\`: $(test_totals "$OUT/$id.log")"
    done
    echo
    echo "### Not verified (integration skipped: tool missing)"
    echo
    echo "A skipped integration passes as \"ok\" in cargo test but proves nothing."
    echo "Set PHPIDE_REQUIRE_INTEGRATIONS=1 to turn these into failures."
    echo
    echo '```'
    grep -h -o 'GATE SKIP: .*' "$OUT"/test*.log "$OUT"/gate-*.log 2>/dev/null | sed 's/GATE SKIP: //' | sort | uniq -c || true
    echo '```'
    echo
    echo "### Tools used by the gates"
    echo
    echo '```'
    grep -h -o 'GATE USING: .*' "$OUT"/test*.log "$OUT"/gate-*.log 2>/dev/null | sed 's/GATE USING: //' | sort -u || true
    echo '```'
    echo
    echo "### Failing tests"
    echo
    echo '```'
    grep -h -E '^test .* FAILED$|^---- .* stdout ----$' "$OUT"/test*.log "$OUT"/gate-*.log 2>/dev/null | sort -u || true
    echo '```'
    echo
    echo "### Clippy (first 40 lines)"
    echo
    echo '```'
    grep -h -E '^(error|warning)' "$OUT"/clippy*.log 2>/dev/null | sort | uniq -c | sort -rn | head -n 40 || true
    echo '```'
    if [ -f "$OUT/bench-editor.log" ]; then
        echo
        echo "## Editor pipeline"
        echo
        echo '```'
        sed -n '/editor pipeline baseline/,$p' "$OUT/bench-editor.log"
        echo '```'
    fi
    if [ -f "$OUT/build-release.log" ]; then
        echo
        echo "## Release binary"
        echo
        echo "- Size: $(du -h target/release/phpide-ui 2>/dev/null | cut -f1 || echo '?')"
    fi
} >>"$SUMMARY"

echo
echo "Report: ${SUMMARY#"$ROOT"/}"
