# ============================================================
# rJSON - cJSON -> Rust port (Port Mortem 2026, Track A)
# Member 3 owns this file.
#
# Usage:
#   docker build -t rjson .
#
# What this does:
#   Stage 1 (builder): compiles the rJSON Rust crate, runs the Rust-side
#     tests, then compiles and runs the six adapter-eligible original C
#     test files against the Rust cdylib facade on Linux.
#   Stage 2 (runtime): copies only the compiled cdylib artifact into a
#     minimal Debian image.
#
# The Rust toolchain version is NOT pinned here. rJSON/rust-toolchain.toml
# is the single source of truth for the channel. rustup inside the builder
# stage reads that file automatically.
# ============================================================

# ---- Stage 1: builder ----------------------------------------
FROM rust:slim-bookworm AS builder

# build-essential provides gcc/ld/ldd for the C adapter tests.
# cmake is kept for the existing bench/fuzz build tooling expectation.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        cmake \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy the Rust crate and tracked, self-contained adapter test support.
# /cJSON is gitignored and not required by this Docker build.
COPY rJSON/ ./rJSON/

WORKDIR /build/rJSON

# Let rust-toolchain.toml drive the channel selection.
RUN rustup show active-toolchain || true

# Build the library (cdylib + rlib) and run Rust-side tests.
RUN cargo build 2>&1
RUN cargo test 2>&1

# Compile all six adapter-eligible C test files from tests/adapter, not
# tests/original. This preserves the quoted-include behavior: "common.h"
# resolves to tests/adapter/common.h instead of the frozen original copy.
RUN set -eux; \
    mkdir -p tests/adapter/out; \
    for test in \
        minify_tests \
        readme_examples \
        parse_examples \
        parse_with_opts \
        compare_tests \
        cjson_add; \
    do \
        gcc -std=c11 \
            -I tests/adapter \
            tests/adapter/${test}.c \
            tests/adapter/unity_setup.c \
            tests/adapter/unity/src/unity.c \
            -L target/debug \
            -lrjson \
            -lm \
            -o tests/adapter/out/${test}; \
    done

# Verify each Linux adapter binary links to librjson.so before trusting any
# Unity result, mirroring the Windows dumpbin discipline.
RUN set -eux; \
    for test in \
        minify_tests \
        readme_examples \
        parse_examples \
        parse_with_opts \
        compare_tests \
        cjson_add; \
    do \
        LD_LIBRARY_PATH=target/debug ldd tests/adapter/out/${test} | tee tests/adapter/out/${test}.ldd; \
        grep 'librjson.so' tests/adapter/out/${test}.ldd; \
    done

# Run from tests/adapter so parse_examples.c sees ./inputs/.
WORKDIR /build/rJSON/tests/adapter
RUN set -eux; \
    for test in \
        minify_tests \
        readme_examples \
        parse_examples \
        parse_with_opts \
        compare_tests \
        cjson_add; \
    do \
        LD_LIBRARY_PATH=../../target/debug ./out/${test}; \
    done

# ---- Stage 1b: benchmark -------------------------------------
FROM builder AS benchmark
WORKDIR /build
COPY bench/ ./bench/
RUN bash bench/run_benchmarks.sh

# ---- Stage 2: runtime ----------------------------------------
FROM debian:bookworm-slim AS runtime

WORKDIR /app

COPY --from=builder /build/rJSON/target/debug/librjson.so ./librjson.so

# Nothing to run as a daemon. This image is a distributable artifact carrier.
CMD ["ls", "-lh", "/app/librjson.so"]
