# DECISIONS.md — rJSON (cJSON → Rust, Track A)

Every non-trivial, non-mechanical divergence or architectural choice made during this port, in the order it was decided. Each entry has the technical reasoning followed by a plain-language recap for quick reading.

---

## 1. Scope: Core `cJSON.c`/`cJSON.h` only

**Decision:** The committed scope for this submission is the core cJSON library that is the parser, printer, and tree-mutation API (`cJSON.c` + `cJSON.h`, ~3,512 LOC). `cJSON_Utils` (JSON Pointer / JSON Patch / JSON Merge Patch, ~1,569 LOC) and `test.c` (a standalone Makefile demo, not part of the CMake/Unity test suite) are explicitly out of scope. `cJSON_Utils` is a promotable stretch goal only if core is complete with time to spare.

Test Parity is measured only against `tests/original/` (the 20 core test files, hashed in `tests-kickoff.sha256`). The 3 `cJSON_Utils`-only test files are preserved unmodified under `tests/original-utils/` (hashed separately in `tests-kickoff-utils.sha256`) so they're ready if `cJSON_Utils` gets promoted, but they are never counted against us while untouched.

**In plain terms:** We're porting the "core" JSON library: parse, print, build/edit a JSON tree. We're *not* porting the optional add-on toolkit (JSON Patch/Pointer) unless we finish early. We kept its tests safely off to the side, unedited, in case we get to it.

---

## 2. Test suite strategy: zero edits, via an external adapter

**Decision:** `tests/original/` is never modified, including build-plumbing files like `common.h`. The original test files transitively pull in `cJSON.c`'s full source (via `common.h`'s `#include "../cJSON.c"`), which cannot resolve against a Rust binary as-is. Rather than editing `common.h` to fix this (an earlier, since-corrected plan), we build a separate adapter outside `tests/original/`. An alternate `common.h` in `tests/adapter/` that resolves via include-path ordering when compiling the original `.c` files against our Rust `cdylib`, instead of against the real `common.h`.

Of the 18 core test files, roughly 6 (`cjson_add.c`, `compare_tests.c`, `minify_tests.c`, `parse_examples.c`, `parse_with_opts.c`, `readme_examples.c`) call only the public API and are adapter-eligible. The remaining ~12 call internal `static` C functions with no Rust equivalent (e.g. `parse_number`, `print_number`) and cannot run against any cross-language port; their behavioral intent is instead re-expressed as new tests in `tests/port/`, mapped file-by-file, and reported separately from literal test-parity.

**In plain terms:** We never touch the original tests, not even to fix a build error. Instead we quietly swap in a different "instructions" file *outside* the tests folder that tells the compiler where to find things, so the original test files can run against our Rust code untouched. About a third of the original tests check C-internal implementation details that simply don't exist in Rust. For those, we wrote new tests that check the same behavior through the public functions instead, and we're upfront that these are new tests, not the literal originals passing.

---

## 3. Core data structure: arena/index-based tree, not raw pointers

**Decision:** The Rust port's internal tree representation uses an arena (`Arena { nodes: Vec<Node> }`), with `next`/`prev`/`child` links stored as `Option<NodeId>` indices rather than C-style raw pointers.

**Rationale:** A direct 1:1 translation of C's pointer-linked tree would require heavy `unsafe` throughout the core parsing/printing/tree-mutation logic. The arena design keeps the borrow checker satisfied naturally and keeps `unsafe` out of the core engine entirely.

**Trade-off, accepted knowingly:** This representation does not match the C `cJSON` struct's memory layout field-for-field. The ~6 adapter-eligible test files that read struct fields directly cannot link against this representation as-is without a translation layer.

**Mitigation, deferred:** A thin `#[repr(C)]` facade layer at the public API boundary — translating between a C-compatible view and the arena internally, would let those files link through the adapter while keeping the arena as the real engine underneath. This is treated as a checkpoint decision around the ~hour-24 mark, not built speculatively now. If it isn't built, adapter-based test linkage will be 0%, reported honestly rather than implied otherwise.

