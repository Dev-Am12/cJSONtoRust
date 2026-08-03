# Submission Verification Checklist

Run each command from the repository root unless a command says otherwise.
Record the complete terminal output with the submitted revision.

## 1. Frozen upstream sources

From `rJSON/`:

```text
sha256sum -c tests-kickoff.sha256
sha256sum -c tests-kickoff-utils.sha256
```

Expected: every listed file reports `OK`. The core manifest contains 20
entries; the out-of-scope utilities manifest contains 3 entries.

## 2. Native Rust quality gates

From `rJSON/`:

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo tree -e normal
cargo tree -e dev --depth 1
```

Expected: formatting and clippy succeed, all native tests pass, the normal
dependency tree contains only `rjson`, and development-only dependencies are
reported separately.

## 3. C-ABI adapter evidence

From the repository root:

```text
docker build -t rjson .
```

The Dockerfile compiles and runs the six adapter-eligible, frozen original
test files against `librjson.so`: `minify_tests`, `readme_examples`,
`parse_examples`, `parse_with_opts`, `compare_tests`, and `cjson_add`.

Do not describe this as execution of all original test files: the remaining
12 are white-box tests of C `static` helpers and are represented by separate
black-box Rust tests. See `DECISIONS.md` #2 and #22.

## 4. Differential fuzzing and benchmarks

The Dockerfile exposes dedicated stages:

```text
docker build --target fuzzer -t rjson-fuzzer .
docker build --target benchmark -t rjson-benchmark .
```

The fuzz stage must complete its continuous 65-second run with zero genuine
divergences and zero active exclusion filters. Preserve the final summary in
the submission record rather than quoting an old log without rerunning it.

The benchmark stage refreshes the documented three-way comparison between
original C, the raw Rust engine, and the C-ABI facade. Report the benchmark
environment and raw results together with any performance claim.

## 5. Documentation checks before submission

- Confirm `STDLIB.md`, `DEPENDENCY_PROOF.md`, and this checklist are present.
- Confirm README statements use the current test count and accurately scope
  the C-ABI surface that has actually been implemented and tested.
- Add the human-recorded demo-video link before submission.
- Do not claim a bonus criterion without the corresponding fresh evidence.
