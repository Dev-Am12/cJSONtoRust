use rjson::minify;

#[test]
fn minifies_reference_line_comment_case() {
    let input = br#"{
  // this is a comment
  "a": 1
}"#;
    assert_eq!(minify(input), br#"{"a":1}"#);
}

#[test]
fn minifies_reference_block_comment_case() {
    let input = br#"{
  /* block comment */
  "a": 1,
  "b": 2
}"#;
    assert_eq!(minify(input), br#"{"a":1,"b":2}"#);
}

#[test]
fn preserves_whitespace_and_comment_markers_inside_strings() {
    let input = br#" { "a" : "hello world // not a comment /* nor this */" } "#;
    assert_eq!(
        minify(input),
        br#"{"a":"hello world // not a comment /* nor this */"}"#
    );
}

#[test]
fn escaped_quote_does_not_end_string_tracking() {
    let input = br#"{"a": "escaped \" quote // still text", "b": 1}"#;
    assert_eq!(
        minify(input),
        br#"{"a":"escaped \" quote // still text","b":1}"#
    );
}

#[test]
fn unterminated_inputs_stop_cleanly_like_cjson() {
    assert_eq!(minify(br#" { "a": "unfinished  "#), br#"{"a":"unfinished  "#);
    assert_eq!(minify(br#" { /* unfinished comment"#), br#"{"#);
    assert_eq!(minify(br#" { // unfinished comment"#), br#"{"#);
}
