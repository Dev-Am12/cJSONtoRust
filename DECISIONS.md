# 1. Scope Decision before start
Scope: Core cJSON.c/cJSON.h is the committed scope for this submission. Test Parity is measured against tests/original/ (20 files, hashed in tests-kickoff.sha256). cJSON_Utils is an explicit stretch goal. Its tests are preserved unmodified under tests/original-utils/ (hashed separately in tests-kickoff-utils.sha256) so they're ready to use if time permits, but they are not part of the core deliverable and won't be counted against us if untouched.

# 2. Core Data Structure: Arena/Index-Based Tree (not raw pointers)

Decision: The Rust port's internal tree representation uses an arena
(a single Vec<Node>, indexed by NodeId) instead of C-style raw pointers
for next/prev/child links.

Rationale: The original C struct uses raw next/prev/child pointers with
manual cJSON_Delete-based ownership. A direct 1:1 translation to Rust
would require heavy use of raw pointers and `unsafe` throughout the core
parsing, printing, and tree-mutation logic.

An arena/index design (nodes stored in a Vec, linked via NodeId indices
rather than pointers) avoids this: the borrow checker is satisfied
naturally, `unsafe` stays out of the core engine entirely, and detach/
reattach/replace operations (which the cJSON API relies on heavily) are
straightforward index rewrites instead of pointer surgery.

Trade-off, accepted knowingly: this representation does not match the C
`cJSON` struct's memory layout field-for-field. Per the hackathon FAQ
("run them against your port via a thin adapter or FFI shim"), the
original test files under tests/original/ are never edited — instead,
a separate adapter (tests/adapter/, outside tests/original/) provides
an alternate common.h that resolves via include-path ordering when
compiling the original .c files against our Rust cdylib, rather than
against the original cJSON.c. tests/original/ remains byte-identical
to its kickoff hash at all times; the adapter is new, clearly-labeled
infrastructure, not an edit to the original.

Mitigation (stretch goal, not yet committed): a thin `#[repr(C)]` facade
layer at the public API boundary, translating between the C-compatible
view and the arena internally, would let those specific test files link
unmodified while keeping the arena as the real engine underneath. This
is deliberately deferred — we're treating it as a checkpoint decision
around the ~hour-24 mark, once the core parser/printer/tree logic is
stable, rather than building it speculatively now. If it doesn't get
built, black-box test-file linkage against the original C-ABI-style
tests will be 0%, and we will report that honestly rather than implying
otherwise.

Impact: blocks/shapes Member 1 (parser) and Member 2 (data model,
tree mutation, printer) — both build against `NodeId`/`Arena` from the
start. Communicated to the team before either began implementation.