// Black-box re-expression of `tests/original/parse_value.c`'s intent
// against the public `Parser`/`Arena` API, per `DECISIONS.md` #2 (the
// original file's `assert_parse_value` helper calls the internal, static
// `parse_value` directly with a hand-built `parse_buffer` -- this port's
// `Parser::parse_value` is already `pub`, so this is a direct, not
// re-expressed, translation of the original assertions).
//
// Every case in this file is drawn from an original assertion in
// `tests/original/parse_value.c`. See `rJSON/DECISIONS_personal.md` for
// the entry documenting this file.

use rjson::{Arena, NodeType, Parser};

/// Mirrors `assert_parse_value(string, type)` / `assert_is_value`:
/// `assert_not_in_list` -- no next/prev; `assert_has_type` == the given
/// type; `assert_has_no_reference`; `assert_has_no_const_string`;
/// `assert_has_no_string`, i.e. no key. (Deliberately does *not* assert
/// `assert_has_no_child` or `assert_has_no_valuestring`, matching upstream
/// exactly: `assert_is_value` doesn't check those either, since it's used
/// across every value type, including array/object, which *do* have
/// children, and string, which *does* have a `valuestring`.)
fn assert_parse_value(json_bytes: &[u8], expected_type: NodeType) {
    let mut arena = Arena::new();
    let mut parser = Parser::new(json_bytes, &mut arena);
    let id = parser
        .parse_value()
        .unwrap_or_else(|_| panic!("expected {:?} to parse", String::from_utf8_lossy(json_bytes)));
    let node = arena.get(id);
    assert_eq!(node.node_type, expected_type, "wrong node type");
    assert_eq!(node.next, None);
    assert_eq!(node.prev, None);
    assert!(!node.is_reference);
    assert!(!node.key_is_const);
    assert_eq!(node.key, None);
}

/// Mirrors `parse_value_should_parse_null`.
#[test]
fn parses_null() {
    assert_parse_value(b"null", NodeType::Null);
}

/// Mirrors `parse_value_should_parse_true`.
#[test]
fn parses_true() {
    assert_parse_value(b"true", NodeType::True);
}

/// Mirrors `parse_value_should_parse_false`.
#[test]
fn parses_false() {
    assert_parse_value(b"false", NodeType::False);
}

/// Mirrors `parse_value_should_parse_number`.
#[test]
fn parses_number() {
    assert_parse_value(b"1.5", NodeType::Number);
}

/// Mirrors `parse_value_should_parse_string`.
#[test]
fn parses_string() {
    assert_parse_value(b"\"\"", NodeType::String);
    assert_parse_value(b"\"hello\"", NodeType::String);
}

/// Mirrors `parse_value_should_parse_array`.
#[test]
fn parses_array() {
    assert_parse_value(b"[]", NodeType::Array);
}

/// Mirrors `parse_value_should_parse_object`.
#[test]
fn parses_object() {
    assert_parse_value(b"{}", NodeType::Object);
}
