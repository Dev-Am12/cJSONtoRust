//! cargo-fuzz harness for rJSON's public parser surface.
//!
//! See `rJSON/DECISIONS_personal.md` ("Fuzzing harness" entry) for the
//! full strategy writeup. Summary of the constraints this file follows,
//! per this task's explicit requirements:
//!
//! - Only the public parser API is exercised (`cjson_parse`,
//!   `cjson_parse_with_opts`, `cjson_parse_with_length_opts`) -- no
//!   internal/private parser methods are reached from here.
//! - `Err(_)` from any of the three functions is an ordinary, expected
//!   outcome for malformed input (invalid UTF-8, truncated JSON, a
//!   nesting-limit rejection, ...), never treated as a failure condition
//!   by this harness. Accordingly, **no `unwrap()`, `expect()`, or
//!   `panic!()` is ever called on a parser `Result` here** -- every
//!   `Result` is consumed with `if let Ok(..)`/`match`, and the `Err`
//!   arm is always either absent (silently ignored) or a plain no-op.
//! - This harness does **not** wrap parsing in `std::panic::catch_unwind`
//!   to suppress panics. Swallowing a genuine panic would defeat the
//!   entire point of fuzzing this crate -- an *unexpected* panic (a
//!   `unwrap()`/`expect()`/index-out-of-bounds/integer-overflow inside
//!   `rjson` itself) is exactly the class of bug this harness exists to
//!   surface, and libFuzzer's own runtime already detects and reports it
//!   (as a crash, with a minimized reproducer written to
//!   `fuzz/artifacts/fuzz_parse/`) without any extra handling here.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rjson::{cjson_parse, cjson_parse_with_length_opts, cjson_parse_with_opts, Arena};

fuzz_target!(|data: &[u8]| {
    // The first byte selects which of the three public entry points (and
    // which `require_null_terminated` setting) this run exercises; the
    // rest of `data` is the JSON payload itself. This lets one corpus of
    // raw bytes drive every public parsing path -- including the
    // embedded-NUL-truncation difference between `cjson_parse_with_opts`
    // and `cjson_parse_with_length_opts`, and the trailing-content check
    // gated by `require_null_terminated` -- rather than only ever
    // reaching the outermost `cjson_parse` wrapper.
    let Some((&mode, payload)) = data.split_first() else {
        // Empty input: exercise the same empty-slice short-circuit
        // `cjson_parse_with_length_opts` takes for real callers.
        let mut arena = Arena::new();
        let _ = cjson_parse(&mut arena, data);
        return;
    };

    let mut arena = Arena::new();
    match mode % 4 {
        0 => {
            // Equivalent to `cJSON_Parse`. Covers requirement 1/2/3: raw
            // arbitrary bytes (including invalid UTF-8 and truncated
            // JSON) go straight into the parser exactly as an external
            // caller would pass them.
            if let Ok(root) = cjson_parse(&mut arena, payload) {
                arena.delete(root);
            }
        }
        1 => {
            // Equivalent to `cJSON_ParseWithOpts(value, NULL, false)`:
            // truncates at the first embedded NUL byte before parsing.
            if let Ok((root, _parse_end)) = cjson_parse_with_opts(&mut arena, payload, false) {
                arena.delete(root);
            }
        }
        2 => {
            // Equivalent to `cJSON_ParseWithLengthOpts(value, len, NULL,
            // false)`: sees the whole payload, including any embedded
            // NUL bytes `mode == 1` above would have truncated at.
            if let Ok((root, _parse_end)) =
                cjson_parse_with_length_opts(&mut arena, payload, false)
            {
                arena.delete(root);
            }
        }
        _ => {
            // `require_null_terminated = true`: exercises the
            // trailing-garbage-after-a-valid-value rejection path on the
            // same bytes the other three branches would otherwise parse
            // successfully (e.g. `"1 garbage"`).
            if let Ok((root, _parse_end)) =
                cjson_parse_with_length_opts(&mut arena, payload, true)
            {
                arena.delete(root);
            }
        }
    }
});
