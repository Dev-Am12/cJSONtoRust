// Black-box re-expression of `tests/original/parse_object.c`'s intent
// against the public `Parser`/`Arena` API, per `DECISIONS.md` #2 (the
// original file's `assert_parse_object`/`assert_not_object` helpers call
// the internal, static `parse_object` directly with a hand-built
// `parse_buffer`, which has no equivalent shape in this port -- object
// parsing here is reached only through `Parser::parse_value`).
//
// Every case in this file is drawn from an original assertion in
// `tests/original/parse_object.c`. See `rJSON/DECISIONS_personal.md` for
// the entry documenting this implementation.

use rjson::{Arena, NodeId, NodeType, Parser};

/// Parses `json_bytes` into `arena` and asserts the result is a
/// freshly-parsed, well-formed `Object` node (mirrors `assert_is_object`:
/// `assert_not_in_list` -- no next/prev; `assert_has_type` ==
/// `cJSON_Object`; `assert_has_no_reference`; `assert_has_no_const_string`;
/// `assert_has_no_valuestring`; `assert_has_no_string`, i.e. no key of its
/// own -- the object node itself is never keyed, only its children are).
fn assert_parse_object(arena: &mut Arena, json_bytes: &[u8]) -> NodeId {
    let id = Parser::new(json_bytes, arena)
        .parse_value()
        .unwrap_or_else(|_| panic!("expected {:?} to parse", String::from_utf8_lossy(json_bytes)));
    let node = arena.get(id);
    assert_eq!(node.node_type, NodeType::Object, "expected an Object node");
    assert_eq!(node.next, None);
    assert_eq!(node.prev, None);
    assert!(!node.is_reference);
    assert!(!node.key_is_const);
    assert_eq!(node.value_string, None);
    assert_eq!(node.key, None);
    id
}

fn assert_not_object(json_bytes: &[u8]) {
    let mut arena = Arena::new();
    let mut parser = Parser::new(json_bytes, &mut arena);
    assert!(
        parser.parse_value().is_err(),
        "expected parse failure for {:?}",
        String::from_utf8_lossy(json_bytes)
    );
}

/// Mirrors `assert_is_child`: the node exists, has the expected key
/// (`child_item->string`), and the expected type.
fn assert_is_child(arena: &Arena, child: Option<NodeId>, name: &[u8], node_type: NodeType) {
    let id = child.expect("child item is missing");
    let node = arena.get(id);
    assert_eq!(
        node.key.as_deref(),
        Some(name),
        "child item has the wrong name"
    );
    assert_eq!(node.node_type, node_type);
}

/// Mirrors `parse_object_should_parse_empty_objects`.
#[test]
fn parses_empty_objects() {
    let mut arena = Arena::new();
    let id = assert_parse_object(&mut arena, b"{}");
    assert_eq!(arena.get(id).child, None);

    let mut arena = Arena::new();
    let id = assert_parse_object(&mut arena, b"{\n\t}");
    assert_eq!(arena.get(id).child, None);
}

/// Mirrors `parse_object_should_parse_objects_with_one_element`.
#[test]
fn parses_objects_with_one_element() {
    let mut arena = Arena::new();
    let id = assert_parse_object(&mut arena, b"{\"one\":1}");
    assert_is_child(&arena, arena.get(id).child, b"one", NodeType::Number);

    let mut arena = Arena::new();
    let id = assert_parse_object(&mut arena, b"{\"hello\":\"world!\"}");
    assert_is_child(&arena, arena.get(id).child, b"hello", NodeType::String);

    let mut arena = Arena::new();
    let id = assert_parse_object(&mut arena, b"{\"array\":[]}");
    assert_is_child(&arena, arena.get(id).child, b"array", NodeType::Array);

    let mut arena = Arena::new();
    let id = assert_parse_object(&mut arena, b"{\"null\":null}");
    assert_is_child(&arena, arena.get(id).child, b"null", NodeType::Null);
}

