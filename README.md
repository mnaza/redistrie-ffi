# RediSearch rune trie — Rust FFI sketch

A signature-level sketch of a safe FFI surface for a Rust re-implementation of
RediSearch's rune trie (`src/trie/trie.h`, `src/trie/rune_util.h`). Bodies are
`todo!()` on purpose — the deliverable is the API shape, the safety contracts,
and the memory-ownership model. `cargo build` (and `cargo build --features
runes_32bit`) both compile, so the signatures type-check against both ABIs.

## 1. What actually needs exposing

Walking `trie.h` / `rune_util.h` for symbols used *outside* the module and
dropping everything iteration-related (`TrieIterator`, `Trie_Iterate*`,
`Trie_CollectFuzzy`, `Trie_GetNode`'s offset machinery), the basic surface is:

| Concern      | C API                                              | Rust FFI here |
|--------------|----------------------------------------------------|---------------|
| Construct    | `NewTrie(freecb, sortMode)`                        | `Trie_New` |
| Insert       | `Trie_InsertRune`, `Trie_InsertStringBuffer`       | `Trie_InsertRunes`, `Trie_InsertStringBuffer` |
| Find         | `Trie_GetNode(...)` (opaque node + offset)          | `Trie_Find` (see §3) |
| Remove       | `Trie_DeleteRunes`, `Trie_Delete`                  | `Trie_DeleteRunes`, `Trie_Delete` |
| Destroy      | `TrieType_Free(void*)`                             | `Trie_Free(*mut TrieMap)` |
| Size         | `Trie_Size`                                         | `Trie_Size` |
| Rune convert | `strToRunes`, `runesToStr`                          | `Trie_StrToRunes`, `Trie_RunesToStr` (+ frees) |

Deliberately **not** exposed: `Trie_Insert(RedisModuleString*)` (couples the API
to the Redis module type — the caller can go through the string-buffer form);
`Trie_RandomKey`, `Trie_DecrementNumDocs`, fuzzy/iterator entry points (out of
scope); `runeBuf`/`runeBufFill`/`strToRunesN` (a stack-buffer optimisation that
is an implementation detail, not part of the trie contract).

`rune` is `uint16_t` today but the module supports `uint32_t` under
`TRIE_32BIT_RUNES`. That flag changes the ABI of every rune-taking function, so
it's a compile-time `rune` type alias gated by the `runes_32bit` cargo feature —
the Rust build flag must track the C one. `t_len` (a `uint16_t`) is kept as its
own alias so key-length ABI is explicit.

## 2. Memory: who allocates, who frees

This is the crux of doing the FFI safely. Four categories, each with one owner:

1. **The trie handle.** Rust-owned. `Trie_New` boxes it and returns `*mut TrieMap`
   (opaque — C never sees the layout); `Trie_Free` reclaims it. C holds only the
   pointer.
2. **Input keys** (`*const rune` / `*const c_char`). Caller-owned, **borrowed**
   for the duration of the call. Rust copies whatever it stores into its own
   nodes, so the caller's buffer can be freed immediately after the call.
3. **Payloads.** On insert the payload bytes are borrowed and copied into the
   trie, which then *owns* its copy. On remove/destroy the trie frees each owned
   payload through the `TrieFreeCallback` — preserving the C contract where the
   caller supplies the destructor. `Trie_Find` hands back a **borrowed** view
   (`RSPayload` pointing at trie-owned bytes) valid only until the next mutation;
   documented per-function so a caller knows to copy, not free.
4. **Buffers Rust returns** (`Trie_StrToRunes`, `Trie_RunesToStr`). Rust-allocated,
   so they must return through `Trie_FreeRunes` / `Trie_FreeStr`. **This is the
   one place a naive drop-in would be unsound:** the C `rune_util.h` returns
   `malloc`'d memory that C frees with `free()`, but a Rust `Vec`/`Box` uses
   Rust's allocator — handing that to `free()` is UB. Providing matching Rust
   free functions is a small, necessary divergence from the C API.

## 3. Where I diverge from the C API, and why

- **`Trie_Find` instead of `Trie_GetNode`.** `Trie_GetNode` returns an opaque
  `TrieNode*` plus a shared-prefix `offsetOut` — that shape exists to support
  iteration and fuzzy traversal, which are explicitly out of scope. For "does
  this key exist, and what's its score/payload?", returning a `bool` with
  out-params is simpler, leaks no internal node type across the boundary, and
  keeps the Rust structure free to change. If exposing nodes later proves
  necessary, that's an opaque `*mut TrieNode` handle with accessor functions —
  never the raw struct.
- **Explicit `Trie_Free*` functions** for returned buffers (see §2.4).
- **Typed `Trie_Free(*mut TrieMap)`** rather than `TrieType_Free(void*)`. The
  `void*` form exists so Redis's generic type-table can call it; a typed wrapper
  is safer for direct callers, and a `void*` shim can trivially forward to it if
  the module registration still needs that exact symbol.
- Dropped the `RedisModuleString` insert overload (§1).

## 4. Safety-doc convention

Every exposed function is `pub unsafe extern "C"` with a `# Safety` section
stating its invariants: pointers non-null and valid, `len` matching the buffer,
returned pointers' provenance and lifetime, and the "borrowed, don't free"
rule on `Trie_Find`'s payload. Internally the intended pattern is a thin
`unsafe` shell that validates inputs, converts raw pointers to slices with
`slice::from_raw_parts`, and delegates immediately to a 100% safe inner Rust
trie — so `unsafe` lives only at the boundary.

## 5. What I left out (next steps)

Function bodies; iteration/fuzzy surface; the `RedisModuleString` and
`numDocs`/`DecrementNumDocs` reference-counting semantics (they matter for a
true drop-in but are orthogonal to the create/insert/find/remove core asked
for); and a `cbindgen` config to generate the matching C header from these
signatures, which is how I'd keep the two sides in sync in a real port.
