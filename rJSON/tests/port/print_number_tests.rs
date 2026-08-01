use rjson::Arena;

fn number(arena: &mut Arena, value: f64) -> String {
    let id = arena.create_number(value);
    arena.print_number(id).expect("live number prints")
}

#[test]
fn whole_numbers_use_the_valueint_shortcut() {
    let mut arena = Arena::new();

    assert_eq!(number(&mut arena, 42.0), "42");
    assert_eq!(number(&mut arena, -42.0), "-42");
}

#[test]
fn fifteen_digit_format_round_trips_cleanly() {
    let mut arena = Arena::new();

    assert_eq!(number(&mut arena, 1.23456789012345), "1.23456789012345");
}

#[test]
fn confirmed_reference_case_uses_seventeen_digit_fallback() {
    let mut arena = Arena::new();
    let input = "123456789012345678901234567890";
    let value = input.parse::<f64>().expect("reference input parses as f64");

    assert_eq!(number(&mut arena, value), "1.2345678901234568e+29");
}

#[test]
fn non_finite_values_print_as_null() {
    let mut arena = Arena::new();

    assert_eq!(number(&mut arena, f64::NAN), "null");
    assert_eq!(number(&mut arena, f64::INFINITY), "null");
    assert_eq!(number(&mut arena, f64::NEG_INFINITY), "null");
}

#[test]
fn large_integer_outside_i32_uses_g_formatting_path() {
    let mut arena = Arena::new();

    assert_eq!(number(&mut arena, 2_147_483_648.0), "2147483648");
}

#[test]
fn small_magnitude_numbers_use_two_digit_signed_exponents() {
    let mut arena = Arena::new();

    assert_eq!(number(&mut arena, 0.0000001), "1e-07");
    assert_eq!(number(&mut arena, 5e-10), "5e-10");
    assert_eq!(number(&mut arena, 1e300), "1e+300");
}
