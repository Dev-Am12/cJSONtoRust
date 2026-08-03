// Regression coverage for the cargo-fuzz harness at
// `rJSON/fuzz/fuzz_targets/fuzz_parse.rs`.
//
// This file is deliberately *not* the fuzz harness itself -- cargo-fuzz
// targets aren't runnable via plain `cargo test` (they need a nightly
// toolchain + libFuzzer instrumentation), so this task's explicit
// "discoverable by Cargo's standard `cargo test`" requirement is met
// here instead: a fixed, hand-picked set of inputs from the same four
// categories the fuzz harness targets (invalid UTF-8, truncated JSON,
// deep nesting, massive numeric input), run through the exact same
// public entry points, with an explicit assertion that parsing neither
// panics nor aborts. New crash reproducers cargo-fuzz finds under
// `fuzz/artifacts/fuzz_parse/` should get a corresponding case added
// here, per `DECISIONS_personal.md`'s "Fuzzing harness" entry.
//
// Per this task's constraint, no `unwrap()`/`expect()`/`panic!()` is
// called on any *parser* `Result` in this file either -- `Err` is always
// matched explicitly, never unwrapped. `std::panic::catch_unwind` is
// used deliberately (unlike in the fuzz harness itself) specifically so
// a genuine panic becomes a normal, readable `assert!` failure under
// `cargo test` instead of aborting the whole test binary -- appropriate
// here because, unlike libFuzzer, a plain `#[test]` has no built-in
// per-case crash reporting to fall back on.

use rjson::{cjson_parse, cjson_parse_with_length_opts, cjson_parse_with_opts, Arena};
use std::panic::{self, AssertUnwindSafe};

/// Runs `f` (a closure that parses `input` through one of the public
/// entry points) inside `catch_unwind` and asserts it did not panic.
/// Returns nothing about the parse *result* -- callers that care whether
/// it was `Ok`/`Err` check that separately, outside this helper, so a
/// panic-freedom assertion is never accidentally conflated with a
/// parse-success assertion.
fn assert_no_panic<F: FnOnce()>(label: &str, f: F) {
    let result = panic::catch_unwind(AssertUnwindSafe(f));
    assert!(
        result.is_ok(),
        "parsing panicked (this is exactly the class of bug fuzzing exists to catch): {label}"
    );
}

// ---------------------------------------------------------------------
// Category 1: invalid UTF-8
// ---------------------------------------------------------------------

#[test]
fn invalid_utf8_inside_string_does_not_panic() {
    // Raw, non-UTF-8 bytes inside a JSON string. Per `parser.rs`'s
    // documented raw-passthrough behavior (DECISIONS_personal.md #8),
    // this is expected to parse *successfully*, preserving the bytes
    // verbatim -- but the panic-freedom check is what this test exists
    // for, independent of which outcome (`Ok`/`Err`) results.
    let input: &[u8] = b"\"\xff\xfe\x00\x80\xc0\xc1\"";
    assert_no_panic("invalid_utf8_inside_string", || {
        let mut arena = Arena::new();
        let _ = cjson_parse(&mut arena, input);
    });
}

#[test]
fn invalid_utf8_bare_bytes_do_not_panic() {
    // Not inside a string at all -- bytes that aren't a valid JSON
    // value's first byte and aren't valid UTF-8 either. Expected to be
    // an ordinary `Err`, not a panic.
    let input: &[u8] = &[0xff, 0xfe, 0xfd, 0xfc];
    let mut arena = Arena::new();
    let mut observed_result = None;
    assert_no_panic("invalid_utf8_bare_bytes", || {
        observed_result = Some(cjson_parse(&mut arena, input).is_ok());
    });
    assert_eq!(
        observed_result,
        Some(false),
        "bare invalid-UTF-8 bytes are not a valid JSON value and should fail to parse (gracefully)"
    );
}

#[test]
fn invalid_utf8_byte_at_every_position_does_not_panic() {
    // Sweep every possible byte value (0x00..=0xff) at every position of
    // a short template string, catching position-dependent panics a
    // single fixed fixture could miss.
    let template = b"{\"a\":\"XXXX\"}";
    for pos in 0..template.len() {
        for byte in 0u8..=255 {
            let mut input = template.to_vec();
            input[pos] = byte;
            assert_no_panic(&format!("byte {byte:#04x} at position {pos}"), || {
                let mut arena = Arena::new();
                let _ = cjson_parse(&mut arena, &input);
            });
        }
    }
}

// ---------------------------------------------------------------------
// Category 2: truncated JSON
// ---------------------------------------------------------------------

