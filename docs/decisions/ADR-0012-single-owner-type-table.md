# ADR-0012: Single-owner, immutable `CheckerTyTable` (Arc + interned names)

## Status
Accepted. Step 2 of Ley 3 (single owner). Supersedes the snapshot/`absorb` model for
in-process sharing.

## Context
`CheckerTyTable` (and `AtomInterner`) are today cloned per `Binder`/`Checker` and grown
locally, then merged with `absorb`/`set_ty_table`. Because two copies can grow in
parallel from a common snapshot, they diverge at the same index — the root cause of the
cross-module type-lineage family of bugs. Every crossing then needs a translation
(`reintern`, `reintern_foreign_ty`, `decode_source`, portable codec) to compensate.

The blocking property of a `RefCell`-shared table is re-entrancy: `resolve_type_node` /
`infer_expr_type` hold `&mut table` while calling the resolver, which binds nested
modules and would borrow the same table again.

## Decision
Make the table **immutable and shared by `Arc`**, published once per phase:

```rust
pub struct CheckerTyTable { /* internals as today, no RefCell */ }
// Binder/Checker hold Arc<CheckerTyTable>; mutation is copy-on-write:
let t: &mut CheckerTyTable = Arc::make_mut(&mut self.ty_table);
```
- `Arc::make_mut` clones only when the table is shared, so a nested bind that needs to
  grow gets its own copy — **no re-entrancy, no `unsafe`**.
- Snapshots become `Arc::clone` (O(1)); the resolver keeps the live `Arc`.
- Intra-process `absorb`/`set_ty_table`/`reintern` disappear once every phase receives
  its table as input and returns the grown one; the portable codec stays only for the
  on-disk interface.

**Names inside the table become `Atom`, not `Rc<str>`.** `FunctionType::type_params`,
`FunctionParam::name`, `ObjectTypeMember` names and `ClassMemberInfo::name` currently use
`Rc<str>`, which makes the table `!Send + !Sync` and defeats `Arc` for parallelism. Since
the per-compilation `AtomInterner` already exists, interning these names (a) removes the
`Rc<str>` duplication, (b) makes the table `Send + Sync`, and (c) enables parallel
module checking and incremental caching.

`Arc<Vec>` + copy-on-publish is O(n) per phase; tables are hundreds-to-thousands of
entries, so this is sufficient. `im` (persistent, O(log n)) is a measured follow-up only
if publish cost shows up (Ley 10: no pre-optimisation without data).

## Ley 10 declaration (breaking change)
- **Gain**: eliminates the divergence class (Ley 2/3), O(1) snapshots, `Send + Sync` ⇒
  parallel/incremental module checking, deletes `absorb`/`set_ty_table`/`reintern`
  in-process.
- **Verification**: full suite (`dev`/`@embedded`/no-JIT 1223/0), bytecode SHA256 stable
  across processes, `CheckerTyTable` invariant tests, plus a test checking two modules in
  parallel.
- **Deleted**: `RefCell` in `DiskResolver.ty_table`, `Binder::sync_ty_table`,
  `CheckerTyTable::absorb` (in-process), `ImportResolver::set_ty_table`,
  `reintern_foreign_ty`, `decode_source`.
- **Counterweight honoured**: no `unsafe`; the gain is correctness/scale, not a
  compile-time micro-benchmark.

## Consequences
- `ImportResolver::ty_table_snapshot` returns `Arc<CheckerTyTable>`.
- `Binder`/`Checker` fields change type; the 60 `&mut …ty_table` sites become
  `Arc::make_mut`.
- ~96 type constructors and ~420 `Rc` sites migrate as names move to `Atom`.

## Migration (each step part of the final design, each green)
1. `get` returns the shape by value — **done** (`d6bfff10`), prerequisite so no call site
   borrows the table.
2. `Arc<CheckerTyTable>` + `Arc::make_mut` at the 60 mutation sites; snapshots as `Arc`.
3. Names in table contents: `Rc<str>` → `Atom`; table becomes `Send + Sync`.
4. Delete `absorb`/`set_ty_table`/`reintern` in-process paths; keep the disk codec.
5. Parallel module checking test.
