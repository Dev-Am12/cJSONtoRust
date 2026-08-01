// Black-box re-expression of `tests/original/parse_hex4.c`'s intent
// against the public `Parser`/`Arena` API, per `AI_GUARDRAILS.md` §1.3:
// `parse_hex4` is a purely internal helper (no public cJSON API calls it
// directly -- it exists only to serve `\uXXXX` escape decoding inside
// `parse_string`), so this port does *not* create a same-named/same-shape
// function purely to let the original white-box test call it. Its
// behavioral intent -- correct, case-insensitive 4-hex-digit decoding for
// every possible value -- is instead re-expressed by parsing a `\uXXXX`
// JSON string literal through the public parser and checking the decoded
// UTF-8 output, for every codepoint from `0x0000` to `0xFFFF`.
//
// Two of upstream's failure-reporting details are *not* observable this
// way and are intentionally not asserted on here (see `DECISIONS_personal.md`):
// `parse_hex4`'s own out-of-range/invalid-digit behavior is already
// re-expressed separately in `parse_string_tests.rs`
// (`invalid_hex_digit_collapses_to_u0000_matching_upstream_quirk`, per
// `DECISIONS_personal.md` #8). What *is* new here is coverage matching
// `parse_hex4_should_parse_all_combinations`' full `0..=0xFFFF` sweep and
// `parse_hex4_should_parse_mixed_case`'s case-insensitivity check.

use rjson::{Arena, NodeType, Parser};

/// Parses `"\uXXXX"` (for a lowercase or uppercase 4-hex-digit `hex4`) as a
/// standalone JSON string value and returns the decoded UTF-8 bytes, or
/// `None` if the parse failed (expected for lone surrogate halves, which
/// `parse_hex4` itself decodes correctly but `utf16_literal_to_utf8`
/// rejects as an incomplete/invalid surrogate pair).
fn parse_single_unicode_escape(hex4: &str) -> Option<Vec<u8>> {
    let json = format!("\"\\u{}\"", hex4);
    let mut arena = Arena::new();
    let mut parser = Parser::new(json.as_bytes(), &mut arena);
    match parser.parse_value() {
        Ok(id) => {
            let node = arena.get(id);
            assert_eq!(node.node_type, NodeType::String, "expected a String node");
            node.value_string.clone()
        }
        Err(_) => None,
    }
}

/// Mirrors `parse_hex4_should_parse_all_combinations`: every 4-hex-digit
/// value from `0x0000` to `0xFFFF`, in both lowercase and uppercase,
/// decodes to the exact codepoint the digits spell out -- checked here by
/// comparing against Rust's own UTF-8 encoding of that codepoint (for
/// non-surrogate values) or confirming rejection (for lone surrogate
/// halves, which cannot be represented as a standalone Unicode scalar
/// value at all, so there is no "expected UTF-8 bytes" to compare against
/// -- upstream's own `parse_hex4` also runs on these ranges without
/// complaint, it's the *surrogate-pairing* logic one level up that
/// rejects them, matching this port's `utf16_literal_to_utf8`, see
/// `parse_string_tests.rs`'s dedicated surrogate-rejection tests for that
/// layer specifically).
#[test]
fn parses_every_hex4_value_lowercase_and_uppercase() {
    for codepoint in 0u32..=0xFFFF {
        let lower = format!("{:04x}", codepoint);
        let upper = format!("{:04X}", codepoint);

        let is_surrogate_half = (0xD800..=0xDFFF).contains(&codepoint);

        if is_surrogate_half {
            assert!(
                parse_single_unicode_escape(&lower).is_none(),
                "expected lone surrogate half {:04x} (lowercase) to be rejected",
                codepoint
            );
            assert!(
                parse_single_unicode_escape(&upper).is_none(),
                "expected lone surrogate half {:04X} (uppercase) to be rejected",
                codepoint
            );
            continue;
        }

        let expected: Vec<u8> = char::from_u32(codepoint)
            .unwrap_or_else(|| panic!("{:04x} is not a valid non-surrogate codepoint", codepoint))
            .to_string()
            .into_bytes();

        assert_eq!(
            parse_single_unicode_escape(&lower),
            Some(expected.clone()),
            "lowercase {:04x} decoded incorrectly",
            codepoint
        );
        assert_eq!(
            parse_single_unicode_escape(&upper),
            Some(expected),
            "uppercase {:04X} decoded incorrectly",
            codepoint
        );
    }
}

/// Mirrors `parse_hex4_should_parse_mixed_case`: every case-permutation of
/// the literal digits `beef` decodes to the same codepoint (`0xBEEF`).
#[test]
fn parses_beef_in_every_case_permutation() {
    let expected: Vec<u8> = char::from_u32(0xBEEF).unwrap().to_string().into_bytes();
    for digits in [
        "beef", "beeF", "beEf", "beEF", "bEef", "bEeF", "bEEf", "bEEF", "Beef", "BeeF", "BeEf",
        "BeEF", "BEef", "BEeF", "BEEf", "BEEF",
    ] {
        assert_eq!(
            parse_single_unicode_escape(digits),
            Some(expected.clone()),
            "{} did not decode to U+BEEF",
            digits
        );
    }
}