**In plain terms:** Instead of using memory pointers to link tree nodes together (the C way), our nodes live in one big list and point at each other using plain numbers (positions in that list). This makes our Rust code much safer with far less risky "trust me" code but it means our internal node shape doesn't look like C's anymore, which affects how much of the original test suite can literally run against us. We're planning a small translation layer to bridge that gap later, once the core engine is solid.

---

## 4. Sibling-list tail tracking: conventional invariant over C's optimization

**Decision:** The original C implementation uses an internal optimization where the *first* child's `prev` pointer doesn't point backward — it points to the *last* child, enabling O(1) tail lookup for appends. Our `append_child` does not replicate this; instead it walks the sibling list to find the tail (O(n) append), preserving the conventional invariant that every node's `prev` is its true immediate predecessor (and the first child has none).

**Rationale:** This C behavior is a pure internal implementation detail that is invisible to every public API caller and not asserted on by any in-scope test file (checked `cjson_add.c` specifically). Preserving the conventional invariant makes bidirectional traversal direct and unambiguous with `NodeId` links, at the cost of O(n) append time. Reproducing C's optimization would require breaking the "prev = true predecessor" invariant for no observable behavioral benefit.

**In plain terms:** The original C code has a clever but confusing trick: the first item in a list stores a shortcut to the tail, to make adding new items faster. We chose not to copy that trick, our "previous" pointers always mean exactly what they say. Ours is a little slower when appending to a very long list, but far easier to reason about and never causes confusion later.

---

## 5. Arena deletion: logical, not physical and matches C's dual-ownership flags exactly

**Decision:** `Arena::delete(NodeId)` cannot physically remove an entry from the `Vec` without invalidating every later `NodeId` still in use elsewhere. Deletion is therefore logical: a private, index-aligned `deleted: Vec<bool>` sidecar on `Arena` (not a field on the locked `Node` struct, the interface stays exactly as specified for the team) marks nodes as gone. Deletion is idempotent (safe to call twice).

Ownership during deletion follows the original's actual dual-flag rule precisely, not a simplified single-flag version: `is_reference` gates whether a node's `child` chain and `value_string` are cleared (a reference node's children/string belong to someone else). `key_is_const` separately gates whether the `key` is cleared. This matches `cJSON_Delete`'s real behavior of checking `IsReference` and `StringIsConst` independently.

**In plain terms:** We don't actually erase deleted nodes from memory, we just mark them as "gone," because our node list can't safely have holes punched in it. Deleting something twice is safe, it just does nothing the second time. And we correctly copied a subtle rule from the original: a node can independently "not own" its content and "not own" its name-tag, and deleting it respects both of those separately, exactly like the original C library does.

---

## 6. Delete cascade: deleting an unattached child also deletes its later siblings

**Decision:** `Arena::delete` follows the node's own `next` chain, meaning deleting an attached-but-not-detached child also logically deletes every sibling that comes after it in that same chain, this is not a bug, it's a deliberate match of `cJSON_Delete`'s real C behavior (`while (item != NULL) { ...; item = item->next; }`, which walks and frees the whole remaining chain from whatever node it's given).

**Consequence:** Callers must `detach` a child (removing it from its sibling chain first) before deleting only that one item matching the exact same usage contract the original C library has always had.

**In plain terms:** If you delete one item out of the middle of a list without first "unplugging" it from its neighbors, everything after it in that list gets deleted too.This mirrors a real, sometimes-surprising quirk of the original C library, not a mistake in our port. If you only want to remove one item, detach it first.

---

## 7. NodeId validation: two-tier policy

**Decision:** Public, externally-callable API functions (`add_item_to_array`, `add_item_to_object`, and all subsequent public entry points) validate that a `NodeId` is both in-bounds and not already deleted (`is_live_node`) before acting, returning `false`/`None` on invalid input rather than panicking. Internal, module-private accessors (`Arena::get`/`get_mut`) continue to trust their caller and panic on an invalid `NodeId`, per the original Task 1 design.

