use rjson::{Arena, Node, NodeType};

fn test_node() -> Node {
    Node {
        next: None,
        prev: None,
        child: None,
        node_type: NodeType::String,
        value_string: Some(b"before".to_vec()),
        value_double: 1.5,
        key: Some(b"key".to_vec()),
        is_reference: false,
        key_is_const: false,
    }
}

#[test]
fn alloc_returns_id_and_get_reads_node() {
    let mut arena = Arena::new();
    let id = arena.alloc(test_node());

    assert_eq!(id.0, 0);
    assert_eq!(arena.get(id).value_string, Some(b"before".to_vec()));
    assert_eq!(arena.get(id).value_double, 1.5);
}

#[test]
fn allocations_receive_distinct_stable_ids() {
    let mut arena = Arena::new();
    let first = arena.alloc(test_node());
    let second = arena.alloc(test_node());

    assert_ne!(first, second);
    assert_eq!(first.0, 0);
    assert_eq!(second.0, 1);
    assert_eq!(arena.get(first).key, Some(b"key".to_vec()));
}

#[test]
fn get_mut_updates_allocated_node() {
    let mut arena = Arena::new();
    let id = arena.alloc(test_node());

    arena.get_mut(id).value_string = Some(b"after".to_vec());
    arena.get_mut(id).value_double = 42.0;

    assert_eq!(arena.get(id).value_string, Some(b"after".to_vec()));
    assert_eq!(arena.get(id).value_double, 42.0);
}
