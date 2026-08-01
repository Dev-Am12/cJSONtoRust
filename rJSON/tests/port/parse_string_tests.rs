// Black-box re-expression of `tests/original/parse_string.c`'s intent
// against the public `Parser`/`Arena` API, per `DECISIONS.md` #2 (the
// original file's `assert_parse_string`/`assert_not_parse_string`
// helpers call the internal, static `parse_string` directly, which has
// no equivalent shape in this port -- string parsing here is reached
// only through `Parser::parse_value`).
//
// Every case in this file is drawn from either an original assertion in
// `tests/original/parse_string.c` or a specific behavioral requirement
// from this task (Unicode escapes, surrogate pairs, raw UTF-8
// passthrough for invalid byte sequences). See `rJSON/DECISIONS_personal.md`
// for the entry documenting this implementation.

use rjson::{Arena, NodeType, Parser};

fn parse_string_value(json_bytes: &[u8]) -> Option<Vec<u8>> {
    let mut arena = Arena::new();
    let mut parser = Parser::new(json_bytes, &mut arena);
    match parser.parse_value() {
        Ok(id) => {
            let node = arena.get(id);
            assert_eq!(node.node_type, NodeType::String, "expected a String node");
            node.value_string.clone()
        }
        Err(_) => None,
    }
}

fn assert_parse_string(json_bytes: &[u8], expected: &[u8]) {
    let result = parse_string_value(json_bytes);
    assert_eq!(
        result,
        Some(expected.to_vec()),
        "parsing {:?} did not produce the expected bytes",
        String::from_utf8_lossy(json_bytes)
    );
}

fn assert_not_parse_string(json_bytes: &[u8]) {
    let mut arena = Arena::new();
    let mut parser = Parser::new(json_bytes, &mut arena);
    assert!(
        parser.parse_value().is_err(),
        "expected parse failure for {:?}",
        String::from_utf8_lossy(json_bytes)
    );
}

/// Mirrors `parse_string_should_parse_strings`' empty-string case.
#[test]
fn parses_empty_string() {
    assert_parse_string(b"\"\"", b"");
}

/// Mirrors `parse_string_should_parse_strings`' printable-ASCII case,
/// including the `\/` escape and literal `/`.
#[test]
fn parses_printable_ascii_and_slash_escape() {
    let input: &[u8] = b"\" !\\\"#$%&'()*+,-./\\/0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\\\]^_'abcdefghijklmnopqrstuvwxyz{|}~\"";
    let expected: &[u8] = b" !\"#$%&'()*+,-.//0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_'abcdefghijklmnopqrstuvwxyz{|}~";
    assert_parse_string(input, expected);
}

/// Mirrors `parse_string_should_parse_strings`' combined case: every
/// single-character escape plus two `\uXXXX` escapes (one BMP symbol,
/// one BMP CJK character -- neither requires a surrogate pair).
#[test]
fn parses_all_simple_escapes_and_bmp_unicode_escapes() {
    let input: &[u8] = b"\"\\\"\\\\\\/\\b\\f\\n\\r\\t\\u20AC\\u732b\"";
    let mut expected: Vec<u8> = vec![b'"', b'\\', b'/', 0x08, 0x0C, b'\n', b'\r', b'\t'];
    expected.extend_from_slice("\u{20AC}".as_bytes()); // €
    expected.extend_from_slice("\u{732b}".as_bytes()); // 猫
    assert_parse_string(input, &expected);
}

/// Mirrors `parse_string_should_parse_strings`' bare-control-escapes case.
#[test]
fn parses_bare_control_escapes() {
    assert_parse_string(b"\"\\b\\f\\n\\r\\t\"", &[0x08, 0x0C, b'\n', b'\r', b'\t']);
}

/// Mirrors `parse_string_should_parse_utf16_surrogate_pairs`.
#[test]
fn parses_utf16_surrogate_pair() {
    assert_parse_string(b"\"\\uD83D\\udc31\"", "\u{1F431}".as_bytes()); // 🐱
}

/// Mirrors `parse_string_should_not_parse_non_strings`.
#[test]
fn rejects_input_without_leading_quote() {
    assert_not_parse_string(b"this\" is not a string\"");
}

/// Mirrors `parse_string_should_not_parse_non_strings`' empty-input case.
#[test]
fn rejects_empty_input() {
    assert_not_parse_string(b"");
}

#[test]
fn rejects_unterminated_string() {
    assert_not_parse_string(b"\"abc");
}

#[test]
fn rejects_trailing_backslash_at_end_of_input() {
    assert_not_parse_string(b"\"abc\\");
}

#[test]
fn rejects_unknown_escape_character() {
    assert_not_parse_string(b"\"\\q\"");
}

#[test]
fn rejects_lone_high_surrogate() {
    assert_not_parse_string(b"\"\\uD800\"");
}

#[test]
fn rejects_lone_low_surrogate() {
    assert_not_parse_string(b"\"\\uDC00\"");
}

#[test]
fn rejects_high_surrogate_not_followed_by_low_surrogate() {
    assert_not_parse_string(b"\"\\uD800\\u0041\"");
}

#[test]
fn rejects_truncated_unicode_escape() {
    assert_not_parse_string(b"\"\\u12\"");
}

/// Documents a real upstream quirk (`parse_hex4` returning `0` on an
/// invalid hex digit instead of failing, see `rJSON/DECISIONS_personal.md`):
/// `\u00zz` is indistinguishable from a literal `\u0000` upstream, and
/// this port reproduces that exactly rather than "fixing" it into a
/// parse failure.
#[test]
fn invalid_hex_digit_collapses_to_u0000_matching_upstream_quirk() {
    assert_parse_string(b"\"\\u00zz\"", &[0u8]);
}

/// Requirement 5: invalid UTF-8 / malformed byte sequences in the raw
/// (non-escaped) string content must be preserved byte-for-byte, not
/// rejected -- matches upstream's raw passthrough of `*input_pointer`
/// with no validation.
#[test]
fn preserves_invalid_utf8_raw_bytes_unmodified() {
    let input: Vec<u8> = vec![b'"', 0xFF, 0xFE, 0xC0, b'"'];
    let expected: Vec<u8> = vec![0xFF, 0xFE, 0xC0];
    assert_parse_string(&input, &expected);
}

/// Mirrors `parse_string_should_parse_bug_94`: a real upstream regression
/// fixture with deeply nested backslash escapes (LDAP distinguished-name
/// style content). Input/expected bytes were derived by mechanically
/// decoding the original C string literal's own escaping (`\\` -> `\`,
/// `\"` -> `"`) and independently verified by simulating cJSON's escape
/// table against the result before transcribing here.
#[test]
fn parses_bug_94_nested_backslash_escapes() {
    assert_parse_string(
        b"\"~!@\\\\#$%^&*()\\\\\\\\-\\\\+{}[]:\\\\;\\\\\\\"\\\\<\\\\>?/.,DC=ad,DC=com\"",
        b"~!@\\#$%^&*()\\\\-\\+{}[]:\\;\\\"\\<\\>?/.,DC=ad,DC=com",
    );
}

/// Confirms the offset lands one past the closing quote, matching
/// `input_buffer->offset = (input_end - content) + 1`.
#[test]
fn offset_advances_past_closing_quote() {
    let mut arena = Arena::new();
    let mut parser = Parser::new(b"\"ab\" ", &mut arena);
    let id = parser.parse_value().unwrap();
    assert_eq!(parser.current_offset(), 4);
    assert_eq!(arena.get(id).value_string.clone().unwrap(), b"ab".to_vec());
}