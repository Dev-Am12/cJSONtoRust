use rjson::Arena;

#[test]
fn equal_simple_values_compare_equal() {
    let mut arena = Arena::new();

    let null_a = arena.create_null();
    let null_b = arena.create_null();
    let true_a = arena.create_true();
    let true_b = arena.create_true();
    let false_a = arena.create_false();
    let false_b = arena.create_false();
    let number_a = arena.create_number(1.0);
    let number_b = arena.create_number(1.0 + f64::EPSILON);
    let string_a = arena.create_string(vec![0xff, b'x']);
    let string_b = arena.create_string(vec![0xff, b'x']);
    let raw_a = arena.create_raw(b"raw".to_vec());
    let raw_b = arena.create_raw(b"raw".to_vec());

    assert!(arena.compare(null_a, null_b, true));
    assert!(arena.compare(true_a, true_b, true));
    assert!(arena.compare(false_a, false_b, true));
    assert!(arena.compare(number_a, number_b, true));
    assert!(arena.compare(string_a, string_b, true));
    assert!(arena.compare(raw_a, raw_b, true));
}

#[test]
fn type_mismatches_and_different_raw_bytes_are_not_equal() {
    let mut arena = Arena::new();

    let true_node = arena.create_true();
    let false_node = arena.create_false();
    let number = arena.create_number(1.0);
    let string = arena.create_string(b"1".to_vec());
    let raw_a = arena.create_raw(vec![b'a', 0, b'b']);
    let raw_b = arena.create_raw(vec![b'a', 0, b'c']);

    assert!(!arena.compare(true_node, false_node, true));
    assert!(!arena.compare(number, string, true));
    assert!(!arena.compare(raw_a, raw_b, true));
}

#[test]
fn array_comparison_is_order_sensitive() {
    let mut arena = Arena::new();
    let first = arena.create_array();
    let first_one = arena.create_number(1.0);
    let first_two = arena.create_number(2.0);
    assert!(arena.add_item_to_array(first, first_one));
    assert!(arena.add_item_to_array(first, first_two));

    let second = arena.create_array();
    let second_two = arena.create_number(2.0);
    let second_one = arena.create_number(1.0);
    assert!(arena.add_item_to_array(second, second_two));
    assert!(arena.add_item_to_array(second, second_one));

    assert!(!arena.compare(first, second, true));
}

#[test]
fn object_comparison_ignores_key_order() {
    let mut arena = Arena::new();
    let first = arena.create_object();
    let first_a = arena.create_number(1.0);
    let first_b = arena.create_number(2.0);
    assert!(arena.add_item_to_object(first, b"a".to_vec(), first_a));
    assert!(arena.add_item_to_object(first, b"b".to_vec(), first_b));

    let second = arena.create_object();
    let second_b = arena.create_number(2.0);
    let second_a = arena.create_number(1.0);
    assert!(arena.add_item_to_object(second, b"b".to_vec(), second_b));
    assert!(arena.add_item_to_object(second, b"a".to_vec(), second_a));

    assert!(arena.compare(first, second, true));
}

#[test]
fn object_key_case_flag_does_not_affect_string_values() {
    let mut arena = Arena::new();
    let first = arena.create_object();
    let first_value = arena.create_string(b"Value".to_vec());
    assert!(arena.add_item_to_object(first, b"Name".to_vec(), first_value));

    let second = arena.create_object();
    let second_value = arena.create_string(b"Value".to_vec());
    assert!(arena.add_item_to_object(second, b"name".to_vec(), second_value));

    assert!(!arena.compare(first, second, true));
    assert!(arena.compare(first, second, false));

    arena.get_mut(second_value).value_string = Some(b"value".to_vec());
    assert!(!arena.compare(first, second, false));
}

#[test]
fn duplicate_keys_follow_first_match_behavior_without_counting_multiplicity() {
    let mut arena = Arena::new();
    let first = arena.create_object();
    let first_one = arena.create_number(1.0);
    let first_two = arena.create_number(1.0);
    assert!(arena.add_item_to_object(first, b"x".to_vec(), first_one));
    assert!(arena.add_item_to_object(first, b"x".to_vec(), first_two));

    let second = arena.create_object();
    let second_one = arena.create_number(1.0);
    assert!(arena.add_item_to_object(second, b"x".to_vec(), second_one));

    assert!(arena.compare(first, second, true));
}
