//! C-ABI facade layer for rJSON.
//!
//! Exposes a subset of the cJSON 1.7.19 public API as `extern "C"` symbols
//! so the original C test files (compiled with the adapter `common.h`) can
//! link against our Rust `cdylib` without modification.
//!
//! Design: materialise-and-free (DECISIONS.md §11).
//! Each parse call builds a fresh Arena, parses into it, then walks the
//! resulting arena tree to heap-allocate a mirror image as C `cJSON` structs
//! with real `next`/`prev`/`child` pointer links.  The Arena is dropped
//! before returning.  `cJSON_Delete` walks and frees the C-heap structs;
//! Rust is not involved.  Functions that receive a `*const CJson` back
//! (Print, Compare) re-walk the C struct tree to rebuild a temporary Arena.
//!
//! Note on Rust 2024 edition: `unsafe fn` no longer grants implicit unsafety
//! inside the body; every unsafe operation needs its own `unsafe { }` block
//! with a one-line invariant comment per AI_GUARDRAILS §2.1.

#![allow(non_snake_case)]

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};

use crate::arena::{Arena, Node, NodeId, NodeType};
use crate::parser::{
    cjson_parse_with_length_opts, cjson_parse_with_opts, clamped_int_value, CJsonParseError,
};

// ---------------------------------------------------------------------------
// C type bits — mirror cJSON.h defines exactly
// ---------------------------------------------------------------------------
const CJSON_FALSE: c_int = 1 << 0;
const CJSON_TRUE: c_int = 1 << 1;
const CJSON_NULL: c_int = 1 << 2;
const CJSON_NUMBER: c_int = 1 << 3;
const CJSON_STRING: c_int = 1 << 4;
const CJSON_ARRAY: c_int = 1 << 5;
const CJSON_OBJECT: c_int = 1 << 6;
const CJSON_RAW: c_int = 1 << 7;

// ---------------------------------------------------------------------------
// The C-compatible struct — MUST match cJSON.h layout exactly
// ---------------------------------------------------------------------------

/// Mirror of `struct cJSON` from cJSON.h.
///
/// Field order, sizes, and alignment must match exactly so C code can read
/// `->next`, `->type`, `->valuestring`, etc. directly.
/// `type_` maps to the C field `type`; Rust keyword collision resolved by
/// the adapter header declaring the field as `type` in C.
#[repr(C)]
pub struct CJson {
    pub next: *mut CJson,
    pub prev: *mut CJson,
    pub child: *mut CJson,
    pub type_: c_int,
    pub valuestring: *mut c_char,
    pub valueint: c_int,
    pub valuedouble: c_double,
    pub string: *mut c_char,
}

// ---------------------------------------------------------------------------
// Global error pointer — one static mut, required for cJSON_GetErrorPtr
// ---------------------------------------------------------------------------

/// Last parse error position.  Invariant: null (no error) or a pointer into
/// a C string buffer owned by the C caller, valid until the next parse call.
///
/// SAFETY: original cJSON is not thread-safe; we match that assumption.
/// This is the only required `static mut`; no thread-safe alternative exists
/// without a new crate dependency (AI_GUARDRAILS §4).
static mut GLOBAL_ERROR_PTR: *const c_char = std::ptr::null();

// ---------------------------------------------------------------------------
// Pure helper functions (safe Rust)
// ---------------------------------------------------------------------------

fn node_type_to_cjson_type(nt: NodeType) -> c_int {
    match nt {
        NodeType::Null => CJSON_NULL,
        NodeType::False => CJSON_FALSE,
        NodeType::True => CJSON_TRUE,
        NodeType::Number => CJSON_NUMBER,
        NodeType::String => CJSON_STRING,
        NodeType::Array => CJSON_ARRAY,
        NodeType::Object => CJSON_OBJECT,
        NodeType::Raw => CJSON_RAW,
    }
}

fn cjson_type_to_node_type(t: c_int) -> Option<NodeType> {
    match t & 0xFF {
        v if v == CJSON_NULL => Some(NodeType::Null),
        v if v == CJSON_FALSE => Some(NodeType::False),
        v if v == CJSON_TRUE => Some(NodeType::True),
        v if v == CJSON_NUMBER => Some(NodeType::Number),
        v if v == CJSON_STRING => Some(NodeType::String),
        v if v == CJSON_ARRAY => Some(NodeType::Array),
        v if v == CJSON_OBJECT => Some(NodeType::Object),
        v if v == CJSON_RAW => Some(NodeType::Raw),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Low-level allocation helpers (all unsafe, all annotated)
// ---------------------------------------------------------------------------

/// Allocate a zeroed CJson on the C heap.
/// SAFETY: caller must eventually free via cJSON_Delete. Returns null on OOM.
unsafe fn alloc_cjson() -> *mut CJson {
    // SAFETY: malloc returns valid pointer or null; size is nonzero.
    let ptr = unsafe { libc::malloc(std::mem::size_of::<CJson>()) as *mut CJson };
    if !ptr.is_null() {
        // SAFETY: ptr is a freshly allocated block of the right size.
        unsafe { std::ptr::write_bytes(ptr, 0, 1) };
    }
    ptr
}

/// Copy bytes into a NUL-terminated C string on the C heap.
/// SAFETY: caller must eventually free the returned pointer.
unsafe fn bytes_to_cstring_heap(bytes: &[u8]) -> *mut c_char {
    // SAFETY: malloc returns valid pointer or null; len+1 is nonzero.
    let ptr = unsafe { libc::malloc(bytes.len() + 1) as *mut c_char };
    if ptr.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: ptr is valid for bytes.len()+1 bytes; bytes slice is valid.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len()) };
    // SAFETY: ptr+bytes.len() is within the allocation.
    unsafe { *ptr.add(bytes.len()) = 0 };
    ptr
}

