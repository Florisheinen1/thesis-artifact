# Safe Array Translation for C2Rust: artifact

This is the artifact of the master thesis _Safe Array Translation for C2Rust_ (Floris Heinen, University of Twente,
2026). It contains the thesis itself, the tools it uses and builds, the benchmarks, and the scripts that produce
every table in the evaluation from scratch.

- Code (this repository, all versions): [10.5281/zenodo.24452651](https://doi.org/10.5281/zenodo.24452651)
- Docker image: [10.5281/zenodo.24451932](https://doi.org/10.5281/zenodo.24451932)

## Contents

| Folder                                   | What                                                                                                                                     |
| ---------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `final_project.typ`, `final_project.pdf` | The thesis (Typst source and the submitted PDF)                                                                                          |
| `thesis/`                                | Typst template, snippets, bibliography, and `results.typ`, which turns `results/` into the thesis tables                                 |
| `tools/crown/`                           | Crown, the ownership analysis the rewriter builds on (a fork with three small additions, see `tools/README.md`)                          |
| `tools/rewriter/`                        | The array rewriter: the contribution of this thesis                                                                                      |
| `benchmarks/`                            | The 20 benchmarks: original C code, its c2rust translation, and the script that reproduces that translation (see `benchmarks/README.md`) |
| `pipeline/`                              | The scripts that run the evaluation, one per stage                                                                                       |
| `evaluation/evaluate.py`                 | Turns the pipeline output into `results/`                                                                                                |
| `results/`                               | The numbers behind the thesis tables, as committed JSON files                                                                            |

## Quick check, without building anything

The thesis tables are not typed by hand: `thesis/results.typ` reads them from `results/*.json`, so the PDF always
matches the committed results. With [Typst](https://typst.app) 0.14.1:

```bash
typst compile final_project.typ
```

## Reproducing the results

There are two ways: the Docker image, which has everything installed and built, or a native setup. Both run the
same scripts.

### With Docker

Load the prebuilt image from Zenodo (see the DOI above), or build it yourself from this repository (about 10
minutes):

```bash
docker load -i thesis-artifact-image.tar.gz     # the prebuilt image, or:
docker build -t thesis-artifact .               # build it yourself
```

Then start a shell in it. The repository is at `/artifact`, and everything is built already, so no network is
needed:

```bash
docker run -it --name thesis thesis-artifact
```

Inside the container, follow the steps under [Running the pipeline](#running-the-pipeline) below, skipping
`0_setup.sh`. To copy files out of the container afterwards, for example the new results:

```bash
docker cp thesis:/artifact/results ./results-docker
```

The image is x86-64 (amd64). On ARM machines, such as Apple Silicon Macs, Docker runs it through emulation, which
is much slower.

### Natively

The scripts check what they need before they start, and never install anything themselves. You need:

- `rustup`: every tool and benchmark pins its Rust toolchain in a `rust-toolchain.toml`, which rustup installs
  automatically the first time `cargo` runs there (see `tools/README.md` for the versions)
- `cmake`, a C++ compiler and `python3`, for Crown's build of Z3
- `python3` 3.9 or newer (standard library only), for `evaluation/evaluate.py`
- Typst 0.14.1, to compile the thesis
- only to reproduce the C to Rust translation: LLVM/Clang 14, `jq` and the ncurses headers (see
  `benchmarks/README.md`)

```bash
git clone --recurse-submodules <this repository>
cd thesis-artifact
pipeline/0_setup.sh        # builds Crown and the rewriter
```

The released version on Zenodo already contains all submodules, so there `git clone` is not needed.

### Running the pipeline

Each stage reads the output of the previous one from `work/` (which is not committed), and refuses to overwrite
its own output: remove that folder to run a stage again.

```bash
pipeline/1_crown.sh        # benchmarks/rust-code → work/crown      Crown's ownership analysis
pipeline/2_analyze.sh      # work/crown → work/analyzed             how every array pointer is used
pipeline/3_rewrite.sh      # work/analyzed → work/rewritten         rewrite, then check that it still builds
pipeline/4_results.sh      # work/ → results/                       the numbers behind the tables
pipeline/5_test.sh         # work/analyzed, work/rewritten → work/tested   test suites before and after the rewrite
typst compile final_project.typ                                    # the thesis, with the new results
```

`4_results.sh` overwrites `results/`. Natively, `git diff results/` then shows any difference with the committed
results. The Docker image has no git history, so there, keep a copy first:

```bash
cp -r results /tmp/results-committed
# ... run the pipeline ...
diff -r /tmp/results-committed results && echo "identical"
```

Every stage writes a log per benchmark to `work/logs/<stage>/`.

### Time and resources

Measured on a 12-core x86-64 machine with 16 GB of memory (natively and in Docker alike):

| Step                               | Time                                                     |
| ---------------------------------- | -------------------------------------------------------- |
| Building the Docker image          | about 10 minutes                                         |
| `0_setup.sh` (natively, first run) | most of the image build time: Crown, Z3 and the rewriter |
| `1_crown.sh`                       | about 1.5 minutes                                        |
| `2_analyze.sh`                     | about 1.5 minutes                                        |
| `3_rewrite.sh`                     | about 35 seconds                                         |
| `4_results.sh`, `5_test.sh`        | a few seconds                                            |
| `benchmarks/transpile.sh`          | about 1.5 minutes                                        |

Disk space: the Docker image is about 9 GB, or 3 GB as a compressed download. Natively, the Rust toolchains take
about 8 GB, the builds a few GB, and `work/` about 2 GB.

## From thesis to command

| In the thesis                                                                                                                        | Produced by                                                               | Check                                                         |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------- | ------------------------------------------------------------- |
| Table of named pointer sources per benchmark (chapter _Array representation in C2Rust output_, section _Results_)                    | stages 1, 2, 4                                                            | `results/pointer_sources.json`                                |
| Table of pointer interactions per usage type (same section)                                                                          | stages 1, 2, 4                                                            | `results/usage_types.json`                                    |
| Table of rewrites per usage type (chapter _Evaluation_, section _Rewrite coverage_)                                                  | stages 1 to 4                                                             | `results/rewrite_outcomes.json`                               |
| Table of the safety improvement per benchmark (same section)                                                                         | stages 1 to 4                                                             | `results/safety.json`                                         |
| "All 20 benchmarks compile after the rewrite" (chapter _Evaluation_, section _Behavior preservation_)                                | stage 3                                                                   | `3_rewrite.sh` prints `rewritten, builds` for every benchmark |
| The test suites of five benchmarks pass before and after the rewrite (same section)                                                  | stage 5, with the test entry points from `benchmarks/rust-code-testable/` | `5_test.sh` prints `passes` twice for every benchmark         |
| The benchmarks are C programs translated with c2rust (chapter _Array representation in C2Rust output_, section _Experimental setup_) | `benchmarks/transpile.sh`                                                 | the expected differences are listed in `benchmarks/README.md` |

The numbers quoted in the text come from the same JSON files. `results/extra/` holds two more overviews of
Crown's classification that the thesis does not show as tables.

## Using the tools on other code

`pipeline/1_crown.sh` takes another folder as an argument instead of `benchmarks/rust-code`:

```bash
pipeline/1_crown.sh path/to/projects     # one c2rust-translated Cargo project per subfolder
```

The following stages then work on its output as usual. See `tools/README.md` for the tools themselves.
