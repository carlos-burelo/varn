# ADR-0013: Content-addressed `CheckerTyId` (Merkle hashing)

## Status
Accepted. Supersedes the `absorb`/`reintern`/`has_prefix` snapshot model of
ADR-0012. The `Arc<CheckerTyTable>` sharing and the `Arc<str>` names from
ADR-0012 stay; what changes is what an id **is**.

## Context
`CheckerTyId` was a positional index into `entries: Vec<InternedTypeKind>`. A
shape's id therefore depended on the *insertion order* of the table that minted
it. Two tables grown independently from a common snapshot could hold different
shapes at the same index — the root cause of the cross-module type-lineage bug
family. Every crossing needed a translation (`reintern`, `reintern_foreign_ty`,
`decode_source`, the `PortableType` remap) and a merge (`absorb`,
`set_ty_table`) whose result depended on who interned first. Determinism (Ley 4)
and parallelism both fought this model.

## Decision
Make a `CheckerTyId` the **128-bit hash of the shape it names**:

```rust
pub struct CheckerTyId(u128);   // 128-bit content hash
pub type InternedTypeKind = TypeKind<CheckerTyId, Atom, TyListId, FunctionTypeId, ObjectMembersId, ()>;
```

- The hash is Merkle: a shape's hash is computed from its children's hashes, so
  `TyListId`/`FunctionTypeId`/`ObjectMembersId` are content hashes of the list /
  function / member vector, and `Array<T>`'s id depends on `T`'s id.
- The ~21 intrinsic shapes keep reserved small ids (`0..=THIS`) so
  `Type::Int`/`Type::Str`/... remain `const`. Non-intrinsic ids set the top bit,
  so they can never collide with that range.
- The hash is `rustc_hash::FxHasher` (fixed seed, no `RandomState`) run twice
  with different salts into 128 bits. 128 bits is the standard collision width
  (rustc uses it for stable hashes); the birthday bound for 2^32 shapes is
  ~2^-64. `intern` also `debug_assert`s structural equality on a hit.
- `CheckerTyTable` is a memo `id -> shape` (plus the three sub-tables). It is a
  **pure cache**: identity never depends on it, only resolution does.
- `absorb` becomes a **commutative, idempotent set union** with no remap.
- `reintern`, `reintern_foreign_ty`, `reintern_list/function/object_members`,
  `has_prefix`, `index()`, `is_portable()` and `sanitize_foreign()` are deleted:
  ids are portable by construction.

## Ley 10 declaration (breaking change)
- **Gain**: the divergence class is gone by construction (Ley 2/3); determinism
  is structural, not enforced by ordering (Ley 4); ids are portable across
  tables and workers, which removes the serialization point that blocked
  parallel checking and enables incremental caching; deletes the recursive
  `reintern` machinery and the `has_prefix`/index-range heuristics.
- **Verification**: `checker_ty_table_invariants` (order-independence, union,
  Send + Sync), full `cargo test`, `tests/main.vn` `PASSED 1223 / FAILED 0` in
  dev/`@embedded` × JIT/no-JIT, bytecode dump SHA256 stable across processes and
  **identical** to the pre-change hash.
- **Deleted**: `CheckerTyTable::reintern*`, `absorb`-as-remap, `has_prefix`,
  `CheckerTyId::index/is_portable/sanitize_foreign`, the index-range checks in
  `cache.rs`/`checker`/`binder`.
- **Counterweight honoured**: no `unsafe`; the cost is `u128` ids (compile-time
  memory, not the runtime axis) and the gain is correctness + scale.

## Consequences
- `Type` grows from `(u32, bool)` to `(u128, bool)`; `Type` is compile-time only
  and is not on the runtime axis.
- `get`/`get_list`/`get_function`/`get_object_members` now panic if asked for a
  shape never interned into the table (the old `Vec` index did too); the absorb
  paths keep that from happening in practice.
- **Names caveat**: `TypeKind::Named`/`Generic` carry `Atom`, which is
  session-local. Ids are consistent for every worker sharing the interner
  (in-process parallel checking), but not across processes; the on-disk cache
  still re-interns shapes through the portable codec, which recomputes ids.
  Making `Atom` itself content-addressed is the follow-up that would make ids
  stable across processes too.

## Pending
- Parallel module-checking scheduler over the module DAG + a parallel checking
  test — designed and **deferred** in ADR-0014 (Option A, phase-parallel).
- Optionally: content-addressed `Atom` for cross-process id stability.
