# Dependency Proof and Verification

## Current dependency policy

The shipping `rJSON` library has an empty normal dependency section in
`rJSON/Cargo.toml`. The C-ABI facade uses direct declarations of the platform
C runtime's `malloc` and `free` with `core::ffi::c_void`; it does not depend on
the `libc` crate.

Two tooling dependencies remain deliberately isolated from the shipping
library surface:

- `criterion = "0.5"` is a root **dev-dependency** used only by
  `rJSON/benches/raw_bench.rs`.
- `libfuzzer-sys` belongs to the separate `rJSON/fuzz/` workspace, which is
  excluded from ordinary root builds and requires its own nightly toolchain.

This file therefore proves an empty **normal** dependency graph. It does not
claim that `cargo tree -e all` is empty. If the submission rules require no
development or isolated fuzz-tool dependencies either, that policy decision
must be made before presenting the repository as fully dependency-free.

## Reproducible commands

Run from `rJSON/`:

```text
cargo tree -e normal
cargo tree -e dev --depth 1
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Expected normal dependency graph:

```text
rjson v0.1.0
```

The second command intentionally reports Criterion as benchmark-only tooling.

## Frozen upstream-test integrity

From `rJSON/`, verify the pinned original test inputs with:

```text
sha256sum -c tests-kickoff.sha256
sha256sum -c tests-kickoff-utils.sha256
```

On PowerShell, `Get-FileHash` can be used to check the same SHA-256 values in
the manifest files. The frozen `tests/original/` and `tests/original-utils/`
directories are never edited by the port.
