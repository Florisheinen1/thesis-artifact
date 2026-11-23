#!/usr/bin/env bash
# Runs the rewriter's array analysis on a copy of Crown's output: how every array pointer is
# used, before anything is rewritten.
#
#   pipeline/2_analyze.sh
#
# Input:   work/crown/                      (from 1_crown.sh)
# Output:  work/analyzed/<project>/         + analysis_results/ with the rewriter's JSON files
#          work/logs/2_analyze/<project>.log
set -euo pipefail
source "$(dirname "$0")/common.sh"

IN="$WORK/crown"
OUT="$WORK/analyzed"
LOG_DIR="$LOGS/2_analyze"

require cargo "loading each benchmark into the rewriter"
require_built "$REWRITER" "The rewriter"
[ -d "$IN" ] || die "'${IN#$ROOT/}' does not exist; run pipeline/1_crown.sh first"
[ -e "$OUT" ] && die "'${OUT#$ROOT/}' already exists; remove it to start over"

step "Copying ${IN#$ROOT/} to ${OUT#$ROOT/}"
copy_stage "$IN" "$OUT"
mkdir -p "$LOG_DIR"

failed=()
for bench in "$OUT"/*/; do
    bench=${bench%/}
    name=$(basename "$bench")
    step "$name"
    if "$REWRITER" analyze one "$bench" > "$LOG_DIR/$name.log" 2>&1; then
        echo "ok"
    else
        failed+=("$name")
        echo "FAILED, see ${LOG_DIR#$ROOT/}/$name.log"
    fi
done

step "Done: results in ${OUT#$ROOT/}/<project>/analysis_results/"
[ ${#failed[@]} -eq 0 ] || die "analysis failed for: ${failed[*]}"