/// Walk an arena tree rooted at `id` and build a C-heap CJson tree.
/// SAFETY: arena is valid; returned tree must be freed with cJSON_Delete.
unsafe fn arena_to_cjson(arena: &Arena, id: NodeId) -> *mut CJson {
    let node = arena.get(id);
    // SAFETY: alloc_cjson uses malloc internally.
    let ptr = unsafe { alloc_cjson() };
    if ptr.is_null() {
        return std::ptr::null_mut();
    }

    let type_bits = node_type_to_cjson_type(node.node_type)
        | if node.is_reference { 256 } else { 0 }
        | if node.key_is_const { 512 } else { 0 };

    let valuestring_ptr = if let Some(vs) = &node.value_string {
        // SAFETY: bytes_to_cstring_heap allocates on C heap.
        unsafe { bytes_to_cstring_heap(vs) }
    } else {
        std::ptr::null_mut()
    };

    let key_ptr = if let Some(k) = &node.key {
        // SAFETY: bytes_to_cstring_heap allocates on C heap.
        unsafe { bytes_to_cstring_heap(k) }
    } else {
        std::ptr::null_mut()
    };

    // SAFETY: ptr is valid and zeroed; filling all fields.
    unsafe {
        (*ptr).type_ = type_bits;
        (*ptr).valuedouble = node.value_double;
        (*ptr).valueint = clamped_int_value(node.value_double);
        (*ptr).valuestring = valuestring_ptr;
        (*ptr).string = key_ptr;
    }

    // Recurse into child chain
    if let Some(child_id) = node.child {
        // SAFETY: child_id is a valid NodeId in this arena.
        let child_ptr = unsafe { arena_to_cjson(arena, child_id) };
        // SAFETY: ptr is valid.
        unsafe { (*ptr).child = child_ptr };

        let mut prev_ptr = child_ptr;
        let mut cur_id = arena.get(child_id).next;
        while let Some(sib_id) = cur_id {
            // SAFETY: sib_id is a valid NodeId in this arena.
            let sib_ptr = unsafe { arena_to_cjson(arena, sib_id) };
            // SAFETY: prev_ptr and sib_ptr are valid CJson nodes we allocated.
            unsafe {
                (*prev_ptr).next = sib_ptr;
                (*sib_ptr).prev = prev_ptr;
            }
            prev_ptr = sib_ptr;
            cur_id = arena.get(sib_id).next;
        }
    }

    ptr
}

/// Walk a C-struct CJson tree and build a temporary Arena.
/// SAFETY: ptr must be non-null and point to a structurally valid CJson.
unsafe fn cjson_to_arena(arena: &mut Arena, ptr: *const CJson) -> Option<NodeId> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: ptr is a valid CJson pointer (caller contract).
    let c = unsafe { &*ptr };

    let node_type = cjson_type_to_node_type(c.type_)?;

    let value_string = if !c.valuestring.is_null() {
        // SAFETY: valuestring is a NUL-terminated C string we allocated.
        Some(unsafe { CStr::from_ptr(c.valuestring) }.to_bytes().to_vec())
    } else {
        None
    };

    let key = if !c.string.is_null() {
        // SAFETY: string is a NUL-terminated C string we allocated.
        Some(unsafe { CStr::from_ptr(c.string) }.to_bytes().to_vec())
    } else {
        None
    };

    let id = arena.alloc(Node {
        next: None,
        prev: None,
        child: None,
        node_type,
        value_string,
        value_double: c.valuedouble,
        key,
        is_reference: (c.type_ & 256) != 0,
        key_is_const: (c.type_ & 512) != 0,
    });

    if !c.child.is_null() {
        // SAFETY: c.child is a valid CJson pointer in our tree.
        let first_child_id = unsafe { cjson_to_arena(arena, c.child) }?;
        arena.get_mut(id).child = Some(first_child_id);

        let mut prev_child_id = first_child_id;
        // SAFETY: c.child is valid; accessing ->next on a valid node.
        let mut sib_ptr = unsafe { (*c.child).next };
        while !sib_ptr.is_null() {
            // SAFETY: sib_ptr is in our linked chain.
            let sib_id = unsafe { cjson_to_arena(arena, sib_ptr) }?;
            arena.get_mut(prev_child_id).next = Some(sib_id);
            arena.get_mut(sib_id).prev = Some(prev_child_id);
            prev_child_id = sib_id;
            // SAFETY: sib_ptr is a valid node we built.
            sib_ptr = unsafe { (*sib_ptr).next };
        }
    }

    Some(id)
}

