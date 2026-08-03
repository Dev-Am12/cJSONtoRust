# DECISIONS.md — rJSON (cJSON → Rust, Track A)

Every non-trivial, non-mechanical divergence or architectural choice made during this port, in the order it was decided. Each entry has the technical reasoning followed by a plain-language recap for quick reading.

## Table of Contents

- [1. Scope: Core `cJSON.c`/`cJSON.h` only](#1-scope-core-cjsonccjsonh-only)
- [2. Test suite strategy: zero edits, via an external adapter](#2-test-suite-strategy-zero-edits-via-an-external-adapter)
- [3. Core data structure: arena/index-based tree, not raw pointers](#3-core-data-structure-arenaindex-based-tree-not-raw-pointers)
- [4. Sibling-list tail tracking: conventional invariant over C's optimization](#4-sibling-list-tail-tracking-conventional-invariant-over-cs-optimization)
- [5. Arena deletion: logical, not physical and matches C's dual-ownership flags exactly](#5-arena-deletion-logical-not-physical-and-matches-cs-dual-ownership-flags-exactly)
- [6. Delete cascade: deleting an unattached child also deletes its later siblings](#6-delete-cascade-deleting-an-unattached-child-also-deletes-its-later-siblings)
- [7. NodeId validation: two-tier policy](#7-nodeid-validation-two-tier-policy)
- [8. Known future requirement: `valueint` is referenced by an adapter-eligible test file](#8-known-future-requirement-valueint-is-referenced-by-an-adapter-eligible-test-file)
- [9. Docker base image: `rust:slim-bookworm` builder, `debian:bookworm-slim` runtime](#9-docker-base-image-rustslim-bookworm-builder-debianbookworm-slim-runtime)
- [10. Improvement in recursive safety](#10-improvement-in-recursive-safety)
- [11. Facade layer: materialise-and-free design; `cJSON_Delete` owns C-heap, not the arena](#11-facade-layer-materialise-and-free-design-cjson_delete-owns-c-heap-not-the-arena)
- [12. Public parser entry points: `cjson_parse`, `cjson_parse_with_opts`, `cjson_parse_with_length_opts`](#12-public-parser-entry-points-cjson_parse-cjson_parse_with_opts-cjson_parse_with_length_opts)
- [13. Parsed string and object-key values are raw `Vec<u8>`, never validated UTF-8 or Rust `String`](#13-parsed-string-and-object-key-values-are-raw-vecu8-never-validated-utf-8-or-rust-string)
- [14. Numeric overflow produces `f64::INFINITY`, not a parse error; printing must special-case it](#14-numeric-overflow-produces-f64infinity-not-a-parse-error-printing-must-special-case-it)
- [15. Object parsing permits duplicate keys; no lookup, rejection, or de-duplication occurs](#15-object-parsing-permits-duplicate-keys-no-lookup-rejection-or-de-duplication-occurs)
- [16. `valueint`-equivalent clamping is a single free function: `clamped_int_value`](#16-valueint-equivalent-clamping-is-a-single-free-function-clamped_int_value)
- [17. Nightly-toolchain tooling sub-crates must be isolated as their own Cargo workspace](#17-nightly-toolchain-tooling-sub-crates-must-be-isolated-as-their-own-cargo-workspace)
- [18. `cJSON_InitHooks` now routes facade C-heap allocations; adapter total corrected to 72/72](#18-cjson_inithooks-now-routes-facade-c-heap-allocations-adapter-total-corrected-to-7272)
- [19. Self-contained Docker build: Unity and fixtures vendored into adapter/](#19-self-contained-docker-build-unity-and-fixtures-vendored-into-adapter)
- [20. Dual A/B Benchmarking: Raw Rust Core vs. Facade-Wrapped C-Bridge vs. Original C](#20-dual-ab-benchmarking-raw-rust-core-vs-facade-wrapped-c-bridge-vs-original-c)
- [21. Differential Fuzzing Established Parser Behavioral Parity with Upstream cJSON](#21-differential-fuzzing-established-parser-behavioral-parity-with-upstream-cjson)

---

## 1. Scope: Core `cJSON.c`/`cJSON.h` only

**Decision:** The committed scope for this submission is the core cJSON library (the parser, printer, and tree-mutation API in `cJSON.c` and `cJSON.h`, ~3,512 LOC). `cJSON_Utils` (JSON Pointer, JSON Patch, and JSON Merge Patch, ~1,569 LOC) and `test.c` (a standalone Makefile demo, not part of the CMake/Unity test suite) are explicitly out of scope. `cJSON_Utils` is a promotable stretch goal only if the core is complete with time to spare.

**Rationale:** Test parity is measured only against `tests/original/` (the 20 core test files, hashed in `tests-kickoff.sha256`). The three `cJSON_Utils`-only test files are preserved unmodified under `tests/original-utils/` (hashed separately in `tests-kickoff-utils.sha256`) so they are ready if `cJSON_Utils` gets promoted, but they are never counted against us while untouched.

**In plain terms:** We are porting the "core" JSON library: parsing, printing, and building or editing a JSON tree. We are not porting the optional add-on toolkit (JSON Patch/Pointer) unless we finish early. We kept its tests safely off to the side, unedited, in case we get to it.

---

## 2. Test suite strategy: zero edits, via an external adapter

**Decision:** The `tests/original/` directory is never modified, including build-plumbing files like `common.h`. The original test files transitively pull in `cJSON.c`'s full source (via `common.h`'s `#include "../cJSON.c"`), which cannot resolve against a Rust binary as-is. Rather than editing `common.h` to fix this (an earlier, since-corrected plan), we build a separate adapter outside `tests/original/`. An alternate `common.h` in `tests/adapter/` resolves via include-path ordering when compiling the original `.c` files against our Rust `cdylib`, instead of against the real `common.h`.

**Rationale:** Of the 18 core test files, roughly 6 (`cjson_add.c`, `compare_tests.c`, `minify_tests.c`, `parse_examples.c`, `parse_with_opts.c`, `readme_examples.c`) call only the public API and are adapter-eligible. The remaining ~12 call internal `static` C functions with no Rust equivalent (e.g., `parse_number`, `print_number`) and cannot run against any cross-language port; their behavioral intent is instead re-expressed as new tests in `tests/port/`, mapped file-by-file, and reported separately from literal test-parity.

**In plain terms:** We never touch the original tests, not even to fix a build error. Instead, we quietly swap in a different "instructions" file outside the tests folder that tells the compiler where to find things, so the original test files can run against our Rust code untouched. About a third of the original tests check C-internal implementation details that simply do not exist in Rust. For those, we wrote new tests that check the same behavior through the public functions instead, and we are upfront that these are new tests, not the literal originals passing.

---

## 3. Core data structure: arena/index-based tree, not raw pointers

**Decision:** The Rust port's internal tree representation uses an arena (`Arena { nodes: Vec<Node> }`), with `next`, `prev`, and `child` links stored as `Option<NodeId>` indices rather than C-style raw pointers.

**Rationale:** A direct 1:1 translation of C's pointer-linked tree would require heavy `unsafe` usage throughout the core parsing, printing, and tree-mutation logic. The arena design keeps the borrow checker satisfied naturally and keeps `unsafe` out of the core engine entirely.

This representation does not match the C `cJSON` struct's memory layout field-for-field. The ~6 adapter-eligible test files that read struct fields directly cannot link against this representation as-is without a translation layer. To mitigate this, a thin `#[repr(C)]` facade layer at the public API boundary translates between a C-compatible view and the arena internally. This allows those files to link through the adapter while keeping the arena as the real engine underneath. This is treated as a checkpoint decision around the ~hour-24 mark, not built speculatively now. If it is not built, adapter-based test linkage will be 0%, reported honestly rather than implied otherwise.

**In plain terms:** Instead of using memory pointers to link tree nodes together (the C way), our nodes live in one big list and point at each other using plain numbers (positions in that list). This makes our Rust code much safer with far less risky "trust me" code, but it means our internal node shape doesn't look like C's anymore, which affects how much of the original test suite can literally run against us. We are planning a small translation layer to bridge that gap later, once the core engine is solid.

---

## 4. Sibling-list tail tracking: conventional invariant over C's optimization

**Decision:** The original C implementation uses an internal optimization where the first child's `prev` pointer does not point backward—it points to the last child, enabling O(1) tail lookup for appends. Our `append_child` does not replicate this; instead, it walks the sibling list to find the tail (O(n) append), preserving the conventional invariant that every node's `prev` is its true immediate predecessor (and the first child has none).

**Rationale:** This C behavior is a pure internal implementation detail that is invisible to every public API caller and not asserted on by any in-scope test file (checked `cjson_add.c` specifically). Preserving the conventional invariant makes bidirectional traversal direct and unambiguous with `NodeId` links, at the cost of O(n) append time. Reproducing C's optimization would require breaking the "prev = true predecessor" invariant for no observable behavioral benefit.

**In plain terms:** The original C code has a clever but confusing trick: the first item in a list stores a shortcut to the tail, to make adding new items faster. We chose not to copy that trick; our "previous" pointers always mean exactly what they say. Ours is a little slower when appending to a very long list, but far easier to reason about and never causes confusion later.

---

## 5. Arena deletion: logical, not physical and matches C's dual-ownership flags exactly

**Decision:** `Arena::delete(NodeId)` cannot physically remove an entry from the `Vec` without invalidating every later `NodeId` still in use elsewhere. Deletion is therefore logical: a private, index-aligned `deleted: Vec<bool>` sidecar on `Arena` (not a field on the locked `Node` struct, the interface stays exactly as specified for the team) marks nodes as gone. Deletion is idempotent (safe to call twice).

**Rationale:** Ownership during deletion follows the original's actual dual-flag rule precisely, not a simplified single-flag version. The `is_reference` flag gates whether a node's `child` chain and `value_string` are cleared (a reference node's children and string belong to someone else). The `key_is_const` flag separately gates whether the `key` is cleared. This matches `cJSON_Delete`'s real behavior of checking `IsReference` and `StringIsConst` independently.

**In plain terms:** We don't actually erase deleted nodes from memory; we just mark them as "gone," because our node list can't safely have holes punched in it. Deleting something twice is safe; it just does nothing the second time. We correctly copied a subtle rule from the original: a node can independently "not own" its content and "not own" its name-tag, and deleting it respects both of those separately, exactly like the original C library does.

---

## 6. Delete cascade: deleting an unattached child also deletes its later siblings

**Decision:** `Arena::delete` follows the node's own `next` chain, meaning deleting an attached-but-not-detached child also logically deletes every sibling that comes after it in that same chain.

**Rationale:** This is not a bug; it is a deliberate match of `cJSON_Delete`'s real C behavior (`while (item != NULL) { ...; item = item->next; }`), which walks and frees the whole remaining chain from whatever node it is given. Consequently, callers must `detach` a child (removing it from its sibling chain first) before deleting only that one item, matching the exact same usage contract the original C library has always had.

**In plain terms:** If you delete one item out of the middle of a list without first "unplugging" it from its neighbors, everything after it in that list gets deleted too. This mirrors a real, sometimes-surprising quirk of the original C library, not a mistake in our port. If you only want to remove one item, detach it first.

---

## 7. NodeId validation: two-tier policy

**Decision:** Public, externally-callable API functions (`add_item_to_array`, `add_item_to_object`, and all subsequent public entry points) validate that a `NodeId` is both in-bounds and not already deleted (`is_live_node`) before acting, returning `false` or `None` on invalid input rather than panicking. Internal, module-private accessors (`Arena::get` and `Arena::get_mut`) continue to trust their caller and panic on an invalid `NodeId`, per the original Task 1 design.

**Rationale:** This gives a clean, predictable boundary. Code entirely internal to the arena's own implementation can assume well-formed IDs (a programmer error there is a real bug, and panicking surfaces it immediately during development). Code reachable from outside the module treats an invalid ID as ordinary bad input to be reported through a normal return value, not a crash.

**In plain terms:** Functions meant to be called from outside our module politely say "no" (return false/nothing) if you hand them a bad ID. Functions meant only for internal use trust that whoever is calling them already did that checking, and will crash loudly if that trust is violated. This is intentional, so internal bugs get caught immediately during development instead of silently producing wrong results.

---

## 8. Known future requirement: `valueint` is referenced by an adapter-eligible test file

**Decision:** Our locked `Node` struct intentionally has no equivalent to C's deprecated `valueint` field (an `int`-truncated view of `value_double`).

**Rationale:** A grep of `tests/original/` confirms 6 direct references across `cjson_add.c`, `parse_number.c`, and `misc_tests.c`. Critically, `cjson_add.c` is one of the ~6 adapter-eligible files. This means the eventual `#[repr(C)]` facade, if built, must synthesize a `valueint`-equivalent field (or accessor) at the C-ABI boundary to keep `cjson_add.c` linkable, even though the arena's internal `Node` never carries one. This is logged as a finding for whoever builds the facade layer.

**In plain terms:** One of the original test files we're hoping to run unmodified checks an old, deprecated integer field that our clean internal design deliberately doesn't have. This isn't a problem yet, but whoever builds the translation layer later needs to remember to add this field back in, just for outside compatibility, without polluting our actual internal node type.

---

## 9. Docker base image: `rust:slim-bookworm` builder, `debian:bookworm-slim` runtime

**Decision:** The `Dockerfile` uses `rust:slim-bookworm` as the builder stage and `debian:bookworm-slim` as the runtime stage. The Rust channel is not pinned in the Dockerfile; `rJSON/rust-toolchain.toml` is the single source of truth for the channel, read automatically by the `rustup` already present in the base image. The `build-essential` and `cmake` packages are installed in the builder stage proactively so future bench and fuzz C-compilation stages can reuse the cached layer without a rebuild.

**Rationale:** `rust:slim-bookworm` is the official Rust Docker image on a stable Debian Bookworm slim base; it ships rustup and resolves the `rust-toolchain.toml` channel pin with no extra scaffolding. Pinning the channel in both the Dockerfile and `rust-toolchain.toml` would create a drift risk—a later channel bump in `rust-toolchain.toml` would silently be ignored by Docker unless the Dockerfile was also updated. Letting `rust-toolchain.toml` own the pin eliminates that class of inconsistency entirely.

**In plain terms:** The Docker build uses the official Rust image on Debian. The Rust version comes from the `rust-toolchain.toml` file that already exists in the repo, and we don't duplicate it in the Dockerfile, so there's no chance the two get out of sync later. We also pre-install the C compiler tools in the build stage now, so future Docker work (benchmarks, fuzzing) doesn't have to wait for those to download from scratch.

---

## 10. Improvement in recursive safety

**Decision:** `print_array_at` and `print_object_at` enforce `CJSON_NESTING_LIMIT` (1000).

**Rationale:** This is an intentional safety improvement, not strict parity. Original cJSON enforces the limit during parsing but does not guard its recursive printer. The guard prevents stack overflow for programmatically constructed trees deeper than the parser limit.

**In plain terms:** We added a safety limit to printing that matches the parsing limit. The original library could crash if you manually built an extremely deep tree and tried to print it; our version stops safely before that happens.

---

## 11. Facade layer: materialise-and-free design; `cJSON_Delete` owns C-heap, not the arena

**Decision:** The `#[repr(C)]` facade layer (`rJSON/src/facade.rs`) uses the "materialise-on-return" approach. Each `cJSON_Parse*` call creates a short-lived Rust `Arena`, parses into it via the existing Rust engine, then walks the arena tree to allocate a mirror image as C-heap `cJSON` structs with real `next`, `prev`, and `child` pointer links. The `valueint` is synthesised from `value_double` via `clamped_int_value()`, and `valuestring`/`string` are `malloc`-allocated C strings. The arena is dropped before returning. `cJSON_Delete` on the C side walks and frees the C-heap structs; Rust is not involved. Functions that receive a `*const cJSON` back (e.g., `cJSON_Print`, `cJSON_Compare`) walk the C-struct tree and rebuild a temporary arena internally.

**Rationale:** This keeps `unsafe` code tightly bounded at the `extern "C"` boundary. There is no global mutable state beyond the single required `static mut GLOBAL_ERROR_PTR` for `cJSON_GetErrorPtr`. There are no new crate dependencies beyond `libc` (a thin FFI shim). `cJSON_Delete` is a straightforward `free`-walk with zero risk of double-free from Rust's side.

On `cJSON_InitHooks`: This is an intentional no-op. Our allocator is Rust's arena (internal) and `libc::malloc` (materialisation). The 13 `*_on_allocation_failure` tests in `cjson_add.c` fail when genuinely tested against the Rust DLL—this is the correct, expected outcome per the original plan.

Adapter architecture correction (2026-08-02): The initial adapter design used `-I rJSON/tests/adapter` to shadow the original `common.h`. This did not work because `#include "common.h"` in C uses quoted-include semantics, which searches the source file's own directory first, bypassing `-I` paths entirely. Additionally, the available GCC is MinGW 32-bit while our Rust DLL is MSVC x64, which caused crashes on DLL load. Test files are now copied verbatim to `rJSON/tests/adapter/` and compiled with MSVC `cl.exe` x64. The real adapter results show 59 tests passing and 13 failing (all 13 are `*_on_allocation_failure` tests).

**In plain terms:** When we hand a JSON tree to C, we convert it entirely into the C format: real pointer links, and all fields filled in. C owns that memory and frees it itself via `cJSON_Delete`. Rust only gets involved again if C hands the pointer back to Print or Compare. The 13 allocation-failure tests correctly report failures, which is honest and expected for our setup.

---

## 12. Public parser entry points: `cjson_parse`, `cjson_parse_with_opts`, `cjson_parse_with_length_opts`

**Decision:** The parser exposes exactly three public entry points, mirroring upstream's `cJSON_Parse`, `cJSON_ParseWithOpts`, and `cJSON_ParseWithLengthOpts` subset relationship. Each returns `Result<(NodeId, usize), CJsonParseError>`, where the `usize` is the parse-end offset on success and `CJsonParseError { position: usize }` carries the same offset upstream would have exposed through both `return_parse_end` and `cJSON_GetErrorPtr` on failure. `cjson_parse_with_opts` truncates its input at the first `0x00` byte before delegating to `cjson_parse_with_length_opts`, which is given the full slice and does not truncate at an embedded NUL.

**Rationale:** These three functions are the parser's entire public contract. Any consumer of parsed trees (the eventual facade layer, printer, benchmarks, fuzzing) must call through them and cannot assume a hidden global error channel exists. Collapsing error and end-of-parse reporting into a single return value keeps the parser thread-safe and side-effect-free by construction, while giving the facade layer everything it needs to synthesize a C-ABI `cJSON_GetErrorPtr()` and `return_parse_end` later. Truncating vs. not truncating at a NUL byte mirrors upstream's real `strlen` vs. explicit-length distinction and must be preserved as-is.

**In plain terms:** There are three ways to kick off a parse, matching the three original C functions. Each one hands back either the parsed tree plus where it stopped, or an error with a position number—never a hidden global you have to remember to check separately. One function stops reading at the first embedded NUL byte, and the other two don't; that's intentional and matches the original library, so don't try to make them artificially consistent.

---

## 13. Parsed string and object-key values are raw `Vec<u8>`, never validated UTF-8 or Rust `String`

**Decision:** Every string value and object key produced by the parser is stored and passed around as raw bytes (`Vec<u8>`), copied through without any UTF-8 validation or lossy replacement. No parser code path ever constructs a Rust `String` or calls `str::from_utf8` on string content. Malformed or non-UTF-8 byte sequences inside a JSON string are preserved exactly as upstream cJSON's raw pointer-copy would preserve them.

**Rationale:** Upstream cJSON has no concept of "invalid UTF-8" inside a string; it copies bytes verbatim. Converting to a validated Rust `String` (even via a lossy conversion) would silently replace malformed sequences with replacement characters, creating a real behavioral divergence from upstream. Any module that reads, compares, or prints string/key content must treat these fields as opaque byte buffers rather than assuming they are valid UTF-8.

**In plain terms:** Text values coming out of the parser aren't checked or "cleaned up" as valid text. They're kept as raw bytes, exactly like the original C library does. Anything built later that reads these values needs to treat them as byte buffers, not assume they're always well-formed text.

---

## 14. Numeric overflow produces `f64::INFINITY`, not a parse error; printing must special-case it

**Decision:** Extremely large numeric literals (for example, `1e400`) are accepted by the parser and stored with `value_double == f64::INFINITY`, matching upstream's unconditional acceptance of `HUGE_VAL` at parse time. The parser does not reject or clamp such values.

**Rationale:** Upstream only converts an infinite or NaN `value_double` to the text `"null"` at print time, not at parse time. Rejecting overflow during parsing would therefore diverge from upstream behavior. Whoever implements the printer must reproduce the `isnan`/`isinf` to `"null"` special case, or numbers like `1e400` will round-trip incorrectly once printing exists.

**In plain terms:** A huge number like `1e400` is allowed to parse successfully; it simply ends up stored as "infinity." The original library only turns that into the text `null` when printing it back out, not when reading it in. Whoever writes the printer needs to remember to add that same "infinity/NaN becomes null" rule.

---

## 15. Object parsing permits duplicate keys; no lookup, rejection, or de-duplication occurs

**Decision:** Object members are linked purely in encounter order during parsing. No key-lookup structure is consulted, and duplicate keys are neither detected nor rejected, matching upstream cJSON's `parse_object`, which performs no duplicate-key check either.

**Rationale:** This is an intentional match of upstream behavior, not an oversight. Any future code that looks up object members by key (for example, a `cJSON_GetObjectItem` equivalent) must decide its own first-match/last-match policy over a member list that may legitimately contain duplicates. It cannot assume keys are unique.

**In plain terms:** Objects can end up with the same key appearing more than once, just like in the original library, since nothing checks for or removes duplicates while parsing. Anything built later that looks things up by key needs to be written with that possibility in mind.

---

## 16. `valueint`-equivalent clamping is a single free function: `clamped_int_value`

**Decision:** The saturating integer-truncation behavior of upstream's `valueint` field (referenced as a finding in Decision #8) is implemented as a pure free function, `clamped_int_value(value_double: f64) -> i32`, reproducing upstream's exact saturating comparison (`>=` and `<=` at `INT_MAX` and `INT_MIN`) rather than being stored as a field on `Node`.

**Rationale:** `Node` intentionally carries no `valueint` field. Computing the clamped value on demand from `value_double` avoids adding that field prematurely while still giving every consumer a single, correct place to obtain this value. The facade layer should call this function rather than reimplementing the saturation logic a second time.

**In plain terms:** There's one shared function that turns a stored floating-point number into the same clamped integer the original C library's `valueint` field would have held. It's computed on demand instead of being stored, and anything that needs a `valueint`-like value later should call this function instead of re-deriving the same logic.

---

## 17. Nightly-toolchain tooling sub-crates must be isolated as their own Cargo workspace

**Decision:** Any sub-crate that requires a nightly Rust toolchain (the fuzzing harness today; future benchmarking or similar tooling) declares its own empty `[workspace]` table in its `Cargo.toml`. This makes it its own workspace root rather than a member of the root crate's workspace, and it pins nightly only in its own `rust-toolchain.toml`.

**Rationale:** The root crate is pinned to a stable channel via its own `rust-toolchain.toml`, and everyone builds against that pin. Without workspace isolation, a nightly-only sub-crate's toolchain file or dependencies could be silently pulled into a root `cargo build` or `cargo test` run. An isolated workspace guarantees the nightly requirement stays fully contained to the tooling that needs it.

**In plain terms:** Any tool that needs a bleeding-edge (nightly) Rust compiler—like the fuzzer, and likely future benchmarks—lives in its own self-contained mini-project instead of being folded into the main one. That way, the main project keeps using its normal, stable Rust version no matter what nightly-only tooling gets added alongside it.

---

## 18. `cJSON_InitHooks` now routes facade C-heap allocations; adapter total corrected to 72/72

**Decision:** `cJSON_InitHooks` is implemented for the facade layer. The facade stores one process-global malloc hook and one process-global free hook, matching original cJSON's non-thread-safe global-hook model. Passing `NULL` resets both hooks to the default libc allocation path. All C-heap allocations owned by the facade (`cJSON` structs and copied C strings used for `valuestring`/`string`) now go through the installed malloc hook, and all corresponding frees go through the installed free hook. This applies only to the C-compatible materialised tree at the facade boundary. The internal Rust arena continues to use Rust's normal allocator and is not redirected through cJSON hooks.

**Rationale:** Hook-driven allocation failure returns `NULL` cleanly. If materialising an arena tree into C structs fails partway through, the facade deletes the partial C tree already allocated in that call before returning `NULL`, so no partially-built C nodes or copied strings are left dangling. A standalone `hook_repro.c` confirmed the hook mechanism in isolation. Re-testing the adapter-eligible original test files against the updated facade (genuinely linked to `rjson.dll` via MSVC) results in all 31 `cjson_add.c` tests passing, bringing the total adapter score to 72/72. (Entry #11's 59/72 result was accurate at the time when hooks were intentionally unimplemented; this entry updates that status.)

**In plain terms:** After actually implementing the memory allocation hooks in the translation layer, the remaining 13 allocation-failure tests pass for real. The fake earlier 72/72 result stayed corrected to 59/72 in entry #11, but now the adapter-eligible original tests actually hit 72/72.

---

## 19. Self-contained Docker build: Unity and fixtures vendored into adapter/

**Decision:** `rJSON/tests/adapter/` now contains everything needed for a clean `git clone` and `docker build -t rjson .` with no external dependencies. The `unity/` framework (v2.5) and the `inputs/` folder (cJSON's own JSON test fixtures) are vendored directly into the adapter directory, whereas they were previously only in the gitignored `/cJSON/` directory.

**Rationale:** The Dockerfile runs `cargo test`, compiles all six adapter `.c` files against `librjson.so`, verifies linkage with `ldd`, and runs the tests from the adapter directory so paths resolve cleanly.

A correction was required for line-ending corruption under Docker on Windows. Initially, the build failed with 8 failures in `parse_examples.c` because Windows Git checked out the `.expected` files with `\r\n` line endings, while our Rust printer correctly outputs `\n` (matching original C cJSON). A `.gitattributes` file was added strictly scoped to `rJSON/tests/adapter/inputs/*` to force LF line endings, while deliberately avoiding repository-wide rules to preserve the kickoff-hashed verification directories. With this fix, the Docker build genuinely verifies 72/72 tests passing.

**In plain terms:** We packaged all the test files and the testing framework into the repository so Docker can build and test everything without downloading extra pieces. We had to fix a bug where Windows was silently changing the line endings of our test files, which made our perfectly-matching Linux output look wrong. Now, running the Docker build gets a completely verified green test pass.

---

## 20. Dual A/B Benchmarking: Raw Rust Core vs. Facade-Wrapped C-Bridge vs. Original C

**Decision:** Implemented a reproducible, self-contained benchmarking harness under `/bench` at the true repository root to quantify performance across three progressive payload scales (`small.json`: 583 B; `medium.json`: 3.5 KB; `large.json`: 586 KB). We separately measured two distinct operational profiles against original C (`cJSON.c` v1.7.19): the core logic directly evaluating our zero-copy internal parsing arena, and the C-caller experience evaluating standard dynamic linkage via our compiled drop-in library (`librjson.so`).

**Rationale:** To maintain architectural transparency, all comparative data is gathered exclusively inside a unified Docker Linux runtime utilizing consistent POSIX monotonic timing. The benchmark harness resides in an isolated stage within the Dockerfile, adding zero overhead to the default deliverable build. Across all scales, our memory-safe Rust parsing arena executes within ~1.2x to 1.5x of pure hand-optimized C pointer arithmetic. On the heavy 586 KB dataset, our raw Rust parser completes in a median of 3.66 ms versus original C's 3.00 ms (a ~22% overhead for complete memory safety). The dynamic C-bridge executes in a median of 5.78 ms (~1.9x original C), cleanly demonstrating the ~2.0 ms translation overhead of our "materialise-on-return" architectural decision.

**In plain terms:** When using our Rust JSON parser natively, it is nearly as fast as hand-tuned C—running a massive half-megabyte file in just 3.6 milliseconds (compared to 3.0 milliseconds in C). When C programs plug in our dynamic library replacement, it runs in about 5.7 milliseconds. That extra 2-millisecond difference represents the literal cost of taking our fast internal Rust data and translating it into standard C memory blocks so traditional C programs can understand it without modifying their code.

---

## 21. Differential Fuzzing Established Parser Behavioral Parity with Upstream cJSON

**Decision:** Continuous differential fuzzing between the original `cJSON` implementation and the Rust port is the authoritative mechanism for detecting parser behavioral divergences. During fuzzing, a discrepancy was discovered for numeric literals ending in a trailing decimal point (for example `1.`, `-3.`, and `1.e2`). Rather than enforcing strict RFC 8259 grammar, the parser was updated to emulate the observable behavior of upstream `cJSON`, whose `parse_number()` delegates numeric parsing to the C runtime's `strtod`.

**Rationale:** The project's primary objective is behavioral compatibility with upstream `cJSON`, not independent RFC interpretation. Differential fuzzing exposed a real compatibility difference that was invisible to the existing test suite. Matching the semantics of `strtod`, rather than introducing an isolated special case for trailing decimal literals, preserves compatibility for the broader class of numeric forms accepted by the original library while keeping parser offset advancement, token boundaries, and error reporting consistent with upstream behavior. Re-validating the implementation through sustained differential fuzzing provides confidence that the parser now reproduces `cJSON`'s externally observable behavior without introducing regressions.

**In plain terms:** The fuzzer found that the original C library accepts numbers like `1.` because it relies on the C library's `strtod`, while the Rust parser originally rejected them because it followed the JSON specification more strictly. Instead of adding a one-off fix for that case, the parser was updated to behave like `strtod` in general. After the change, another differential fuzzing run compared roughly 2 million randomly generated inputs against the original library and found zero genuine behavioral differences, confirming that the parser now matches upstream `cJSON` much more closely.

## 22. White-box test parity: assertion-level audit against the original test suite

**Decision:** For the 12 original test files whose assertions test internal
C statics with no Rust equivalent (see entry #2's zero-edit adapter
strategy), we audited how thoroughly their behavioral intent is
represented in `tests/port/`, rather than simply asserting coverage
exists. Method: literal `TEST_ASSERT_*` call sites counted in each
original file; literal `assert!`/`assert_eq!`/`assert_ne!` call sites
counted in the corresponding port file(s), verified by reading content,
not filename-matching alone.

**Result:** 6 of 12 files have Full coverage (`parse_array.c`,
`parse_hex4.c`, `parse_number.c`, `parse_object.c`, `parse_string.c`,
`parse_value.c`). 6 have Partial coverage (`misc_tests.c`,
`print_array.c`, `print_number.c`, `print_object.c`, `print_string.c`,
`print_value.c`) — genuine, named gaps, not filler. None are fully
Missing. Across all 12 originals: 290 literal assertions total. Across
their unique corresponding port files: 349 literal assertions, 143
`#[test]` functions (a port file can cover more ground than a 1:1
assertion count implies, since many tests route through shared
assertion helpers rather than inlining every check).

`misc_tests.c` is the largest identified gap: no dedicated port file
exists for it; its ~224 original assertions are only partially
represented, distributed incidentally across other port files that
happen to exercise related functionality. Remaining named gaps in
`print_number.c` (specific values `0.123`, `1.23e+129`, `1.23e-126`,
pi) were closed as a direct follow-up and independently verified
against a from-scratch C oracle, not just re-run against the port
itself.

**In plain terms:** We checked, file by file and assertion by assertion,
how much of the original's internal-only tests we'd actually managed
to cover with new public-API tests, not just assumed we had. About
half the files are fully covered, half have real, specifically-named
gaps rather than vague ones. The biggest gap is `misc_tests.c`, which
tests a wide grab-bag of internals we haven't built a single dedicated
test file for yet.