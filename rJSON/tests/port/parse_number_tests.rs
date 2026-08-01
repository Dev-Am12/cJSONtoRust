// Black-box re-expression of `tests/original/parse_number.c`'s intent
// against the public `Parser`/`Arena` API, per `DECISIONS.md` #2 (the
// original file's `assert_parse_number`/`assert_parse_big_number` helpers
// call the internal, static `parse_number` directly with a hand-built
// `parse_buffer`, which has no equivalent shape in this port -- number
// parsing here is reached only through `Parser::parse_value`).
//
// Every case in this file is drawn from an original assertion in
// `tests/original/parse_number.c`. `item->valueint` is re-expressed via the
// free function `clamped_int_value` (see `DECISIONS_personal.md` #6),
// applied to the parsed `value_double`, since `Node` deliberately has no
// `valueint` field. See `rJSON/DECISIONS_personal.md` for the entry
// documenting this file.

use rjson::{clamped_int_value, Arena, NodeType, Parser};

/// Parses `json_bytes` and asserts the result is a freshly-parsed,
/// well-formed `Number` node (mirrors `assert_is_number`:
/// `assert_not_in_list` -- no next/prev; `assert_has_no_child`;
/// `assert_has_type` == `cJSON_Number`; `assert_has_no_reference`;
/// `assert_has_no_const_string`; `assert_has_no_valuestring`;
/// `assert_has_no_string`, i.e. no key). Returns the parsed `value_double`.
fn assert_parse_number(json_bytes: &[u8]) -> f64 {
    let mut arena = Arena::new();
    let mut parser = Parser::new(json_bytes, &mut arena);
    let id = parser
        .parse_value()
        .unwrap_or_else(|_| panic!("expected {:?} to parse", String::from_utf8_lossy(json_bytes)));
    let node = arena.get(id);
    assert_eq!(node.node_type, NodeType::Number, "expected a Number node");
    assert_eq!(node.next, None);
    assert_eq!(node.prev, None);
    assert_eq!(node.child, None);
    assert!(!node.is_reference);
    assert!(!node.key_is_const);
    assert_eq!(node.value_string, None);
    assert_eq!(node.key, None);
    node.value_double
}

/// Mirrors `assert_parse_number(string, integer, real)`: checks both the
/// `value_double` and the `clamped_int_value` (`valueint`-equivalent).
fn assert_parse_number_full(json_bytes: &[u8], integer: i32, real: f64) {
    let value_double = assert_parse_number(json_bytes);
    assert_eq!(
        value_double,
        real,
        "value_double mismatch for {:?}",
        String::from_utf8_lossy(json_bytes)
    );
    assert_eq!(
        clamped_int_value(value_double),
        integer,
        "clamped_int_value mismatch for {:?}",
        String::from_utf8_lossy(json_bytes)
    );
}

/// Mirrors `parse_number_should_parse_zero`.
#[test]
fn parses_zero() {
    assert_parse_number_full(b"0", 0, 0.0);
    assert_parse_number_full(b"0.0", 0, 0.0);
    assert_parse_number_full(b"-0", 0, -0.0);
}

/// Mirrors `parse_number_should_parse_negative_integers`.
#[test]
fn parses_negative_integers() {
    assert_parse_number_full(b"-1", -1, -1.0);
    assert_parse_number_full(b"-32768", -32768, -32768.0);
    assert_parse_number_full(b"-2147483648", i32::MIN, -2147483648.0);
}

/// Mirrors `parse_number_should_parse_positive_integers`.
#[test]
fn parses_positive_integers() {
    assert_parse_number_full(b"1", 1, 1.0);
    assert_parse_number_full(b"32767", 32767, 32767.0);
    assert_parse_number_full(b"2147483647", 2147483647, 2147483647.0);
}

/// Mirrors `parse_number_should_parse_positive_reals`, including the
/// `INT_MAX`-saturation cases (`"10e10"`, `"123e+127"`).
#[test]
fn parses_positive_reals() {
    assert_parse_number_full(b"0.001", 0, 0.001);
    assert_parse_number_full(b"10e-10", 0, 10e-10);
    assert_parse_number_full(b"10E-10", 0, 10e-10);
    assert_parse_number_full(b"10e10", i32::MAX, 10e10);
    assert_parse_number_full(b"123e+127", i32::MAX, 123e127);
    assert_parse_number_full(b"123e-128", 0, 123e-128);
}

/// Mirrors `parse_number_should_parse_negative_reals`, including the
/// `INT_MIN`-saturation cases (`"-10e20"`, `"-123e+127"`).
#[test]
fn parses_negative_reals() {
    assert_parse_number_full(b"-0.001", 0, -0.001);
    assert_parse_number_full(b"-10e-10", 0, -10e-10);
    assert_parse_number_full(b"-10E-10", 0, -10e-10);
    assert_parse_number_full(b"-10e20", i32::MIN, -10e20);
    assert_parse_number_full(b"-123e+127", i32::MIN, -123e127);
    assert_parse_number_full(b"-123e-128", 0, -123e-128);
}

/// Mirrors `parse_number_should_parse_big_numbers`: these only assert the
/// parse *succeeds* as a well-formed Number node (`assert_is_number`, no
/// specific value check) -- matching upstream's own
/// `assert_parse_big_number`, which never calls `TEST_ASSERT_EQUAL_DOUBLE`
/// or checks `valueint` for these three cases.
#[test]
fn parses_big_numbers_without_checking_exact_value() {
    assert_parse_number(b"9999999999999999999999999999999999999999999999912345678901234567");
    assert_parse_number(
        b"9999999999999999999999999999999999999999999999912345678901234567E10",
    );
    assert_parse_number(b"999999999999999999999999999999999999999999999991234567890.1234567");
}