/// Run a parse, update GLOBAL_ERROR_PTR on failure, return C-heap tree or null.
/// SAFETY: input_ptr + position stays within the C buffer (parser invariant).
unsafe fn run_parse(input_ptr: *const c_char, slice: &[u8]) -> *mut CJson {
    let mut arena = Arena::new();
    match cjson_parse_with_length_opts(&mut arena, slice, false) {
        Ok((root_id, _)) => {
            // SAFETY: arena valid; arena_to_cjson allocates on C heap.
            unsafe { arena_to_cjson(&arena, root_id) }
        }
        Err(CJsonParseError { position }) => {
            // SAFETY: position <= slice.len() <= original buffer length.
            unsafe { GLOBAL_ERROR_PTR = input_ptr.add(position) };
            std::ptr::null_mut()
        }
    }
}

/// Like run_parse but uses NUL-truncating opts and writes return_parse_end.
/// SAFETY: same as run_parse; return_parse_end if non-null must be writable.
unsafe fn run_parse_with_opts(
    input_ptr: *const c_char,
    slice: &[u8],
    require_null_terminated: bool,
    return_parse_end: *mut *const c_char,
) -> *mut CJson {
    let mut arena = Arena::new();
    match cjson_parse_with_opts(&mut arena, slice, require_null_terminated) {
        Ok((root_id, parse_end)) => {
            if !return_parse_end.is_null() {
                // SAFETY: return_parse_end is a valid out-pointer from C caller.
                unsafe { *return_parse_end = input_ptr.add(parse_end) };
            }
            // SAFETY: arena valid; arena_to_cjson allocates on C heap.
            unsafe { arena_to_cjson(&arena, root_id) }
        }
        Err(CJsonParseError { position }) => {
            // SAFETY: position <= slice.len() <= original buffer length.
            unsafe { GLOBAL_ERROR_PTR = input_ptr.add(position) };
            if !return_parse_end.is_null() {
                // SAFETY: return_parse_end is a valid out-pointer from C caller.
                unsafe { *return_parse_end = input_ptr.add(position) };
            }
            std::ptr::null_mut()
        }
    }
}

/// Allocate a zeroed CJson leaf node of the given type.
/// SAFETY: caller must free via cJSON_Delete.
unsafe fn make_leaf(type_bits: c_int) -> *mut CJson {
    // SAFETY: alloc_cjson uses malloc internally.
    let ptr = unsafe { alloc_cjson() };
    if ptr.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: ptr is valid and zeroed; setting type_ field.
    unsafe { (*ptr).type_ = type_bits };
    ptr
}