**Rationale:** This gives a clean, predictable boundary: code entirely internal to the arena's own implementation can assume well-formed IDs (a programmer error there is a real bug, and panicking surfaces it immediately during development); code reachable from outside the module treats an invalid ID as ordinary bad input to be reported through a normal return value, not a crash.

**In plain terms:** Functions meant to be called from outside our module politely say "no" (return false/nothing) if you hand them a bad ID. Functions meant only for internal use trust that whoever's calling them already did that checking, and will crash loudly if that trust is violated, which is intentional, so internal bugs get caught immediately during development instead of silently producing wrong results.

---

## 8. Known future requirement: `valueint` is referenced by an adapter-eligible test file

**Finding, not yet a design decision — logged for whoever builds the facade layer:** Our locked `Node` struct intentionally has no equivalent to C's deprecated `valueint` field (an `int`-truncated view of `value_double`). A grep of `tests/original/` confirms 6 direct references across `cjson_add.c`, `parse_number.c`, and `misc_tests.c`. Critically, `cjson_add.c` is one of the ~6 adapter-eligible files from §2. This means the eventual `#[repr(C)]` facade, if built, must synthesize a `valueint`-equivalent field (or accessor) at the C-ABI boundary to keep `cjson_add.c` linkable, even though the arena's internal `Node` never carries one.

**In plain terms:** One of the original test files we're hoping to run unmodified checks an old, deprecated integer field that our clean internal design deliberately doesn't have. This isn't a problem yet but whoever builds the translation layer later needs to remember to add this field back in, just for outside compatibility, without polluting our actual internal node type.

---

## 9. Docker base image: `rust:slim-bookworm` builder, `debian:bookworm-slim` runtime

**Decision:** The `Dockerfile` uses `rust:slim-bookworm` as the builder stage and `debian:bookworm-slim` as the runtime stage. The Rust channel is **not** pinned in the Dockerfile — `rJSON/rust-toolchain.toml` is the single source of truth for the channel, read automatically by the `rustup` already present in the base image. `build-essential` and `cmake` are installed in the builder stage proactively so future bench and fuzz C-compilation stages can reuse the cached layer without a rebuild.

**Rationale:** `rust:slim-bookworm` is the official Rust Docker image on a stable Debian Bookworm slim base; it ships rustup and resolves the `rust-toolchain.toml` channel pin with no extra scaffolding. Pinning the channel in both the Dockerfile and `rust-toolchain.toml` would create a drift risk — a later channel bump in `rust-toolchain.toml` would silently be ignored by Docker unless the Dockerfile was also updated. Letting `rust-toolchain.toml` own the pin eliminates that class of inconsistency entirely.

**In plain terms:** The Docker build uses the official Rust image on Debian. The Rust version comes from the `rust-toolchain.toml` file that already exists in the repo — we don't duplicate it in the Dockerfile, so there's no chance the two get out of sync later. We also pre-install the C compiler tools in the build stage now, so future Docker work (benchmarks, fuzz) doesn't have to wait for those to download from scratch.

## 10. Improvement in recursive safety
> `print_array_at` and `print_object_at` enforce `CJSON_NESTING_LIMIT` (1000). This is an intentional safety improvement, not strict parity: original cJSON enforces the limit during parsing but does not guard its recursive printer. The guard prevents stack overflow for programmatically constructed trees deeper than the parser limit.

---

## 11. Facade layer: materialise-and-free design; `cJSON_Delete` owns C-heap, not the arena

**Decision:** The `#[repr(C)]` facade layer (`rJSON/src/facade.rs`) uses the "materialise-on-return" approach: each `cJSON_Parse*` call creates a short-lived Rust `Arena`, parses into it via the existing Rust engine, then walks the arena tree to allocate a mirror image as C-heap `cJSON` structs with real `next`/`prev`/`child` pointer links, `valueint` synthesised from `value_double` via `clamped_int_value()`, and `valuestring`/`string` as `malloc`-allocated C strings. The arena is dropped before returning. `cJSON_Delete` on the C side walks and frees the C-heap structs; Rust is not involved. Functions that receive a `*const cJSON` back (e.g. `cJSON_Print`, `cJSON_Compare`) walk the C-struct tree and rebuild a temporary arena internally.