#[test]
fn truncated_inputs_fail_gracefully_not_panic() {
    let cases: &[&[u8]] = &[
        b"{\"key\": \"val",
        b"{\"a\":1,",
        b"[1,2,[3,4,",
        b"\"abc\\",
        b"{",
        b"[",
        b"\"",
        b"-",
        b"tru",
        b"nul",
        b"{\"a\"",
        b"{\"a\":",
    ];
    for case in cases {
        let mut arena = Arena::new();
        let mut is_ok = None;
        assert_no_panic(&String::from_utf8_lossy(case), || {
            is_ok = Some(cjson_parse(&mut arena, case).is_ok());
        });
        assert_eq!(
            is_ok,
            Some(false),
            "truncated input {:?} should fail to parse, not succeed",
            String::from_utf8_lossy(case)
        );
    }
}

#[test]
fn every_truncation_prefix_of_a_valid_document_does_not_panic() {
    // Every prefix of a valid, moderately-nested document -- most are
    // truncated/invalid JSON by construction; the full-length prefix is
    // the only one expected to succeed.
    let full: &[u8] = b"{\"a\":[1,2,{\"b\":\"c\\u00e9\"},null,true,false],\"d\":-1.5e10}";
    for len in 0..=full.len() {
        let prefix = &full[..len];
        assert_no_panic(&format!("prefix length {len}"), || {
            let mut arena = Arena::new();
            let _ = cjson_parse(&mut arena, prefix);
        });
    }
}

// ---------------------------------------------------------------------
// Category 3: deep nesting
// ---------------------------------------------------------------------

#[test]
fn nesting_exactly_at_limit_parses_without_panic() {
    let input: Vec<u8> = std::iter::repeat_n(b'[', 1000)
        .chain(std::iter::repeat_n(b']', 1000))
        .collect();
    assert_no_panic("nesting exactly at CJSON_NESTING_LIMIT", || {
        let mut arena = Arena::new();
        let _ = cjson_parse(&mut arena, &input);
    });
}

#[test]
fn nesting_one_past_limit_fails_gracefully_not_panic() {
    let input: Vec<u8> = std::iter::repeat_n(b'[', 1001).collect();
    let mut arena = Arena::new();
    let mut is_ok = None;
    assert_no_panic("nesting one past CJSON_NESTING_LIMIT", || {
        is_ok = Some(cjson_parse(&mut arena, &input).is_ok());
    });
    assert_eq!(
        is_ok,
        Some(false),
        "one array past the nesting limit should be rejected, not accepted or a panic"
    );
}

#[test]
fn extreme_nesting_far_past_limit_fails_gracefully_not_panic() {
    // Two orders of magnitude past the limit -- guards against a
    // recursion-depth or stack-overflow-shaped panic that only a much
    // deeper input would trigger, since the nesting check itself bails
    // out at 1000 long before this depth is reached.
    let input: Vec<u8> = std::iter::repeat_n(b'[', 100_000).collect();
    assert_no_panic("100,000 unclosed '[' characters", || {
        let mut arena = Arena::new();
        let _ = cjson_parse(&mut arena, &input);
    });
}

#[test]
fn deeply_nested_object_over_limit_fails_gracefully_not_panic() {
    let mut input = Vec::new();
    for _ in 0..1200 {
        input.extend_from_slice(b"{\"a\":");
    }
    input.push(b'1');
    input.extend(std::iter::repeat_n(b'}', 1200));
    assert_no_panic("1200-deep nested object", || {
        let mut arena = Arena::new();
        let _ = cjson_parse(&mut arena, &input);
    });
}

#[test]
fn deleting_a_deeply_nested_parsed_tree_does_not_panic() {
    // Exercises `Arena::delete`'s recursive child-chain walk
    // (DECISIONS.md #6) on the deepest tree the parser will actually
    // accept (nesting limit - 1), since deletion recursion depth is a
    // distinct risk from parse recursion depth.
    let input: Vec<u8> = std::iter::repeat_n(b'[', 999)
        .chain(std::iter::repeat_n(b']', 999))
        .collect();
    assert_no_panic("deleting a 999-deep parsed array", || {
        let mut arena = Arena::new();
        if let Ok(root) = cjson_parse(&mut arena, &input) {
            arena.delete(root);
        }
    });
}

// ---------------------------------------------------------------------
// Category 4: massive numeric input
// ---------------------------------------------------------------------

#[test]
fn massive_digit_run_does_not_panic() {
    let mut input = vec![b'1'];
    input.extend(std::iter::repeat_n(b'2', 5000));
    assert_no_panic("5001-digit integer literal", || {
        let mut arena = Arena::new();
        let _ = cjson_parse(&mut arena, &input);
    });
}

