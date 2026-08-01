// Black-box tests for the new top-level `cjson_parse` / `cjson_parse_with_opts`
// / `cjson_parse_with_length_opts` public wrappers, equivalent in behavior to
// upstream's `cJSON_Parse` / `cJSON_ParseWithOpts` / `cJSON_ParseWithLengthOpts`
// (`cJSON.c` lines ~1131-1235).
//
// Note on scope: `tests/original/parse_with_opts.c` is *not* one of the six
// files this task asked to examine (`parse_number.c`, `parse_string.c`,
// `parse_hex4.c`, `parse_array.c`, `parse_object.c`, `parse_value.c`) --
// `DECISIONS.md` #2 already lists it as one of the ~6 adapter-eligible,
// public-API-only original files. It is used here only as a source of
// well-known upstream scenarios for testing the wrappers this task *does*
// require, since it is the file that originally tested these exact three
// functions. See `rJSON/DECISIONS_personal.md` for the entry documenting
// this implementation, including a bugfix in `Parser::error_offset` found
// while writing the `handles_incomplete_json_...` test below.

use rjson::{cjson_parse, cjson_parse_with_length_opts, cjson_parse_with_opts, Arena, NodeType};

/// Mirrors `parse_with_opts_should_handle_empty_strings`: an empty input
/// fails, with the reported error position at `0`.
#[test]
fn empty_input_fails_at_position_zero() {
    let mut arena = Arena::new();
    let err = cjson_parse_with_opts(&mut arena, b"", false).unwrap_err();
    assert_eq!(err.position, 0);

    let err = cjson_parse(&mut arena, b"").unwrap_err();
    assert_eq!(err.position, 0);
}

/// Mirrors `parse_with_opts_should_handle_incomplete_json`: a parse that
/// runs out of input while expecting a value reports the error position at
/// the very end of input (`json + strlen(json)` upstream) -- not one byte
/// short of it. This is the scenario that surfaced the off-by-one bug fixed
/// in `Parser::error_offset` (see `DECISIONS_personal.md`).
#[test]
fn handles_incomplete_json_error_position_is_end_of_input() {
    let json = b"{ \"name\": ";
    let mut arena = Arena::new();
    let err = cjson_parse_with_opts(&mut arena, json, false).unwrap_err();
    assert_eq!(err.position, json.len());
}

/// Mirrors `parse_with_opts_should_require_null_if_requested`: trailing
/// whitespace is fine under `require_null_terminated`, trailing non-whitespace
/// content is not.
#[test]
fn require_null_terminated_allows_trailing_whitespace_only() {
    let mut arena = Arena::new();
    assert!(cjson_parse_with_opts(&mut arena, b"{}", true).is_ok());

    let mut arena = Arena::new();
    assert!(cjson_parse_with_opts(&mut arena, b"{} \n", true).is_ok());

    let mut arena = Arena::new();
    assert!(cjson_parse_with_opts(&mut arena, b"{}x", true).is_err());
}

/// Mirrors `parse_with_opts_should_return_parse_end`: on success, the
/// reported parse-end offset points just past the parsed value, not the end
/// of the whole input.
#[test]
fn returns_parse_end_just_past_the_value() {
    let json = b"[] empty array XD";
    let mut arena = Arena::new();
    let (id, parse_end) = cjson_parse_with_opts(&mut arena, json, false).unwrap();
    assert_eq!(parse_end, 2);
    assert_eq!(arena.get(id).node_type, NodeType::Array);
}

/// Mirrors `parse_with_opts_should_parse_utf8_bom`: a leading UTF-8 BOM is
/// skipped and has no effect on the parsed result.
#[test]
fn parses_and_skips_utf8_bom() {
    let mut arena_with_bom = Arena::new();
    let (with_bom, _) =
        cjson_parse_with_opts(&mut arena_with_bom, b"\xEF\xBB\xBF{}", true).unwrap();
    assert_eq!(arena_with_bom.get(with_bom).node_type, NodeType::Object);
    assert_eq!(arena_with_bom.get(with_bom).child, None);

    let mut arena_without_bom = Arena::new();
    let (without_bom, _) = cjson_parse_with_opts(&mut arena_without_bom, b"{}", true).unwrap();
    assert_eq!(arena_without_bom.get(without_bom).node_type, NodeType::Object);
    assert_eq!(arena_without_bom.get(without_bom).child, None);
}

/// Not present in `parse_with_opts.c`, but a direct requirement of this
/// task: `cJSON_ParseWithOpts` computes its length via a NUL-stopped scan
/// (`strlen`), so an embedded NUL byte in `value` truncates what gets
/// parsed, while `cJSON_ParseWithLengthOpts` (given the full slice
/// explicitly) parses straight through it. Uses a raw NUL inside what would
/// otherwise be a string body to make the divergence observable.
#[test]
fn with_opts_stops_at_embedded_nul_but_with_length_opts_does_not() {
    let mut value = b"[1,2]".to_vec();
    value.push(0);
    value.extend_from_slice(b",3]"); // "[1,2]\0,3]"

    // cJSON_ParseWithOpts-equivalent: strlen-style truncation at the NUL
    // means only "[1,2]" is ever seen, which parses successfully as a
    // 2-element array.
    let mut arena = Arena::new();
    let (id, parse_end) = cjson_parse_with_opts(&mut arena, &value, false).unwrap();
    assert_eq!(arena.get(id).node_type, NodeType::Array);
    assert_eq!(parse_end, 5); // stops right after the first "]"

    // cJSON_ParseWithLengthOpts-equivalent: the full slice (including the
    // embedded NUL and what follows) is in play. Nothing in `parse_value`
    // treats a NUL byte as meaningful JSON syntax, so parsing "[1,2]" still
    // succeeds and still stops after the first "]" -- the point being that
    // it *can* see the trailing content (e.g. under `require_null_terminated`,
    // where it's treated as trailing garbage and fails) whereas
    // `cjson_parse_with_opts` never gets the chance to look past the NUL.
    //
    // The embedded NUL byte (0x00) is itself `<= 32`, so upstream's (and
    // this port's) whitespace definition swallows it too -- the first
    // *non*-whitespace trailing byte `require_null_terminated` actually
    // reports is the comma one position after it.
    let mut arena = Arena::new();
    let err = cjson_parse_with_length_opts(&mut arena, &value, true).unwrap_err();
    assert_eq!(err.position, 6);
}

/// Basic success/failure smoke coverage for `cjson_parse` (the
/// `cJSON_Parse`-equivalent entry point) across each `parse_value` branch,
/// complementing the dedicated dispatch coverage in `parse_value_tests.rs`.
#[test]
fn cjson_parse_smoke_test_across_value_types() {
    for (input, expected_type) in [
        (&b"null"[..], NodeType::Null),
        (&b"true"[..], NodeType::True),
        (&b"false"[..], NodeType::False),
        (&b"1.5"[..], NodeType::Number),
        (&b"\"hello\""[..], NodeType::String),
        (&b"[1,2,3]"[..], NodeType::Array),
        (&b"{\"a\":1}"[..], NodeType::Object),
    ] {
        let mut arena = Arena::new();
        let id = cjson_parse(&mut arena, input)
            .unwrap_or_else(|_| panic!("expected {:?} to parse", String::from_utf8_lossy(input)));
        assert_eq!(arena.get(id).node_type, expected_type);
    }

    let mut arena = Arena::new();
    assert!(cjson_parse(&mut arena, b"not json").is_err());
}
