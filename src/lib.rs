//! FFI surface for a Rust re-implementation of RediSearch's rune trie
//! (`src/trie/trie.h` + `src/trie/rune_util.h`).
//!
//! This is a *signature + safety-doc sketch*: the bodies are `todo!()` on
//! purpose. The goal is the shape of a memory-safe, *safer-compatible* C API
//! (near-drop-in, with a few deliberate and documented deviations — see
//! README.md §3), and an explicit account of who allocates and frees what. The
//! per-function `# Safety` sections are the contract a C caller must uphold.
//!
//! Design in one paragraph: the trie itself is Rust-owned and handed to C as an
//! opaque `*mut TrieMap` — C never sees its layout, so the Rust side is free to
//! change. Keys crossing the boundary are *borrowed* for the duration of a call
//! (C keeps ownership of its buffers). Anything Rust *returns by pointer* is
//! Rust-allocated and must come back through a Rust free function — never
//! `libc::free`, since mixing allocators is undefined behaviour.

#![allow(clippy::missing_safety_doc)] // safety is documented prose-style per fn below

use core::ffi::{c_char, c_int, c_void};

// ---------------------------------------------------------------------------
// Types crossing the boundary
// ---------------------------------------------------------------------------

/// A Unicode code unit, ABI-compatible with the C `rune`.
///
/// `uint16_t` by default (Basic Multilingual Plane only, matching current
/// RediSearch); `uint32_t` when built with the `runes_32bit` feature, which
/// must be kept in lock-step with the C `TRIE_32BIT_RUNES` compile flag.
#[cfg(not(feature = "runes_32bit"))]
#[allow(non_camel_case_types)]
pub type rune = u16;
#[cfg(feature = "runes_32bit")]
#[allow(non_camel_case_types)]
pub type rune = u32;

/// Length type used by the C trie for key lengths (`t_len` in RediSearch,
/// a `uint16_t`). Kept as its own alias so the ABI is explicit.
#[allow(non_camel_case_types)]
pub type t_len = u16;

/// Opaque handle to a Rust-owned trie. C code only ever holds a pointer to it;
/// the fields are private so the Rust implementation can evolve freely.
pub struct TrieMap {
    _private: [u8; 0],
}

/// How the map orders entries — mirrors `TrieSortMode` in C.
///
/// `#[repr(C)]` fixes the discriminant values so the enum is ABI-compatible.
/// Values match the C enum exactly: `Trie_Sort_Lex = 0`, `Trie_Sort_Score = 1`.
/// (Getting these backwards is a silent ABI bug — C would ask for one ordering
/// and Rust would apply the other — so they are pinned to the C values here.)
#[repr(C)]
pub enum TrieSortMode {
    Lex = 0,
    Score = 1,
}

/// A `(ptr, len)` view over payload bytes — matches `RSPayload { char *data;
/// uint32_t len; }`, which is the payload type the C `Trie_Insert*` functions
/// already take, so insertion stays byte-for-byte compatible.
///
/// Note: internally the C trie *stores* payloads as `TriePayload { uint32_t len;
/// char data[]; }` (a flexible-array-member struct). That type is deliberately
/// **not** exposed across the FFI: FAM structs are awkward and error-prone to
/// construct from Rust, and callers only ever need a view. On insert `data` is
/// borrowed and copied in; on lookup ([`Trie_Find`]) the same `(ptr, len)` shape
/// is handed back as a borrowed view over the trie-owned `TriePayload`.
#[repr(C)]
pub struct RSPayload {
    pub data: *mut c_char,
    pub len: u32,
}

/// Callback the trie invokes to free a stored payload it owns, matching
/// `TrieFreeCallback`. `None` means "no destructor" (payloads are plain data).
pub type TrieFreeCallback = Option<unsafe extern "C" fn(payload: *mut c_void)>;

// ---------------------------------------------------------------------------
// Lifecycle: allocate / free the map
// ---------------------------------------------------------------------------

/// Create a new, empty trie. Returns an owning handle; free it with
/// [`Trie_Free`]. Never returns null (allocation failure aborts, as elsewhere
/// in RediSearch).
///
/// # Safety
/// - `freecb`, if `Some`, must be a valid function pointer callable for the
///   whole lifetime of the returned trie.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Trie_New(
    freecb: TrieFreeCallback,
    sort_mode: TrieSortMode,
) -> *mut TrieMap {
    let _ = (freecb, sort_mode);
    todo!()
}

/// Destroy a trie and every entry/payload it owns (invoking the free callback
/// per payload). After this call the pointer is dangling and must not be reused.
///
/// This is the typed destructor; the existing `void`-typed
/// [`TrieType_Free`] is kept as a shim that forwards here (see README §3).
///
/// # Safety
/// - `t` must be a non-null pointer returned by [`Trie_New`] and not already freed.
/// - No other pointer into the trie (e.g. a borrowed payload from [`Trie_Find`])
///   may be used after this returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Trie_Free(t: *mut TrieMap) {
    let _ = t;
    todo!()
}

/// Drop-in shim for the existing `void`-typed destructor. RediSearch registers
/// `TrieType_Free(void*)` in its Redis type table, so the exact symbol must keep
/// existing; it simply forwards to [`Trie_Free`]. Direct callers should prefer
/// the typed [`Trie_Free`].
///
/// # Safety
/// - `value` must be null or a pointer returned by [`Trie_New`], not already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn TrieType_Free(value: *mut c_void) {
    let _ = value; // forwards to Trie_Free(value as *mut TrieMap)
    todo!()
}

