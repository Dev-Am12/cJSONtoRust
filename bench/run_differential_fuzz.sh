#!/usr/bin/env bash
set -euo pipefail

echo "============================================================"
echo "Compiling Release Binaries for Differential Fuzzer (Linux)"
echo "============================================================"
cd /build/rJSON
cargo build --release 2>&1
cd /build

mkdir -p /build/bench/out
# Compile Original C as optimized shared library (-O3)
gcc -O3 -std=c11 -shared -fPIC -I /build/bench/cjson /build/bench/cjson/cJSON.c -lm -o /build/bench/out/libcjson_orig.so

# Compile Differential Fuzz Harness (-O3)
gcc -O3 -std=c11 -D_GNU_SOURCE -I /build/bench/cjson /build/bench/c/fuzz_diff_main.c -ldl -o /build/bench/out/fuzz_diff

echo "--> Commencing 65+ Second Continuous Differential Fuzz Run..."
/build/bench/out/fuzz_diff
