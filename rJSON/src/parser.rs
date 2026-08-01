//! Recursive-descent parser state and value dispatcher.
//!
//! This module is the Rust equivalent of the `parse_buffer` handling in
//! upstream `cJSON.c` (`parse_value`, `parse_number`, the literal branches
//! of `parse_value`, `buffer_skip_whitespace`, and the `can_read` /
//! `can_access_at_index` macros). It intentionally does **not** implement
//! string, array, or object parsing -- see `DECISIONS_personal.md` for the
//! member-ownership split.
//!
//! Behavioral parity with upstream cJSON takes priority over idiomatic
//! Rust throughout this file, per `AI_GUARDRAILS.md` §3.

use crate::arena::{Arena, NodeId};

/// Mirrors `cJSON.h`'s `CJSON_NESTING_LIMIT` (default build value). Not
/// enforced by anything in this file yet -- `depth` exists on `Parser`
/// now so the array/object member doesn't need to change this struct's
/// layout later, but the limit check itself belongs with `parse_array`/
/// `parse_object`, which are out of scope here.
pub const CJSON_NESTING_LIMIT: usize = 1000;

/// A failed parse. Upstream cJSON reports failure as a `cJSON_bool`
/// (`false`) plus a side-channel global error position; we report it as
/// `Err(ParseError)` plus `Parser::error_offset()`, which reproduces the
/// same position. See the "Error type" entry in `DECISIONS_personal.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError;

