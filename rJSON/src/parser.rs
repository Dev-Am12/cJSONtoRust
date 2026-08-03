//! Recursive-descent parser state and value dispatcher.
//!
//! This module is the Rust equivalent of the `parse_buffer` handling in
//! upstream `cJSON.c` (`parse_value`, `parse_number`, `parse_string`,
//! `parse_array`, `parse_object`, the literal branches of `parse_value`,
//! `buffer_skip_whitespace`, `skip_utf8_bom`, and the `can_read` /
//! `can_access_at_index` macros), plus the top-level
//! `cJSON_Parse`/`cJSON_ParseWithOpts`/`cJSON_ParseWithLengthOpts` entry
//! points (`cjson_parse`, `cjson_parse_with_opts`,
//! `cjson_parse_with_length_opts`, near the bottom of this file) -- see
//! `DECISIONS_personal.md` for the entry documenting these.
//!
//! Behavioral parity with upstream cJSON takes priority over idiomatic
//! Rust throughout this file, per `AI_GUARDRAILS.md` §3.

use crate::arena::{Arena, NodeId};

/// Mirrors `cJSON.h`'s `CJSON_NESTING_LIMIT` (default build value).
/// Enforced by `parse_array`/`parse_object`, exactly as upstream's
/// `input_buffer->depth >= CJSON_NESTING_LIMIT` check at the top of each
/// function (`cJSON.c` lines ~1502 and ~1667).
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
    /// `parse_buffer.depth` directly. Incremented/decremented by
    /// `parse_array`/`parse_object` around each array/object body.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// The byte offset `cJSON_GetErrorPtr` would report right now.
    ///
    /// Reproduces the clamping `cJSON_ParseWithLengthOpts` performs in its
    /// `fail:` branch: `local_error.position = buffer.offset` if
    /// `buffer.offset < buffer.length`, else `buffer.length - 1` if
    /// `buffer.length > 0`, else `0`.
    ///
    /// **Bugfix (flagged per `AI_GUARDRAILS.md` §3.2/§7, made while wiring
    /// up `cjson_parse_with_length_opts` -- see `DECISIONS_personal.md`):**
    /// the in-bounds comparison here must be `<=`, not `<`. Upstream's
    /// `buffer.length` (in the dominant `cJSON_Parse`/`cJSON_ParseWithOpts`
    /// call path) is `strlen(value) + 1` -- one *past* the real content
    /// length, to hold the NUL terminator -- so `buffer.offset < buffer.length`
    /// is true even when `offset` has advanced exactly to the end of the
    /// *real* content (`offset == strlen(value)`, i.e. `offset == input.len()`
    /// in this port's terms, per the no-NUL-sentinel design in this file's
    /// `skip_whitespace` doc comment). A strict `<` against `input.len()`
    /// (this port's equivalent of `strlen(value)`, not `buffer.length`)
    /// under-reports that case by one, clamping to `input.len() - 1`
    /// instead of reporting `input.len()` -- observable, for example, when
    /// parsing `"{ \"name\": "` fails for want of a value at the very end
    /// of input: upstream reports the error at `json + strlen(json)`, not
    /// one byte before it. Since every internal offset-advance in this
    /// file is bounds-checked (`can_read`/`can_access_at_index`),
    /// `current_offset` can never exceed `input.len()`, so this only ever
    /// takes the `current_offset` branch or the (dead, kept only for
    /// documentation/defensive-fallback parity with upstream's shape) `0`
    /// branch for empty input -- the `input.len() - 1` branch is
    /// unreachable in practice but left in place rather than deleted, so
    /// this function's shape still visibly mirrors upstream's three-way
    /// clamp.
    pub fn error_offset(&self) -> usize {
        if self.current_offset <= self.input.len() {
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

    /// Equivalent to `skip_utf8_bom`: skips a leading UTF-8
    /// byte-order-mark (`EF BB BF`) at the very start of `input`, if
    /// present. A no-op if `current_offset != 0` (matches upstream's
    /// `buffer->offset != 0` guard -- only ever meaningful immediately
    /// after `Parser::new`, before any whitespace skip or parse call).
    ///
    /// **Length-check translation (flagged per `AI_GUARDRAILS.md` §3.2,
    /// documented in `DECISIONS_personal.md`):** upstream's guard is
    /// `can_access_at_index(buffer, 4)`, i.e. `offset + 4 < buffer.length`.
    /// In the dominant `cJSON_Parse`/`cJSON_ParseWithOpts` call path,
    /// `buffer.length == strlen(value) + 1` (one past the real content,
    /// for the NUL terminator -- see `error_offset`'s doc comment for the
    /// same translation), so that guard reduces, in terms of *real*
    /// content length, to `4 < real_len + 1`, i.e. `real_len >= 4`. This
    /// port has no NUL-terminator slot (`input.len()` already *is* the
    /// real content length), so the direct translation is
    /// `input.len() >= 4` at `current_offset == 0` -- confirmed against
    /// upstream's own test fixtures (`misc_tests.c`'s
    /// `skip_utf8_bom_should_skip_bom`, `sizeof("\xEF\xBB\xBF{}")` == 6,
    /// i.e. real content length 5, well above this threshold; and
    /// `parse_with_opts.c`'s `parse_with_opts_should_parse_utf8_bom`,
    /// `"\xEF\xBB\xBF{}"` has real content length exactly 5).
    pub fn skip_utf8_bom(&mut self) {
        if self.current_offset != 0 {
            return;
        }
        if self.input.len() >= 4 && self.input[0..3] == *b"\xEF\xBB\xBF" {
            self.current_offset += 3;
        }
    }

    /// Recursive-descent value dispatcher. Equivalent to `parse_value`.
    ///
    /// Tries literals in exactly the order upstream does (`null`,
    /// `false`, `true`), then string, then number, then array, then
    /// object -- the exact dispatch order of upstream's `parse_value`.
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

        if self.can_access_at_index(0) && self.byte_at(0) == b'[' {
            return self.parse_array();
        }

        if self.can_access_at_index(0) && self.byte_at(0) == b'{' {
            return self.parse_object();
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

        // Rust's `str::parse::<f64>()` does not accept a decimal point without
        // fractional digits (e.g. "1." or "1.e2"), unlike C's `strtod` which
        // treats them as "1.0" and "1.0e2".
        // We normalize these cases to strictly valid Rust floats ("1", "1e2")
        // without changing the numeric value, adapting to Rust's stricter
        // float-literal grammar without changing the outcome.
        let parse_text = text.strip_suffix('.').unwrap_or(text);

        let value: f64 = parse_text
            .parse()
            .expect("strtod_prefix_len only returns text matching strtod's grammar");

        let id = self.arena.create_number(value);
        self.current_offset = start + consumed;
        Ok(id)
    }

    /// O(1) sibling-list append used while parsing array/object elements.
    ///
    /// `Arena::append_child` (per `DECISIONS.md` #4) walks the sibling
    /// chain to find the current tail on every call -- fine for one-off
    /// public-API insertions, but it would make parsing an N-element
    /// array/object O(n^2). While parsing, the tail is already known (the
    /// item most recently parsed), so linking it directly is a single
    /// pair of field writes: `tail.next = item`, `item.prev = tail`. This
    /// keeps the conventional "prev = true predecessor" invariant intact
    /// (same as `append_child`) and does not introduce any new
    /// collection -- just plain `NodeId` links on `Node`, per this task's
    /// constraint to preserve O(1) sibling insertion without `Vec<NodeId>`
    /// or a `HashMap`.
    fn link_as_next_sibling(&mut self, tail: NodeId, item: NodeId) {
        self.arena.get_mut(tail).next = Some(item);
        self.arena.get_mut(item).prev = Some(tail);
    }

    /// Equivalent to `parse_array`.
    ///
    /// Enforces `CJSON_NESTING_LIMIT` exactly as upstream does: the depth
    /// check happens *before* confirming the current byte is even `[`,
    /// matching `cJSON.c`'s literal statement order (this is only ever
    /// observable if `parse_array` were called on non-`[` input directly,
    /// which never happens through `parse_value`'s dispatch, since it
    /// only calls this after already checking `byte_at(0) == '['`).
    ///
    /// Children are parsed one at a time through `parse_value` and linked
    /// with `link_as_next_sibling` as each one completes -- O(1) per
    /// element, not upstream's O(1)-via-tail-shortcut trick (rejected in
    /// `DECISIONS.md` #4) and not `Arena::append_child`'s O(n) walk
    /// either.
    ///
    /// **Depth accounting divergence (flagged per `AI_GUARDRAILS.md`
    /// §3.2):** upstream only executes `input_buffer->depth--;` on the
    /// success path -- every `goto fail;` in the C function skips it,
    /// leaving `depth` incremented on failure. This is invisible in C
    /// because a failed parse aborts the *entire* `cJSON_Parse` call
    /// immediately (every enclosing `parse_array`/`parse_object` also
    /// hits its own `goto fail` and returns, all the way to the top, with
    /// no further use of `depth` afterward). This port's `Parser` can
    /// outlive a failed `parse_value()` call (e.g. `depth()` is a public
    /// accessor), so this implementation decrements `depth` on *every*
    /// exit path, success or failure, keeping the increment/decrement
    /// balanced. This cannot change observable parse results (nothing
    /// reads `depth` again after a failure within the same top-level
    /// parse, in either implementation) and avoids leaving `Parser` in a
    /// misleading state if inspected or reused after an error.
    ///
    /// On failure after one or more elements were already linked, the
    /// partially-built sibling chain is torn down with a single
    /// `Arena::delete(head)` call, matching `cJSON_Delete(head)` in
    /// upstream's `fail:` label (which walks the whole `next` chain from
    /// `head`, per `DECISIONS.md` #6).
    fn parse_array(&mut self) -> Result<NodeId, ParseError> {
        if self.depth >= CJSON_NESTING_LIMIT {
            // too deeply nested
            return Err(ParseError);
        }
        self.depth += 1;

        if !self.can_access_at_index(0) || self.byte_at(0) != b'[' {
            // not an array
            self.depth -= 1;
            return Err(ParseError);
        }

        self.current_offset += 1;
        self.skip_whitespace();
        if self.can_access_at_index(0) && self.byte_at(0) == b']' {
            // empty array
            self.depth -= 1;
            let array = self.arena.create_array();
            self.current_offset += 1;
            return Ok(array);
        }

        // check if we skipped to the end of the buffer
        if !self.can_access_at_index(0) {
            self.current_offset -= 1;
            self.depth -= 1;
            return Err(ParseError);
        }

        // step back to character in front of the first element
        self.current_offset -= 1;

        let mut head: Option<NodeId> = None;
        let mut tail: Option<NodeId> = None;

        // loop through the comma separated array elements
        loop {
            self.current_offset += 1;
            self.skip_whitespace();

            let item = match self.parse_value() {
                Ok(id) => id,
                Err(err) => {
                    // failed to parse value
                    self.depth -= 1;
                    if let Some(head) = head {
                        self.arena.delete(head);
                    }
                    return Err(err);
                }
            };
            self.skip_whitespace();

            match tail {
                None => {
                    // start the linked list
                    head = Some(item);
                    tail = Some(item);
                }
                Some(previous_tail) => {
                    // add to the end and advance
                    self.link_as_next_sibling(previous_tail, item);
                    tail = Some(item);
                }
            }

            if self.can_access_at_index(0) && self.byte_at(0) == b',' {
                continue;
            }
            break;
        }

        if !self.can_access_at_index(0) || self.byte_at(0) != b']' {
            // expected end of array
            self.depth -= 1;
            if let Some(head) = head {
                self.arena.delete(head);
            }
            return Err(ParseError);
        }

        self.depth -= 1;

        let array = self.arena.create_array();
        if let Some(head) = head {
            self.arena.get_mut(array).child = Some(head);
        }

        self.current_offset += 1;
        Ok(array)
    }

    /// Equivalent to `parse_object`.
    ///
    /// Structurally mirrors `parse_array` (see its doc comment for the
    /// nesting-limit-order note and the depth-accounting divergence,
    /// which both apply here identically). The one shape difference from
    /// `parse_array` is that each element is a *key: value* pair rather
    /// than a bare value:
    ///
    /// - The key is parsed with `parse_string` (the same private method
    ///   `parse_value` uses for string *values* -- see
    ///   `DECISIONS_personal.md` #8's follow-up, which flagged exactly
    ///   this reuse), kept as the raw `Vec<u8>` it already returns rather
    ///   than being wrapped in a `String` node the way upstream
    ///   temporarily wraps it in `current_item->valuestring` before the
    ///   swap into `current_item->string`. There is no swap to perform
    ///   here: the key never becomes a node's `value_string` in the first
    ///   place, it goes directly into the eventual value node's `key`
    ///   field once that node exists.
    /// - The value is then parsed with `parse_value` (a fresh node,
    ///   unlike upstream reusing the same `cJSON` struct for both the key
    ///   and the value), and the key `Vec<u8>` computed above is moved
    ///   into that node's `key` field.
    /// - Duplicate keys are never checked for or rejected -- object
    ///   members are linked in encounter order exactly like array
    ///   elements, matching upstream (which has no duplicate-key check in
    ///   `parse_object` either) and this task's explicit requirement to
    ///   preserve, not reject, duplicates.
    ///
    /// Matches upstream's `cannot_access_at_index(input_buffer, 1)`
    /// "nothing comes after the comma" guard, checked at the same point
    /// in the loop (before consuming the byte after `{`/`,`, so before
    /// attempting to parse a key) and using the same offset.
    fn parse_object(&mut self) -> Result<NodeId, ParseError> {
        if self.depth >= CJSON_NESTING_LIMIT {
            // too deeply nested
            return Err(ParseError);
        }
        self.depth += 1;

        if !self.can_access_at_index(0) || self.byte_at(0) != b'{' {
            // not an object
            self.depth -= 1;
            return Err(ParseError);
        }

        self.current_offset += 1;
        self.skip_whitespace();
        if self.can_access_at_index(0) && self.byte_at(0) == b'}' {
            // empty object
            self.depth -= 1;
            let object = self.arena.create_object();
            self.current_offset += 1;
            return Ok(object);
        }

        // check if we skipped to the end of the buffer
        if !self.can_access_at_index(0) {
            self.current_offset -= 1;
            self.depth -= 1;
            return Err(ParseError);
        }

        // step back to character in front of the first element
        self.current_offset -= 1;

        let mut head: Option<NodeId> = None;
        let mut tail: Option<NodeId> = None;

        // loop through the comma separated object members
        loop {
            if !self.can_access_at_index(1) {
                // nothing comes after the comma
                self.depth -= 1;
                if let Some(head) = head {
                    self.arena.delete(head);
                }
                return Err(ParseError);
            }

            // parse the name of the child
            self.current_offset += 1;
            self.skip_whitespace();
            let key = match self.parse_string() {
                Some(bytes) => bytes,
                None => {
                    // failed to parse name
                    self.depth -= 1;
                    if let Some(head) = head {
                        self.arena.delete(head);
                    }
                    return Err(ParseError);
                }
            };
            self.skip_whitespace();

            if !self.can_access_at_index(0) || self.byte_at(0) != b':' {
                // invalid object
                self.depth -= 1;
                if let Some(head) = head {
                    self.arena.delete(head);
                }
                return Err(ParseError);
            }

            // parse the value
            self.current_offset += 1;
            self.skip_whitespace();
            let item = match self.parse_value() {
                Ok(id) => id,
                Err(err) => {
                    // failed to parse value
                    self.depth -= 1;
                    if let Some(head) = head {
                        self.arena.delete(head);
                    }
                    return Err(err);
                }
            };
            self.arena.get_mut(item).key = Some(key);
            self.skip_whitespace();

            match tail {
                None => {
                    // start the linked list
                    head = Some(item);
                    tail = Some(item);
                }
                Some(previous_tail) => {
                    // add to the end and advance
                    self.link_as_next_sibling(previous_tail, item);
                    tail = Some(item);
                }
            }

            if self.can_access_at_index(0) && self.byte_at(0) == b',' {
                continue;
            }
            break;
        }

        if !self.can_access_at_index(0) || self.byte_at(0) != b'}' {
            // expected end of object
            self.depth -= 1;
            if let Some(head) = head {
                self.arena.delete(head);
            }
            return Err(ParseError);
        }

        self.depth -= 1;

        let object = self.arena.create_object();
        if let Some(head) = head {
            self.arena.get_mut(object).child = Some(head);
        }

        self.current_offset += 1;
        Ok(object)
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
        } else if int_digits > 0 {
            // Trailing decimal point after integer digits (e.g. "1."):
            // strtod consumes the dot and treats it as "1.0". Advance
            // past the dot to match strtod's actual behavior -- upstream
            // cJSON delegates to strtod unconditionally, so "1." is a
            // valid number in upstream's grammar.
            i = dot + 1;
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

/// A failed top-level parse, equivalent to what `cJSON_GetErrorPtr` would
/// report after `cJSON_Parse`/`cJSON_ParseWithOpts`/`cJSON_ParseWithLengthOpts`
/// returns `NULL`.
///
/// Upstream reports this via a mutable global (`global_error`) read
/// separately through `cJSON_GetErrorPtr()`. Per `DECISIONS_personal.md`
/// ("Error reporting" entry), this port returns the position directly
/// instead of introducing `static mut`-equivalent shared state -- `position`
/// is exactly the value `cJSON_GetErrorPtr() - value` would yield (a byte
/// offset into the original `value` slice passed to whichever of the three
/// functions below was called), with the same clamping upstream's `fail:`
/// branch performs (see `Parser::error_offset`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CJsonParseError {
    pub position: usize,
}

/// Equivalent to `cJSON_ParseWithLengthOpts`.
///
/// `value` plays the role of upstream's `(value, buffer_length)` pair
/// together: a Rust `&[u8]` is already an explicitly-bounded buffer, so
/// there is no separate `buffer_length` parameter here (unlike upstream,
/// nothing in this port ever needs a NUL terminator to know where the
/// buffer ends -- see `skip_whitespace`'s doc comment). This means, unlike
/// upstream, this function is given the *exact* bytes to consider; it does
/// not stop early at an embedded NUL byte the way `cjson_parse_with_opts`
/// (below) deliberately does, matching the real difference between
/// upstream's two functions (`cJSON_ParseWithOpts` computes its length via
/// `strlen`, which cannot see past an embedded NUL; `cJSON_ParseWithLengthOpts`
/// trusts its caller's explicit length instead).
///
/// On success, returns the root `NodeId` plus the "parse end" byte offset
/// into `value` (equivalent to `*return_parse_end`/`buffer_at_offset(&buffer)`
/// on upstream's success path). On failure, returns a `CJsonParseError`
/// carrying the same offset upstream would expose via `cJSON_GetErrorPtr`
/// (and would also have written to `*return_parse_end`, since upstream sets
/// that out-parameter on both its success *and* failure paths with the same
/// underlying position).
///
/// `require_null_terminated` matches upstream exactly: after a successful
/// parse, any remaining bytes (other than whitespace) before the end of
/// `value` cause this to fail instead, discarding the already-parsed tree
/// -- mirroring upstream's `cJSON_Delete(item)` in that specific `fail:`
/// path (see `DECISIONS.md` #6 for why a single `Arena::delete(root)` call
/// is sufficient cleanup here, same as everywhere else in this port).
pub fn cjson_parse_with_length_opts(
    arena: &mut Arena,
    value: &[u8],
    require_null_terminated: bool,
) -> Result<(NodeId, usize), CJsonParseError> {
    // Matches `value == NULL || 0 == buffer_length` in
    // `cJSON_ParseWithLengthOpts`: a Rust `&[u8]` can't be NULL (that
    // upstream case is a C-FFI-layer concern, out of scope for this safe
    // core engine per `DECISIONS.md` #3/#8's deferred facade layer -- see
    // `DECISIONS_personal.md`), but an empty slice is the direct analogue
    // of `buffer_length == 0`. Both short-circuit to failure before any
    // node is allocated, at position `0`, exactly like upstream's `goto
    // fail` before `buffer.content` is ever set.
    if value.is_empty() {
        return Err(CJsonParseError { position: 0 });
    }

    let mut parser = Parser::new(value, arena);
    parser.skip_utf8_bom();
    parser.skip_whitespace();

    let root = match parser.parse_value() {
        Ok(id) => id,
        Err(_) => {
            let position = parser.error_offset();
            return Err(CJsonParseError { position });
        }
    };

    if require_null_terminated {
        parser.skip_whitespace();
        if parser.current_offset() != value.len() {
            // Trailing, non-whitespace content remains: upstream's
            // `require_null_terminated` check fails here too, and its
            // `fail:` label deletes the already-built `item` in this case
            // exactly as it would for a parse-time failure.
            let position = parser.error_offset();
            arena.delete(root);
            return Err(CJsonParseError { position });
        }
    }

    let parse_end = parser.current_offset();
    Ok((root, parse_end))
}

/// Equivalent to `cJSON_ParseWithOpts`.
///
/// Upstream computes `buffer_length = strlen(value) + sizeof("")` before
/// delegating to `cJSON_ParseWithLengthOpts` -- meaning it only ever sees
/// bytes up to (and not including) the first NUL byte in `value`, even if
/// more bytes follow it in memory. This is reproduced here by truncating
/// `value` at its first `0x00` byte (if any) before delegating to
/// `cjson_parse_with_length_opts`, rather than simply forwarding the whole
/// slice -- forwarding the whole slice would silently give this function
/// `cJSON_ParseWithLengthOpts`'s actual behavior (seeing past an embedded
/// NUL) instead of `cJSON_ParseWithOpts`'s (stopping at it), which upstream
/// callers can and do rely on being different. Byte offsets returned (both
/// the success `parse_end` and the failure `position`) are indices into
/// this truncated prefix, which -- being a prefix -- are numerically
/// identical to the corresponding indices into the original `value`.
pub fn cjson_parse_with_opts(
    arena: &mut Arena,
    value: &[u8],
    require_null_terminated: bool,
) -> Result<(NodeId, usize), CJsonParseError> {
    let truncated_len = value.iter().position(|&b| b == 0).unwrap_or(value.len());
    cjson_parse_with_length_opts(arena, &value[..truncated_len], require_null_terminated)
}

/// Equivalent to `cJSON_Parse`: `cJSON_ParseWithOpts(value, NULL, false)`,
/// i.e. no `return_parse_end` out-parameter and `require_null_terminated`
/// off. The `(NodeId, usize)` pair upstream would have communicated only
/// through the `return_parse_end` out-parameter and the return value is
/// collapsed to just the `NodeId` here, since nothing reads `return_parse_end`
/// through this entry point (matching upstream passing a literal `0` for
/// it).
pub fn cjson_parse(arena: &mut Arena, value: &[u8]) -> Result<NodeId, CJsonParseError> {
    cjson_parse_with_opts(arena, value, false).map(|(id, _parse_end)| id)
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
