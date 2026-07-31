use rjson::{Arena, NodeType};

fn assert_unattached(arena: &Arena, id: rjson::NodeId) {
    let node = arena.get(id);
    assert_eq!(node.next, None);
    assert_eq!(node.prev, None);
    assert_eq!(node.child, None);
    assert_eq!(node.key, None);
    assert!(!node.is_reference);
    assert!(!node.key_is_const);
}

#[test]
fn creates_null_true_false_and_bool_nodes() {
    let mut arena = Arena::new();
    let null = arena.create_null();
    let true_node = arena.create_true();
    let false_node = arena.create_false();
    let bool_true = arena.create_bool(true);
    let bool_false = arena.create_bool(false);

    assert_eq!(arena.get(null).node_type, NodeType::Null);
    assert_eq!(arena.get(true_node).node_type, NodeType::True);
    assert_eq!(arena.get(true_node).value_double, 0.0);
    assert_eq!(arena.get(false_node).node_type, NodeType::False);
    assert_eq!(arena.get(bool_true).node_type, NodeType::True);
    assert_eq!(arena.get(bool_false).node_type, NodeType::False);

    for id in [null, true_node, false_node, bool_true, bool_false] {
        assert_unattached(&arena, id);
    }
}

#[test]
fn creates_number_with_double_payload() {
    let mut arena = Arena::new();
    let id = arena.create_number(-12.5);

    assert_eq!(arena.get(id).node_type, NodeType::Number);
    assert_eq!(arena.get(id).value_double, -12.5);
    assert_eq!(arena.get(id).value_string, None);
    assert_unattached(&arena, id);
}

#[test]
fn creates_string_with_raw_bytes() {
    let mut arena = Arena::new();
    let bytes = vec![0, 0xff, 0xfe, b'c'];
    let id = arena.create_string(bytes.clone());

    assert_eq!(arena.get(id).node_type, NodeType::String);
    assert_eq!(arena.get(id).value_string, Some(bytes));
    assert_unattached(&arena, id);
}

#[test]
fn creates_raw_with_raw_bytes() {
    let mut arena = Arena::new();
    let bytes = vec![b'{', 0xff, b'}'];
    let id = arena.create_raw(bytes.clone());

    assert_eq!(arena.get(id).node_type, NodeType::Raw);
    assert_eq!(arena.get(id).value_string, Some(bytes));
    assert_unattached(&arena, id);
}