**On `cJSON_InitHooks`:** Intentional no-op. Our allocator is Rust's arena (internal) + `libc::malloc` (materialisation). The 13 `*_on_allocation_failure` tests in `cjson_add.c` **fail** when genuinely tested against the Rust DLL — this is the correct, expected outcome per the original plan. See "Real test results" below.

**Rationale:** This keeps `unsafe` code tightly bounded at the `extern "C"` boundary. There is no global mutable state beyond the single required `static mut GLOBAL_ERROR_PTR` for `cJSON_GetErrorPtr`. No new crate dependencies beyond `libc` (a thin FFI shim). `cJSON_Delete` is a straightforward `free`-walk with zero risk of double-free from Rust's side.

**Adapter — architecture correction (2026-08-02):** The initial adapter design used `-I rJSON/tests/adapter` to shadow the original `common.h`. This did not work: `#include "common.h"` in C uses quoted-include semantics, which searches the *source file's own directory first*, bypassing `-I` paths entirely. All six test files reside in `rJSON/tests/original/`, so they found `original/common.h` first, which `#include`s `../cJSON.c` (the C implementation). Additionally, the available GCC is MinGW 32-bit while our Rust DLL is MSVC x64; the 32-bit test binaries crashed on DLL load (`0xC000007B`). As a result the first run's 72/72 result was testing the original C implementation, not the Rust facade.

**Fix:** Test files are copied verbatim (no edits) to `rJSON/tests/adapter/`, making their "own directory" the adapter directory. Compiled with MSVC `cl.exe` x64 (matching DLL target). Import confirmed via `dumpbin /IMPORTS`.

**Hook interception proof (`hook_trace_msvc.exe`):** Direct evidence:
- Q1: `cJSON_CreateIntArray` without hooks → NON-NULL (allocates correctly)
- Q2: `cJSON_InitHooks(&failing_hooks)` → no-op (nothing printed)
- Q3: `malloc(8)` in the *test binary's* process → NON-NULL (hook never affects it)
- Q4: `cJSON_CreateIntArray` with failing hooks installed → NON-NULL (hook NOT intercepted — our `libc::malloc` in the DLL bypasses the hook entirely)
- Conclusion: hook does not intercept DLL-internal malloc; `*_on_allocation_failure` tests correctly FAIL.

**Real test results (MSVC x64, genuinely linked to rjson.dll):**
- `minify_tests.c`: 7 Tests 0 Failures 0 Ignored ✓
- `readme_examples.c`: 3 Tests 0 Failures 0 Ignored ✓
- `parse_examples.c`: 15 Tests 0 Failures 0 Ignored ✓
- `parse_with_opts.c`: 6 Tests 0 Failures 0 Ignored ✓
- `compare_tests.c`: 10 Tests 0 Failures 0 Ignored ✓
- `cjson_add.c`: 18 Tests 13 Failures 0 Ignored — 13 allocation-failure tests fail as designed
- **Honest total: 59 Tests passing, 13 failing (all 13 are `*_on_allocation_failure` tests)**

**In plain terms:** When we hand a JSON tree to C, we convert it entirely into the C format — real pointer links, all fields filled in. C owns that memory and frees it itself via `cJSON_Delete`. Rust only gets involved again if C hands the pointer back to Print or Compare. `cJSON_InitHooks` does nothing in our port; the hook cannot reach the DLL's internal malloc. The 13 allocation-failure tests correctly report FAIL — honest per AI_GUARDRAILS §0.

---

## 12. cJSON_InitHooks: implemented for real; allocation-failure tests now pass

