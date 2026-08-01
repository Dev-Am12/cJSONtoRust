use rjson::{Arena, NodeType};

#[test]
fn string_reference_preserves_bytes_and_sets_reference_flag() {
    let mut arena = Arena::new();
    let bytes = vec![0, 0xff, b'x'];
    let id = arena.create_string_reference(bytes.clone());
    let node = arena.get(id);

    assert_eq!(node.node_type, NodeType::String);
    assert_eq!(node.value_string, Some(bytes));
    assert!(node.is_reference);
    assert!(!node.key_is_const);
    assert_eq!(node.next, None);
    assert_eq!(node.prev, None);
    assert_eq!(node.child, None);
    assert_eq!(node.key, None);
}

#[test]
fn object_reference_shares_source_child_id() {
    let mut arena = Arena::new();
    let source = arena.create_object();
    let child = arena.create_null();
    arena.append_child(source, child, Some(b"member".to_vec()));

    let reference = arena.create_object_reference(source);
    let node = arena.get(reference);

    assert_eq!(node.node_type, NodeType::Object);
    assert_eq!(node.child, arena.get(source).child);
    assert_eq!(node.child, Some(child));
    assert!(node.is_reference);
    assert!(!node.key_is_const);
    assert_eq!(node.next, None);
    assert_eq!(node.prev, None);
    assert_eq!(node.key, None);
}

#[test]
fn array_reference_shares_source_child_id() {
    let mut arena = Arena::new();
    let source = arena.create_array();
    let child = arena.create_string(vec![b'x']);
    arena.append_child(source, child, None);

    let reference = arena.create_array_reference(source);
    let node = arena.get(reference);

    assert_eq!(node.node_type, NodeType::Array);
    assert_eq!(node.child, arena.get(source).child);
    assert_eq!(node.child, Some(child));
    assert!(node.is_reference);
    assert!(!node.key_is_const);
    assert_eq!(node.next, None);
    assert_eq!(node.prev, None);
    assert_eq!(node.key, None);
}