/// Append `item` to `parent`'s child list, setting item->string = key.
/// SAFETY: parent and item must be valid non-null CJson pointers.
unsafe fn append_to_parent(parent: *mut CJson, item: *mut CJson, key: *mut c_char) -> bool {
    if parent.is_null() || item.is_null() {
        return false;
    }
    // SAFETY: parent and item are valid CJson pointers (caller contract).
    unsafe { (*item).string = key };
    // SAFETY: parent is valid; walking its ->next chain.
    if unsafe { (*parent).child.is_null() } {
        // SAFETY: parent is valid.
        unsafe { (*parent).child = item };
    } else {
        let mut tail = unsafe { (*parent).child };
        // SAFETY: tail stays within our allocated chain.
        while !unsafe { (*tail).next }.is_null() {
            tail = unsafe { (*tail).next };
        }
        // SAFETY: tail and item are valid CJson nodes.
        unsafe {
            (*tail).next = item;
            (*item).prev = tail;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// WAVE 1 — Parse / Delete / Error
// ---------------------------------------------------------------------------

/// cJSON_Parse: parse a NUL-terminated JSON string.
/// Returns a C-heap CJson tree (caller frees with cJSON_Delete) or null.
///
/// # Safety
/// `value` must be a valid NUL-terminated C string or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_Parse(value: *const c_char) -> *mut CJson {
    if value.is_null() {
        // SAFETY: null input → error pointer stays null.
        unsafe { GLOBAL_ERROR_PTR = std::ptr::null() };
        return std::ptr::null_mut();
    }
    // SAFETY: value is a valid NUL-terminated C string (caller contract).
    let bytes = unsafe { CStr::from_ptr(value) }.to_bytes();
    unsafe { run_parse(value, bytes) }
}

/// cJSON_ParseWithLength: parse exactly `buffer_length` bytes.
///
/// # Safety
/// `value` must point to at least `buffer_length` readable bytes, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_ParseWithLength(
    value: *const c_char,
    buffer_length: usize,
) -> *mut CJson {
    if value.is_null() || buffer_length == 0 {
        // SAFETY: null/zero → failure at position 0.
        unsafe { GLOBAL_ERROR_PTR = if value.is_null() { std::ptr::null() } else { value } };
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees buffer_length readable bytes at value.
    let slice = unsafe { std::slice::from_raw_parts(value as *const u8, buffer_length) };
    unsafe { run_parse(value, slice) }
}

/// cJSON_ParseWithOpts: parse with options, NUL-truncating.
///
/// # Safety
/// `value` must be a valid NUL-terminated C string or null.
/// `return_parse_end` if non-null must be a valid writable pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_ParseWithOpts(
    value: *const c_char,
    return_parse_end: *mut *const c_char,
    require_null_terminated: c_int,
) -> *mut CJson {
    if value.is_null() {
        // SAFETY: null input; set error pointer and out-param to null.
        unsafe { GLOBAL_ERROR_PTR = std::ptr::null() };
        if !return_parse_end.is_null() {
            // SAFETY: return_parse_end is a valid writable pointer (caller).
            unsafe { *return_parse_end = std::ptr::null() };
        }
        return std::ptr::null_mut();
    }
    // SAFETY: value is a valid NUL-terminated C string (caller contract).
    let bytes = unsafe { CStr::from_ptr(value) }.to_bytes();
    unsafe { run_parse_with_opts(value, bytes, require_null_terminated != 0, return_parse_end) }
}

/// cJSON_Delete: free a CJson tree and all its children/siblings.
///
/// Walks the `next` chain, recursing into `child` — mirrors cJSON's own
/// delete logic (DECISIONS.md §6).
///
/// # Safety
/// `item` must be null or a valid CJson pointer returned by a facade function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_Delete(item: *mut CJson) {
    let mut current = item;
    while !current.is_null() {
        // SAFETY: current is in our linked chain; always a valid pointer.
        let next = unsafe { (*current).next };
        // SAFETY: child is null or a valid subtree we allocated.
        let child = unsafe { (*current).child };
        if !child.is_null() {
            // SAFETY: child is a valid CJson pointer we allocated.
            unsafe { cJSON_Delete(child) };
        }
        // Free valuestring unless it's a reference (IsReference flag set)
        let type_bits = unsafe { (*current).type_ };
        let vs = unsafe { (*current).valuestring };
        if !vs.is_null() && (type_bits & 256 == 0) {
            // SAFETY: valuestring was malloc'd by bytes_to_cstring_heap.
            unsafe { libc::free(vs as *mut libc::c_void) };
        }
        // Free string (key) unless StringIsConst flag is set
        let s = unsafe { (*current).string };
        if !s.is_null() && (type_bits & 512 == 0) {
            // SAFETY: string was malloc'd by bytes_to_cstring_heap.
            unsafe { libc::free(s as *mut libc::c_void) };
        }
        // SAFETY: current was allocated by alloc_cjson (libc::malloc).
        unsafe { libc::free(current as *mut libc::c_void) };
        current = next;
    }
}

/// cJSON_GetErrorPtr: return the last parse error location.
///
/// # Safety
/// Valid only until the next parse call; do not free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_GetErrorPtr() -> *const c_char {
    // SAFETY: GLOBAL_ERROR_PTR written only by parse functions on same thread.
    unsafe { GLOBAL_ERROR_PTR }
}

// ---------------------------------------------------------------------------
// WAVE 1 — Print
// ---------------------------------------------------------------------------

/// cJSON_Print: render a cJSON tree to a malloc'd NUL-terminated string.
/// Caller must free the result with `free()`.
///
/// # Safety
/// `item` must be null or a valid CJson pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_Print(item: *const CJson) -> *mut c_char {
    if item.is_null() {
        return std::ptr::null_mut();
    }
    let mut arena = Arena::new();
    // SAFETY: item is a valid CJson pointer (caller contract).
    let Some(root_id) = (unsafe { cjson_to_arena(&mut arena, item) }) else {
        return std::ptr::null_mut();
    };
    let Some(bytes) = arena.print(root_id) else {
        return std::ptr::null_mut();
    };
    // SAFETY: bytes_to_cstring_heap allocates on C heap; caller frees.
    unsafe { bytes_to_cstring_heap(&bytes) }
}

// ---------------------------------------------------------------------------
// WAVE 1 — Minify
// ---------------------------------------------------------------------------

/// cJSON_Minify: strip whitespace and comments from JSON in-place.
///
/// # Safety
/// `json` must be a valid writable NUL-terminated C string or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_Minify(json: *mut c_char) {
    if json.is_null() {
        return;
    }
    // SAFETY: json is a valid NUL-terminated C string (caller contract).
    let bytes = unsafe { CStr::from_ptr(json) }.to_bytes();
    let result = crate::arena::minify(bytes);
    // Write result back in-place; result is always <= input length.
    // SAFETY: result.len() <= bytes.len(); json buffer holds bytes.len()+1.
    unsafe { std::ptr::copy_nonoverlapping(result.as_ptr(), json as *mut u8, result.len()) };
    // NUL-terminate at new length.
    // SAFETY: result.len() is within the original buffer allocation.
    unsafe { *json.add(result.len()) = 0 };
}

