# Tools

| Folder | What | Pinned at | Toolchain |
|---|---|---|---|
| `crown/` | Ownership analysis (fork of Crown) | branch `array-analysis` | `nightly-2023-01-26` + `rustc-dev`, `rust-src`, `llvm-tools-preview` |
| `rewriter/` | The array rewriter of this thesis | submodule commit | `1.94.0` |

The benchmarks themselves build with `nightly-2026-06-01` (each benchmark's own `rust-toolchain.toml`).
Every tool has its own `rust-toolchain.toml`, so rustup picks the right one automatically.
rustup installs these toolchains automatically the first time `cargo` runs in such a folder, so nothing has
to be installed by hand. `pipeline/0_setup.sh` builds both tools.

c2rust, which produced `benchmarks/rust-code` from `benchmarks/c-code`, is not part of this pipeline. It lives in
the benchmarks repository together with the script to reproduce that step; see `benchmarks/README.md`.

## Crown: what changed compared to upstream

The fork adds four commits on top of upstream commit `e46e2284`:

- `659712cd` Add array specific analysis file
- `d33713dc` Emit function return qualifiers in ownership/fatness under `@return`
- `b023eb41` Store output parameters in dedicated JSON analysis file
- `TODO` Exclude `extra_deps/` from the package, so that a checkout without `.git` does not rebuild Crown on
  every `cargo build` (its `build.rs` recreates that folder each time)

Crown links against `rustc` internals (`rustc_private`), hence its pinned nightly. It also builds Z3 from
source (`static-link-z3`), which needs `cmake`, a C++ compiler and `python3`.
