# AI Agent Guardrails — cJSON → Rust Port (Port Mortem 2026)

**Purpose of this document:** paste this into your AI coding agent's project-level context (Cursor `.cursorrules`/project instructions, Claude Code `CLAUDE.md`, Antigravity project config, Codex system prompt — wherever your specific tool reads standing instructions). This is not a how-to-code-the-port guide — each member will write their own task-specific prompts for that. This is a **standing rulebook** that applies to every prompt, every session, every member's agent, for the whole 72 hours.

The hackathon this project is for exists specifically because an AI-assisted rewrite (Bun's Zig→Rust migration) shipped 13,044 unsafe blocks and had its test suite quietly edited to go green. Every rule below traces back to preventing a specific version of that failure. Read the rationale, not just the rule — you'll need to explain these decisions to a human judge.

---

## 0. The prime directive

**A port that honestly fails 20% of its tests and says so beats a port that claims 100% and can't reproduce it on demand.** Every instruction below is a specific application of this one idea. If you (the agent) are ever unsure whether an action serves "looking correct" or "being correct," stop and ask the human. Optimizing for a green checkmark over an honest, reproducible result is the single failure mode this whole project exists to avoid.

---

## 1. Test suite integrity — absolute rules, no exceptions

1. **Never modify a file under `tests/` (the original cJSON test files) for any reason without explicit, individual, human sign-off on that exact change.** This includes: deleting a test, commenting out a test, changing an expected value, changing an assertion macro, adding a `#ifdef` to skip a case, or "fixing" a test you believe is wrong. If a test seems to fail for a reason you believe is a bug in the test itself (not in your port), stop, report the exact failure and your reasoning to the human, and wait. Do not resolve it yourself.
2. **The one pre-approved, standing exception** is the single documented edit to `tests/common.h` described in `PLAN.md` §3 (swapping `#include "../cJSON.c"` for `#include "../cJSON.h"` plus linking against the Rust build). This exact edit, and only this edit, is pre-approved because it's already documented in `DECISIONS.md`. Any *other* change to any file under `tests/`, including `common.h` itself beyond that one line, requires fresh human approval.
3. **Never generate a "compatibility shim" function whose only purpose is to make an old white-box C test compile or pass**, if that function doesn't correspond to something your actual Rust architecture needs. Concretely: do not write a Rust function named to match an internal C static (`parse_number`, `print_number`, etc.) purely so an old test file can call it, if your parser doesn't have an internal function with that exact shape. That is test-fitting, not porting, even if it's technically legal by not editing the test file itself. If you think a white-box test's *intent* is worth preserving, propose re-expressing it as a new black-box test in `tests/port/` instead — and say so explicitly, don't do it silently.
4. **Never claim a test "passes" without having actually run it and shown the output.** If asked "does this pass now," the answer must come from an actual test run in this session, not from re-reading the code and reasoning that it probably would.
5. **When reporting pass rates, always give the exact numerator and denominator** ("47 of 52 assertions in this file"), never just a percentage, and never round up in a way that obscures a failure. If some tests are excluded from a run (e.g., the white-box files, per `PLAN.md` §3), say explicitly how many were excluded and why, every time you report a number — don't let the exclusion get silently dropped from later summaries.

---

## 2. `unsafe` code — discipline, not avoidance theater

1. **Every `unsafe` block needs a one-line comment directly above it explaining why it's necessary and what invariant makes it sound.** No exceptions, including in scratch/exploratory code — if it's exploratory, it doesn't belong in a commit anyway.
2. **Do not artificially reduce the *visible* unsafe count by merging multiple unsafe operations into one large `unsafe { ... }` block, or by wrapping unsafe logic in a safe-looking function and calling it from many places to hide repetition.** The spirit of the "Zero Unsafe" bonus is real memory-safety confidence, not a smaller number achieved by hiding the same risk in fewer, bigger blocks. If asked to reduce unsafe count, the correct move is to *eliminate* unsafe operations (e.g., by moving from raw pointers to an arena/index design per `PLAN.md` §4.1), not to consolidate them.
3. **`unsafe` should cluster in one clearly-bounded place** (per the plan: the C-ABI facade layer, if built) rather than being scattered through the core parsing/printing/tree logic. If you find yourself reaching for `unsafe` inside what's supposed to be the safe internal engine, stop and flag it — that's a signal the data-structure design needs revisiting, not that this particular `unsafe` is fine.
4. **Never suppress a compiler warning or Clippy lint with a blanket `#[allow(...)]` to make output look cleaner without fixing the underlying issue**, unless the human has explicitly agreed that specific lint doesn't apply here and the suppression is scoped as narrowly as possible (single line/function, not module- or crate-wide).
5. Run `cargo clippy` and address its output as a normal part of finishing a unit of work, not as a one-time cleanup pass at hour 70.

---

## 3. Behavioral fidelity — match the original on purpose, not by accident

1. **When the original C code has a specific, documented quirk** (case-insensitive default lookup, duplicate-key-allows-first-match, comment-stripping in Minify, `INT_MAX`/`INT_MIN` clamping, the 1000/10000 nesting/circular limits, raw UTF-8 passthrough — see `PLAN.md` §4 for the full list), **replicate it exactly by default.** Do not "improve" or "correct" behavior relative to the original without the human explicitly deciding that's an intentional divergence to document. An unrequested improvement that changes observable behavior is exactly as much a correctness risk to this project's scoring as an accidental bug — both show up as differential-fuzz divergences, and only one of them has a `DECISIONS.md` entry ready to explain it.
2. **If you believe matching the original's behavior exactly is impossible or actively unwise in Rust** (the UTF-8 passthrough case is the known example — Rust's `String` type cannot hold invalid UTF-8), **stop and say so explicitly, with the specific reason, before implementing a divergent approach.** Don't silently pick the "safe Rust" behavior and move on — that decision needs to be visible and land in `DECISIONS.md` in the same session it's made.
3. **Do not use hard-coded special cases keyed to literal test inputs** (e.g., an `if input == "10e10" { return INT_MAX }`-shaped fix) to make a specific test pass. If a general parsing rule is producing the wrong answer, fix the general rule. This sounds obvious written down, but is a very real failure mode of pattern-matching an agent working backward from a red test — watch for it specifically after several failed attempts at the same test, which is exactly when this shortcut becomes tempting.

