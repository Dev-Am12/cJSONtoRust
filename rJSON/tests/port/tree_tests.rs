use rjson::{Arena, NodeId, NodeType};

#[test]
fn creates_unattached_array_and_object() {
    let mut arena = Arena::new();
    let array = arena.create_array();
    let object = arena.create_object();

    for (id, node_type) in [(array, NodeType::Array), (object, NodeType::Object)] {
        let node = arena.get(id);
        assert_eq!(node.node_type, node_type);
        assert_eq!(node.next, None);
        assert_eq!(node.prev, None);
        assert_eq!(node.child, None);
        assert_eq!(node.value_string, None);
        assert_eq!(node.value_double, 0.0);
        assert_eq!(node.key, None);
        assert!(!node.is_reference);
        assert!(!node.key_is_const);
    }
}

#[test]
fn appending_one_child_sets_parent_child_link() {
    let mut arena = Arena::new();
    let array = arena.create_array();
    let child = arena.create_null();

    arena.append_child(array, child, None);

    assert_eq!(arena.get(array).child, Some(child));
    assert_eq!(arena.get(child).next, None);
    assert_eq!(arena.get(child).prev, None);
    assert_eq!(arena.get(child).key, None);
}

#[test]
fn appending_multiple_children_links_siblings_in_both_directions() {
    let mut arena = Arena::new();
    let array = arena.create_array();
    let first = arena.create_number(1.0);
    let second = arena.create_number(2.0);
    let third = arena.create_number(3.0);

    arena.append_child(array, first, None);
    arena.append_child(array, second, None);
    arena.append_child(array, third, None);

    assert_eq!(arena.get(array).child, Some(first));
    assert_eq!(arena.get(first).prev, None);
    assert_eq!(arena.get(first).next, Some(second));
    assert_eq!(arena.get(second).prev, Some(first));
    assert_eq!(arena.get(second).next, Some(third));
    assert_eq!(arena.get(third).prev, Some(second));
    assert_eq!(arena.get(third).next, None);

    let mut forward = Vec::new();
    let mut current = arena.get(array).child;
    while let Some(id) = current {
        forward.push(id);
        current = arena.get(id).next;
    }
    assert_eq!(forward, vec![first, second, third]);

    let mut backward = Vec::new();
    let mut current: Option<NodeId> = Some(third);
    while let Some(id) = current {
        backward.push(id);
        current = arena.get(id).prev;
    }
    assert_eq!(backward, vec![third, second, first]);
}

#[test]
fn appending_keyed_child_to_object_sets_key() {
    let mut arena = Arena::new();
    let object = arena.create_object();
    let child = arena.create_string(vec![0xff, b'x']);
    let key = vec![b'n', 0xff, b'm', b'e'];

    arena.append_child(object, child, Some(key.clone()));

    assert_eq!(arena.get(object).child, Some(child));
    assert_eq!(arena.get(child).key, Some(key));
    assert_eq!(arena.get(child).value_string, Some(vec![0xff, b'x']));
}