/// Recursive-descent parser state, equivalent to cJSON's `parse_buffer`.
///
/// Fields intentionally mirror `parse_buffer`'s shape:
/// - `input` is `parse_buffer.content` (+ `.length`, via `input.len()`).
/// - `current_offset` is `parse_buffer.offset`.
/// - `depth` is `parse_buffer.depth`.
///
/// Unlike `parse_buffer`, there is no `hooks` field: allocation happens
/// through the arena, not through swappable malloc/free hooks.
pub struct Parser<'a> {
    input: &'a [u8],
    arena: &'a mut Arena,
    current_offset: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    /// Creates a parser over `input`, allocating nodes into `arena`.
    pub fn new(input: &'a [u8], arena: &'a mut Arena) -> Self {
        Parser {
            input,
            arena,
            current_offset: 0,
            depth: 0,
        }
    }

    /// The current byte offset into `input`. Equivalent to reading
    /// `parse_buffer.offset` directly.
    pub fn current_offset(&self) -> usize {
        self.current_offset
    }

    /// The current nesting depth. Equivalent to reading
    /// `parse_buffer.depth` directly. Unused until array/object parsing
    /// is implemented.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// The byte offset `cJSON_GetErrorPtr` would report right now.
    ///
    /// Reproduces the clamping `cJSON_ParseWithLengthOpts` performs in its
    /// `fail:` branch: report the offset itself if it's still a valid
    /// index into `input`, otherwise clamp to the last valid byte (or `0`
    /// for empty input). Call this immediately after `parse_value`
    /// returns `Err` for exact `cJSON_GetErrorPtr` parity.
    pub fn error_offset(&self) -> usize {
        if self.current_offset < self.input.len() {
            self.current_offset
        } else if !self.input.is_empty() {
            self.input.len() - 1
        } else {
            0
        }
    }

    /// Equivalent to cJSON's `can_read` macro: are there at least `size`
    /// more bytes available starting at `current_offset`?
    fn can_read(&self, size: usize) -> bool {
        self.current_offset + size <= self.input.len()
    }

    /// Equivalent to cJSON's `can_access_at_index` macro: is
    /// `current_offset + index` a valid index into `input`?
    fn can_access_at_index(&self, index: usize) -> bool {
        self.current_offset + index < self.input.len()
    }

    /// Equivalent to `buffer_at_offset(buffer)[index]`.
    fn byte_at(&self, index: usize) -> u8 {
        self.input[self.current_offset + index]
    }

    /// Equivalent to `buffer_at_offset(buffer)` itself: everything from
    /// the current offset to the end of input.
    fn remaining(&self) -> &[u8] {
        &self.input[self.current_offset..]
    }

    /// Equivalent to `buffer_skip_whitespace`: advances past any run of
    /// bytes `<= 32` (cJSON's definition of whitespace, which also
    /// happens to swallow other C0 control characters -- replicated
    /// exactly here, not "corrected").
    ///
    /// See `DECISIONS_personal.md` ("Whitespace skip and the missing NUL
    /// terminator") for why this does *not* include the trailing
    /// `buffer->offset--` line found in the C version: that line
    /// compensates for C's buffer length including a NUL terminator byte
    /// that a Rust `&[u8]` slice never has. Omitting it here reaches the
    /// same final offset, not a different one.
    pub fn skip_whitespace(&mut self) {
        while self.can_access_at_index(0) && self.byte_at(0) <= 32 {
            self.current_offset += 1;
        }
    }

    /// Recursive-descent value dispatcher. Equivalent to `parse_value`.
    ///
    /// Tries literals in exactly the order upstream does (`null`,
    /// `false`, `true`), then numbers. String/array/object dispatch is
    /// intentionally not implemented here (see module docs); reaching
    /// one of those leading bytes (`"`, `[`, `{`) falls through to the
    /// same `Err(ParseError)` as any other unrecognized input, exactly
    /// as it would in upstream cJSON if those branches didn't exist.
    ///
    /// Note this replicates a real upstream quirk on purpose: the literal
    /// matches are a fixed-length byte comparison with no word-boundary
    /// check, so e.g. `"nullish"` parses as `null` and leaves `"ish"`
    /// unconsumed (the caller then fails on trailing garbage, same as
    /// upstream).
    pub fn parse_value(&mut self) -> Result<NodeId, ParseError> {
        if self.can_read(4) && self.remaining().starts_with(b"null") {
            let id = self.arena.create_null();
            self.current_offset += 4;
            return Ok(id);
        }

        if self.can_read(5) && self.remaining().starts_with(b"false") {
            let id = self.arena.create_false();
            self.current_offset += 5;
            return Ok(id);
        }

        if self.can_read(4) && self.remaining().starts_with(b"true") {
            let id = self.arena.create_true();
            self.current_offset += 4;
            return Ok(id);
        }

        if self.can_access_at_index(0) && self.byte_at(0) == b'"' {
            return match self.parse_string() {
                Some(bytes) => Ok(self.arena.create_string(bytes)),
                None => Err(ParseError),
            };
        }

        if self.can_access_at_index(0) {
            let c = self.byte_at(0);
            if c == b'-' || c.is_ascii_digit() {
                return self.parse_number();
            }
        }

        Err(ParseError)
    }

    /// Equivalent to `parse_string` in upstream `cJSON.c`, minus the
    /// `cJSON` item plumbing (the caller, `parse_value`, wraps the
    /// returned bytes into a `String` node via `Arena::create_string`).
    ///
    /// Decodes the JSON string literal starting at `current_offset`
    /// (which must be the opening `"`), and returns the *raw, unescaped*
    /// byte content -- deliberately `Vec<u8>`, never `String`, since
    /// upstream's raw UTF-8 passthrough (`DECISIONS.md` #3, "invalid
    /// UTF-8 bytes preserved as-is") cannot be represented in a Rust
    /// `String`. Un-escaped literal bytes (anything not part of a `\`
    /// sequence) are copied through completely unvalidated, exactly as
    /// upstream's `*output_pointer++ = *input_pointer++;` does -- this
    /// function never calls `str::from_utf8` or any other UTF-8
    /// validation on string *content*, per this task's constraints.
    ///
    /// On success, advances `current_offset` to one past the closing
    /// `"`, matching `input_buffer->offset = (input_end - content) + 1`.
    /// On failure, `current_offset` is left at the exact byte offset
    /// upstream's `fail:` label would set (`input_pointer - content`),
    /// so `error_offset()` stays byte-for-byte compatible with
    /// `cJSON_GetErrorPtr` even for string-parse failures.
    fn parse_string(&mut self) -> Option<Vec<u8>> {
        // Not a string: caller checked byte_at(0) == '"' already, but
        // upstream re-checks this itself at the top of parse_string, so
        // this mirrors that (also makes the method safe to call
        // standalone).
        if !self.can_access_at_index(0) || self.byte_at(0) != b'"' {
            return None;
        }

        let content_start = self.current_offset + 1;

        // First pass: find the closing quote, matching upstream's
        // "calculate approximate size of the output" scan exactly,
        // including its failure modes (unterminated escape at EOF,
        // unterminated string).
        let mut input_end = content_start;
        let mut skipped_bytes = 0usize;
        while input_end < self.input.len() && self.input[input_end] != b'"' {
            if self.input[input_end] == b'\\' {
                if input_end + 1 >= self.input.len() {
                    // Backslash is the last byte in the input: matches
                    // upstream's buffer-overflow guard.
                    self.current_offset = input_end;
                    return None;
                }
                skipped_bytes += 1;
                input_end += 1;
            }
            input_end += 1;
        }
        if input_end >= self.input.len() || self.input[input_end] != b'"' {
            // String ended unexpectedly (ran off the end of input
            // without finding a closing quote).
            self.current_offset = input_end;
            return None;
        }

        // This is at most how much output we need -- upstream allocates
        // exactly this; we just use it as a `Vec` capacity hint since
        // the arena/Vec design has no separate allocator to size.
        let allocation_length = (input_end - content_start) - skipped_bytes;
        let mut output = Vec::with_capacity(allocation_length);

        // Second pass: decode the literal.
        let mut input_pointer = content_start;
        while input_pointer < input_end {
            if self.input[input_pointer] != b'\\' {
                // Raw passthrough: no UTF-8 validation, matches upstream
                // exactly (this is what preserves invalid UTF-8 /
                // malformed byte sequences byte-for-byte, per this
                // task's requirement 5).
                output.push(self.input[input_pointer]);
                input_pointer += 1;
                continue;
            }

            // Escape sequence. Upstream's `(input_end - input_pointer) < 1`
            // guard is unreachable dead code here (the enclosing `while
            // input_pointer < input_end` already guarantees at least 1
            // byte remains), reproduced only in spirit via the loop
            // condition itself -- there is no behavior to diverge on.
            let mut sequence_length: usize = 2;
            let escape_char = self.input[input_pointer + 1];
            match escape_char {
                b'b' => output.push(0x08),
                b'f' => output.push(0x0C),
                b'n' => output.push(b'\n'),
                b'r' => output.push(b'\r'),
                b't' => output.push(b'\t'),
                b'"' | b'\\' | b'/' => output.push(escape_char),
                b'u' => match self.utf16_literal_to_utf8(input_pointer, input_end, &mut output) {
                    Some(len) => sequence_length = len,
                    None => {
                        self.current_offset = input_pointer;
                        return None;
                    }
                },
                _ => {
                    self.current_offset = input_pointer;
                    return None;
                }
            }
            input_pointer += sequence_length;
        }

        self.current_offset = input_end + 1;
        Some(output)
    }

    /// Equivalent to `utf16_literal_to_utf8`. `first_sequence` and
    /// `input_end` are absolute byte offsets into `self.input`:
    /// `first_sequence` is the offset of the leading backslash of a
    /// `\uXXXX` escape (or the first half of a `\uXXXX\uXXXX` surrogate
    /// pair); `input_end` is the offset of the string's closing quote
    /// (the same bound `parse_string`'s decode loop uses). On success,
    /// pushes the UTF-8 encoding of the resulting codepoint onto
    /// `output` and returns how many input bytes the escape consumed (6
    /// for a lone `\uXXXX`, 12 for a surrogate pair). Returns `None` for
    /// every upstream failure mode (`goto fail`).
    fn utf16_literal_to_utf8(
        &self,
        first_sequence: usize,
        input_end: usize,
        output: &mut Vec<u8>,
    ) -> Option<usize> {
        if input_end - first_sequence < 6 {
            // input ends unexpectedly
            return None;
        }

        // get the first utf16 sequence
        let first_code = self.parse_hex4(first_sequence + 2);

        // check that the code is valid
        if (0xDC00..=0xDFFF).contains(&first_code) {
            return None;
        }

        let (codepoint, sequence_length): (u32, usize) = if (0xD800..=0xDBFF).contains(&first_code)
        {
            // UTF16 surrogate pair
            let second_sequence = first_sequence + 6;
            if input_end - second_sequence < 6 {
                // input ends unexpectedly
                return None;
            }
            if self.input[second_sequence] != b'\\' || self.input[second_sequence + 1] != b'u' {
                // missing second half of the surrogate pair
                return None;
            }
            // get the second utf16 sequence
            let second_code = self.parse_hex4(second_sequence + 2);
            if !(0xDC00..=0xDFFF).contains(&second_code) {
                // invalid second half of the surrogate pair
                return None;
            }
            // calculate the unicode codepoint from the surrogate pair
            let cp = 0x10000u32 + (((first_code & 0x3FF) << 10) | (second_code & 0x3FF));
            (cp, 12)
        } else {
            (first_code, 6)
        };

        // Encode as UTF-8 -- hand-written to match upstream's manual
        // encoder byte-for-byte, deliberately not routed through Rust's
        // `char`/`String` (a lone surrogate half can never reach here
        // given the checks above, but a manual encoder also sidesteps
        // relying on `char::from_u32` ever agreeing with upstream at the
        // boundary).
        match codepoint {
            0x0000..=0x007F => {
                // normal ascii, encoding 0xxxxxxx
                output.push(codepoint as u8);
            }
            0x0080..=0x07FF => {
                // two bytes, encoding 110xxxxx 10xxxxxx
                output.push(0xC0 | ((codepoint >> 6) as u8));
                output.push(0x80 | ((codepoint & 0x3F) as u8));
            }
            0x0800..=0xFFFF => {
                // three bytes, encoding 1110xxxx 10xxxxxx 10xxxxxx
                output.push(0xE0 | ((codepoint >> 12) as u8));
                output.push(0x80 | (((codepoint >> 6) & 0x3F) as u8));
                output.push(0x80 | ((codepoint & 0x3F) as u8));
            }
            0x10000..=0x10FFFF => {
                // four bytes, encoding 1110xxxx 10xxxxxx 10xxxxxx 10xxxxxx
                output.push(0xF0 | ((codepoint >> 18) as u8));
                output.push(0x80 | (((codepoint >> 12) & 0x3F) as u8));
                output.push(0x80 | (((codepoint >> 6) & 0x3F) as u8));
                output.push(0x80 | ((codepoint & 0x3F) as u8));
            }
            // invalid unicode codepoint -- unreachable: the surrogate
            // branch's max is 0x10FFFF and the non-surrogate branch's
            // max is 0xFFFF (0xD800..=0xDFFF is excluded above), but
            // upstream has this arm and we keep it for exact parity.
            _ => return None,
        }

        Some(sequence_length)
    }

    /// Equivalent to `parse_hex4`. Reproduces its exact upstream quirk on
    /// purpose (per `AI_GUARDRAILS.md` §3.1, "replicate exactly by
    /// default"): an invalid hex digit does *not* fail the parse -- the
    /// whole 4-digit value silently collapses to `0`, indistinguishable
    /// from a literal `\u0000`. This is because upstream's loop does
    /// `return 0;` the instant it sees a non-hex-digit byte, abandoning
    /// the remaining digits rather than reporting failure to its caller.
    ///
    /// Caller contract: `at + 4 <= self.input.len()` must already hold
    /// (both call sites establish this via the `< 6` bounds checks in
    /// `utf16_literal_to_utf8` before calling this).
    fn parse_hex4(&self, at: usize) -> u32 {
        let mut h: u32 = 0;
        for i in 0..4 {
            let c = self.input[at + i];
            let digit = match c {
                b'0'..=b'9' => (c - b'0') as u32,
                b'A'..=b'F' => 10 + (c - b'A') as u32,
                b'a'..=b'f' => 10 + (c - b'a') as u32,
                _ => return 0, // exact upstream quirk -- see doc comment
            };
            h += digit;
            if i < 3 {
                h <<= 4;
            }
        }
        h
    }

    /// Equivalent to `parse_number`.
    ///
    /// cJSON's version copies a locale-adjusted, filtered substring into
    /// a temporary NUL-terminated C string and hands it to `strtod`,
    /// which may consume less than the whole filtered substring (e.g.
    /// `"1.2.3"` filters to the 5-byte candidate `"1.2.3"`, but `strtod`
    /// only consumes `"1.2"` of it, leaving `.3` in the buffer for
    /// whatever parses next -- almost always then a parse error, since
    /// `.3` isn't a valid value on its own).
    ///
    /// This is reproduced in two steps: `strtod_prefix_len` finds how
    /// many bytes of the filtered candidate a `strtod` call would
    /// actually consume, and only that many bytes advance
    /// `current_offset` -- matching
    /// `input_buffer->offset += (after_end - number_c_string)` exactly,
    /// not `number_string_length`.
    ///
    /// Locale handling (`get_decimal_point`) is not reproduced: without
    /// `ENABLE_LOCALES` (the default build), cJSON's decimal point is
    /// always `'.'`, which is what Rust's float parser already expects.
    /// See `DECISIONS_personal.md` for the explicit scoping of this.
    fn parse_number(&mut self) -> Result<NodeId, ParseError> {
        let start = self.current_offset;

        // Filter step: collect the run of bytes cJSON's parse_number
        // would copy into its temporary buffer.
        let mut filtered_len = 0usize;
        while self.can_access_at_index(filtered_len) {
            match self.byte_at(filtered_len) {
                b'0'..=b'9' | b'+' | b'-' | b'.' | b'e' | b'E' => filtered_len += 1,
                _ => break,
            }
        }
        let candidate = &self.input[start..start + filtered_len];

        // strtod step: how much of that filtered candidate is actually a
        // valid number, left to right?
        let consumed = strtod_prefix_len(candidate);
        if consumed == 0 {
            // Matches `number_c_string == after_end` in the C version:
            // strtod consumed nothing, so this is a parse failure and the
            // offset does not move.
            return Err(ParseError);
        }

        // Safety note (not an `unsafe` block, just an invariant worth
        // stating): `candidate[..consumed]` only ever contains bytes from
        // the ASCII set matched above, so this is always valid UTF-8.
        let text = std::str::from_utf8(&candidate[..consumed])
            .expect("strtod_prefix_len only selects ASCII digit/sign/exponent bytes");
        let value: f64 = text
            .parse()
            .expect("strtod_prefix_len only returns text matching strtod's grammar");

        let id = self.arena.create_number(value);
        self.current_offset = start + consumed;
        Ok(id)
    }
}

