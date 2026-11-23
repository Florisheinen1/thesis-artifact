#!/usr/bin/env bash
# Aggregates the analysed and rewritten benchmarks into the JSON files in results/, which the
# thesis reads. results/ is committed: after a full run, `git diff results/` shows whether the
# thesis numbers were reproduced.
#
#   pipeline/4_results.sh
#
# Input:   work/analyzed/  (from 2_analyze.sh), work/rewritten/  (from 3_rewrite.sh)
# Output:  results/*.json  (see evaluation/evaluate.py)
set -euo pipefail
source "$(dirname "$0")/common.sh"

require python3 "aggregating the results (standard library only, Python 3.9 or newer)"
[ -d "$WORK/analyzed" ] || die "'work/analyzed' does not exist; run pipeline/2_analyze.sh first"
[ -d "$WORK/rewritten" ] || die "'work/rewritten' does not exist; run pipeline/3_rewrite.sh first"

python3 "$ROOT/evaluation/evaluate.py" --analyzed "$WORK/analyzed" --rewritten "$WORK/rewritten" --out "$ROOT/results"