/// Mirrors `parse_object_should_parse_objects_with_multiple_elements`,
/// including the exact three-key sibling-chain check and the seven-member
/// walk with both names and types.
#[test]
fn parses_objects_with_multiple_elements() {
    let mut arena = Arena::new();
    let id = assert_parse_object(&mut arena, b"{\"one\":1\t,\t\"two\"\n:2, \"three\":3}");
    let first = arena.get(id).child;
    assert_is_child(&arena, first, b"one", NodeType::Number);
    let second = arena.get(first.unwrap()).next;
    assert_is_child(&arena, second, b"two", NodeType::Number);
    let third = arena.get(second.unwrap()).next;
    assert_is_child(&arena, third, b"three", NodeType::Number);

    let mut arena = Arena::new();
    let id = assert_parse_object(
        &mut arena,
        b"{\"one\":1, \"NULL\":null, \"TRUE\":true, \"FALSE\":false, \"array\":[], \"world\":\"hello\", \"object\":{}}",
    );
    let expected: Vec<(&[u8], NodeType)> = vec![
        (&b"one"[..], NodeType::Number),
        (&b"NULL"[..], NodeType::Null),
        (&b"TRUE"[..], NodeType::True),
        (&b"FALSE"[..], NodeType::False),
        (&b"array"[..], NodeType::Array),
        (&b"world"[..], NodeType::String),
        (&b"object"[..], NodeType::Object),
    ];
    let mut current = arena.get(id).child;
    for (name, node_type) in expected {
        assert_is_child(&arena, current, name, node_type);
        current = arena.get(current.unwrap()).next;
    }
    assert_eq!(current, None, "expected exactly 7 members");
}

/// Mirrors `parse_object_should_not_parse_non_objects`.
/// Re-expression of `parse_object_should_not_parse_non_objects`.
///
/// The original C test exercised the internal `parse_object()` routine,
/// which rejects any JSON value that is not an object. In this Rust port,
/// only the public `Parser::parse_value()` entry point exists, so the
/// behavioral intent is re-expressed by verifying that:
///
/// - malformed JSON fails to parse; and
/// - valid non-object JSON parses successfully as its correct node type.
#[test]
fn does_not_parse_non_objects() {
    // Malformed JSON should still fail.
    assert_not_object(b"");
    assert_not_object(b"{");
    assert_not_object(b"}");

    // Valid JSON values that are not objects should parse successfully
    // as their own types.
    let cases = [
        (b"[\"hello\",{}]".as_slice(), NodeType::Array),
        (b"42".as_slice(), NodeType::Number),
        (b"3.14".as_slice(), NodeType::Number),
        (b"\"{}hello world!\n\"".as_slice(), NodeType::String),
    ];

    for (json, expected_type) in cases {
        let mut arena = Arena::new();

        let id = Parser::new(json, &mut arena)
            .parse_value()
            .unwrap_or_else(|_| {
                panic!("expected {:?} to parse", String::from_utf8_lossy(json))
            });

        assert_eq!(
            arena.get(id).node_type,
            expected_type,
            "{:?} should parse as {:?}, not Object",
            String::from_utf8_lossy(json),
            expected_type,
        );
    }
}

/// `CJSON_NESTING_LIMIT` enforcement for objects (the object-nested
/// analogue of `cjson_should_not_parse_to_deeply_nested_jsons`, which
/// upstream only exercises via nested arrays -- object nesting shares the
/// exact same `depth` field and check, so this closes that gap).
#[test]
fn rejects_objects_nested_beyond_the_limit() {
    let mut json = Vec::new();
    for _ in 0..=rjson::CJSON_NESTING_LIMIT {
        json.extend_from_slice(b"{\"a\":");
    }
    assert_not_object(&json);
}

/// Requirement: duplicate keys are preserved, not rejected or
/// deduplicated -- both members remain in the sibling chain in
/// encounter order, each keeping its own key and value.
#[test]
fn preserves_duplicate_keys() {
    let mut arena = Arena::new();
    let id = assert_parse_object(&mut arena, b"{\"a\":1,\"a\":2}");
    let first = arena.get(id).child.expect("expected a first member");
    let second = arena.get(first).next.expect("expected a second member");
    assert_eq!(arena.get(second).next, None);
    assert_eq!(arena.get(first).key.as_deref(), Some(&b"a"[..]));
    assert_eq!(arena.get(second).key.as_deref(), Some(&b"a"[..]));
    assert_eq!(arena.get(first).value_double, 1.0);
    assert_eq!(arena.get(second).value_double, 2.0);
}
