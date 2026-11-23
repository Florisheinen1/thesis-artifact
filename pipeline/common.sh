# Shared paths and settings for the pipeline scripts. Sourced, not executed.

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BENCHMARKS="$ROOT/benchmarks"
TOOLS="$ROOT/tools"
WORK="$ROOT/work"   # pipeline output (gitignored)
LOGS="$WORK/logs"

# The Rust toolchains are pinned by the rust-toolchain.toml of each tool and benchmark; rustup
# installs them automatically the first time cargo runs there.

CROWN="$TOOLS/crown"
REWRITER_DIR="$TOOLS/rewriter"
REWRITER="$REWRITER_DIR/target/release/rewriter"

# Copies a stage folder to the next one, without build output (target/ folders).
copy_stage() {
    mkdir -p "$2"
    cp -r "$1/." "$2/"
    find "$2" -type d -name target -prune -exec rm -rf {} +
}

step() { echo; echo "=== $* ==="; }
die()  { echo "Error: $*" >&2; exit 1; }

# Checks that commands are installed, before a stage starts. Nothing is installed automatically.
#   require "cmake" "Crown's Z3 build" "python3" "Crown's Z3 build" ...
require() {
    local missing=()
    while [ $# -gt 0 ]; do
        command -v "$1" >/dev/null || missing+=("  $1  (needed for $2)")
        shift 2
    done
    [ ${#missing[@]} -eq 0 ] || die "missing commands, please install them first:
$(printf '%s\n' "${missing[@]}")"
}

# Checks that a tool was built by 0_setup.sh.
require_built() {  # <binary> <name>
    [ -x "$1" ] || die "$2 is not built; run pipeline/0_setup.sh first"
}
