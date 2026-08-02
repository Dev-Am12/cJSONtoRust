# rJSON Benchmarking Methodology & Experimental Design

## 1. Experimental Objectives & Dual-Tier Design
To provide complete architectural transparency, this benchmarking suite separately measures two distinct operational profiles against original C (`cJSON.c` **v1.7.19**):
1. **Core Logic Speed (Raw Rust API vs. Original C):** Evaluates `rjson::cjson_parse` and internal zero-copy arena memory management without C-bridge structural translation. Answers: *How efficient is our core Rust parsing engine and arena allocator?*
2. **C-Caller Experience (Facade-Wrapped API vs. Original C):** Evaluates `cJSON_Parse` and `cJSON_Delete` dynamically linked against `librjson.so`. Answers: *What operational difference does a C application experience when substituting original cJSON with our compiled drop-in Rust replacement?*

## 2. Vendored Reference Source & Attribution (MIT License)
To guarantee reproducible, offline container builds without depending on gitignored external folders, minimal reference copies of original cJSON **v1.7.19** (`cJSON.c` and `cJSON.h`) are vendored into `bench/cjson/`. Both files remain fully covered under Dave Gamble's original **MIT License**, consistent with our vendoring of Unity and cJSON test fixtures under `rJSON/tests/adapter/`.

## 3. Environment Parity Guarantee
All comparative measurements are captured inside a unified **Docker / Linux container environment** (`debian:bookworm-slim` / `rust:slim-bookworm`, x86-64 Architecture). 
* Mixing Windows/MSVC host measurements with Docker/Linux container metrics is strictly avoided to prevent scheduler, system call, and libc allocator (`malloc`/`free`) discrepancies from polluting A/B comparisons.
* The benchmarking suite executes in an isolated build stage (`FROM builder AS benchmark`) within `Dockerfile`, ensuring zero additional layers or slowdowns are introduced to the default deliverable runtime build (`docker build -t rjson .`).

## 4. Tooling & Optimization Discipline
* **Optimization Flags:** All binaries are evaluated under strict release optimizations. Rust targets are built with `cargo build --release` (`-C opt-level=3`). C harness binaries are compiled with `gcc -O3 -std=c11`.
* **Timing Mechanism:** Monotonic high-resolution timers are utilized across both language harnesses: POSIX `clock_gettime(CLOCK_MONOTONIC)` for C executables, and `std::time::Instant::now()` (which binds to Linux monotonic system clocks) alongside **Criterion** for standalone Rust executables.
* **Lifecycle Symmetry:** All timed loops measure the complete operational memory lifecycle: allocation of temporary parser structures, evaluation of the input payload, construction of the AST, and explicit deallocation/destruction (`cJSON_Delete(item)` in C, and `drop(arena)` in Rust) prior to stopping the timer.

## 5. Input Payloads & Zero Pre-Processing Rule
All three implementations read identical, immutable raw bytes directly from `/bench/inputs/` without pre-processing, stripping whitespace, or string translation prior to timed evaluation:
* `small.json` (583 B): Minimal real-world JSON object with nested primitive fields (derived from cJSON `test1`). Evaluated across **5,000 iterations**.
* `medium.json` (3,464 B): Medium structural configuration payload (~3.5 KB, derived from cJSON `test4`). Evaluated across **5,000 iterations**.
* `large.json` (586,893 B): Synthetic stress-test payload (~586 KB, an array of 3,000 detailed JSON objects). Evaluated across **200 iterations** (sufficient to accrue 0.6–1.2s of cumulative execution time per implementation, stabilizing standard deviation without excess runtimes).

## 6. Execution Protocol (Warmup vs. Timed Runs)
Each trial execution commences with a **100-iteration untimed warmup phase** to populate filesystem buffer caches, instruction caches, and memory allocator arenas. Following warmup, the timed trial loop commences, generating raw microsecond (`us`) samples which are sorted to compute Mean, Median, Minimum, Maximum, and Standard Deviation (spread) across the runs.
