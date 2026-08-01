use rjson::Arena;

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).expect("test output is valid UTF-8")
}

#[test]
fn print_and_unformatted_have_the_same_content_and_different_whitespace() {
    let mut arena = Arena::new();
    let object = arena.create_object();
    arena.add_string_to_object(object, b"name".to_vec(), b"Ada".to_vec());
    let values = arena.create_array();
    let one = arena.create_number(1.0);
    let truth = arena.create_true();
    arena.append_child(values, one, None);
    arena.append_child(values, truth, None);
    arena.append_child(object, values, Some(b"values".to_vec()));

    let formatted = text(arena.print(object).unwrap());
    let unformatted = text(arena.print_unformatted(object).unwrap());
    assert_eq!(formatted, "{\n\t\"name\":\t\"Ada\",\n\t\"values\":\t[1, true]\n}");
    assert_eq!(unformatted, "{\"name\":\"Ada\",\"values\":[1,true]}");
    assert!(formatted.contains("Ada"));
    assert!(formatted.contains("values"));
}

#[test]
fn print_buffered_rejects_negative_hints_but_zero_still_grows() {
    let mut arena = Arena::new();
    let string = arena.create_string(b"a sufficiently long value".to_vec());

    assert_eq!(arena.print_buffered(string, -1, false), None);
    assert_eq!(
        arena.print_buffered(string, 0, false),
        Some(b"\"a sufficiently long value\"".to_vec())
    );
}

#[test]
fn print_preallocated_requires_capacity_but_does_not_partially_write_on_failure() {
    let mut arena = Arena::new();
    let string = arena.create_string(b"hello".to_vec());
    let expected = b"\"hello\"";

    let mut exact = vec![0; expected.len()];
    assert!(arena.print_preallocated(string, &mut exact, false));
    assert_eq!(exact, expected);

    let mut too_small = vec![0xaa; expected.len() - 1];
    assert!(!arena.print_preallocated(string, &mut too_small, false));
    assert_eq!(too_small, vec![0xaa; expected.len() - 1]);
}
