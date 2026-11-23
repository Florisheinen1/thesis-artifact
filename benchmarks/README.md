# Thesis benchmarks

The benchmarks are taken from [Crown](https://github.com/KomaEc/crown) (Zhang et al., *Ownership guided C to Rust
translation*, CAV 2023), which collected them from elsewhere.

| Folder | Contents |
|---|---|
| `c-code/` | The original C source of each project |
| `rust-code/` | Its translation by c2rust v0.16.0, with a few manual edits. This is the input of the thesis evaluation. |
| `rust-code-testable/` | Hand-modified copies of five projects, so that `cargo run` runs their tests |
| `c2rust/` | c2rust v0.16.0 (submodule), only needed to reproduce `rust-code` |

## Reproducing `rust-code` from `c-code`

**1. Get LLVM/Clang 14.** c2rust v0.16.0 does not build against newer LLVM versions. The verified version is the
official 14.0.0 build:

```bash
curl -LO https://github.com/llvm/llvm-project/releases/download/llvmorg-14.0.0/clang+llvm-14.0.0-x86_64-linux-gnu-ubuntu-18.04.tar.xz
echo "61582215dafafb7b576ea30cc136be92c877ba1f1c31ddbbd372d6d65622fef5  clang+llvm-14.0.0-x86_64-linux-gnu-ubuntu-18.04.tar.xz" | sha256sum -c -
tar xf clang+llvm-14.0.0-x86_64-linux-gnu-ubuntu-18.04.tar.xz
```

On Ubuntu/Debian, `apt install llvm-14 libclang-14-dev` also works (`LLVM14_DIR=/usr/lib/llvm-14`), but that version
(14.0.6) has not been verified.

**2. Run the script.** It also needs `rustup`, `jq`, `cmake`, a C/C++ compiler and the ncurses headers.

```bash
git submodule update --init c2rust
rustup toolchain install nightly-2022-11-14 --profile minimal --component rustc-dev,rust-src,rustfmt
LLVM14_DIR=$PWD/clang+llvm-14.0.0-x86_64-linux-gnu-ubuntu-18.04 ./transpile.sh
```

This builds c2rust with Rust `nightly-2022-11-14` (c2rust's own default nightly prints some characters
differently, e.g. `'\u{0}'` instead of `'\0'`), translates every project, and compares the result with `rust-code`:

- `transpiled/<project>/`: the fresh translation
- `transpiled/diffs/<project>.diff`: the differences, ignoring formatting
- `transpiled/summary.txt`: the number of differences per project

Formatting is ignored because c2rust's own dependencies print some code with a different indentation than the
version that produced `rust-code`. That changes the layout, but not the code.

## What to expect

The translation is not byte-identical to `rust-code`. These differences are expected:

- **Manual edits** made after translating: `libc` is pinned to `=0.2.149`, some `#![feature(...)]` lines were
  added, `Cargo.lock` and `rust-toolchain.toml` were added, some translated `main()` functions were commented out,
  `brotli` has a few type changes (see the git history), and `urlparser` has enlarged allocations (see below).
- **Run-to-run noise:** the numbers in `current_block_<N>` constants, and the order of some declarations.
- **A different system:** c2rust also translates the system headers a project includes (glibc, ncurses), so the
  output depends on the machine it runs on. For example, some projects were originally translated with an older
  Clang/glibc, which shows in declarations such as `free(__ptr)` instead of `free(_)`.

Apart from that, all projects match, except:

| project | why it differs |
|---|---|
| genann, quadtree | `c-code` is a different version of the C source than the one `rust-code` was made from |
| heman | one file (`ops.rs`) was made from a different version of `image.h` |
| bzip2 | `rust-code` was made with an older c2rust on macOS |
| tulipindicators | no C source available, so it is not translated |

Because the output depends on the system headers, the exact counts below are only reproducible in the artifact's
Docker image (Ubuntu 22.04, glibc 2.35, LLVM 14.0.0). On another system, such as Ubuntu 24.04 (glibc 2.39), about
half of the projects show a few more or fewer differing lines, in declarations from those headers.

`transpiled/summary.txt` in the Docker image:

```
avl                 6 files,     89 lines differ (raw:     89 lines; 0 files compared unformatted)
binn                6 files,     13 lines differ (raw:     72 lines; 0 files compared unformatted)
brotli             23 files, 514530 lines differ (raw: 515039 lines; 0 files compared unformatted)
bst                 6 files,     44 lines differ (raw:     44 lines; 0 files compared unformatted)
buffer              7 files,    187 lines differ (raw:    187 lines; 0 files compared unformatted)
bzip2              15 files,  16343 lines differ (raw:  23141 lines; 0 files compared unformatted)
genann             11 files,    515 lines differ (raw:    558 lines; 0 files compared unformatted)
heman              14 files,     50 lines differ (raw:     95 lines; 1 files compared unformatted)
ht                  5 files,      6 lines differ (raw:      7 lines; 0 files compared unformatted)
json.h              6 files,      9 lines differ (raw:    303 lines; 0 files compared unformatted)
libcsv              6 files,     11 lines differ (raw:     61 lines; 0 files compared unformatted)
libtree             7 files,    140 lines differ (raw:    201 lines; 0 files compared unformatted)
libzahl            16 files,    187 lines differ (raw:    187 lines; 0 files compared unformatted)
lil                 6 files,     51 lines differ (raw:    131 lines; 0 files compared unformatted)
lodepng             6 files,    117 lines differ (raw:    622 lines; 0 files compared unformatted)
quadtree           10 files,     60 lines differ (raw:     64 lines; 0 files compared unformatted)
rgba                6 files,     45 lines differ (raw:     47 lines; 0 files compared unformatted)
robotfindskitten    6 files,     51 lines differ (raw:     53 lines; 0 files compared unformatted)
urlparser           6 files,    275 lines differ (raw:    204 lines; 0 files compared unformatted)
```

(brotli's large count is almost entirely `src/enc/encoder_dict.rs`: the 31,705-entry table
`kStaticDictionaryWords` is emitted at a different place in the file, so `diff` reports it as removed and added.)

urlparser's count includes the enlarged allocations described below.

## urlparser: enlarged allocations

The C source of urlparser writes URL parts into heap buffers that are too small, which is a heap buffer overflow on
the inputs of its own test suite. With the C allocator the program happens to survive it, but it crashes once the
thesis' allocation header sits next to the buffer. In both `rust-code/urlparser` and `rust-code-testable/urlparser`,
`src/test.rs` therefore enlarges 17 allocations:

- 13 single-byte buffers, `malloc(sizeof(char))`, now take 256 bytes;
- `search`, `malloc(sizeof(char *))` (8 bytes), now takes 256 bytes;
- `host` gets one extra byte for its terminating null byte, `strlen(tmp_hostname) + 1`;
- both `path` buffers get two extra bytes, for the terminating null byte and the `/` that `"/%s"` adds,
  `strlen(tmp_path) + 2`.

256 bytes is enough for the test inputs (the longest URL has 67 characters), but longer URLs still overflow. The fix
only changes allocation sizes, so it adds no pointer interactions and leaves the analysis results unchanged. It does
not remove all undefined behavior: urlparser still reads out of bounds (in `strff`, `strrwd` and `strdup`) and calls
`strcat` on uninitialized buffers.