/// Number of entries in the trie.
///
/// # Safety
/// - `t` must be a valid, non-null trie pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Trie_Size(t: *const TrieMap) -> usize {
    let _ = t;
    todo!()
}

// ---------------------------------------------------------------------------
// Insertion
// ---------------------------------------------------------------------------

/// Insert a key given as a rune array. Returns 1 if a new entry was created,
/// 0 if an existing one was updated (matching `Trie_InsertRune`'s `int`).
///
/// If `incr` is non-zero the score is added to any existing score rather than
/// replacing it. `payload` may be null.
///
/// # Safety
/// - `t` must be a valid trie pointer.
/// - `s` must point to `len` readable `rune`s; it is only borrowed for the call.
/// - `payload`, if non-null, must point to a valid [`RSPayload`] whose `data`
///   covers `len` bytes; the trie copies what it retains.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Trie_InsertRunes(
    t: *mut TrieMap,
    s: *const rune,
    len: t_len,
    score: f64,
    incr: c_int,
    payload: *const RSPayload,
) -> c_int {
    let _ = (t, s, len, score, incr, payload);
    todo!()
}

/// Insert a key given as a UTF-8 buffer (converted to runes internally).
/// Semantics otherwise identical to [`Trie_InsertRunes`]; mirrors
/// `Trie_InsertStringBuffer`.
///
/// # Safety
/// - `t` must be a valid trie pointer.
/// - `s` must point to `len` readable bytes of UTF-8; borrowed for the call only.
/// - `payload` as in [`Trie_InsertRunes`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Trie_InsertStringBuffer(
    t: *mut TrieMap,
    s: *const c_char,
    len: usize,
    score: f64,
    incr: c_int,
    payload: *const RSPayload,
) -> c_int {
    let _ = (t, s, len, score, incr, payload);
    todo!()
}

// ---------------------------------------------------------------------------
// Lookup
// ---------------------------------------------------------------------------

/// Exact-match lookup. Returns `true` if the key exists; on success writes its
/// score to `*score_out` (when non-null) and a *borrowed* view of its payload
/// to `*payload_out` (when non-null).
///
/// This intentionally departs from `Trie_GetNode`, which returns an opaque
/// `TrieNode*` plus a shared-prefix offset — machinery that only makes sense
/// for iteration/fuzzy walks, which are out of scope. See README.
///
/// # Safety
/// - `t` must be a valid trie pointer; `s` must point to `len` readable runes.
/// - `score_out`/`payload_out`, if non-null, must be valid for a single write.
/// - The `data` pointer written into `*payload_out` borrows trie-owned memory
///   and is invalidated by the next mutation of `t` or by [`Trie_Free`]; the
///   caller must copy it before then and must not free it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Trie_Find(
    t: *const TrieMap,
    s: *const rune,
    len: t_len,
    score_out: *mut f64,
    payload_out: *mut RSPayload,
) -> bool {
    let _ = (t, s, len, score_out, payload_out);
    todo!()
}

// ---------------------------------------------------------------------------
// Removal
// ---------------------------------------------------------------------------

/// Remove an entry by rune key, freeing its payload via the trie's callback.
/// Returns 1 if an entry was removed, 0 if it was not found. Mirrors
/// `Trie_DeleteRunes`.
///
/// # Safety
/// - `t` must be a valid trie pointer; `s` must point to `len` readable runes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Trie_DeleteRunes(t: *mut TrieMap, s: *const rune, len: t_len) -> c_int {
    let _ = (t, s, len);
    todo!()
}

/// Remove an entry by UTF-8 key. Mirrors `Trie_Delete`.
///
/// # Safety
/// - `t` must be a valid trie pointer; `s` must point to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Trie_Delete(t: *mut TrieMap, s: *const c_char, len: usize) -> c_int {
    let _ = (t, s, len);
    todo!()
}

// ---------------------------------------------------------------------------
// rune_util: conversions whose results Rust allocates (and must free)
// ---------------------------------------------------------------------------

/// Convert a UTF-8 string to a newly-allocated rune array, writing the rune
/// count to `*len`. Mirrors `strToRunes`.
///
/// # Safety
/// - `str` must point to a valid buffer; `len` must be a valid pointer for one write.
/// - The returned pointer is **Rust-allocated** and must be released with
///   [`Trie_FreeRunes`] — not `free()`. Returns null on allocation/decoding failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Trie_StrToRunes(str: *const c_char, len: *mut usize) -> *mut rune {
    let _ = (str, len);
    todo!()
}

/// Convert a rune array back to a newly-allocated UTF-8 C string, writing the
/// byte length to `*utflen`. Mirrors `runesToStr`.
///
/// # Safety
/// - `runes` must point to `len` readable runes; `utflen` valid for one write.
/// - The returned pointer is **Rust-allocated**; release it with
///   [`Trie_FreeStr`], not `free()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Trie_RunesToStr(
    runes: *const rune,
    len: usize,
    utflen: *mut usize,
) -> *mut c_char {
    let _ = (runes, len, utflen);
    todo!()
}

/// Free a rune buffer previously returned by [`Trie_StrToRunes`].
///
/// # Safety
/// - `runes` must be null or a pointer returned by [`Trie_StrToRunes`] with the
///   matching `len`, not previously freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Trie_FreeRunes(runes: *mut rune, len: usize) {
    let _ = (runes, len);
    todo!()
}

/// Free a C string previously returned by [`Trie_RunesToStr`].
///
/// # Safety
/// - `s` must be null or a pointer returned by [`Trie_RunesToStr`], not
///   previously freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Trie_FreeStr(s: *mut c_char) {
    let _ = s;
    todo!()
}