// ---------------------------------------------------------------------------
// WAVE 1 — Tree navigation (C-struct walks, no arena)
// ---------------------------------------------------------------------------

/// cJSON_GetObjectItemCaseSensitive: find a child by key (case-sensitive).
///
/// # Safety
/// `object` and `string` must be valid pointers or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_GetObjectItemCaseSensitive(
    object: *const CJson,
    string: *const c_char,
) -> *mut CJson {
    if object.is_null() || string.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: string is a valid NUL-terminated C string (caller contract).
    let key = unsafe { CStr::from_ptr(string) }.to_bytes();
    // SAFETY: object is a valid CJson pointer; walking its child chain.
    let mut current = unsafe { (*object).child };
    while !current.is_null() {
        // SAFETY: current is in our child chain; always valid.
        let cur_string = unsafe { (*current).string };
        if !cur_string.is_null() {
            // SAFETY: cur_string is a NUL-terminated C string we allocated.
            let cur_key = unsafe { CStr::from_ptr(cur_string) }.to_bytes();
            if cur_key == key {
                return current;
            }
        }
        current = unsafe { (*current).next };
    }
    std::ptr::null_mut()
}

/// cJSON_IsString: return 1 if item has type cJSON_String, else 0.
///
/// # Safety
/// `item` must be null or a valid CJson pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_IsString(item: *const CJson) -> c_int {
    if item.is_null() {
        return 0;
    }
    // SAFETY: item is a valid CJson pointer (caller contract).
    c_int::from(unsafe { ((*item).type_ & 0xFF) == CJSON_STRING })
}

/// cJSON_IsNumber: return 1 if item has type cJSON_Number, else 0.
///
/// # Safety
/// `item` must be null or a valid CJson pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_IsNumber(item: *const CJson) -> c_int {
    if item.is_null() {
        return 0;
    }
    // SAFETY: item is a valid CJson pointer (caller contract).
    c_int::from(unsafe { ((*item).type_ & 0xFF) == CJSON_NUMBER })
}

// ---------------------------------------------------------------------------
// WAVE 1 — Constructors
// ---------------------------------------------------------------------------

/// cJSON_CreateObject: allocate an empty JSON object node.
///
/// # Safety
/// Returns a heap-allocated pointer; caller must free with cJSON_Delete.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_CreateObject() -> *mut CJson {
    // SAFETY: make_leaf allocates via malloc.
    unsafe { make_leaf(CJSON_OBJECT) }
}

/// cJSON_CreateArray: allocate an empty JSON array node.
///
/// # Safety
/// Returns a heap-allocated pointer; caller must free with cJSON_Delete.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_CreateArray() -> *mut CJson {
    // SAFETY: make_leaf allocates via malloc.
    unsafe { make_leaf(CJSON_ARRAY) }
}

/// cJSON_CreateString: allocate a JSON string node copying `string`.
///
/// # Safety
/// `string` must be a valid NUL-terminated C string or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_CreateString(string: *const c_char) -> *mut CJson {
    if string.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: make_leaf allocates via malloc.
    let ptr = unsafe { make_leaf(CJSON_STRING) };
    if ptr.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: string is a valid NUL-terminated C string (caller contract).
    let bytes = unsafe { CStr::from_ptr(string) }.to_bytes();
    // SAFETY: bytes_to_cstring_heap allocates on C heap.
    let vs = unsafe { bytes_to_cstring_heap(bytes) };
    if vs.is_null() {
        // SAFETY: ptr was allocated by make_leaf.
        unsafe { libc::free(ptr as *mut libc::c_void) };
        return std::ptr::null_mut();
    }
    // SAFETY: ptr is a valid CJson node.
    unsafe { (*ptr).valuestring = vs };
    ptr
}

/// cJSON_CreateNumber: allocate a JSON number node.
///
/// # Safety
/// Returns a heap-allocated pointer; caller must free with cJSON_Delete.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_CreateNumber(num: c_double) -> *mut CJson {
    // SAFETY: make_leaf allocates via malloc.
    let ptr = unsafe { make_leaf(CJSON_NUMBER) };
    if ptr.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: ptr is a valid CJson node.
    unsafe {
        (*ptr).valuedouble = num;
        (*ptr).valueint = clamped_int_value(num);
    }
    ptr
}

// ---------------------------------------------------------------------------
// WAVE 1 — AddItemTo*
// ---------------------------------------------------------------------------

