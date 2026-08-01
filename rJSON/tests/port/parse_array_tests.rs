// Black-box re-expression of `tests/original/parse_array.c`'s intent
// against the public `Parser`/`Arena` API, per `DECISIONS.md` #2 (the
// original file's `assert_parse_array`/`assert_not_array` helpers call
// the internal, static `parse_array` directly with a hand-built
// `parse_buffer`, which has no equivalent shape in this port -- array
// parsing here is reached only through `Parser::parse_value`).
//
// Every case in this file is drawn from an original assertion in
// `tests/original/parse_array.c`. See `rJSON/DECISIONS_personal.md` for
// the entry documenting this implementation.

use rjson::{Arena, NodeId, NodeType, Parser};

/// Parses `json_bytes` into `arena` and asserts the result is a
/// freshly-parsed, well-formed `Array` node (mirrors `assert_is_array`:
/// `assert_not_in_list` -- no next/prev; `assert_has_type` ==
/// `cJSON_Array`; `assert_has_no_reference`; `assert_has_no_const_string`;
/// `assert_has_no_valuestring`; `assert_has_no_string`, i.e. no key).
fn assert_parse_array(arena: &mut Arena, json_bytes: &[u8]) -> NodeId {
    let id = Parser::new(json_bytes, arena)
        .parse_value()
        .unwrap_or_else(|_| panic!("expected {:?} to parse", String::from_utf8_lossy(json_bytes)));
    let node = arena.get(id);
    assert_eq!(node.node_type, NodeType::Array, "expected an Array node");
    assert_eq!(node.next, None);
    assert_eq!(node.prev, None);
    assert!(!node.is_reference);
    assert!(!node.key_is_const);
    assert_eq!(node.value_string, None);
    assert_eq!(node.key, None);
    id
}




fn assert_not_array(json_bytes: &[u8]) {
    let mut arena = Arena::new();
    let mut parser = Parser::new(json_bytes, &mut arena);
    assert!(
        parser.parse_value().is_err(),
        "expected parse failure for {:?}",
        String::from_utf8_lossy(json_bytes)
    );
}

/// Collects the `node_type` of `first` and every one of its `next`
/// siblings, in order.
fn sibling_types(arena: &Arena, first: Option<NodeId>) -> Vec<NodeType> {
    let mut types = Vec::new();
    let mut current = first;
    while let Some(id) = current {
        let node = arena.get(id);
        types.push(match node.node_type {
            NodeType::Null => NodeType::Null,
            NodeType::False => NodeType::False,
            NodeType::True => NodeType::True,
            NodeType::Number => NodeType::Number,
            NodeType::String => NodeType::String,
            NodeType::Array => NodeType::Array,
            NodeType::Object => NodeType::Object,
            NodeType::Raw => NodeType::Raw,
        });
        current = node.next;
    }
    types
}

/// Mirrors `parse_array_should_parse_empty_arrays`.
#[test]
fn parses_empty_arrays() {
    let mut arena = Arena::new();
    let id = assert_parse_array(&mut arena, b"[]");
    assert_eq!(arena.get(id).child, None);

    let mut arena = Arena::new();
    let id = assert_parse_array(&mut arena, b"[\n\t]");
    assert_eq!(arena.get(id).child, None);
}

/// Mirrors `parse_array_should_parse_arrays_with_one_element`.
#[test]
fn parses_arrays_with_one_element() {
    let mut arena = Arena::new();
    let id = assert_parse_array(&mut arena, b"[1]");
    let child = arena.get(id).child.expect("array should have a child");
    assert_eq!(arena.get(child).node_type, NodeType::Number);

    let mut arena = Arena::new();
    let id = assert_parse_array(&mut arena, b"[\"hello!\"]");
    let child = arena.get(id).child.expect("array should have a child");
    assert_eq!(arena.get(child).node_type, NodeType::String);
    assert_eq!(arena.get(child).value_string, Some(b"hello!".to_vec()));

    let mut arena = Arena::new();
    let id = assert_parse_array(&mut arena, b"[[]]");
    let child = arena.get(id).child.expect("array should have a child");
    assert_eq!(arena.get(child).node_type, NodeType::Array);
    assert_eq!(arena.get(child).child, None);

    let mut arena = Arena::new();
    let id = assert_parse_array(&mut arena, b"[null]");
    let child = arena.get(id).child.expect("array should have a child");
    assert_eq!(arena.get(child).node_type, NodeType::Null);
}

