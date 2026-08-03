/*
 * rJSON Differential Fuzzing Harness (Hackathon Anatomy Spec Proxy)
 *
 * This file satisfies the root `fuzz/harness.*` anatomy specification.
 * To maintain a single source of truth without code duplication across our
 * containerized evaluation stages, the core POSIX differential fuzzer
 * (which uses dlopen/dlsym to continuously compare libcjson_orig.so and librjson.so)
 * is maintained under bench/c/fuzz_diff_main.c and orchestrated by Docker Stage 1c (`fuzzer`).
 *
 * Compiling this file directly compiles the exact differential fuzzer entry point:
 *   gcc -O3 -std=c11 fuzz/harness.c -ldl -o fuzz_harness
 */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#ifndef _POSIX_C_SOURCE
#define _POSIX_C_SOURCE 199309L
#endif
#include "../bench/c/fuzz_diff_main.c"
