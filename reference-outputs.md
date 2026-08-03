# Reference Outputs — original C cJSON (DaveGamble/cJSON)

Captured by running `cJSON_Parse` + `cJSON_Print` (and `cJSON_Minify`) directly
against the unmodified C library. Ground truth for the Rust port — if Rust
disagrees with this file, Rust is wrong (or the file needs an entry added).

Build used: `gcc -o probe probe.c cJSON.c -I.`

---

### Large integer (bigger than what an int/int64 can hold)
input:  `123456789012345678901234567890`
output: `1.2345678901234568e+29`
→ cJSON stores all numbers as `double`. No arbitrary precision — it silently
  loses precision and switches to scientific notation.

### Very large number (1e300)
input:  `1e300`
output: `1e+300`

### Number bigger than double max (1e400 — overflows double)
input:  `1e400`
output: `null`
→ Parses as `cJSON_NULL`, not an error, not `inf`. Overflow is silently
  swallowed into a null node.

### Tiny exponent (1e-300)
input:  `1e-300`
output: `1e-300`
→ Note: positive exponent prints with `+` (`1e+300`), negative does not add
  an extra sign (`1e-300`, not `1e-300` — already has the `-` from input).

### Small decimal exponent (5e-10)
input:  `5e-10`
output: `5e-10`

### Small decimal written as a plain fraction (0.0000001)
input:  `0.0000001`
output: `1e-07`
→ cJSON reformats it into scientific notation on print even though the
  input wasn't written that way. Two-digit exponent gets a leading zero
  (`e-07`, not `e-7`).

### Minify: `//` line comment
input:
```
{
  // this is a comment
  "a": 1
}
```
output: `{"a":1}`
→ Comment is stripped cleanly, no error.

### Minify: `/* ... */` block comment
input:
```
{
  /* block comment */
  "a": 1,
  "b": 2
}
```
output: `{"a":1,"b":2}`
→ Also stripped cleanly.

### Duplicate keys
input: `{"a":1,"a":2}`
output (pretty-printed, since Print not Minify was used):
```
{
	"a":	1,
	"a":	2
}
```
→ Both duplicates are kept as separate nodes — cJSON does NOT dedupe or
  overwrite on parse. (Lookup behavior — which one `cJSON_GetObjectItem`
  returns — is a separate question this probe didn't test yet.)

### Key casing
input: `{"Key":1}`
output:
```
{
	"Key":	1
}
```
→ Printing preserves original case as-is (case-insensitivity, per the plan,
  is about *lookup*, not about what gets printed back out).

---

### print_number regression values

Captured using the original unmodified cJSON.

input: 0.123
output: 0.123

input: 123e+127
output: 1.23e+129

input: 123e-128
output: 1.23e-126

input: pi (3.141592653589793)
output: 3.1415926535897931

These values were used as the oracle for the Rust
print_number regression tests.

## ⚠️ Platform-dependent behavior — exponent digit padding

The above outputs for `5e-10` and `0.0000001` were captured on **Linux
(glibc)**. The same `probe.c` compiled with gcc on **Windows** produces a
different result for the exponent width:

| input | Linux (glibc) output | Windows output |
|---|---|---|
| `5e-10` | `5e-10` | `5e-010` |
| `0.0000001` | `1e-07` | `1e-007` |

Root cause: cJSON hands number formatting off to the C standard library's
`%e`-style printf. glibc pads the exponent to a minimum of 2 digits;
Windows' C runtime pads to 3. Same cJSON source, same logic — the
difference is entirely the OS's C runtime underneath.

**Action item: confirm which OS the team's actual build/test/CI target is**
(the plan's one-command Docker build is almost certainly Linux-based) —
that platform's output is the one the Rust port needs to match. Don't chase
a "bug" if a local Windows test doesn't match this file; check it against a
Linux run (e.g. inside the team's Docker container) before assuming
something's wrong.

## Notes for whoever ports this
- All parse/print cases above were run with plain `cJSON_Print` (pretty,
  tab-indented) on Linux. The minify cases used `cJSON_Minify`, which
  produces compact single-line output — that's a different function, not a
  different result, so don't read anything into the visual difference
  between the two styles (duplicate-key and casing behavior is identical
  either way, just formatted differently).
- This is NOT exhaustive — it's the handful of cases from the plan's "tricky
  spots" list, captured fast. Add to this file as more edge cases come up
  instead of guessing.
- Overflow → `null` (not error, not panic, not inf) is probably the single
  most surprising one here — flag it to Member 1/2 early.
- Number formatting is platform-dependent (see section above) — flag this
  to whoever writes the Rust number-printing code, since Rust's own default
  formatting won't automatically match either OS's C runtime.
