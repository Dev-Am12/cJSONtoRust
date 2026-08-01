use std::panic::{AssertUnwindSafe, catch_unwind};

use rjson::Arena;

#[test]
fn deleting_a_tree_marks_owned_descendants_and_clears_owned_content() {
    let mut arena = Arena::new();
    let root = arena.create_array();
    let child = arena.create_object();
    let grandchild = arena.create_string(b"value".to_vec());

    arena.append_child(root, child, None);
    arena.append_child(child, grandchild, Some(b"key".to_vec()));
    arena.delete(root);

    for id in [root, child, grandchild] {
        assert!(arena.is_deleted(id));
    }
    assert_eq!(arena.get(root).child, None);
    assert_eq!(arena.get(child).child, None);
    assert_eq!(arena.get(grandchild).value_string, None);
    assert_eq!(arena.get(grandchild).key, None);
}

#[test]
fn deleting_reference_does_not_delete_shared_children() {
    let mut arena = Arena::new();
    let source = arena.create_array();
    let child = arena.create_string(b"shared".to_vec());
    arena.append_child(source, child, None);
    let reference = arena.create_array_reference(source);

    arena.delete(reference);

    assert!(arena.is_deleted(reference));
    assert!(!arena.is_deleted(source));
    assert!(!arena.is_deleted(child));
    assert_eq!(arena.get(source).child, Some(child));
    assert_eq!(arena.get(child).value_string, Some(b"shared".to_vec()));
}

#[test]
fn deleting_string_reference_retains_non_owned_value() {
    let mut arena = Arena::new();
    let reference = arena.create_string_reference(b"shared".to_vec());

    arena.delete(reference);

    assert!(arena.is_deleted(reference));
    assert_eq!(arena.get(reference).value_string, Some(b"shared".to_vec()));
}

#[test]
fn deleting_the_same_node_twice_is_safe() {
    let mut arena = Arena::new();
    let node = arena.create_string(b"value".to_vec());

    arena.delete(node);
    let result = catch_unwind(AssertUnwindSafe(|| arena.delete(node)));

    assert!(result.is_ok());
    assert!(arena.is_deleted(node));
    assert_eq!(arena.get(node).value_string, None);
}
