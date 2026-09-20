# ADR-0014: Parallel module checking — phase-parallel scheduler (Option A)

## Status
**Deferred / archived.** Designed, not implemented. Implement when compile-time
scalability on multi-module projects becomes the priority. The prerequísites are
already in place (see below), so this is a scheduler, not a data-model change.

## Context
Checking is currently single-threaded: `vn check`/`run`/`build` analyse one
program on one thread, resolving imports on demand (nested binds during the
entry check). Options 1/3 from the ADR-0013 discussion were rejected; Option 2
(content-addressed ids) was implemented, which removed the serialization point
that would have made parallel checking non-deterministic.

Prereqísites that are now true (ADR-0012/0013):

- `CheckerTyTable`, `BindResult`, `ModuleGraph`, `DiskResolver` are all
  `Send + Sync` (compile-time assertions in `checker_ty_table_invariants.rs`).
- `CheckerTyId` is content-addressed, so two workers agree on every id and
  merging tables is a commutative, idempotent union (`CheckerTyTable::absorb`).
- `DiskResolver`'s interior mutability is `parking_lot` locks, so one resolver
  can be shared across worker threads.

## Decision (deferred): phase-parallel scheduler over the module DAG
Option A — parallelise by **phase** and by **topological level**, not by
continuous work stealing.

1. **Discovery**: resolve the whole module graph (imports) before checking.
   This is I/O-bound and already has a canonical resolver (`varn-modules`).
2. **Level sort**: compute the DAG's level order (a module's level = 1 +
   max(level of its imports)); independent modules share a level.
3. **Parallel parse + bind**: process each level's modules in parallel on a
   thread pool sharing one `DiskResolver`. Each worker keeps its own
   `Arc<CheckerTyTable>` snapshot; content-addressed ids make them compatible.
4. **Parallel check**: same level-parallel scheme per module.
5. **Deterministic merge**: union all worker tables into the live table
   (`absorb`, order-independent). Sort diagnostics by `ModuleId` (then span)
   before emitting, so the observable order does not depend on thread
   scheduling (Ley 4).

### Why A, not work stealing (Option B)
- **Determinism is trivial**: fixed level barriers + a final sort give the same
  diagnostics/bytecode regardless of thread timing. Work stealing needs
  per-node deterministic tie-breaking everywhere.
- **Locality**: a level's modules are independent, so no lock contention on the
  shared resolver beyond the graph/lock-table touch points.
- **Simplicity**: no scheduler state machine; a `thread::scope` per level (or
  `rayon::scope`) suffices. Can be upgraded to B later if profiling shows the
  barriers dominate.

## Verification plan
- **Equality**: parallel and sequential runs must produce identical diagnostics
  (after the canonical sort) and identical bytecode SHA256.
- **Gate**: `tests/main.vn` `PASSED: N / FAILED: 0` in dev/`@embedded` ×
  JIT/no-JIT, same matrix as today.
- **Scaling check**: time a many-module project at 1 vs N threads; report the
  speedup (Ley 10: the gain must be measured, not assumed).

## Risks / open questions
- **Shared `AtomInterner`**: names are still session-local (`Atom` is not
  content-addressed). Workers sharing the interner see the same `Atom` for the
  same text, so in-process parallel is correct; but if a worker interns new
  names concurrently, `Atom` numbering depends on order. Discovery/pre-pass
  interning of all names before the parallel phase removes the race; otherwise
  content-address `Atom` (follow-up from ADR-0013).
- **Diagnostics ordering**: must be canonicalised (sort by `ModuleId`, then
  span) or the "deterministic" claim is false.
- **Cache writes**: the on-disk interface cache writes per module; make the
  artifact write path collision-free under concurrency (already uses a unique
  temp name — re-verify).
- **`thread_local!` pipeline resolver**: `varn-pipeline/src/resolver.rs` holds
  one resolver per thread; a shared-resolver pool supersedes it.

## Trigger to implement
- Compile-time wall clock on multi-module projects becomes a bottleneck, or the
  editor/LSP needs to analyse many modules concurrently.
