use rjson::{Arena, NodeType};

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).expect("test output is valid UTF-8")
}

#[test]
fn prints_each_json_type() {
    let mut arena = Arena::new();
    let null = arena.create_null();
    let truth = arena.create_true();
    let falsehood = arena.create_false();
    let number = arena.create_number(12.5);
    let string = arena.create_string(b"hello".to_vec());
    let raw = arena.create_raw(b"{raw}".to_vec());

    assert_eq!(arena.get(null).node_type, NodeType::Null);
    assert_eq!(text(arena.print_value(null, false).unwrap()), "null");
    assert_eq!(text(arena.print_value(truth, false).unwrap()), "true");
    assert_eq!(text(arena.print_value(falsehood, false).unwrap()), "false");
    assert_eq!(text(arena.print_value(number, false).unwrap()), "12.5");
    assert_eq!(text(arena.print_value(string, false).unwrap()), "\"hello\"");
    assert_eq!(text(arena.print_value(raw, false).unwrap()), "{raw}");
}

#[test]
fn string_escapes_named_and_generic_controls_without_decoding_bytes() {
    let mut arena = Arena::new();
    let string = arena.create_string(vec![
        b'"', b'\\', 0x08, 0x0c, b'\n', b'\r', b'\t', 0x00, 0x01, 0x1f, 0x80,
    ]);

    assert_eq!(
        arena.print_string(string).unwrap(),
        b"\"\\\"\\\\\\b\\f\\n\\r\\t\\u0000\\u0001\\u001f\x80\""
    );
}

#[test]
fn prints_arrays_objects_and_nested_values() {
    let mut arena = Arena::new();
    let array = arena.create_array();
    let object = arena.create_object();
    let nested = arena.create_string(b"value".to_vec());
    arena.append_child(object, nested, Some(b"key".to_vec()));
    arena.append_child(array, object, None);
    let truth = arena.create_true();
    arena.append_child(array, truth, None);

    assert_eq!(
        text(arena.print_value(array, false).unwrap()),
        "[{\"key\":\"value\"},true]"
    );
}

#[test]
fn pretty_and_unformatted_preserve_content_and_order() {
    let mut arena = Arena::new();
    let object = arena.create_object();
    arena.add_string_to_object(object, b"name".to_vec(), b"Ada".to_vec());
    let items = arena.create_array();
    let number = arena.create_number(1.0);
    let truth = arena.create_true();
    arena.append_child(items, number, None);
    arena.append_child(items, truth, None);
    arena.append_child(object, items, Some(b"items".to_vec()));

    let unformatted = text(arena.print_object(object, false).unwrap());
    let pretty = text(arena.print_object(object, true).unwrap());
    assert_eq!(unformatted, "{\"name\":\"Ada\",\"items\":[1,true]}");
    assert_eq!(pretty, "{\n\t\"name\":\t\"Ada\",\n\t\"items\":\t[1, true]\n}");
}