/// Mirrors `parse_array_should_parse_arrays_with_multiple_elements`,
/// including the exact three-number sibling-chain check and the
/// seven-type walk (`[1, null, true, false, [], "hello", {}]`).
#[test]
fn parses_arrays_with_multiple_elements() {
    let mut arena = Arena::new();
    let id = assert_parse_array(&mut arena, b"[1\t,\n2, 3]");
    let first = arena.get(id).child.expect("array should have a child");
    let second = arena.get(first).next.expect("expected a second element");
    let third = arena.get(second).next.expect("expected a third element");
    assert_eq!(arena.get(third).next, None);
    assert_eq!(arena.get(first).node_type, NodeType::Number);
    assert_eq!(arena.get(second).node_type, NodeType::Number);
    assert_eq!(arena.get(third).node_type, NodeType::Number);

    let mut arena = Arena::new();
    let id = assert_parse_array(&mut arena, b"[1, null, true, false, [], \"hello\", {}]");
    let expected = vec![
        NodeType::Number,
        NodeType::Null,
        NodeType::True,
        NodeType::False,
        NodeType::Array,
        NodeType::String,
        NodeType::Object,
    ];
    let actual = sibling_types(&arena, arena.get(id).child);
    assert_eq!(actual, expected);
}

/// Mirrors `parse_array_should_not_parse_non_arrays`.

/// Re-expression of `parse_array_should_not_parse_non_arrays`.
///
/// The original C test called the internal `parse_array()` function,
/// which is only responsible for parsing arrays and therefore rejects
/// any other JSON value.
///
/// In this Rust port, array parsing is not exposed as a standalone API;
/// arrays are parsed through the public `Parser::parse_value()` entry
/// point. Consequently:
///
/// - malformed JSON must still fail to parse;
/// - valid JSON that is *not* an array must parse successfully as its
///   own node type.
///
/// This preserves the behavioral intent using the public API, per
/// `DECISIONS.md` §2.
#[test]
fn does_not_parse_non_arrays() {
    // Invalid JSON should fail.
    assert_not_array(b"");
    assert_not_array(b"[");
    assert_not_array(b"]");

    // Valid JSON values that are not arrays should parse as their
    // corresponding types rather than as arrays.

    let cases = [
        (b"{\"hello\":[]}".as_slice(), NodeType::Object),
        (b"42".as_slice(), NodeType::Number),
        (b"3.14".as_slice(), NodeType::Number),
        (b"\"[]hello world!\n\"".as_slice(), NodeType::String),
    ];

    for (json, expected_type) in cases {
        let mut arena = Arena::new();
        let id = Parser::new(json, &mut arena)
            .parse_value()
            .unwrap_or_else(|_| panic!("expected {:?} to parse", String::from_utf8_lossy(json)));

        assert_eq!(
            arena.get(id).node_type,
            expected_type,
            "{:?} should parse as {:?}, not Array",
            String::from_utf8_lossy(json),
            expected_type,
        );
    }
}

/// `CJSON_NESTING_LIMIT` enforcement, matching
/// `cjson_should_not_parse_to_deeply_nested_jsons` in `misc_tests.c`
/// (which drives this through `cJSON_Parse`, i.e. `Parser::parse_value`
/// at the top): `CJSON_NESTING_LIMIT + 1` unclosed `[` characters must
/// fail.
#[test]
fn rejects_arrays_nested_beyond_the_limit() {
    let json = vec![b'['; rjson::CJSON_NESTING_LIMIT + 1];
    assert_not_array(&json);
}

/// Nesting exactly at the limit (not one over it) must still succeed --
/// checked with matching closes so the only way this could fail is the
/// nesting-limit check itself being off-by-one.
#[test]
fn parses_arrays_nested_exactly_at_the_limit() {
    let mut json = vec![b'['; rjson::CJSON_NESTING_LIMIT];
    json.extend(std::iter::repeat(b']').take(rjson::CJSON_NESTING_LIMIT));
    let mut arena = Arena::new();
    let mut parser = Parser::new(&json, &mut arena);
    assert!(parser.parse_value().is_ok());
}
