#!/usr/bin/env bash
# Builds Crown and the rewriter. Safe to rerun: builds are incremental.
#
#   pipeline/0_setup.sh
#
# The Rust toolchains are pinned by each folder's rust-toolchain.toml; rustup installs them
# automatically the first time cargo runs there (unless RUSTUP_AUTO_INSTALL=0).
# (Translating the C benchmarks again is not part of this pipeline; see benchmarks/README.md.)
set -euo pipefail
source "$(dirname "$0")/common.sh"

step "Checking system tools"
require \
    rustup "the Rust toolchains pinned in each rust-toolchain.toml" \
    cargo "building Crown and the rewriter" \
    cmake "Crown's Z3 build" \
    c++ "Crown's Z3 build" \
    python3 "Crown's Z3 build"
[ -f "$CROWN/Cargo.toml" ] || die "tools/crown is empty; run: git submodule update --init --recursive"
[ -f "$REWRITER_DIR/rust-analyzer/Cargo.toml" ] || die "tools/rewriter is incomplete; run: git submodule update --init --recursive"
[ -d "$BENCHMARKS/rust-code" ] || die "benchmarks is empty; run: git submodule update --init --recursive"

step "Building Crown"
(cd "$CROWN" && cargo build --release)

step "Building the rewriter"
(cd "$REWRITER_DIR" && cargo build --release --locked --bin rewriter)

step "Done"
