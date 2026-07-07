# RediSearch rune trie — Rust FFI sketch

A signature-level sketch of a **safer-compatible** FFI surface (near-drop-in for
`src/trie/trie.h` + `src/trie/rune_util.h`, with a few deliberate deviations
called out in §3) for a Rust re-implementation of RediSearch's rune trie. Bodies
are `todo!()` on purpose — the deliverable is the API shape, the safety
contracts, and the memory-ownership model. `cargo build` (and `cargo build
--features runes_32bit`) both compile, so the signatures type-check against both
ABIs.

## 1. What actually needs exposing

**How I scoped this:** the exercise is "what's used *outside* the module". In a
real port I'd `grep` the codebase for call sites of each symbol and keep only
those referenced outside `src/trie/*` — e.g. `grep -rn "Trie_Insert\|NewTrie\|
Trie_Delete\|Trie_GetNode\|TrieType_Free" src/ --include=*.c` — then drop
anything only reachable from the trie's own `.c`/tests. Working from the headers
here, that leaves the basic lifecycle/CRUD surface below, and drops everything
iteration-related (`TrieIterator`, `Trie_Iterate*`, `Trie_CollectFuzzy`, and
`Trie_GetNode`'s shared-prefix offset machinery):

| Concern      | C API                                              | Rust FFI here |
|--------------|----------------------------------------------------|---------------|
| Construct    | `NewTrie(freecb, sortMode)`                        | `Trie_New` |
| Insert       | `Trie_InsertRune`, `Trie_InsertStringBuffer`       | `Trie_InsertRunes`, `Trie_InsertStringBuffer` |
| Find         | `Trie_GetNode(...)` (opaque node + offset)          | `Trie_Find` (see §3) |
| Remove       | `Trie_DeleteRunes`, `Trie_Delete`                  | `Trie_DeleteRunes`, `Trie_Delete` |
| Destroy      | `TrieType_Free(void*)`                             | `Trie_Free(*mut TrieMap)` + `TrieType_Free` shim |
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
3. **Payloads.** Insert takes the C API's own `RSPayload { char *data; uint32_t
   len; }` (a `ptr+len` view); the bytes are borrowed and copied into the trie,
   which then *owns* its copy. Internally the C trie stores payloads as
   `TriePayload { uint32_t len; char data[]; }` (a flexible-array-member struct) —
   that type is kept off the FFI boundary on purpose, since FAM structs are
   awkward and unsafe to build from Rust and callers only need a view. On
   remove/destroy the trie frees each owned payload through the
   `TrieFreeCallback`, preserving the C contract where the caller supplies the
   destructor. `Trie_Find` hands back a **borrowed** `RSPayload` view over the
   trie-owned `TriePayload`, valid only until the next mutation — documented
   per-function so a caller knows to copy, not free.
4. **Buffers Rust returns** (`Trie_StrToRunes`, `Trie_RunesToStr`). Rust-allocated,
   so they must return through `Trie_FreeRunes` / `Trie_FreeStr`. **This is the
   one place a naive drop-in would be unsound:** the C `rune_util.h` returns
   `malloc`'d memory that C frees with `free()`, but a Rust `Vec`/`Box` uses
   Rust's allocator — handing that to `free()` is UB. Providing matching Rust
   free functions is a small, necessary divergence from the C API.

## 3. Where I diverge from the C API, and why

This is a *safer-compatible* surface, not a 100% drop-in: insertion, deletion,
sizing and the rune converters keep the C signatures, but three things change on
purpose. Each is a safety/ergonomics win, and each has a compatibility escape
hatch.

- **`Trie_Find` instead of `Trie_GetNode`.** `Trie_GetNode` returns an opaque
  `TrieNode*` plus a shared-prefix `offsetOut` — that shape exists to support
  iteration and fuzzy traversal, which are explicitly out of scope. For "does
  this key exist, and what's its score/payload?", returning a `bool` with
  out-params is simpler, leaks no internal node type across the boundary, and
  keeps the Rust structure free to change. If exposing nodes later proves
  necessary, that's an opaque `*mut TrieNode` handle with accessor functions —
  never the raw struct. *(This is the one signature a caller of `Trie_GetNode`
  would have to adapt to.)*
- **Explicit `Trie_FreeRunes` / `Trie_FreeStr`** for Rust-allocated buffers
  (see §2.4) — additive, no existing signature changes.
- **Typed `Trie_Free(*mut TrieMap)`** as the primary destructor, **but
  `TrieType_Free(void*)` is kept** as a thin shim forwarding to it, so the exact
  symbol Redis's type-table registers still exists. Additive, fully compatible.
- Dropped the `RedisModuleString` insert overload (§1) — callers use the
  string-buffer form.

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
