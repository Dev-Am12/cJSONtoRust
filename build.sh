#!/usr/bin/env bash
# ============================================================
# build.sh — one-command local build for rJSON (Linux / macOS)
# Member 3 owns this file.
#
# Usage:  ./build.sh
#
# Requirements: Rust toolchain managed by rustup.
#   The channel is read from rJSON/rust-toolchain.toml automatically.
#
# What this does:
#   1. Compiles the rJSON Rust crate (both cdylib and rlib).
#   2. Runs the Rust-side port tests.
#
# What this deliberately does NOT do:
#   - Link or run tests/original/ (requires the C-ABI facade —
#     DECISIONS.md §3, hour-24 checkpoint, not yet built).
#   - Touch anything under rJSON/src/ or rJSON/tests/original*.
#   - Fetch or modify /cJSON (intentionally gitignored, read-only).
# ============================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RJSON_DIR="$SCRIPT_DIR/rJSON"

if [[ ! -d "$RJSON_DIR" ]]; then
    echo "ERROR: rJSON/ directory not found at $SCRIPT_DIR" >&2
    exit 1
fi

echo "=== rJSON build ==="
echo "Crate: $RJSON_DIR"
echo "Toolchain: $(cd "$RJSON_DIR" && rustup show active-toolchain 2>/dev/null || echo '(rustup not found — install from https://rustup.rs)')"
echo ""

cd "$RJSON_DIR"

echo "--- cargo build ---"
cargo build
echo ""

echo "--- cargo test ---"
cargo test
echo ""

echo "=== Done. Build artifact: rJSON/target/debug/librjson.so (or .dylib on macOS) ==="
