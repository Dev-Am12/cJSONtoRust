# Standard-Library Mapping — rJSON

This document records how the cJSON v1.7.19 implementation's common C
library and memory-management operations map to the Rust port. It describes
the implementation as shipped; it is not a claim that every C operation has a
one-to-one Rust spelling.

| cJSON / C concern | rJSON mapping | Location and rationale |
|---|---|---|
| Heap-owned internal nodes | `Vec<Node>` inside `Arena` | `rJSON/src/arena.rs`. Nodes are addressed by stable `NodeId` indices, keeping raw pointers and `unsafe` out of the core tree engine. |
| C heap for C-ABI objects | Direct `extern "C"` calls to the platform C runtime's `malloc` and `free` | `rJSON/src/facade.rs`. The facade must return real C-compatible allocations to C callers. These declarations use `core::ffi::c_void`; no Rust crate provides the bindings. |
| Custom cJSON allocation hooks | Stored C function pointers and allocation routing | `rJSON/src/facade.rs`. This preserves cJSON's process-global hook model at the FFI boundary. |
| `strlen` / NUL-terminated input | `CStr::from_ptr(...).to_bytes()` | `rJSON/src/facade.rs`. C-string entry points stop at the first NUL, while explicit-length parser APIs use byte slices. |
| Explicit byte buffers and `memcpy` | `&[u8]`, `Vec<u8>`, `extend_from_slice`, and `copy_nonoverlapping` where an FFI copy is required | Core strings and object keys remain raw bytes to preserve cJSON's invalid-UTF-8 passthrough behavior. |
| `strtod`-style numeric conversion | ASCII numeric-prefix scan followed by `str::parse::<f64>()` | `rJSON/src/parser.rs`. The parser retains cJSON-compatible accepted forms and stores overflow as infinity; the printer renders non-finite values as `null`. |
| Integer `valueint` clamping | `clamped_int_value(f64) -> i32` | `rJSON/src/parser.rs`. The arena does not duplicate cJSON's deprecated field; the C facade synthesizes it when materialising a C node. |
| `strcmp` / case-insensitive object lookup | Byte-slice equality and ASCII case folding | `rJSON/src/arena.rs`. Keys are never converted to UTF-8 `String`s. |
| `printf`-style number formatting | Rust formatting plus cJSON-compatible precision and exponent normalization | `rJSON/src/arena.rs`. The documented target is Linux/glibc's two-digit exponent convention. |
| Recursion limits | Explicit depth counters | `rJSON/src/parser.rs` and `rJSON/src/arena.rs`. Parsing uses `CJSON_NESTING_LIMIT`; duplication uses `CJSON_CIRCULAR_LIMIT`; printing also applies the documented safety guard. |
| Monotonic timing for benchmarks | `std::time::Instant` in Rust tooling and `clock_gettime` in the C benchmark harness | `rJSON/src/bin/raw_timing.rs` and `bench/`. The cross-language benchmark methodology is documented separately in `bench/methodology.md`. |

The core parser and arena use Rust's standard library only. The C-ABI facade
necessarily calls the platform C runtime for allocations so that C callers can
own and release returned objects with cJSON-compatible semantics.
