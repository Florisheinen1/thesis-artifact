#!/usr/bin/env bash
# Rewrites the owned array pointers of a copy of the analysed benchmarks, then checks that
# every rewritten benchmark still builds.
#
#   pipeline/3_rewrite.sh
#
# Input:   work/analyzed/                   (from 2_analyze.sh)
# Output:  work/rewritten/<project>/        rewritten source (with the CBox runtime installed)
#                                           + analysis_results/rewrite_outcomes.json
#          work/logs/3_rewrite/<project>.log         rewriter output
#          work/logs/3_rewrite/<project>.build.log   cargo build of the result
set -euo pipefail
source "$(dirname "$0")/common.sh"

IN="$WORK/analyzed"
OUT="$WORK/rewritten"
LOG_DIR="$LOGS/3_rewrite"

require cargo "loading each benchmark into the rewriter, and building the result"
require_built "$REWRITER" "The rewriter"
[ -d "$IN" ] || die "'${IN#$ROOT/}' does not exist; run pipeline/2_analyze.sh first"
[ -e "$OUT" ] && die "'${OUT#$ROOT/}' already exists; remove it to start over"

step "Copying ${IN#$ROOT/} to ${OUT#$ROOT/}"
copy_stage "$IN" "$OUT"
mkdir -p "$LOG_DIR"

rewrite_failed=()
build_failed=()
for bench in "$OUT"/*/; do
    bench=${bench%/}
    name=$(basename "$bench")
    step "$name"
    if ! "$REWRITER" rewrite one "$bench" > "$LOG_DIR/$name.log" 2>&1; then
        rewrite_failed+=("$name")
        echo "rewrite FAILED, see ${LOG_DIR#$ROOT/}/$name.log"
        continue
    fi
    if (cd "$bench" && cargo build) > "$LOG_DIR/$name.build.log" 2>&1; then
        echo "rewritten, builds"
    else
        build_failed+=("$name")
        echo "rewritten, does NOT build, see ${LOG_DIR#$ROOT/}/$name.build.log"
    fi
done

step "Done: results in ${OUT#$ROOT/}/"
[ ${#build_failed[@]} -eq 0 ] || echo "Rewritten but not building: ${build_failed[*]}"
[ ${#rewrite_failed[@]} -eq 0 ] || die "rewrite failed for: ${rewrite_failed[*]}"