**Decision (supersedes §11's no-op):** `cJSON_InitHooks` now stores the caller's `malloc_fn`/`free_fn` pair in two `static mut` globals (`GLOBAL_MALLOC_HOOK`, `GLOBAL_FREE_HOOK`). All C-heap allocations in the facade (`alloc_cjson`, `bytes_to_cstring_heap`) and all C-heap frees (`cJSON_Delete` and its callees) route through `cjson_malloc`/`cjson_free` helper functions that consult the hook first, falling back to `libc` when no hook is installed. Passing `NULL` to `cJSON_InitHooks` resets both to `None`. Arena-internal allocations (the Rust parser) are unaffected and cannot be redirected — this is a structural property of the arena design and is unchanged.

**Rationale:** The original plan noted this as a known gap. After confirming via `hook_trace_msvc.exe` (§11) that the no-op cause is structural (DLL C-runtime isolation, not a fundamental impossibility), a targeted fix was identified: route only the *materialisation-layer* allocations through the hook. The arena never hands memory back to C, so hooking the materialisation layer covers all allocations that C callers observe. A single global hook slot matches original cJSON's documented non-thread-safe API contract.

**Scope of coverage:** Every allocation the C caller can reach goes through the hook: `CJson` struct nodes, `valuestring` copies, `string`/key copies, and their corresponding frees in `cJSON_Delete`. The arena's internal allocations for parsing are not hooked — identical behaviour to how the original cJSON's `global_hooks` covers only the user-visible allocation layer, not its internal `parse_buffer` stack.

**On thread safety:** None. Matches original cJSON. The globals are `static mut`; reading and writing them is `unsafe`. This is the only viable approach without a Mutex dependency, and `Mutex` would be a new crate dependency (AI_GUARDRAILS §4) that original cJSON does not require.

**Test results after fix (MSVC x64, rjson.dll genuinely loaded):**
- `cjson_add.c`: 31 Tests 0 Failures 0 Ignored ✓ (was: 18 pass / 13 fail)
- All other 5 files: unchanged, still 0 failures
- **Revised total: 72 Tests 0 Failures 0 Ignored**

---

## 13. Self-contained Docker build: Unity and fixtures vendored into adapter/

**Decision:** `rJSON/tests/adapter/` now contains everything needed for a clean `git clone` + `docker build -t rjson .` with no external dependencies:
- `unity/` — Unity v2.5 (MIT): `src/unity.c`, `src/unity.h`, `src/unity_internals.h`, `examples/unity_config.h`
- `inputs/` — cJSON's own JSON test fixtures (MIT): `test1`–`test11` and their `.expected` counterparts, `test6` (intentionally invalid JSON)

Both were previously only in the gitignored `/cJSON/` directory.

**Dockerfile strategy:**
1. `cargo build` + `cargo test` (Rust-side tests, all pass)
2. `gcc -std=c11` compiles all 6 adapter `.c` files from `tests/adapter/` against the built `librjson.so` via `-L target/debug -lrjson`
3. `ldd` verifies each binary genuinely links `librjson.so` (mirrors the Windows `dumpbin /IMPORTS` discipline)
4. All 6 run from `WORKDIR /build/rJSON/tests/adapter` under `LD_LIBRARY_PATH` so `inputs/` relative paths resolve cleanly.

**Results (Docker, gcc Linux x86-64, librjson.so genuinely linked):**
- `minify_tests`: 7 Tests 0 Failures 0 Ignored ✓
- `readme_examples`: 3 Tests 0 Failures 0 Ignored ✓
- `parse_examples`: 15 Tests 0 Failures 0 Ignored ✓
- `parse_with_opts`: 6 Tests 0 Failures 0 Ignored ✓
- `compare_tests`: 10 Tests 0 Failures 0 Ignored ✓
- `cjson_add`: 31 Tests 0 Failures 0 Ignored ✓
- **Total: 72 Tests 0 Failures 0 Ignored across all 6 original test files**

**In plain terms:** A teammate or judge who has never touched this repo can run `docker build -t rjson .` from a fresh clone and get a fully green build with the complete test suite verified against the Rust cdylib on Linux.
