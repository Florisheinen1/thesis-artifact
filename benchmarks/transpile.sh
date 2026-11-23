#!/usr/bin/env bash
# Translates c-code/ again with the pinned c2rust and compares the result with rust-code/.
# See README.md for the pins, how to get LLVM 14, and the expected differences per project.
#
#   LLVM14_DIR=<llvm-14> ./transpile.sh [project...]      default: all projects
#
# LLVM14_DIR is the folder of an LLVM/Clang 14 installation (the one containing bin/llvm-config).
#
# Output (transpiled/, gitignored):
#   transpiled/<project>/              fresh c2rust output
#   transpiled/diffs/<project>.diff    differences after normalizing formatting (rustfmt + diff -b)
#   transpiled/diffs/raw/<project>.diff  exact differences
#   transpiled/logs/<project>.log
#   transpiled/summary.txt
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
C2RUST_DIR=$(realpath -m "${C2RUST_DIR:-$HERE/c2rust}")
TOOLCHAIN=nightly-2022-11-14   # to build c2rust, and for rustfmt
OUT="$HERE/transpiled"
SCRATCH="$OUT/.scratch"
TRANSPILE="$C2RUST_DIR/target/release/c2rust-transpile"

step() { echo; echo "=== $* ==="; }
die()  { echo "Error: $*" >&2; exit 1; }

# project -> folder under c-code/ to start from (tulipindicators has no C code)
declare -A C_DIR=(
    [avl]=avl [binn]=binn-3.0 [bst]=bst [buffer]=buffer-0.4.0 [bzip2]=bzip2
    [genann]=genann-1.0.0/c [ht]=ht [json.h]=json.h/c [libcsv]=libcsv
    [libtree]=libtree-3.1.1/c [libzahl]=libzahl-1.0/c [lil]=lil [lodepng]=lodepng
    [quadtree]=quadtree-0.1.0 [rgba]=rgba [urlparser]=urlparser
    [brotli]=brotli-1.0.9 [heman]=heman [robotfindskitten]=robotfindskitten
)

if [ $# -gt 0 ]; then projects=("$@"); else
    mapfile -t projects < <(printf '%s\n' "${!C_DIR[@]}" | sort)
fi
for p in "${projects[@]}"; do [ -n "${C_DIR[$p]:-}" ] || die "unknown project '$p'"; done

# --- Requirements ----------------------------------------------------------------------------

missing=()
for cmd in rustup cargo jq cmake c++ diff; do command -v "$cmd" >/dev/null || missing+=("$cmd"); done
[ ${#missing[@]} -eq 0 ] || die "missing: ${missing[*]}"
[ -f "$C2RUST_DIR/Cargo.toml" ] || die "c2rust/ is empty; run: git submodule update --init c2rust"

[ -n "${LLVM14_DIR:-}" ] || die "set LLVM14_DIR to an LLVM/Clang 14 installation, e.g.
    LLVM14_DIR=/usr/lib/llvm-14 $0
(see README.md for how to get one)"
LLVM14_DIR=$(cd "$LLVM14_DIR" && pwd) || die "LLVM14_DIR='$LLVM14_DIR' does not exist"
LLVM_CONFIG="$LLVM14_DIR/bin/llvm-config"
[ -x "$LLVM_CONFIG" ] || die "no bin/llvm-config in LLVM14_DIR='$LLVM14_DIR'"
llvm_version=$("$LLVM_CONFIG" --version)
case "$llvm_version" in
    14.0.0) ;;
    14.*) echo "Warning: using LLVM $llvm_version; only 14.0.0 was verified" >&2 ;;
    *) die "LLVM14_DIR contains LLVM $llvm_version, but c2rust v0.16.0 needs LLVM 14" ;;
esac