#[test]
fn massive_exponent_does_not_panic() {
    let mut input = b"1e".to_vec();
    input.extend(std::iter::repeat_n(b'9', 3000));
    assert_no_panic("1e followed by 3000 nines", || {
        let mut arena = Arena::new();
        let _ = cjson_parse(&mut arena, &input);
    });
}

#[test]
fn massive_negative_decimal_does_not_panic() {
    let mut input = vec![b'-'];
    input.extend(std::iter::repeat_n(b'9', 4000));
    input.push(b'.');
    input.extend(std::iter::repeat_n(b'1', 4000));
    assert_no_panic("massive negative decimal", || {
        let mut arena = Arena::new();
        let _ = cjson_parse(&mut arena, &input);
    });
}

#[test]
fn overflowing_number_produces_infinity_not_panic_and_is_still_a_number_node() {
    // Per DECISIONS_personal.md #4: `1e400` overflows `f64` to
    // `INFINITY` at parse time (not rejected) -- this is expected
    // upstream-matching behavior, not a bug, but it's exactly the kind
    // of edge case a naive re-implementation could turn into a panic
    // (e.g. an `assert!(value.is_finite())` slipping into parse-time
    // code), so it's pinned here explicitly.
    let input: &[u8] = b"1e400";
    let mut arena = Arena::new();
    let mut observed = None;
    assert_no_panic("1e400 overflow", || {
        if let Ok(id) = cjson_parse(&mut arena, input) {
            observed = Some(arena.get(id).value_double);
        }
    });
    assert_eq!(
        observed,
        Some(f64::INFINITY),
        "1e400 should overflow to +inf at parse time, matching upstream's HUGE_VAL acceptance"
    );
}

#[test]
fn malformed_number_like_strings_do_not_panic() {
    let cases: &[&[u8]] = &[
        b"1.2.3",
        b"--1",
        b"1e",
        b"1e+",
        b"1e-",
        b".5",
        b"5.",
        b"-",
        b"1.",
        b"+1",
        b"00001",
        b"1eeee5",
        b"1.2.3.4.5.6.7.8.9",
    ];
    for case in cases {
        assert_no_panic(&String::from_utf8_lossy(case), || {
            let mut arena = Arena::new();
            let _ = cjson_parse(&mut arena, case);
        });
    }
}

// ---------------------------------------------------------------------
// Cross-entry-point coverage (mirrors the fuzz harness's `mode` byte)
// ---------------------------------------------------------------------

#[test]
fn all_three_entry_points_handle_every_category_without_panic() {
    let inputs: &[&[u8]] = &[
        b"\"\xff\xfe\"",
        b"{\"a\":1,",
        b"1e400",
        b"",
        b"\x00",
        b"{\"a\":1}\x00{\"b\":2}",
        b"1 trailing garbage",
    ];
    for input in inputs {
        assert_no_panic(&format!("cjson_parse on {input:?}"), || {
            let mut arena = Arena::new();
            let _ = cjson_parse(&mut arena, input);
        });
        assert_no_panic(&format!("cjson_parse_with_opts on {input:?}"), || {
            let mut arena = Arena::new();
            let _ = cjson_parse_with_opts(&mut arena, input, false);
        });
        assert_no_panic(
            &format!("cjson_parse_with_length_opts(false) on {input:?}"),
            || {
                let mut arena = Arena::new();
                let _ = cjson_parse_with_length_opts(&mut arena, input, false);
            },
        );
        assert_no_panic(
            &format!("cjson_parse_with_length_opts(true) on {input:?}"),
            || {
                let mut arena = Arena::new();
                let _ = cjson_parse_with_length_opts(&mut arena, input, true);
            },
        );
    }
}

#[test]
fn embedded_nul_truncation_divergence_does_not_panic_either_side() {
    // `cjson_parse_with_opts` truncates at the first NUL byte;
    // `cjson_parse_with_length_opts` sees the whole slice
    // (DECISIONS_personal.md #10). Both are expected to succeed here,
    // on different amounts of input -- neither should panic.
    let input: &[u8] = b"{\"a\":1}\x00{\"b\":2}";

    let mut arena_a = Arena::new();
    let mut truncated_end = None;
    assert_no_panic("with_opts embedded NUL", || {
        if let Ok((_root, end)) = cjson_parse_with_opts(&mut arena_a, input, false) {
            truncated_end = Some(end);
        }
    });

    let mut arena_b = Arena::new();
    assert_no_panic("with_length_opts embedded NUL", || {
        let _ = cjson_parse_with_length_opts(&mut arena_b, input, false);
    });

    assert_eq!(
        truncated_end,
        Some(7),
        "with_opts should stop parsing at `{{\"a\":1}}`, before the embedded NUL byte"
    );
}
