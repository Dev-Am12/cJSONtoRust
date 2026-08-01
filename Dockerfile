# ============================================================
# rJSON — cJSON → Rust port (Port Mortem 2026, Track A)
# Member 3 owns this file.
#
# Usage:
#   docker build -t rjson .
#
# What this does:
#   Stage 1 (builder): compiles the rJSON Rust crate and runs the
#     Rust-side port tests (rJSON/tests/*.rs and tests/port/).
#     The original C test suite is NOT linked here — that requires
#     the C-ABI facade layer, a deliberate hour-24 checkpoint per
#     DECISIONS.md §3.
#   Stage 2 (runtime): copies only the compiled cdylib artifact
#     into a minimal Debian image.
#
# The Rust toolchain version is NOT pinned here — rJSON/rust-toolchain.toml
# is the single source of truth for the channel.  rustup inside the
# builder stage reads that file automatically.
# ============================================================

# ---- Stage 1: builder ----------------------------------------
FROM rust:slim-bookworm AS builder

# Build essentials needed for C-side work (cmake, gcc) —
# not used in this stage yet, but installed now so the image
# is ready for the bench/ and fuzz/ stages without a rebuild.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        cmake \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy the Rust crate only.
# /cJSON is gitignored (intentionally untracked) and therefore
# not present in a clean clone — we do not reference it here.
# Do NOT copy anything from rJSON/tests/original/ or
# rJSON/tests/original-utils/ into a build context that would
# modify them; Docker COPY is read-only for this purpose.
COPY rJSON/ ./rJSON/

WORKDIR /build/rJSON

# Let rust-toolchain.toml drive the channel selection.
# rustup is already present in the rust:slim-bookworm base image.
# This installs the pinned channel if not already cached.
RUN rustup show active-toolchain || true

# Build the library (both cdylib and rlib targets).
RUN cargo build 2>&1

# Run the Rust-side tests.
# This covers rJSON/tests/*.rs and tests/port/.
# It does NOT attempt to link or run tests/original/ — those
# depend on the C-ABI facade (DECISIONS.md §3, hour-24 checkpoint).
RUN cargo test 2>&1

# ---- Stage 2: runtime ----------------------------------------
FROM debian:bookworm-slim AS runtime

WORKDIR /app

# Copy the compiled shared library out of the builder.
# The exact filename depends on the target triple inside the
# builder; we use a wildcard-free explicit path.
COPY --from=builder /build/rJSON/target/debug/librjson.so ./librjson.so

# Nothing to run as a daemon — this image is a distributable
# artifact carrier.  Verify the library is present on start.
CMD ["ls", "-lh", "/app/librjson.so"]