# The environment c2rust needs, both to build and to run it: LLVM_CONFIG_PATH makes its build use
# this llvm-config instead of searching PATH, and the binary links libclang-cpp.so.14 dynamically.
with_llvm14() {
    LLVM_CONFIG_PATH="$LLVM_CONFIG" \
    LD_LIBRARY_PATH="$("$LLVM_CONFIG" --libdir)${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
    RUSTUP_TOOLCHAIN="$TOOLCHAIN" \
    "$@"
}

# --- Build c2rust ----------------------------------------------------------------------------

RUSTUP_AUTO_INSTALL=0 rustup which --toolchain "$TOOLCHAIN" rustfmt >/dev/null 2>&1 || die "Rust $TOOLCHAIN is not installed; run:
    rustup toolchain install $TOOLCHAIN --profile minimal --component rustc-dev,rust-src,rustfmt"

step "Building c2rust-transpile (LLVM $llvm_version, $TOOLCHAIN)"
mkdir -p "$OUT/logs"
(cd "$C2RUST_DIR" && with_llvm14 cargo build --release --locked --bin c2rust-transpile) \
    > "$OUT/logs/build-c2rust.log" 2>&1 || die "building c2rust failed, see transpiled/logs/build-c2rust.log"

# --- Transpile -------------------------------------------------------------------------------

crate_name() {  # the package name in rust-code/<p>/Cargo.toml becomes the -o folder name
    sed -n 's/^name *= *"\(.*\)"/\1/p' "$HERE/rust-code/$1/Cargo.toml" | head -1
}

# Copies project $1 to the scratch folder and leaves the shell in the folder with a
# compile_commands.json that c2rust can use there. See README.md, "Recipe details".
prepare() {
    local p=$1 src="$SCRATCH/src/$1"
    rm -rf "$src"; mkdir -p "$src"
    cp -r "$HERE/c-code/${C_DIR[$p]}/." "$src/"
    cd "$src"
    case "$p" in
        brotli)
            # Listed twice (static + shared target), with absolute paths from the original machine.
            cd build
            jq --arg dir "$(pwd)" --arg root "$src/" '
                [.[] | .directory = $dir
                     | .arguments |= map(gsub("/home/vagrant/orc-benchmark/brotli-1.0.9/"; $root))]
                | unique_by(.file)' compile_commands.json > cc.tmp
            mv cc.tmp compile_commands.json
            ;;
        heman)
            # No compile_commands.json shipped; no CMAKE_BUILD_TYPE (Release adds -DNDEBUG).
            cmake -B build -DCMAKE_EXPORT_COMPILE_COMMANDS=ON . >/dev/null
            cd build
            # The original build used ../-relative source paths (visible in __FILE__ strings).
            jq --arg root "$src/" '
                [.[] | .command |= gsub($root; "../") | .file |= gsub($root; "../")]' \
                compile_commands.json > cc.tmp
            mv cc.tmp compile_commands.json
            ;;
        robotfindskitten)
            # config.h was never generated in the original corpus; the json points at src/.
            ./configure >/dev/null
            jq --arg dir "$src/src" '[.[] | .directory = $dir]' compile_commands.json > cc.tmp
            mv cc.tmp compile_commands.json
            ;;
        *)
            jq --arg dir "$(pwd)" '[.[] | .directory = $dir]' compile_commands.json > cc.tmp
            mv cc.tmp compile_commands.json
            ;;
    esac
}

