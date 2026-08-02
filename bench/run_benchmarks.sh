#!/usr/bin/env bash
set -euo pipefail

echo "============================================================"
echo "rJSON Benchmarking Suite (Linux / Docker Environment)"
echo "============================================================"

# Ensure we are in /build
cd /build

# 1. Compile Rust release artifacts (library, raw_timing binary, Criterion benchmarks)
echo "--> Compiling Rust release binaries..."
cd /build/rJSON
cargo build --release 2>&1
cargo build --release --bin raw_timing 2>&1

echo "--> Running Criterion benchmarks (Raw Rust Core Parsing)..."
cargo bench --bench raw_bench 2>&1 || true

cd /build
mkdir -p /build/bench/out

# 2. Compile C benchmark executables with -O3 optimization
echo "--> Compiling C benchmarks (Original C vs Dynamic Librjson Facade)..."
gcc -O3 -std=c11 -I /build/bench/cjson /build/bench/c/bench_main.c /build/bench/cjson/cJSON.c -lm -o /build/bench/out/bench_orig
gcc -O3 -std=c11 -I /build/bench/cjson /build/bench/c/bench_main.c -L /build/rJSON/target/release -lrjson -lm -o /build/bench/out/bench_facade

echo "--> Verifying linkage of bench_facade against librjson.so..."
LD_LIBRARY_PATH=/build/rJSON/target/release ldd /build/bench/out/bench_facade | grep 'librjson.so'

echo ""
echo "============================================================"
echo "Executing Benchmarks & Collecting Metrics"
echo "============================================================"

results_file="/build/bench/results.json"
echo "[" > "$results_file"
first=true

for file in small.json medium.json large.json; do
    if [ "$file" = "large.json" ]; then
        iters=200
    else
        iters=5000
    fi
    echo "--- Testing Payload: $file ($iters iterations) ---"
    
    # Original C (cJSON_Parse + cJSON_Delete)
    out=$(/build/bench/out/bench_orig /build/bench/inputs/$file "$iters" "orig_c")
    echo "$out" | grep -v '^{'
    json=$(echo "$out" | grep '^{')
    if [ "$first" = true ]; then first=false; else echo "," >> "$results_file"; fi
    echo "$json" >> "$results_file"

    # Facade (librjson.so dynamic linking cJSON_Parse + cJSON_Delete)
    out=$(LD_LIBRARY_PATH=/build/rJSON/target/release /build/bench/out/bench_facade /build/bench/inputs/$file "$iters" "facade_rust")
    echo "$out" | grep -v '^{'
    json=$(echo "$out" | grep '^{')
    echo "," >> "$results_file"
    echo "$json" >> "$results_file"

    # Raw Rust (Arena::new + cjson_parse + drop)
    out=$(/build/rJSON/target/release/raw_timing /build/bench/inputs/$file "$iters" "raw_rust")
    echo "$out" | grep -v '^{'
    json=$(echo "$out" | grep '^{')
    echo "," >> "$results_file"
    echo "$json" >> "$results_file"
    echo ""
done

echo "]" >> "$results_file"
echo "=== FULL BENCH/RESULTS.JSON ==="
cat "$results_file"
echo "=== END RESULTS ==="
