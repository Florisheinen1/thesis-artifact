# The thesis artifact with everything installed and built, so that the pipeline runs offline.
#
#   docker build -t thesis-artifact .
#   docker run --rm -it thesis-artifact
#
# The image only installs what the README lists as requirements and then runs the same scripts
# as a native setup (pipeline/0_setup.sh, benchmarks/transpile.sh); there is no Docker-only logic.

FROM ubuntu:22.04@sha256:f64b2ffc2f10ab8f8505aa56b2664bd846cbb9df87c7e2532e30f7667021c0e6

# System packages. Ubuntu 22.04's llvm-14 is LLVM 14.0.0, the version c2rust v0.16.0 needs.
#   build-essential, cmake, python3   Crown's Z3 build; C code of the benchmarks
#   llvm-14-dev, libclang-14-dev, clang-14, jq, libncurses-dev   benchmarks/transpile.sh (c2rust)
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        ca-certificates curl xz-utils git \
        build-essential cmake python3 \
        llvm-14-dev libclang-14-dev clang-14 jq libncurses-dev \
 && rm -rf /var/lib/apt/lists/*

# Typst, to compile the thesis from results/.
ARG TYPST_VERSION=0.14.1
ARG TYPST_SHA256=6ad7b794f71c781fec4a742fcdd105deac12b59e5e7a58db98ccae7f8408d141
RUN curl -sSfLo /tmp/typst.tar.xz \
        https://github.com/typst/typst/releases/download/v${TYPST_VERSION}/typst-x86_64-unknown-linux-musl.tar.xz \
 && echo "${TYPST_SHA256}  /tmp/typst.tar.xz" | sha256sum -c - \
 && tar -xJf /tmp/typst.tar.xz -C /tmp \
 && mv /tmp/typst-x86_64-unknown-linux-musl/typst /usr/local/bin/typst \
 && rm -rf /tmp/typst*

# Everything below runs as an ordinary user, so files written to a mounted folder are not owned
# by root on the host.
RUN useradd --create-home --uid 1000 artifact
USER artifact
WORKDIR /artifact
ENV PATH=/home/artifact/.cargo/bin:$PATH \
    LLVM14_DIR=/usr/lib/llvm-14

# rustup, without a default toolchain: every folder pins its own in rust-toolchain(.toml).
ARG RUSTUP_VERSION=1.29.3
ARG RUSTUP_SHA256=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55
RUN curl -sSfLo /tmp/rustup-init \
        https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/x86_64-unknown-linux-gnu/rustup-init \
 && echo "${RUSTUP_SHA256}  /tmp/rustup-init" | sha256sum -c - \
 && chmod +x /tmp/rustup-init \
 && /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain none \
 && rm /tmp/rustup-init

# The toolchains, installed from the pinned toolchain files (a separate layer, so that editing the
# repository does not download them again). c2rust's toolchain is the one transpile.sh uses.
COPY --chown=artifact tools/crown/rust-toolchain.toml    /tmp/tc/crown/
COPY --chown=artifact tools/rewriter/rust-toolchain.toml /tmp/tc/rewriter/
COPY --chown=artifact benchmarks/rust-code/avl/rust-toolchain.toml /tmp/tc/benchmarks/
RUN for d in /tmp/tc/*/; do (cd "$d" && rustup toolchain install); done \
 && rustup toolchain install nightly-2022-11-14 --profile minimal --component rustc-dev,rust-src,rustfmt \
 && rm -rf /tmp/tc

# The repository itself, and everything built from it.
COPY --chown=artifact . /artifact
#   - Crown and the rewriter, with the pipeline's own setup script
#   - c2rust, by translating one project with transpile.sh (which builds it first)
#   - the crates of every benchmark, so that the pipeline needs no network
#   - the Typst packages of the thesis, by compiling it once
# Build caches that are not needed to run anything are removed in the same step, so they do not
# end up in the image.
RUN pipeline/0_setup.sh \
 && benchmarks/transpile.sh avl \
 && rm -rf benchmarks/transpiled \
 && for b in benchmarks/rust-code/*/; do (cd "$b" && cargo fetch --locked); done \
 && typst compile final_project.typ /tmp/thesis.pdf && rm /tmp/thesis.pdf \
 && rm -rf tools/*/target/debug tools/*/target/release/incremental \
           benchmarks/c2rust/target/release/incremental \
           /home/artifact/.cargo/registry/src /home/artifact/.rustup/downloads /home/artifact/.rustup/tmp

CMD ["bash"]