/// cJSON_AddItemToObject: append `item` to `object`'s child list with `string` key.
///
/// # Safety
/// All pointers must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_AddItemToObject(
    object: *mut CJson,
    string: *const c_char,
    item: *mut CJson,
) -> c_int {
    if object.is_null() || string.is_null() || item.is_null() {
        return 0;
    }
    // SAFETY: string is a valid NUL-terminated C string (caller contract).
    let bytes = unsafe { CStr::from_ptr(string) }.to_bytes();
    // SAFETY: bytes_to_cstring_heap allocates on C heap.
    let key_ptr = unsafe { bytes_to_cstring_heap(bytes) };
    if key_ptr.is_null() {
        return 0;
    }
    // SAFETY: object and item are valid CJson pointers.
    c_int::from(unsafe { append_to_parent(object, item, key_ptr) })
}

/// cJSON_AddItemToArray: append `item` to `array`'s child list (no key).
///
/// # Safety
/// All pointers must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_AddItemToArray(array: *mut CJson, item: *mut CJson) -> c_int {
    if array.is_null() || item.is_null() {
        return 0;
    }
    // SAFETY: array and item are valid CJson pointers; null key for arrays.
    c_int::from(unsafe { append_to_parent(array, item, std::ptr::null_mut()) })
}

// ---------------------------------------------------------------------------
// WAVE 1 — Compare
// ---------------------------------------------------------------------------

/// cJSON_Compare: recursively compare two trees.  Returns 1 or 0.
/// Null or invalid-type items return 0.
///
/// # Safety
/// `a` and `b` must be null or valid CJson pointers (may be stack-allocated).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_Compare(
    a: *const CJson,
    b: *const CJson,
    case_sensitive: c_int,
) -> c_int {
    if a.is_null() || b.is_null() {
        return 0;
    }
    // SAFETY: a and b are valid CJson pointers (caller contract).
    let a_base = unsafe { (*a).type_ } & 0xFF;
    let b_base = unsafe { (*b).type_ } & 0xFF;
    // Invalid type (0) or composite type (more than one bit set) → false
    if a_base == 0 || b_base == 0 {
        return 0;
    }
    if a_base & (a_base - 1) != 0 || b_base & (b_base - 1) != 0 {
        return 0;
    }
    let mut arena = Arena::new();
    // SAFETY: a and b are valid CJson pointers.
    let Some(id_a) = (unsafe { cjson_to_arena(&mut arena, a) }) else {
        return 0;
    };
    let Some(id_b) = (unsafe { cjson_to_arena(&mut arena, b) }) else {
        return 0;
    };
    c_int::from(arena.compare(id_a, id_b, case_sensitive != 0))
}

// ---------------------------------------------------------------------------
// WAVE 2 — cJSON_InitHooks (intentional no-op, DECISIONS.md §11)
// ---------------------------------------------------------------------------

/// cJSON_Hooks: mirrors cJSON.h's malloc_fn + free_fn pair.
#[repr(C)]
pub struct CJsonHooks {
    pub malloc_fn: Option<unsafe extern "C" fn(usize) -> *mut libc::c_void>,
    pub free_fn: Option<unsafe extern "C" fn(*mut libc::c_void)>,
}

/// cJSON_InitHooks: intentional no-op (DECISIONS.md §11).
///
/// Our allocator is Rust's arena (internal) + libc::malloc (materialisation).
/// Tests relying on a failing-malloc hook (cjson_add.c *_on_allocation_failure
/// group) will fail — reported honestly per AI_GUARDRAILS §0.
///
/// # Safety
/// `hooks` may be null (documented reset behaviour in cJSON).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_InitHooks(_hooks: *mut CJsonHooks) {
    // Intentional no-op — see DECISIONS.md §11.
}

// ---------------------------------------------------------------------------
// WAVE 2 — Add*ToObject helpers
// ---------------------------------------------------------------------------

/// Internal: create a typed leaf and append to object under name.
unsafe fn add_typed_to_object(
    object: *mut CJson,
    name: *const c_char,
    type_bits: c_int,
) -> *mut CJson {
    if object.is_null() || name.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: make_leaf allocates via malloc.
    let item = unsafe { make_leaf(type_bits) };
    if item.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: name is a valid NUL-terminated C string (caller contract).
    let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
    // SAFETY: bytes_to_cstring_heap allocates on C heap.
    let key_ptr = unsafe { bytes_to_cstring_heap(bytes) };
    if key_ptr.is_null() {
        // SAFETY: item was allocated by make_leaf.
        unsafe { libc::free(item as *mut libc::c_void) };
        return std::ptr::null_mut();
    }
    // SAFETY: object and item are valid CJson pointers.
    if !unsafe { append_to_parent(object, item, key_ptr) } {
        // SAFETY: key_ptr and item were malloc'd above.
        unsafe {
            libc::free(key_ptr as *mut libc::c_void);
            libc::free(item as *mut libc::c_void);
        }
        return std::ptr::null_mut();
    }
    item
}

/// cJSON_AddNullToObject
///
/// # Safety
/// `object` must be null or valid; `name` must be a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_AddNullToObject(
    object: *mut CJson,
    name: *const c_char,
) -> *mut CJson {
    unsafe { add_typed_to_object(object, name, CJSON_NULL) }
}