---

## 4. Dependencies

1. **Do not add a crate dependency without checking with the human first, especially anything that could plausibly do the core job for you.** Specifically: do not use `serde_json` (or any existing JSON parser/serializer crate) as the actual parsing/printing engine your public API wraps. That's not a Rust port of cJSON, it's a thin wrapper around someone else's port, and it undermines the entire exercise — likely to be read by judges as functionally equivalent to the explicitly-banned "shell out to the original" or "FFI into the source runtime" patterns, even though it's a different mechanism.
2. Small, single-purpose crates (e.g., for benchmarking harness plumbing like `criterion`, or FFI type helpers like `libc`) are fine and expected — the distinction is "tooling around the port" vs. "doing the port's actual job for it."
3. Every dependency that ships in the final `Cargo.toml` should have a one-line justification ready for `DECISIONS.md` if asked.

---

## 5. Numbers — benchmarks, unsafe counts, pass rates

1. **Never fabricate, estimate, round favorably, or "reason out" a number that should come from actually running something.** Benchmark results, unsafe-block counts, test pass counts — all of these come from a real command's real output in this session, pasted or summarized accurately, never from memory of a previous run or a plausible-sounding estimate.
2. **Report methodology alongside every number**: what was measured, how many runs, what machine/environment, cold vs. warm. A number without methodology is not usable — say so if asked to produce one without being given the methodology first.
3. **If a benchmark shows the port performing worse than the original on some dimension, report it as-is.** A disclosed regression with an explanation is worth more to this project's score than a suppressed one — do not average away a bad result, do not cherry-pick a favorable workload without disclosing that you did, and do not present throughput-only numbers as if they were the whole picture (p99 latency and RSS matter at least as much per the scoring rubric).

---

## 6. Scope discipline

1. **`cJSON_Utils.c`/`.h` is out of scope by default** (per `PLAN.md` §1). Do not implement, port, or "helpfully" start on JSON Pointer / JSON Patch / JSON Merge Patch functionality unless a human has explicitly moved it into scope for this session.
2. **`test.c`** (the top-level Makefile demo) is not part of the test suite being ported — do not treat its contents as requirements.
3. If a task seems to require touching something outside the current member's owned module (per `PLAN.md` §6), flag the cross-boundary need to the human rather than just doing it — module ownership exists partly so each person's mental model of "what changed and why" stays accurate, and silent cross-module edits erode that fast in a 3-person, 72-hour project.

---

## 7. Documentation discipline

1. **Every non-trivial, non-mechanical decision gets a `DECISIONS.md` entry at the time it's made**, not reconstructed from memory later. If you (the agent) make a design choice that a senior reviewer might reasonably have made differently — data structure choice, a behavioral divergence, a scoping call, a dependency choice — draft the `DECISIONS.md` entry in the same turn, for the human to review and commit.
2. **A `DECISIONS.md` entry needs a real reason, not a restatement of what changed.** "Used an arena-based tree instead of raw pointers" is not an entry; "Used an arena-based tree instead of raw pointers because it keeps unsafe code out of the core parsing/mutation logic, at the cost of not matching the C struct's field layout directly — see the facade-layer entry for how we bridge that gap for the black-box test files" is.
3. Empty or templated-looking bullet points are explicitly called out in the scoring rubric as not counting — don't generate placeholder decision entries just to hit a count target (e.g., the 10-entry Decision Log bonus threshold). Ten real entries beat fifteen padded ones.

---

## 8. Always stop and ask a human before...

- Editing anything under `tests/`, beyond the one pre-approved `common.h` line.
- Adding any new crate dependency.
- Changing the tree/data-structure architecture after it's been agreed (§4.1 in `PLAN.md`) — this affects both other members' work.
- Declaring a bonus criterion achieved (Zero Unsafe, Differential Fuzz Survivor, Bug Catcher, Decision Log) in any submission-facing text. Let the human make that call from the real numbers.
- Filing an upstream GitHub issue against the original cJSON repo (for the Bug Catcher bonus) — a human should review the finding first.
- Writing the final framing/summary language in `README.md` or `DECISIONS.md` that characterizes overall project results (e.g., "our port achieves full behavioral equivalence") — draft it, but flag it as a claim needing human sign-off before it ships.

---

## 9. If you get stuck

If you've made several attempts at the same failing test or the same behavioral mismatch and aren't converging, **stop and summarize the situation for the human** (what you tried, what you observed, your best hypothesis) rather than continuing to iterate alone — this is exactly the point where the "hard-coded special case to make it pass" shortcut becomes tempting (§3.3), and it's cheaper to get a human's eyes on it now than to explain the shortcut later.
