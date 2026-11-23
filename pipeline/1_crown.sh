#!/usr/bin/env bash
# Runs Crown's preprocessing and ownership analysis on a copy of the benchmarks.
#
#   pipeline/1_crown.sh [source-dir]      default: benchmarks/rust-code
#                                         (e.g. benchmarks/transpiled, from benchmarks/transpile.sh)
#
# Output:  work/crown/<project>/          preprocessed code + analysis_results/*.json
#          work/logs/1_crown/<project>.log
set -euo pipefail
source "$(dirname "$0")/common.sh"

SOURCE=$(cd "${1:-$BENCHMARKS/rust-code}" && pwd)
OUT="$WORK/crown"
LOG_DIR="$LOGS/1_crown"

require cargo "building each benchmark before Crown analyses it"
require_built "$CROWN/target/release/crown" "Crown"
[ -e "$OUT" ] && die "'${OUT#$ROOT/}' already exists; remove it to start over"

step "Copying ${SOURCE#$ROOT/} to ${OUT#$ROOT/}"
copy_stage "$SOURCE" "$OUT"
# benchmarks/transpiled also contains the comparison with rust-code, which is not a benchmark
rm -rf "$OUT/diffs" "$OUT/logs" "$OUT/summary.txt"
mkdir -p "$LOG_DIR"

failed=()
for bench in "$OUT"/*/; do
    bench=${bench%/}
    name=$(basename "$bench")
    log="$LOG_DIR/$name.log"
    step "$name"
    # Crown's scripts build Crown with, and take the rustc sysroot from, the toolchain of the
    # directory they run in, so they must run from tools/crown.
    if (
        set -e
        echo "--- cargo build"; (cd "$bench" && cargo build)
        echo "--- preprocess";  (cd "$CROWN" && ./preprocess.sh "$bench")
        echo "--- analyse";     (cd "$CROWN" && ./analyse.sh "$bench")
       ) > "$log" 2>&1; then
        echo "ok: $(ls "$bench/analysis_results" | wc -l) analysis files"
    else
        failed+=("$name")
        echo "FAILED, see ${log#$ROOT/}"
    fi
done

step "Done: results in ${OUT#$ROOT/}/<project>/analysis_results/"
[ ${#failed[@]} -eq 0 ] || die "Crown failed for: ${failed[*]}"
