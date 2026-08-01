use rjson::Arena;

fn three_item_array(arena: &mut Arena) -> (rjson::NodeId, rjson::NodeId, rjson::NodeId, rjson::NodeId) {
    let array = arena.create_array();
    let first = arena.create_null();
    let second = arena.create_true();
    let third = arena.create_false();
    assert!(arena.add_item_to_array(array, first));
    assert!(arena.add_item_to_array(array, second));
    assert!(arena.add_item_to_array(array, third));
    (array, first, second, third)
}

#[test]
fn detaching_middle_child_relinks_neighbors() {
    let mut arena = Arena::new();
    let (array, first, middle, last) = three_item_array(&mut arena);

    assert_eq!(arena.detach_item_via_pointer(array, middle), Some(middle));

    assert_eq!(arena.get(array).child, Some(first));
    assert_eq!(arena.get(first).next, Some(last));
    assert_eq!(arena.get(last).prev, Some(first));
    assert_eq!(arena.get(middle).next, None);
    assert_eq!(arena.get(middle).prev, None);
    assert_eq!(arena.get(middle).key, None);
}

#[test]
fn detaching_head_updates_parent_child() {
    let mut arena = Arena::new();
    let (array, first, second, _) = three_item_array(&mut arena);

    assert_eq!(arena.detach_item_via_pointer(array, first), Some(first));

    assert_eq!(arena.get(array).child, Some(second));
    assert_eq!(arena.get(second).prev, None);
}

#[test]
fn detaching_missing_item_index_or_key_has_no_side_effects() {
    let mut arena = Arena::new();
    let (array, first, second, _) = three_item_array(&mut arena);
    let unrelated = arena.create_null();
    let object = arena.create_object();
    let object_child = arena.create_null();
    assert!(arena.add_item_to_object(object, b"present".to_vec(), object_child));

    assert_eq!(arena.detach_item_via_pointer(array, unrelated), None);
    assert_eq!(arena.detach_item_from_array(array, 9), None);
    assert_eq!(arena.detach_item_from_object(object, b"absent"), None);

    assert_eq!(arena.get(array).child, Some(first));
    assert_eq!(arena.get(first).next, Some(second));
    assert_eq!(arena.get(object).child, Some(object_child));
    assert_eq!(arena.get(object_child).key, Some(b"present".to_vec()));
}

#[test]
fn delete_from_array_detaches_then_deletes_only_the_item() {
    let mut arena = Arena::new();
    let (array, first, middle, last) = three_item_array(&mut arena);

    assert!(arena.delete_item_from_array(array, 1));

    assert!(arena.is_deleted(middle));
    assert!(!arena.is_deleted(first));
    assert!(!arena.is_deleted(last));
    assert_eq!(arena.get(first).next, Some(last));
    assert_eq!(arena.get(last).prev, Some(first));
}

#[test]
fn object_lookup_is_case_insensitive_by_default_and_exact_when_requested() {
    let mut arena = Arena::new();
    let object = arena.create_object();
    let child = arena.create_number(1.0);
    assert!(arena.add_item_to_object(object, b"MiXeD".to_vec(), child));

    assert_eq!(
        arena.detach_item_from_object_case_sensitive(object, b"mixed"),
        None
    );
    assert_eq!(arena.detach_item_from_object(object, b"mixed"), Some(child));
    assert_eq!(arena.get(object).child, None);
}