/// cJSON_AddTrueToObject
///
/// # Safety
/// See cJSON_AddNullToObject.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_AddTrueToObject(
    object: *mut CJson,
    name: *const c_char,
) -> *mut CJson {
    unsafe { add_typed_to_object(object, name, CJSON_TRUE) }
}

/// cJSON_AddFalseToObject
///
/// # Safety
/// See cJSON_AddNullToObject.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_AddFalseToObject(
    object: *mut CJson,
    name: *const c_char,
) -> *mut CJson {
    unsafe { add_typed_to_object(object, name, CJSON_FALSE) }
}

/// cJSON_AddBoolToObject
///
/// # Safety
/// See cJSON_AddNullToObject.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_AddBoolToObject(
    object: *mut CJson,
    name: *const c_char,
    boolean: c_int,
) -> *mut CJson {
    let type_bits = if boolean != 0 { CJSON_TRUE } else { CJSON_FALSE };
    unsafe { add_typed_to_object(object, name, type_bits) }
}

/// cJSON_AddNumberToObject
///
/// # Safety
/// See cJSON_AddNullToObject.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_AddNumberToObject(
    object: *mut CJson,
    name: *const c_char,
    number: c_double,
) -> *mut CJson {
    if object.is_null() || name.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: cJSON_CreateNumber allocates on C heap.
    let item = unsafe { cJSON_CreateNumber(number) };
    if item.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: name is a valid NUL-terminated C string.
    let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
    // SAFETY: bytes_to_cstring_heap allocates on C heap.
    let key_ptr = unsafe { bytes_to_cstring_heap(bytes) };
    if key_ptr.is_null() {
        // SAFETY: item was allocated by cJSON_CreateNumber.
        unsafe { libc::free(item as *mut libc::c_void) };
        return std::ptr::null_mut();
    }
    // SAFETY: object and item are valid CJson pointers.
    if !unsafe { append_to_parent(object, item, key_ptr) } {
        unsafe {
            libc::free(key_ptr as *mut libc::c_void);
            libc::free(item as *mut libc::c_void);
        }
        return std::ptr::null_mut();
    }
    item
}

/// cJSON_AddStringToObject
///
/// # Safety
/// See cJSON_AddNullToObject; `string` must be a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_AddStringToObject(
    object: *mut CJson,
    name: *const c_char,
    string: *const c_char,
) -> *mut CJson {
    if object.is_null() || name.is_null() || string.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: cJSON_CreateString copies string to C heap.
    let item = unsafe { cJSON_CreateString(string) };
    if item.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: name is a valid NUL-terminated C string.
    let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
    // SAFETY: bytes_to_cstring_heap allocates on C heap.
    let key_ptr = unsafe { bytes_to_cstring_heap(bytes) };
    if key_ptr.is_null() {
        // SAFETY: item was allocated by cJSON_CreateString (which called make_leaf).
        unsafe { cJSON_Delete(item) };
        return std::ptr::null_mut();
    }
    // SAFETY: object and item are valid CJson pointers.
    if !unsafe { append_to_parent(object, item, key_ptr) } {
        unsafe {
            libc::free(key_ptr as *mut libc::c_void);
            cJSON_Delete(item);
        }
        return std::ptr::null_mut();
    }
    item
}

/// cJSON_AddRawToObject
///
/// # Safety
/// See cJSON_AddNullToObject; `raw` must be a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_AddRawToObject(
    object: *mut CJson,
    name: *const c_char,
    raw: *const c_char,
) -> *mut CJson {
    if object.is_null() || name.is_null() || raw.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: make_leaf allocates via malloc.
    let item = unsafe { make_leaf(CJSON_RAW) };
    if item.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: raw is a valid NUL-terminated C string.
    let raw_bytes = unsafe { CStr::from_ptr(raw) }.to_bytes();
    // SAFETY: bytes_to_cstring_heap allocates on C heap.
    let vs = unsafe { bytes_to_cstring_heap(raw_bytes) };
    if vs.is_null() {
        unsafe { libc::free(item as *mut libc::c_void) };
        return std::ptr::null_mut();
    }
    // SAFETY: item is a valid CJson node.
    unsafe { (*item).valuestring = vs };
    // SAFETY: name is a valid NUL-terminated C string.
    let name_bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
    // SAFETY: bytes_to_cstring_heap allocates on C heap.
    let key_ptr = unsafe { bytes_to_cstring_heap(name_bytes) };
    if key_ptr.is_null() {
        unsafe { cJSON_Delete(item) };
        return std::ptr::null_mut();
    }
    // SAFETY: object and item are valid CJson pointers.
    if !unsafe { append_to_parent(object, item, key_ptr) } {
        unsafe {
            libc::free(key_ptr as *mut libc::c_void);
            cJSON_Delete(item);
        }
        return std::ptr::null_mut();
    }
    item
}

