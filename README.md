# rJSON — a Rust port of cJSON

**Port Mortem 2026 · Track A (C → Rust) · [DaveGamble/cJSON](https://github.com/DaveGamble/cJSON) v1.7.19**

rJSON is a from-scratch Rust reimplementation of cJSON, a small, extremely widely-used ANSI C JSON parser/printer/tree-manipulation library. It ships as both an idiomatic native Rust crate and a drop-in, C-ABI-compatible shared library (`librjson.so`/`.dll`/`.dylib`) that the *original, unmodified* cJSON test suite can link against and pass.

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
- [What's still open](#whats-still-open)
- [Team](#team)
- [License](#license)

---

## Quick start

**One-command build:**
```bash
docker build -t rjson .
```
This builds the Rust library, runs the full native Rust test suite (132 tests), compiles the six adapter-eligible original C test files against the built `librjson.so`, and links them, all inside the container, with no dependency on anything outside this repo.

**To see the original test suite pass against the port directly**, the Dockerfile's test stage runs each of the six compiled adapter binaries and reports pass/fail counts per file. See [Test strategy](#test-strategy-and-honest-parity-numbers) below for exactly what "adapter-eligible" means and why not all 18 original test files can run this way.

**Native development build** (outside Docker):
```bash
cd rJSON
cargo build --release       # builds librjson.{so,dylib,dll} + the rlib
cargo test                  # runs the 132 native Rust tests
```

**Verifying the original test suite hasn't been touched:**
```bash
cd rJSON
sha256sum -c tests-kickoff.sha256          # core cJSON test files
sha256sum -c tests-kickoff-utils.sha256    # cJSON_Utils test files (stretch scope)
```
Both should report every file as `OK`. If they don't, something in `tests/original/` or `tests/original-utils/` has changed since kickoff.

---

## What this actually is

[cJSON](https://github.com/DaveGamble/cJSON) is a ~3,500-line, ANSI C89 JSON library: a recursive-descent parser, a printer (with pretty, compact, buffered, and zero-allocation variants), and a mutable in-memory tree API for building and editing JSON documents by hand (add/delete/detach/replace/duplicate/compare). It's been in production use for over a decade and has a genuinely thorough test suite which is exactly what made it a demanding, honest target for this hackathon's actual question: not "can you make something that compiles," but "can you make something that *behaves the same*."

rJSON reimplements all of that from scratch in Rust. Its not a wrapper around an existing JSON crate, not a transpiler, nor an FFI shim into the original library. It's structured in two layers:

1. **A safe, idiomatic Rust engine** — an arena-indexed tree, a recursive-descent parser, and a printer, none of which use `unsafe`.
2. **A thin C-ABI facade** — `#[repr(C)]`, `extern "C"`, exporting the same public function signatures as `cJSON.h` — that lets C code (including the *original, byte-for-byte unmodified* cJSON test files) link against this library exactly as they would against the original.

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
                     directly via 132 native Rust tests)
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

**For the 12 white-box files:** rather than building fake internal Rust functions purely to satisfy old C test files calling things like `parse_number` directly, which would be exactly the "make the tests green without proving real correctness" pattern this hackathon is scoring against. We re-express each file's behavioral intent as new tests calling only the public API (`tests/parse_number_tests.rs`, `tests/print_number_tests.rs`, etc., naming-matched to their white-box originals for traceability). *A full file-by-file assertion-count mapping table is in progress and will be added to `DECISIONS.md` before final submission.*

**132 native Rust tests pass** across the full crate: arena, constructors, tree mutation, deletion, references, duplication, comparison, the printer, and the parser.

---

## Behavioral fidelity

Full detail is in `DECISIONS.md` (20 entries as of this draft); a few highlights judges are likely to look for first:

- **Raw byte passthrough, not lossy UTF-8 handling.** cJSON parses and stores strings as raw bytes without validating UTF-8, and passes invalid UTF-8 straight through. Our `value_string`/key fields are `Vec<u8>`, never `String`, specifically to preserve this rather than silently "fixing" malformed input into something safer-but-different.
- **Numeric edge cases matched deliberately**, including `INT_MAX`/`INT_MIN` clamping on out-of-range integers, the classic `%1.15g` -> round-trip-check -> `%1.17g` float-formatting fallback (verified byte-for-byte against an independently-built C oracle across 39 hand-picked edge cases, including negative zero and values right at `f64::MAX`), overflow parsing to infinity internally and printing as `"null"` (matching the original's actual `strtod`/`isinf` behavior, not a guess), and the Linux/glibc exponent-formatting convention specifically chosen over Windows' 3-digit-padded MSVC convention.
- **Duplicate object keys are preserved, not deduplicated** — including a faithful reimplementation of the original's genuinely odd O(n^2) two-pass, first-match comparison semantics in `cJSON_Compare`, where an object with a duplicate key can compare equal to one without it. This is a real, documented quirk of the original (the original source itself has a `/* TODO horrible O(n^2) */` comment on it), not something we invented.
- **One disclosed improvement**: the printer enforces the same 1000-level nesting limit the parser does. The original only enforces this at parse time. A sufficiently deep *programmatically constructed* tree could exhaust the C stack when printed, with no guard at all. We added one and documented it as a deliberate, disclosed divergence rather than silently changing behavior.

---

## Benchmarks

Full methodology in [`bench/methodology.md`](./bench/methodology.md); raw data in [`bench/results.json`](./bench/results.json). Measured inside the same Docker/Linux environment as the test suite, release optimizations on both sides, full lifecycle timing (allocation through teardown), distributions reported.

| Payload | Original C | Raw Rust engine | Facade (`librjson.so`) |
|---|---|---|---|
| Small (583 B) | 1.18 us median | 2.03 us median | 2.02 us median |
| Medium (3.5 KB) | 7.00 us median | 10.91 us median | 10.32 us median |
| Large (586 KB) | 2.998 ms median | 3.659 ms median | 5.781 ms median |

**Honestly: the port is slower than the original**, roughly 1.2-1.5x on the raw engine, and up to ~1.9x through the facade on large payloads (the cost of materializing a real C-heap pointer tree from the internal arena on every call, a real, quantified, disclosed structural cost of the two-layer architecture, not hidden anywhere). We are actively working on closing this gap before final submission; if further optimization attempts don't succeed, that attempt and its results will be documented honestly here rather than left unmentioned.

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
|   +-- original-utils/      -- the 3 cJSON_Utils test files (stretch-goal scope)
|   +-- adapter/             -- untouched copies + alternate common.h/cJSON.h, vendored
|   |                            Unity framework + fixtures, for the 6 adapter-eligible files
|   +-- *.rs                  -- new Rust tests, including white-box behavioral re-expression
+-- fuzz/                    -- libFuzzer crash-fuzzing target (differential fuzzer in progress)
+-- benches/, bin/raw_timing.rs -- benchmark harness
+-- tests-kickoff.sha256, tests-kickoff-utils.sha256
+-- rust-toolchain.toml
bench/                        -- cross-language benchmark harness (C + Rust)
DECISIONS.md                   -- every non-trivial decision, with rationale
AI_GUARDRAILS.md                -- standing rules given to AI coding agents on this project
reference-outputs.md             -- captured ground-truth C behavior used throughout porting
Dockerfile, build.sh, build.ps1
.port-mortem.toml
```

---

## What's still open

In the interest of the same honesty this whole document is trying to model:

- **Differential fuzzing** — a Rust-only crash fuzzer exists (`fuzz/fuzz_targets/fuzz_parse.rs`); a true differential harness comparing original-vs-port output on shared input is in active development and not yet complete.
- **White-box test parity table** — the underlying re-expressed tests exist; the formal file-by-file assertion-count mapping documenting them is not yet written up.
- **Two small cleanup items**: `append_child` is currently `pub` rather than `pub(crate)` (it's only ever called through validated public entry points today, but its visibility doesn't yet reflect that), and `cargo fmt --check` currently reports formatting differences not yet applied.
- **Performance** — see [Benchmarks](#benchmarks) above; optimization work is planned before submission.
- **`cJSON_Utils`** (JSON Pointer/Patch/Merge Patch) is out of scope by design (see `DECISIONS.md` #1), tests preserved unmodified in case it's promoted to a stretch goal.

---

## Team

- **[Maanas Chawan](https://github.com/MVC2408)** — parser
- **[Ashutosh Mishra](https://github.com/Dev-Am12)** — data model, tree mutation, printer
- **[Shivam Kshirsagar](https://github.com/ShivammKshirsagar)** — C-ABI facade, build/adapter infrastructure, benchmarking, fuzzing

## License

MIT — see [`LICENSE`](./LICENSE). This port is an independent reimplementation; no code is copied from the original cJSON, which is also MIT-licensed and copyright Dave Gamble and cJSON contributors.
