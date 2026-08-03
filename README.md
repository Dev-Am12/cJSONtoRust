# rJSON — a Rust port of cJSON

**Port Mortem 2026 · Track A (C → Rust) · [DaveGamble/cJSON](https://github.com/DaveGamble/cJSON) v1.7.19**

rJSON is a from-scratch Rust reimplementation of cJSON, a small, extremely widely-used ANSI C JSON parser/printer/tree-manipulation library. It ships as an idiomatic native Rust crate plus a C-ABI facade (`librjson.so`/`.dll`/`.dylib`) covering the API subset exercised by the six adapter-eligible original test files.

This README is written for two audiences at once: judges evaluating this submission against the hackathon's scoring rubric, and anyone who wants to build, test, or use the port. If you only read one other document, read [`DECISIONS.md`](./DECISIONS.md) Every non-mechanical choice below is expanded there with full reasoning, including the mistakes we found and fixed along the way.

---

## Table of contents

- [Quick start](#quick-start)
- [What this actually is](#what-this-actually-is)
- [Why cJSON, why this shape of port](#why-cjson-why-this-shape-of-port)
- [Architecture](#architecture)
- [Test strategy and honest parity numbers](#test-strategy-and-honest-parity-numbers)
- [Behavioral fidelity](#behavioral-fidelity)
- [Benchmarks](#benchmarks)
- [Code quality and `unsafe`](#code-quality-and-unsafe)
- [Repository layout](#repository-layout)
- [Fuzzing, robustness, and scope boundaries](#fuzzing-robustness-and-scope-boundaries)
- [Team](#team)
- [License](#license)

---

## Quick start

**One-command build:**
```bash
docker build -t rjson .
```
This builds the Rust library, runs the full native Rust test suite (134 tests), compiles the six adapter-eligible original C test files against the built `librjson.so`, and links them, all inside the container, with no dependency on anything outside this repo.

**To see the original test suite pass against the port directly**, the Dockerfile's test stage runs each of the six compiled adapter binaries and reports pass/fail counts per file. See [Test strategy](#test-strategy-and-honest-parity-numbers) below for exactly what "adapter-eligible" means and why not all 18 original test files can run this way.

**Native development build** (outside Docker):
```bash
cd rJSON
cargo build --release       # builds librjson.{so,dylib,dll} + the rlib
cargo test                  # runs the 134 native Rust tests
```

**Verifying the original test suite hasn't been touched:**
```bash
cd rJSON
sha256sum -c tests-kickoff.sha256          # core cJSON test files
sha256sum -c tests-kickoff-utils.sha256    # cJSON_Utils test files (out-of-scope utils suite)
```
Both should report every file as `OK`. If they don't, something in `tests/original/` or `tests/original-utils/` has changed since kickoff.

**Submission evidence:** [`STDLIB.md`](./STDLIB.md) records the C-to-Rust
standard-library mappings; [`DEPENDENCY_PROOF.md`](./DEPENDENCY_PROOF.md)
documents the dependency graph and its verification commands; and
[`SUBMISSION_EVIDENCE.md`](./SUBMISSION_EVIDENCE.md) is the reproducible
pre-submission checklist.

---

## What this actually is

[cJSON](https://github.com/DaveGamble/cJSON) is a ~3,500-line, ANSI C89 JSON library: a recursive-descent parser, a printer (with pretty, compact, buffered, and zero-allocation variants), and a mutable in-memory tree API for building and editing JSON documents by hand (add/delete/detach/replace/duplicate/compare). It's been in production use for over a decade and has a genuinely thorough test suite which is exactly what made it a demanding, honest target for this hackathon's actual question: not "can you make something that compiles," but "can you make something that *behaves the same*."

rJSON reimplements all of that from scratch in Rust. Its not a wrapper around an existing JSON crate, not a transpiler, nor an FFI shim into the original library. It's structured in two layers:

1. **A safe, idiomatic Rust engine** — an arena-indexed tree, a recursive-descent parser, and a printer, none of which use `unsafe`.
2. **A thin C-ABI facade** — `#[repr(C)]` and `extern "C"` bindings for the validated adapter surface that lets the six adapter-eligible, byte-for-byte unmodified cJSON test files link against this library.

---

## Why cJSON, why this shape of port

Track A's own framing is that c2rust-style mechanical translation "produces mostly unsafe — the interesting work is what comes after." cJSON is a good testbed for that specifically because its core data structure is a doubly-linked list of nodes with manual, convention-based ownership (`cJSON_Delete`, reference nodes via `cJSON_IsReference`) and is precisely the shape of problem Rust's ownership model exists to make safer, and precisely the shape of problem that's hardest to translate *cleanly* rather than just *mechanically*.

We made the architectural bet explicit rather than accidental: get the internal engine genuinely idiomatic and nearly `unsafe`-free, and pay for C-ABI compatibility with a small, deliberately isolated translation layer at the boundary instead of the alternative (mirror C's raw-pointer struct throughout, satisfy the ABI trivially, and end up with `unsafe` smeared through the whole engine). See `DECISIONS.md` #3 for the full reasoning and the tradeoff we accepted.

---

## Architecture

```
                    +---------------------------------------------+
                    |              Rust callers                   |
                    |   (native, safe, zero unsafe API)           |
                    +---------------------+-----------------------+
                                          |
                          arena.rs -- Arena<Node>, NodeId
                          parser.rs -- recursive-descent parser
                    (the actual engine: 0 unsafe blocks, tested
                     directly via 134 native Rust tests)
                                          |
                    +---------------------+-----------------------+
                    |           facade.rs (C ABI boundary)        |
                    |   #[repr(C)] struct CJson -- field layout   |
                    |   matches C's cJSON struct exactly          |
                    |   materializes an arena tree into real      |
                    |   C-heap pointer structs on the way out;    |
                    |   walks C-heap structs back into a          |
                    |   temporary arena on the way in             |
                    |   (all unsafe/FFI code lives here -- 200    |
                    |   occurrences, 0 elsewhere)                 |
                    +---------------------+-----------------------+
                                          |
                          librjson.so / .dylib / .dll
                                          |
                    +---------------------+-------------------------+
                    |        Original, unmodified C callers         |
                    |   including the original cJSON test suite     |
                    +-----------------------------------------------+
```

**The core engine** (`arena.rs`, `parser.rs`) stores the JSON tree as a `Vec<Node>` arena, with `next`/`prev`/`child` links as `Option<NodeId>` indices rather than raw pointers. This keeps the borrow checker fully satisfied and `unsafe` at zero in the code that does the actual parsing, printing, and tree mutation. See `DECISIONS.md` #3-#7 for the specific tradeoffs this involved (an internal representation that doesn't match C's struct layout field-for-field, a from-scratch reimplementation of C's dual `is_reference`/`key_is_const` ownership semantics, and a two-tier validation policy distinguishing public API entry points from internal accessors).

**The facade** (`facade.rs`) is a "materialize-on-return" design: each `cJSON_Parse*` call builds a short-lived internal arena, parses into it using the real engine, then walks it once to allocate an equivalent tree of real C-heap `cJSON` structs (real pointers, real `malloc`'d strings) for the caller. `cJSON_Delete` on the C side frees that C-heap tree directly, Rust isn't involved in that teardown at all. Functions that receive a `cJSON*` back from the caller (`cJSON_Print`, `cJSON_Compare`, etc.) walk the C structs and rebuild a temporary internal arena to reuse the real engine logic, rather than duplicating it. This is the one part of the codebase with meaningful `unsafe` — see [Code quality](#code-quality-and-unsafe) below for exactly how much and why that's the right place for it to live.

---

## Test strategy and honest parity numbers

cJSON's own test suite has a structural property that shapes everything about how a cross-language port can honestly claim "passes the original tests": every one of its 18 core test files includes a shared `common.h`, which does `#include "../cJSON.c"` — the *source file*, not the header. This means every original test file, as written, compiles the entire C implementation directly into itself, giving 12 of the 18 files direct access to `static` (non-exported) internal functions like `parse_number` and `print_number` that simply have no name, shape, or meaning in a separately-compiled Rust binary.

We split the 18 core files honestly rather than pretend this wasn't a problem:

| Category | Files | Can run against the port? |
|---|---|---|
| **Adapter-eligible** (public-API-only) | `cjson_add.c`, `compare_tests.c`, `minify_tests.c`, `parse_examples.c`, `parse_with_opts.c`, `readme_examples.c` | **Yes. Genuine, unmodified, verified on Linux** |
| **White-box** (calls internal C statics with no Rust equivalent) | the remaining 12 files (`parse_number.c`, `print_number.c`, `misc_tests.c`, etc.) | No. Behavioral intent re-expressed as new black-box tests instead |

**For the 6 adapter-eligible files: genuinely 72 of 72 assertions passing, unmodified, on Linux, against the real compiled `librjson.so`.** Per the hackathon FAQ's own guidance ("keep the original test files unchanged, run them via a thin adapter or FFI shim"), we never edit anything under `tests/original/`, instead, a separate `tests/adapter/` directory provides an alternate `common.h` that C's *quoted*-include path resolution picks up when the (byte-identical, verbatim-copied) test files are compiled from that directory instead. `tests/original/`'s SHA-256 hashes, pinned at kickoff, have never changed. Getting to a genuine 72/72 took two real, documented corrections along the way, an initial adapter design that silently tested the *original* C library instead of the port (a C `#include` path-resolution mistake), and a Docker line-ending bug that corrupted vendored test fixtures on Windows checkout. Both are written up in full, including the honest intermediate failing numbers, in `DECISIONS.md` #11 and #19.

**For the 12 white-box files:** rather than building fake internal Rust functions purely to satisfy old C test files calling things like `parse_number` directly, which would be exactly the "make the tests green without proving real correctness" pattern this hackathon is scoring against, we re-express each file's behavioral intent as new tests calling only the public API (`tests/parse_number_tests.rs`, `tests/print_number_tests.rs`, etc., naming-matched to their white-box originals for traceability). Our assertion-level audit confirms 349 literal assertions across 124 corresponding port functions (see `DECISIONS.md` #22 for the complete file-by-file breakdown).

**134 native Rust tests pass** across the full crate: arena, constructors, tree mutation, deletion, references, duplication, comparison, the printer, and the parser.

---

## Behavioral fidelity

Full detail is in [`DECISIONS.md`](./DECISIONS.md) (25 documented architectural decisions); a few highlights judges are likely to look for first:

- **Raw byte passthrough, not lossy UTF-8 handling.** cJSON parses and stores strings as raw bytes without validating UTF-8, and passes invalid UTF-8 straight through. Our `value_string`/key fields are `Vec<u8>`, never `String`, specifically to preserve this rather than silently "fixing" malformed input into something safer-but-different.
- **Numeric edge cases matched deliberately**, including `INT_MAX`/`INT_MIN` clamping on out-of-range integers, the classic `%1.15g` -> round-trip-check -> `%1.17g` float-formatting fallback (verified byte-for-byte against an independently-built C oracle across 39 hand-picked edge cases, including negative zero and values right at `f64::MAX`), overflow parsing to infinity internally and printing as `"null"` (matching the original's actual `strtod`/`isinf` behavior, not a guess), and the Linux/glibc exponent-formatting convention specifically chosen over Windows' 3-digit-padded MSVC convention.
- **Duplicate object keys are preserved, not deduplicated** — including a faithful reimplementation of the original's genuinely odd O(n^2) two-pass, first-match comparison semantics in `cJSON_Compare`, where an object with a duplicate key can compare equal to one without it. This is a real, documented quirk of the original (the original source itself has a `/* TODO horrible O(n^2) */` comment on it), not something we invented.
- **Recursive formatting depth parity**: the printer enforces the same 1000-level nesting limit as the parser (`CJSON_NESTING_LIMIT`). While early engineering drafts hypothesized this as a divergent safety improvement over upstream, subsequent code verification confirmed that original `cJSON` (v1.7.19) actively enforces depth guards across both parsing and recursive string formatting (`print_array` and `print_object`). Our implementation faithfully reproduces these upstream depth semantics without divergence (see `DECISIONS.md` #25 for chronological details).

---

## Bonus Criteria Highlights

To support evaluating judges with direct repository evidence, the project's architectural execution satisfies three optional Port Mortem bonus categories:

### Differential Fuzz Survivor
The repository incorporates a continuous differential fuzzing harness (`fuzz/harness.c` and `bench/c/fuzz_diff_main.c`) that dynamically compares the observable outputs of original `cJSON` against `librjson.so` across arbitrary JSON inputs. As documented in our recorded execution log (`fuzz/log.txt`), a sustained continuous run executed for 65 seconds, evaluating approximately 1.99 million randomized generated inputs with zero genuine behavioral divergences. Following the resolution of an upstream numeric grammar divergence (`DECISIONS.md` #21), the verification ran clean with zero exclusion filters remaining active in the harness.

### Zero Unsafe
The core JSON parsing engine, tree-mutation mechanics, and internal memory structure (`rJSON/src/parser.rs`, `rJSON/src/arena.rs`, and `rJSON/src/lib.rs`) execute entirely in 100% safe Rust, containing zero `unsafe` operations or blocks in the core implementation. All required `unsafe` usage is intentionally isolated to `rJSON/src/facade.rs`, where it exists solely for C-ABI interoperability (`extern "C"` declarations, raw pointer translation, and direct standard runtime `malloc`/`free` FFI bindings per `DECISIONS.md` #24). This structural isolation aligns directly with project design goals and official Port Mortem guidance that FFI boundary `unsafe` is expected while core port logic must remain demonstrably safe.

### Decision Log
The project maintains a rigorous architectural decision log ([`DECISIONS.md`](./DECISIONS.md)) that documents 25 engineering choices chronologically from kickoff through final verification. Every recorded entry explicitly defines the engineering decision, breaks down the technical rationale and architectural trade-offs, and provides a concise plain-language explanation. Rather than presenting curated promotional claims, the document provides complete technical transparency regarding compatibility decisions, memory safety structures, behavioral parity verification, and overall implementation strategy.

---

## Benchmarks

Full methodology in [`bench/methodology.md`](./bench/methodology.md); raw data in [`bench/results.json`](./bench/results.json). Measured inside the same Docker/Linux environment as the test suite, release optimizations on both sides, full lifecycle timing (allocation through teardown), distributions reported.

| Payload | Engine | Initial Baseline (Median) | Optimized Release (Median) | Speedup & Parity Ratio |
|---|---|---|---|---|
| **Small (605 B)** | Original C | 1.18 us | 0.95 us | ~19% faster |
| | Raw Rust Engine | 2.03 us | 1.46 us | **~28% faster** (1.53x of C) |
| | Facade (`librjson.so`) | 2.02 us | 1.55 us | **~23% faster** (1.63x of C) |
| **Medium (3.5 KB)** | Original C | 7.00 us | 5.36 us | ~23% faster |
| | Raw Rust Engine | 10.91 us | 7.92 us | **~27% faster** (1.47x of C) |
| | Facade (`librjson.so`) | 10.32 us | 8.58 us | **~17% faster** (1.60x of C) |
| **Large (590 KB)** | Original C | 2.998 ms | 2.202 ms | ~26% faster |
| | Raw Rust Engine | 3.659 ms | 2.457 ms | **~33% faster** (**1.11x of C**) |
| | Facade (`librjson.so`) | 5.781 ms | 4.650 ms | **~19% faster** (1.89x of C) |

**Honestly: the port is slower than the original**, running roughly 1.1-1.5x on the raw engine (narrowing to just **1.11x** on large files under our current optimized zero-dependency build), and up to ~1.9x through the facade on large payloads. This difference is the explicit architectural cost of materializing a real C-heap pointer tree from the safe internal arena on every C-ABI invocation (a real, quantified structural cost of our two-layer design, detailed in `DECISIONS.md` #20 and #23). We intentionally traded this localized translation overhead for an ironclad, zero-unsafe parser engine with automatic memory cleanup upon parse failure.

---

## Code quality and `unsafe`

| File | `unsafe` occurrences |
|---|---|
| `arena.rs` (core tree engine) | **0** |
| `parser.rs` | **0** (one code comment mentions the word, no actual unsafe block) |
| `facade.rs` (C-ABI boundary) | 200 |
| `lib.rs` | 0 |

All `unsafe` code is confined to the FFI boundary only. The parsing, printing, and tree-mutation engine that does the actual work has none.

---

## Repository layout

```
rJSON/
+-- src/
|   +-- arena.rs          -- core tree engine (0 unsafe)
|   +-- parser.rs          -- recursive-descent parser (0 unsafe)
|   +-- facade.rs           -- C-ABI boundary (unsafe lives here)
|   +-- bin/raw_timing.rs    -- benchmark timing driver
+-- tests/
|   +-- original/            -- the 18 core cJSON test files, byte-identical to kickoff
|   +-- original-utils/      -- the 3 cJSON_Utils test files (out-of-scope utils suite)
|   +-- adapter/             -- untouched copies + alternate common.h/cJSON.h, vendored
|   |                            Unity framework + fixtures, for the 6 adapter-eligible files
|   +-- *.rs                  -- new Rust tests, including white-box behavioral re-expression
+-- fuzz/                    -- crate-level libFuzzer crash-fuzzing target
+-- benches/, bin/raw_timing.rs -- benchmark harness
+-- tests-kickoff.sha256, tests-kickoff-utils.sha256
+-- rust-toolchain.toml
bench/                        -- cross-language benchmark harness and POSIX differential fuzzer engine
fuzz/                         -- root anatomy differential fuzzer proxy (harness.c and log.txt proof)
DECISIONS.md                   -- every non-trivial decision, with rationale
AI_GUARDRAILS.md                -- standing rules given to AI coding agents on this project
reference-outputs.md             -- captured ground-truth C behavior used throughout porting
Dockerfile, build.sh, build.ps1
.port-mortem.toml
```

---

## Fuzzing, robustness, and scope boundaries

To provide complete technical transparency for evaluating judges:

- **Continuous differential fuzzing** — a crate-level crash fuzzer exists (`rJSON/fuzz/fuzz_targets/fuzz_parse.rs`), alongside a complete root-level continuous differential fuzzer (`fuzz/harness.c` / `bench/c/fuzz_diff_main.c`) comparing original C against `librjson.so`. Verified over 65+ second monotonic runs evaluating ~1.99 million payloads (`fuzz/log.txt`) with zero divergences, after addressing an authentic trailing-dot grammar divergence documented in `DECISIONS.md` #21.
- **White-box test parity** — 6 of the 12 original internal-test files have complete behavioral-intent coverage in `tests/port/`, and 6 have partial coverage with explicitly documented, named boundaries (`misc_tests.c` being a multi-function utility grab-bag). A complete assertion-level audit is recorded in `DECISIONS.md` #22.
- **Out-of-scope utilities** — `cJSON_Utils` (JSON Pointer, JSON Patch, and JSON Merge Patch) is explicitly out of scope by design (see `DECISIONS.md` #1). Its original verification files under `tests/original-utils/` are preserved byte-identical to kickoff.

---

## Team

- **[Maanas Chawan](https://github.com/MVC2408)** — parser core, behavioral compatibility, regression testing & parser validation
- **[Ashutosh Mishra](https://github.com/Dev-Am12)** — data model, tree mutation, printer
- **[Shivam Kshirsagar](https://github.com/ShivammKshirsagar)** — C-ABI facade, build/adapter infrastructure, benchmarking, fuzzing

## License

MIT — see [`LICENSE`](./LICENSE). This port is an independent reimplementation; no code is copied from the original cJSON, which is also MIT-licensed and copyright Dave Gamble and cJSON contributors.