/// cJSON_AddObjectToObject
///
/// # Safety
/// See cJSON_AddNullToObject.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_AddObjectToObject(
    object: *mut CJson,
    name: *const c_char,
) -> *mut CJson {
    unsafe { add_typed_to_object(object, name, CJSON_OBJECT) }
}

/// cJSON_AddArrayToObject
///
/// # Safety
/// See cJSON_AddNullToObject.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_AddArrayToObject(
    object: *mut CJson,
    name: *const c_char,
) -> *mut CJson {
    unsafe { add_typed_to_object(object, name, CJSON_ARRAY) }
}

// ---------------------------------------------------------------------------
// WAVE 2 — Create*Array helpers (called only by failing-hooks tests)
// ---------------------------------------------------------------------------

/// cJSON_CreateIntArray
///
/// # Safety
/// `numbers` must point to at least `count` valid c_int values, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_CreateIntArray(
    numbers: *const c_int,
    count: c_int,
) -> *mut CJson {
    if numbers.is_null() || count <= 0 {
        return std::ptr::null_mut();
    }
    // SAFETY: make_leaf allocates via malloc.
    let arr = unsafe { make_leaf(CJSON_ARRAY) };
    if arr.is_null() {
        return std::ptr::null_mut();
    }
    for i in 0..count as usize {
        // SAFETY: numbers points to at least `count` valid c_int values.
        let n = unsafe { *numbers.add(i) } as c_double;
        // SAFETY: cJSON_CreateNumber allocates on C heap.
        let item = unsafe { cJSON_CreateNumber(n) };
        if item.is_null() {
            // SAFETY: arr is a valid CJson pointer we allocated.
            unsafe { cJSON_Delete(arr) };
            return std::ptr::null_mut();
        }
        // SAFETY: arr and item are valid CJson pointers.
        unsafe { append_to_parent(arr, item, std::ptr::null_mut()) };
    }
    arr
}

/// cJSON_CreateFloatArray
///
/// # Safety
/// `numbers` must point to at least `count` valid f32 values, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_CreateFloatArray(
    numbers: *const f32,
    count: c_int,
) -> *mut CJson {
    if numbers.is_null() || count <= 0 {
        return std::ptr::null_mut();
    }
    // SAFETY: make_leaf allocates via malloc.
    let arr = unsafe { make_leaf(CJSON_ARRAY) };
    if arr.is_null() {
        return std::ptr::null_mut();
    }
    for i in 0..count as usize {
        // SAFETY: numbers points to at least `count` valid f32 values.
        let n = unsafe { *numbers.add(i) } as c_double;
        // SAFETY: cJSON_CreateNumber allocates on C heap.
        let item = unsafe { cJSON_CreateNumber(n) };
        if item.is_null() {
            unsafe { cJSON_Delete(arr) };
            return std::ptr::null_mut();
        }
        unsafe { append_to_parent(arr, item, std::ptr::null_mut()) };
    }
    arr
}

/// cJSON_CreateDoubleArray
///
/// # Safety
/// `numbers` must point to at least `count` valid f64 values, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_CreateDoubleArray(
    numbers: *const c_double,
    count: c_int,
) -> *mut CJson {
    if numbers.is_null() || count <= 0 {
        return std::ptr::null_mut();
    }
    // SAFETY: make_leaf allocates via malloc.
    let arr = unsafe { make_leaf(CJSON_ARRAY) };
    if arr.is_null() {
        return std::ptr::null_mut();
    }
    for i in 0..count as usize {
        // SAFETY: numbers points to at least `count` valid f64 values.
        let n = unsafe { *numbers.add(i) };
        // SAFETY: cJSON_CreateNumber allocates on C heap.
        let item = unsafe { cJSON_CreateNumber(n) };
        if item.is_null() {
            unsafe { cJSON_Delete(arr) };
            return std::ptr::null_mut();
        }
        unsafe { append_to_parent(arr, item, std::ptr::null_mut()) };
    }
    arr
}

/// cJSON_CreateStringArray
///
/// # Safety
/// `strings` must point to at least `count` valid NUL-terminated C string
/// pointers, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cJSON_CreateStringArray(
    strings: *const *const c_char,
    count: c_int,
) -> *mut CJson {
    if strings.is_null() || count <= 0 {
        return std::ptr::null_mut();
    }
    // SAFETY: make_leaf allocates via malloc.
    let arr = unsafe { make_leaf(CJSON_ARRAY) };
    if arr.is_null() {
        return std::ptr::null_mut();
    }
    for i in 0..count as usize {
        // SAFETY: strings points to at least `count` valid C string pointers.
        let s = unsafe { *strings.add(i) };
        // SAFETY: s is a valid NUL-terminated C string.
        let item = unsafe { cJSON_CreateString(s) };
        if item.is_null() {
            unsafe { cJSON_Delete(arr) };
            return std::ptr::null_mut();
        }
        unsafe { append_to_parent(arr, item, std::ptr::null_mut()) };
    }
    arr
}
