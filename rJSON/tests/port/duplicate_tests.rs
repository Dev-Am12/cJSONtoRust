use rjson::{Arena, CJSON_CIRCULAR_LIMIT, NodeType};

fn sample_tree(arena: &mut Arena) -> (rjson::NodeId, rjson::NodeId, rjson::NodeId) {
    let root = arena.create_object();
    let array = arena.create_array();
    let string = arena.create_string(b"original".to_vec());
    assert!(arena.add_item_to_object(root, b"items".to_vec(), array));
    assert!(arena.add_item_to_array(array, string));
    (root, array, string)
}

#[test]
fn deep_duplicate_uses_new_node_ids_and_independent_payloads() {
    let mut arena = Arena::new();
    let (source_root, source_array, source_string) = sample_tree(&mut arena);

    let duplicate_root = arena
        .duplicate(source_root, true)
        .expect("a live tree duplicates");
    let duplicate_array = arena.get(duplicate_root).child.expect("copied child");
    let duplicate_string = arena
        .get(duplicate_array)
        .child
        .expect("copied grandchild");

    assert_ne!(duplicate_root, source_root);
    assert_ne!(duplicate_array, source_array);
    assert_ne!(duplicate_string, source_string);
    assert_eq!(arena.get(duplicate_array).key, Some(b"items".to_vec()));
    assert_eq!(arena.get(duplicate_string).value_string, Some(b"original".to_vec()));

    arena.get_mut(duplicate_array).key = Some(b"changed-key".to_vec());
    arena.get_mut(duplicate_string).value_string = Some(b"changed-value".to_vec());

    assert_eq!(arena.get(source_array).key, Some(b"items".to_vec()));
    assert_eq!(arena.get(source_string).value_string, Some(b"original".to_vec()));

    arena.get_mut(source_array).key = Some(b"source-key".to_vec());
    arena.get_mut(source_string).value_string = Some(b"source-value".to_vec());

    assert_eq!(
        arena.get(duplicate_array).key,
        Some(b"changed-key".to_vec())
    );
    assert_eq!(
        arena.get(duplicate_string).value_string,
        Some(b"changed-value".to_vec())
    );
}

#[test]
fn non_recursive_duplicate_is_childless_copy() {
    let mut arena = Arena::new();
    let (source_root, _, _) = sample_tree(&mut arena);

    let duplicate = arena
        .duplicate(source_root, false)
        .expect("a live node duplicates");

    assert_eq!(arena.get(duplicate).node_type, NodeType::Object);
    assert_eq!(arena.get(duplicate).child, None);
    assert_eq!(arena.get(duplicate).next, None);
    assert_eq!(arena.get(duplicate).prev, None);
}

#[test]
fn duplicate_of_reference_becomes_independent_owning_copy() {
    let mut arena = Arena::new();
    let source = arena.create_string_reference(vec![0xff, b'x']);

    let duplicate = arena
        .duplicate(source, false)
        .expect("a live reference duplicates");

    assert_ne!(duplicate, source);
    assert!(!arena.get(duplicate).is_reference);
    assert_eq!(arena.get(duplicate).value_string, Some(vec![0xff, b'x']));
    arena.get_mut(duplicate).value_string = Some(b"changed".to_vec());
    assert_eq!(arena.get(source).value_string, Some(vec![0xff, b'x']));
}

#[test]
fn cyclic_reference_graph_hits_circular_limit_without_panicking() {
    let mut arena = Arena::new();
    let source = arena.create_array();
    let shared_child = arena.create_array();
    assert!(arena.add_item_to_array(source, shared_child));
    let reference = arena.create_array_reference(source);
    arena.get_mut(shared_child).child = Some(reference);

    assert_eq!(CJSON_CIRCULAR_LIMIT, 10_000);
    assert_eq!(arena.duplicate(source, true), None);
    assert!(!arena.is_deleted(source));
    assert_eq!(arena.get(source).child, Some(shared_child));
}