transpile() {  # $1 = project, $2 = output folder
    local p=$1 out=$2 flags=(--emit-modules --fail-on-error --reduce-type-annotations --emit-build-files)
    prepare "$p"
    if [ "$p" = bzip2 ]; then
        # Without -o, like the original: c2rust then writes a flat crate (c2rust-lib.rs, no src/)
        # next to the C files. With -o it would use a src/ + lib.rs layout.
        with_llvm14 "$TRANSPILE" "${flags[@]}" compile_commands.json
        mkdir -p "$out"
        cp ./*.rs Cargo.toml rust-toolchain "$out/"
    else
        mkdir -p "$(dirname "$out")"
        with_llvm14 "$TRANSPILE" "${flags[@]}" -o "$out" compile_commands.json
    fi
}

# --- Compare ---------------------------------------------------------------------------------

# Copies rust-code/<p> and transpiled/<p> to the scratch folder and formats every .rs file on
# both sides with the same rustfmt, so that pure formatting differences disappear. A file that
# rustfmt cannot parse on either side is compared unformatted on both sides.
# Prints the number of such files.
normalize() {
    local p=$1 ref="$SCRATCH/norm/ref/$1" new="$SCRATCH/norm/new/$1" unparsable=0 f
    rm -rf "$ref" "$new"; mkdir -p "$ref" "$new"
    cp -r "$HERE/rust-code/$p/." "$ref/"; rm -rf "$ref/target"
    cp -r "$OUT/$p/." "$new/"
    while IFS= read -r f; do
        local ok=1 side
        for side in "$ref" "$new"; do
            [ -f "$side/$f" ] || continue
            RUSTUP_TOOLCHAIN="$TOOLCHAIN" rustfmt --edition 2021 --config skip_children=true \
                "$side/$f" 2>/dev/null || ok=0
        done
        if [ $ok = 0 ]; then
            unparsable=$((unparsable + 1))
            [ -f "$HERE/rust-code/$p/$f" ] && cp "$HERE/rust-code/$p/$f" "$ref/$f"
            [ -f "$OUT/$p/$f" ] && cp "$OUT/$p/$f" "$new/$f"
        fi
    done < <( (cd "$ref" && find . -name '*.rs'; cd "$new" && find . -name '*.rs') | sort -u)
    echo "$unparsable"
}

# --- Main ------------------------------------------------------------------------------------

mkdir -p "$OUT/diffs/raw"
rm -rf "$SCRATCH"
summary=()
failed=()
for p in "${projects[@]}"; do
    step "$p"
    rm -rf "$OUT/$p" "$OUT/diffs/$p.diff" "$OUT/diffs/raw/$p.diff"
    log="$OUT/logs/$p.log"
    if ! (transpile "$p" "$SCRATCH/out/$p/$(crate_name "$p")") > "$log" 2>&1; then
        failed+=("$p")
        summary+=("$(printf '%-17s FAILED, see transpiled/logs/%s.log' "$p" "$p")")
        echo "FAILED, see transpiled/logs/$p.log"
        continue
    fi
    mv "$SCRATCH/out/$p/$(crate_name "$p")" "$OUT/$p"

    diff -r --exclude target "$HERE/rust-code/$p" "$OUT/$p" > "$OUT/diffs/raw/$p.diff" || true
    unparsable=$(normalize "$p")
    diff -rb "$SCRATCH/norm/ref/$p" "$SCRATCH/norm/new/$p" \
        | sed "s|$SCRATCH/norm/ref/|rust-code/|g; s|$SCRATCH/norm/new/|transpiled/|g" \
        > "$OUT/diffs/$p.diff" || true

    files=$(grep -cE '^(diff|Only in) ' "$OUT/diffs/$p.diff" || true)
    lines=$(grep -c '^[<>]' "$OUT/diffs/$p.diff" || true)
    raw=$(grep -c '^[<>]' "$OUT/diffs/raw/$p.diff" || true)
    summary+=("$(printf '%-17s %3s files, %6s lines differ (raw: %6s lines; %s files compared unformatted)' \
        "$p" "$files" "$lines" "$raw" "$unparsable")")
    echo "ok: $files files, $lines lines differ after normalizing, see transpiled/diffs/$p.diff"
done
rm -rf "$SCRATCH"

step "Summary (compare with the table in README.md)"
printf '%s\n' "${summary[@]}" | tee "$OUT/summary.txt"
[ ${#failed[@]} -eq 0 ] || die "transpiling failed for: ${failed[*]}"
