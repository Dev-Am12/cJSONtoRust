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

# 3. 
Arena::get and get_mut return references and use direct index access, so invalid NodeId values panic consistently with Vec indexing. This keeps the locked API minimal and exposes invalid internal IDs during development rather than silently converting a structural error into an absent node. Added Default as a Clippy-compatible equivalent of Arena::new(); it does not alter the locked representation or behavior.

# 4. Tradeoff for cJSON 0(1) "find the tail to append" optimisation
The arena append primitive walks the null-terminated sibling list to find its tail, rather than reproducing cJSON’s optimization of storing the tail in the first child’s prev field. This preserves the conventional invariant that each node’s prev is its immediate preceding sibling (and the first child has no predecessor), making bidirectional traversal direct and unambiguous with NodeId links. The trade-off is O(n) append time; adding a tail field would change the locked Node/Arena representation.

# 5. Arena deletion
Arena deletion uses a private Vec<bool> sidecar, index-aligned with nodes, to record logical deletion without adding a field to the locked Node structure or physically removing entries from the arena. This keeps all existing NodeId values stable while making deletion idempotent and externally inspectable through is_deleted. delete follows cJSON ownership rules: it recursively traverses owned child/sibling chains, clears owned string data and non-const keys, and never traverses or clears non-owned data beneath reference nodes. The trade-off is one status bit per allocated node and retained arena capacity until the entire Arena is dropped.
Calling delete on a node also deletes every sibling after it in the chain (via delete_chain following .next), matching the original's while (item != NULL) loop in cJSON_Delete. This means deleting a child directly (without detaching it first) also wipes its later siblings, same sharp edge the original C API has, not a bug.