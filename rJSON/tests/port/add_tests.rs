use rjson::{Arena, NodeId, NodeType};

#[test]
fn adds_item_to_array() {
    let mut arena = Arena::new();
    let array = arena.create_array();
    let item = arena.create_number(3.5);

    assert!(arena.add_item_to_array(array, item));
    assert_eq!(arena.get(array).child, Some(item));
    assert_eq!(arena.get(item).key, None);
    assert_eq!(arena.get(item).node_type, NodeType::Number);
}

#[test]
fn adds_keyed_item_to_object() {
    let mut arena = Arena::new();
    let object = arena.create_object();
    let item = arena.create_string(vec![0xff, b'x']);

    assert!(arena.add_item_to_object(object, b"name".to_vec(), item));
    assert_eq!(arena.get(object).child, Some(item));
    assert_eq!(arena.get(item).key, Some(b"name".to_vec()));
    assert_eq!(arena.get(item).value_string, Some(vec![0xff, b'x']));
}

#[test]
fn add_number_to_object_creates_and_adds_node() {
    let mut arena = Arena::new();
    let object = arena.create_object();

    let item = arena
        .add_number_to_object(object, b"answer".to_vec(), 42.0)
        .expect("adding to a live object succeeds");

    assert_eq!(arena.get(object).child, Some(item));
    assert_eq!(arena.get(item).node_type, NodeType::Number);
    assert_eq!(arena.get(item).value_double, 42.0);
    assert_eq!(arena.get(item).key, Some(b"answer".to_vec()));
}

#[test]
fn duplicate_object_keys_are_accepted() {
    let mut arena = Arena::new();
    let object = arena.create_object();
    let first = arena.create_null();
    let second = arena.create_true();

    assert!(arena.add_item_to_object(object, b"duplicate".to_vec(), first));
    assert!(arena.add_item_to_object(object, b"duplicate".to_vec(), second));

    assert_eq!(arena.get(first).key, Some(b"duplicate".to_vec()));
    assert_eq!(arena.get(second).key, Some(b"duplicate".to_vec()));
    assert_eq!(arena.get(first).next, Some(second));
    assert_eq!(arena.get(second).prev, Some(first));
}

#[test]
fn adding_self_or_invalid_id_returns_false() {
    let mut arena = Arena::new();
    let array = arena.create_array();

    assert!(!arena.add_item_to_array(array, array));
    assert!(!arena.add_item_to_array(array, NodeId(999)));
}