/// Finds the length, in bytes, of the longest prefix of `s` that forms a
/// valid decimal float literal under C's `strtod` grammar:
///
/// ```text
/// [+-]? ( digit+ ('.' digit*)? | '.' digit+ ) ( [eE] [+-]? digit+ )?
/// ```
///
/// Returns `0` if no valid number starts at byte `0` of `s` (mirrors
/// `strtod` leaving `endptr == nptr`).
///
/// This only needs to handle the character set cJSON's `parse_number`
/// pre-filters down to (digits, `+`, `-`, `.`, `e`, `E`) -- `s` is never
/// handed leading whitespace, hex floats, or `inf`/`nan` text, since none
/// of those characters survive that filter.
fn strtod_prefix_len(s: &[u8]) -> usize {
    let n = s.len();
    let mut i = 0;

    if i < n && (s[i] == b'+' || s[i] == b'-') {
        i += 1;
    }

    let mut int_digits = 0;
    while i < n && s[i].is_ascii_digit() {
        i += 1;
        int_digits += 1;
    }

    let mut frac_digits = 0;
    if i < n && s[i] == b'.' {
        let dot = i;
        let mut j = i + 1;
        while j < n && s[j].is_ascii_digit() {
            j += 1;
            frac_digits += 1;
        }
        if frac_digits > 0 {
            i = j;
        } else {
            // A lone '.' with no digits on either side isn't part of the
            // number (matches strtod rejecting bare "." and not
            // consuming the dot when there were no integer digits
            // either).
            i = dot;
        }
    }

    if int_digits == 0 && frac_digits == 0 {
        return 0;
    }

    let mantissa_end = i;

    if i < n && (s[i] == b'e' || s[i] == b'E') {
        let mut j = i + 1;
        if j < n && (s[j] == b'+' || s[j] == b'-') {
            j += 1;
        }
        let exponent_digits_start = j;
        while j < n && s[j].is_ascii_digit() {
            j += 1;
        }
        if j > exponent_digits_start {
            i = j;
        } else {
            // 'e'/'E' with no following digits isn't a valid exponent;
            // don't consume it, matching strtod stopping before the 'e'.
            i = mantissa_end;
        }
    }

    i
}

/// Reproduces cJSON's `INT_MAX`/`INT_MIN` saturation for the deprecated
/// `valueint` view of a number's `value_double`:
///
/// ```c
/// if (number >= INT_MAX) { item->valueint = INT_MAX; }
/// else if (number <= (double)INT_MIN) { item->valueint = INT_MIN; }
/// else { item->valueint = (int)number; }
/// ```
///
/// The arena's `Node` deliberately has no `valueint` field (per
/// `DECISIONS.md` #8, that field belongs to the future C-ABI facade
/// layer, not the core engine), so this is exposed as a pure function
/// computing the same value on demand from `value_double`, rather than
/// stored state -- this keeps the Node layout untouched per
/// `AI_GUARDRAILS.md`, while still giving `parse_number.c`'s ported
/// assertions (`TEST_ASSERT_EQUAL_INT(integer, item->valueint)`)
/// something to call. See `DECISIONS_personal.md`.
pub fn clamped_int_value(value: f64) -> i32 {
    if value >= i32::MAX as f64 {
        i32::MAX
    } else if value <= i32::MIN as f64 {
        i32::MIN
    } else {
        value as i32
    }
}