use rjson::Arena;

fn two_item_array(arena: &mut Arena) -> (rjson::NodeId, rjson::NodeId, rjson::NodeId) {
    let array = arena.create_array();
    let first = arena.create_number(1.0);
    let second = arena.create_number(2.0);
    assert!(arena.add_item_to_array(array, first));
    assert!(arena.add_item_to_array(array, second));
    (array, first, second)
}

#[test]
fn insert_item_shifts_existing_item_and_appends_at_length() {
    let mut arena = Arena::new();
    let (array, first, second) = two_item_array(&mut arena);
    let inserted = arena.create_number(1.5);
    let appended = arena.create_number(3.0);

    assert!(arena.insert_item_in_array(array, 1, inserted));
    assert_eq!(arena.get(first).next, Some(inserted));
    assert_eq!(arena.get(inserted).prev, Some(first));
    assert_eq!(arena.get(inserted).next, Some(second));
    assert_eq!(arena.get(second).prev, Some(inserted));

    assert!(arena.insert_item_in_array(array, 3, appended));
    assert_eq!(arena.get(second).next, Some(appended));
    assert_eq!(arena.get(appended).prev, Some(second));
    assert_eq!(arena.get(appended).next, None);
}

#[test]
fn insert_rejects_invalid_item_without_mutating_array() {
    let mut arena = Arena::new();
    let (array, first, second) = two_item_array(&mut arena);

    assert!(!arena.insert_item_in_array(array, 0, array));

    assert_eq!(arena.get(array).child, Some(first));
    assert_eq!(arena.get(first).next, Some(second));
    assert_eq!(arena.get(second).prev, Some(first));
}

#[test]
fn replace_head_relinks_parent_and_deletes_old_item() {
    let mut arena = Arena::new();
    let (array, old_head, tail) = two_item_array(&mut arena);
    let replacement = arena.create_number(10.0);

    assert!(arena.replace_item_via_pointer(array, old_head, replacement));

    assert_eq!(arena.get(array).child, Some(replacement));
    assert_eq!(arena.get(replacement).prev, None);
    assert_eq!(arena.get(replacement).next, Some(tail));
    assert_eq!(arena.get(tail).prev, Some(replacement));
    assert!(arena.is_deleted(old_head));
}

#[test]
fn replace_tail_relinks_predecessor_and_deletes_old_item() {
    let mut arena = Arena::new();
    let (array, head, old_tail) = two_item_array(&mut arena);
    let replacement = arena.create_number(10.0);

    assert!(arena.replace_item_in_array(array, 1, replacement));

    assert_eq!(arena.get(head).next, Some(replacement));
    assert_eq!(arena.get(replacement).prev, Some(head));
    assert_eq!(arena.get(replacement).next, None);
    assert!(arena.is_deleted(old_tail));
}

#[test]
fn replace_only_child_keeps_a_single_free_standing_child() {
    let mut arena = Arena::new();
    let array = arena.create_array();
    let old = arena.create_null();
    let replacement = arena.create_true();
    assert!(arena.add_item_to_array(array, old));

    assert!(arena.replace_item_via_pointer(array, old, replacement));

    assert_eq!(arena.get(array).child, Some(replacement));
    assert_eq!(arena.get(replacement).prev, None);
    assert_eq!(arena.get(replacement).next, None);
    assert!(arena.is_deleted(old));
}

#[test]
fn replacing_item_with_itself_keeps_tree_unchanged() {
    let mut arena = Arena::new();
    let (array, head, tail) = two_item_array(&mut arena);

    assert!(arena.replace_item_via_pointer(array, head, head));

    assert_eq!(arena.get(array).child, Some(head));
    assert_eq!(arena.get(head).next, Some(tail));
    assert_eq!(arena.get(tail).prev, Some(head));
    assert!(!arena.is_deleted(head));
}

#[test]
fn object_replacement_uses_lookup_key_and_case_mode() {
    let mut arena = Arena::new();
    let object = arena.create_object();
    let old_mixed = arena.create_null();
    let old_exact = arena.create_false();
    assert!(arena.add_item_to_object(object, b"MiXeD".to_vec(), old_mixed));
    assert!(arena.add_item_to_object(object, b"Exact".to_vec(), old_exact));

    let default_replacement = arena.create_true();
    assert!(arena.replace_item_in_object(object, b"mixed", default_replacement));
    assert!(arena.is_deleted(old_mixed));
    assert_eq!(arena.get(default_replacement).key, Some(b"mixed".to_vec()));

    let exact_replacement = arena.create_number(2.0);
    assert!(arena.replace_item_in_object_case_sensitive(object, b"Exact", exact_replacement));
    assert!(arena.is_deleted(old_exact));
    assert_eq!(arena.get(exact_replacement).key, Some(b"Exact".to_vec()));
}
