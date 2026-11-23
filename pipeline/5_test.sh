#!/usr/bin/env bash
# Runs the test suites of the testable benchmarks, before and after the rewrite.
#
#   pipeline/5_test.sh
#
# benchmarks/rust-code-testable/ holds copies of the benchmarks that contain tests, with a test
# entry point added: a `test` binary that calls the project's own test function. For each of
# them, this stage takes the difference between rust-code/<project> and
# rust-code-testable/<project> as a patch, applies it to the analysed version of the project
# (before the rewrite) and to the rewritten version (after the rewrite), and runs the test
# suite of both with `cargo run --bin test`.
#
# Input:   work/analyzed/, work/rewritten/       (from 2_analyze.sh and 3_rewrite.sh)
#          benchmarks/rust-code/, benchmarks/rust-code-testable/
# Output:  work/tested/<project>.entry.patch      the test entry point, as a patch
#          work/tested/<project>/{before,after}/  the patched copies
#          work/tested/<project>/{before,after}.stdout   what the test suite printed
#          work/logs/5_test/<project>.{before,after}.log  patch and cargo output
set -euo pipefail
source "$(dirname "$0")/common.sh"

TESTABLE="$BENCHMARKS/rust-code-testable"
OUT="$WORK/tested"
LOG_DIR="$LOGS/5_test"

require cargo "building and running the test suites" \
        diff "making the test entry point patch" \
        patch "applying the test entry point patch"
[ -d "$WORK/analyzed" ] || die "'work/analyzed' does not exist; run pipeline/2_analyze.sh first"
[ -d "$WORK/rewritten" ] || die "'work/rewritten' does not exist; run pipeline/3_rewrite.sh first"
[ -e "$OUT" ] && die "'${OUT#$ROOT/}' already exists; remove it to start over"
mkdir -p "$OUT" "$LOG_DIR"

failed=()
for testable in "$TESTABLE"/*/; do
    name=$(basename "$testable")
    step "$name"

    # The test entry point is everything rust-code-testable adds to rust-code. diff exits with 1
    # when the folders differ, which is expected here; 2 means it could not compare them.
    patch_file="$OUT/$name.entry.patch"
    status=0
    (cd "$BENCHMARKS" && diff -ruN -x target -x Cargo.lock -x .gitignore \
        "rust-code/$name" "rust-code-testable/$name") > "$patch_file" || status=$?
    [ "$status" -le 1 ] || die "could not compare rust-code/$name with rust-code-testable/$name"

    for phase in before after; do
        if [ "$phase" = before ]; then src="$WORK/analyzed/$name"; else src="$WORK/rewritten/$name"; fi
        dest="$OUT/$name/$phase"
        log="$LOG_DIR/$name.$phase.log"
        copy_stage "$src" "$dest"

        # The paths in the patch start with rust-code/<project>/, hence -p2
        if ! (cd "$dest" && patch -p2 --forward --batch < "$patch_file") > "$log" 2>&1; then
            failed+=("$name ($phase)")
            echo "$phase the rewrite: test entry point does NOT apply, see ${log#$ROOT/}"
            continue
        fi
        if (cd "$dest" && cargo run --bin test) > "$OUT/$name/$phase.stdout" 2>> "$log"; then
            echo "$phase the rewrite: passes"
        else
            failed+=("$name ($phase)")
            echo "$phase the rewrite: FAILS, see ${log#$ROOT/}"
        fi
    done

    # Only informative: a test suite may print timings (genann) or other values that differ per run
    if cmp -s "$OUT/$name/before.stdout" "$OUT/$name/after.stdout"; then
        echo "same output before and after the rewrite"
    else
        echo "output differs before and after the rewrite (see work/tested/$name/*.stdout)"
    fi
done

step "Done: results in ${OUT#$ROOT/}/"
[ ${#failed[@]} -eq 0 ] || die "test suites failing: ${failed[*]}"
