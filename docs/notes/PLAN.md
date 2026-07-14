# PLAN — `varn-opt`: the optimizing backend (AST → HIR → SSA → opt → bytecode)

> **Nota de estado (2026-07-13).** Estas son notas de trabajo, no documentación
> normativa. Las rutas que mencionan `crates/varn-compiler/` son de antes de la
> migración: **ese crate ya no existe**. El compilador es `varn-opt` y los
> post-passes viven en `varn-backend`. Para la arquitectura actual, ver
> [ARCHITECTURE.md](../ARCHITECTURE.md) y
> [COMPILER_ARCHITECTURE.md](../COMPILER_ARCHITECTURE.md).

Living roadmap. **`varn-opt` is now the sole codegen backend** — the legacy
direct AST→bytecode codegen has been deleted (Hito A, §4). What remains is to
grow the SSA layer to full coverage and then land the optimization passes that
are the whole point: specialize on the checker's static types and beat JS with
**no speculation / no deopt**.

---

## 0. Context

### Thesis
V8 must *speculate* to optimize untyped JS (guards + deopt). Varn's checker
*knows* the types, so an SSA optimizer can specialize directly — the performance
ceiling is set by the IR, not the JIT. Measured motivation (earlier sessions):
- a function call costs **~4× the work** of a leaf like `add(a,b)=a+b`;
- native builtins are only **~10%** of execute — VM execution is the bottleneck;
- an AST leaf-inliner gave **~8×** on call-bound code but only for trivial
  callees, because there was **no optimizer IR**. SSA + inline-at-scale is the fix.

### Pipeline (current)
```
AST ──► HIR ──┬─► HIR→bytecode (lower/)            ─┐
              │   the default production emitter     │
              │                                      ├─► FunctionProto ─► regalloc_post ─► slot_kinds ─► VM ─► JIT
              └─► HIR→SSA→bytecode (ssa/)           ─┘   (varn-compiler backend)          (existing)
                  experimental, behind VN_OPT_SSA;
                  per-function fallback to lower/
```
- **Frontend → `varn-opt`**: `compile_direct` (in `varn-compiler`) calls
  `varn_opt::compile` unconditionally, then `run_backend_post_passes`
  (`regalloc_post` + `slot_kinds::infer`) over the whole proto tree.
- **Backend reused unchanged**: `regalloc_post`/`liveness`/`ir` +
  `analysis/slot_kinds` stay in `varn-compiler`; the register VM (`varn-vm`) and
  JIT (`varn-jit`) consume the same `FunctionProto`.

### Crate layout (`crates/varn-opt/src/`)
- `hir/` — AST→HIR lowering (`hir/lower/{mod,decl,stmt,expr}.rs`) + HIR types
  (`hir/mod.rs`). Typed, desugared, control-flow-explicit, names resolved.
- `lower/` — the **HIR→bytecode** emitter (`{mod,stmt,expr,class,control}.rs`).
  Naive register model (fresh temp per read); `regalloc_post` compresses. This is
  the production path today.
- `ssa/` — the **HIR→SSA→bytecode** path: `ir.rs` (block-param SSA IR),
  `build.rs` (Braun on-the-fly construction), `verify.rs` (verifier), `emit.rs`
  (out-of-SSA → bytecode), `dump.rs`. Gated behind `VN_OPT_SSA`.

### Integration contract (preserve)
- Entry: `varn_compiler::codegen::compile_direct(program, annotations,
  extension_calls, extension_members, extension_set_members, export_names) ->
  Result<FunctionProto, Rc<str>>`.
- Output: `varn_types::{Chunk, FunctionProto}`.
- Inputs: `varn_core::TypeAnnotations` (`get_numeric`, `get_intrinsic`,
  `get_call_mapping`, `is_reassigned_name`, …).
- Deps: `varn-opt` → `varn-core`, `varn-types` only (no cycle).

### Dev gates
- `VN_OPT_SSA` — route each function through the SSA path (`ssa::try_compile_function`),
  falling back to `lower/` on `Err(Unsupported)`. Removed once SSA is the default.
- `VN_OPT_TRACE` — prints which modules/functions SSA compiled vs fell back.

---

## 1. Frontend + HIR→bytecode (production path) — ✅ COMPLETE

`AST → HIR → bytecode` covers **100% of the language** the test suite exercises
(51 comprehensive feature files imported by `tests/main.vn`): scalar/typed
arithmetic, all literals, control flow (`if`/`while`/`for`/`do`/`for-of`/`for-in`/
`switch`/`try`-`catch`-`finally`/`using`), closures + upvalues + nested functions,
member/index/method access with inline caches, classes (inheritance, super,
static, getters/setters, decorators, destructors), enums + `match` (incl.
guards), namespaces, extensions, modules (`import`/`export` incl. default/named/
all/re-export), async/generators, default/optional/rest params, generics
(type-erased), optional chaining, assignment targets, etc.

Verified: **suite 728/728** with no env var; `bench` clean (execute p50 ~16.8 ms).

Known intentional simplifications (mirror the deleted legacy, not regressions):
- `match` `Sequence`/`Type` patterns lower as wildcard (no nested destructuring
  yet — a future feature); proper labeled `break`/`continue` target the innermost
  loop; bigint literals cap at i128 (overflow → 0).

The remaining `Err(Unsupported)` sites in `hir/lower` are
**parser/checker-unreachable** defensive cases (non-identifier lvalues, etc.).
They are `Err` (not `panic!`), but with legacy gone there is no fallback — if one
is ever hit, `compile_direct` returns a compile error instead of crashing.

---

## 2. Stage 2 — SSA (65% — ACTIVE)

Build the real optimizer IR from HIR, per function, then lower back to bytecode.
Gated behind `VN_OPT_SSA` with per-function fallback to the `lower/` emitter, so
the compiler stays green while coverage grows.

### Architecture decision (locked) — block params + Braun on-the-fly
- **Block parameters** (Cranelift/MLIR style) instead of phi nodes: merge
  operands live on predecessor edges as block arguments. Isomorphic to phis, no
  in-block phi placement to keep consistent.
- **Braun et al. 2013 on-the-fly construction**: SSA built in one pass during HIR
  lowering (per-block `write`/`read` def maps; a sealed-block read with >1 pred
  spawns a block param and recurses into preds; an unsealed block — a loop header
  whose back-edge isn't built yet — records an incomplete phi filled at seal
  time). No dominance-frontier pass → faster; produces minimal SSA.
- A **dominator tree is computed separately** only where needed (verifier, future
  passes), not for construction.

### 2.1 / 2.2 CFG construction + SSA form 🟡 PARTIAL
Done (`ssa/build.rs`, `ssa/ir.rs`):
- Scalar dataflow (params/locals → `Value`s, each carrying its `HirType`);
  literals, typed `Binary`/`Unary`, assignment-as-expression, `++`/`--`, `Return`.
- **`if`/`else`** (branch + merge block params).
- **Loops** — `while`, C-style `for` (`update` in its own block so `continue`
  runs it), `do-while`, plus `break`/`continue` via a `LoopCtx` target stack.
  Exercises the incomplete-phi / unsealed-header seal path.
- **Calls** — global reads (`LoadGlobal`), plain `Call`, and `SelfCall`
  (self-recursion). Out-of-SSA emits the plain-call ABI (null receiver + args
  contiguous from `call_base`, above every value register); the regalloc
  callee-frame bail (below) keeps it correct. Verified end-to-end: `fib` (self
  recursion), `addOne(x)+addOne(x)` (two calls in one expr → 42), and a call in a
  loop all run identically with/without `VN_OPT_SSA`.
- **Member / index reads** — `object.name` → `GetProperty` (with an inline-cache
  slot; `cache_count`/`ic_cache`/`feedback` sized in `emit`), `object[index]` →
  `GetIndex`. Verified: field read, element read, and `a[i]` summed in a loop run
  identically with/without `VN_OPT_SSA`.
- **Member / index writes** — `object.name = v` → `SetProperty` (IC slot),
  `object[index] = v` → `SetIndex`, as statements (`HirStmt::SetMember/SetIndex`)
  and assignment-expressions (`HirAssignTarget::Member/Index`, yields the value).
  Dest-less side-effect insts (`emit_effect`). Compound member assignment
  (`o.c = o.c + 1`) works via read+write. Verified: field/elem writes, compound
  bump, array fill-then-sum in a loop → identical with/without `VN_OPT_SSA`.
- **Method calls** — `recv.name(args)` → `CallMethod` (IC slot). Receiver passed
  in the opcode (read separately by the VM, not in the args block); args
  contiguous from `call_base`. Required dropping `regalloc_post`'s `hi3 > 1` gate
  on `CallMethod` so the callee-frame bail covers 0/1-arg method calls too
  (contiguity check is a no-op there). Verified: `slice`/`push`/`length`, two
  method results in one expr → identical with/without `VN_OPT_SSA`; the
  method-call-heavy suite stays 728/728.
- **Logical + conditional** — `&&`/`||`/`??` and the ternary `?:` via a
  `lower_branch_value` helper (branch on the condition, evaluate one arm per
  successor, merge through a single block-param result phi). `??` uses a new
  `IsNull` instruction. Verified: `&&`/`||`/`??`, ternary, and nested ternary
  identical with/without `VN_OPT_SSA`.
- **Array / object literals** — `[a, b, …]` → `BuildArray` (elements contiguous
  in the `call_base` block; no spread/holes), `{ k: v, … }` → `BuildObject`
  (static keys, value props; each value reg listed individually — no contiguity).
  Verified: array/object build, index/field read-back, nested
  `{ list: [x, x+1] }` → identical with/without `VN_OPT_SSA`.
- **Template literals** — `` `a${x}b` `` → `ToString` per interpolation + `BuildStr`
  over the parts (literal chunks are `ConstStr`). Verified: interpolation,
  arithmetic + ternary inside `${…}`, plain (no-interp) → identical both paths.
- **Capture-free closures** — arrows / fn-exprs / nested fns with no upvalues →
  `LoadStaticFn` (the nested fn is compiled via `lower::lower_function`, which
  re-enters the SSA gate per-function, so nested arrows are SSA-compiled too).
  Closures that capture upvalues are **deferred** (the captured local needs a
  stable slot, which SSA renaming breaks) → fall back. Verified: `(n) => n*2`,
  `arr.map(n => n+10)`, a stored arrow → identical with/without `VN_OPT_SSA`.
- **Intrinsics** — `Math.*` etc. (`IntrinsicCall`) → `Intrinsic` opcode; operands
  `[object, args…]` laid out contiguously in the `call_base` block, result copied
  to the value's reg. Verified: `Math.abs`/`sqrt`/`floor` lower to `intrinsic#N`
  (SSA dump) and run identically with/without `VN_OPT_SSA`.
- **Simple expressions** — `expr!` (`AssertNotNull`, dest-less side effect),
  comma `Sequence` (eval all, yield last), `?.` member (`GetPropertyMaybe`),
  module-slot reads (`LoadModuleSlot`), and decimal / bigint / regex literals
  (`LoadConst`; regex → `/pat/flags` string). Verified: `x!`, decimal, bigint,
  sequence run identically; suites green.
- **`expr?` try op + `is` type tests** — `?` → `GetEnumTag` + branch with an
  early-`Return` arm (continues in the ok block). `is T` → `IsNull` / `IsArray` /
  `typeof`==name / `instanceof` global / const-false (reusing `Unary`/`Binary`/
  `LoadGlobal`). Verified: `x is int`/`is str` → `true false true`; suites green.
- **`this` + `throw`** — `this` → `Move` from register 0 (`InstKind::This`);
  `throw` → a new `Terminator::Throw` (no successors). **`this` unblocks class
  method bodies**: a class's `constructor`/getters/methods compile to `has_this`
  protos via `lower_function`, which re-enters the SSA gate — so with `this` +
  member read/write they now SSA-compile (the `MakeClass` value itself still uses
  `lower/`). Verified: a `Counter` class (`constructor`/`get`/`bump` all
  SSA-compiled) + a `throw`ing fn → `10 11 10` identical with/without `VN_OPT_SSA`.
- **Range** — `a..b` / `a..=b` → `InvokeRuntimeStatic __range__`; the VM reads
  `start` from `arg_start` and `end` from `end_reg` separately (no contiguity, no
  `call_base`), so `dest`/`start`/`end` are the values' own regs. Verified: range
  values build + print identically with/without `VN_OPT_SSA`.
- **`for-in`** — `ObjectKeys` + an index loop; the index is a **synthetic loop
  variable** (`fresh_synthetic`, id above the real locals) carried by an ordinary
  Braun phi, so it reuses the loop machinery; `var` is bound to `keys[idx]` each
  iteration, `continue` lands on the increment. Verified: `countKeys`/`concatKeys`
  → `3`/`abc`.
  - **Backend bug fixed (benefits the default `lower/` path too):** `liveness.rs`
    only built a live range for a register with **both** a def and uses, so a
    def-only register — e.g. a `for-in` loop variable the body never reads — had
    no range and the colourer aliased another *live* local onto its slot, which
    the loop var's (otherwise dead) `Move` then clobbered (`countKeys` → `c1`
    instead of `3` on the default path). A def-only register now gets a **point
    range** at its def so it interferes there. Validated: default suite 728/728 +
    `bench` clean; `countKeys` now `3` on the default path.
- **`switch`** — a test chain (`disc == val` → branch to the body, else next
  test), then bodies in source order **falling through** to each other; `default`
  via the no-match jump; `break`→exit, `continue`→enclosing loop. All forward
  edges, so each block seals once its preds (test branch / no-match / previous
  body fall-through) are wired. Verified: match, fall-through (`1110`), `break`,
  `default` → identical with/without `VN_OPT_SSA`.
- **`for-of`** — the iterator protocol: `iter = iterable[Symbol.iterator]()`
  (`GetSymbol` + an `IterCall` = `Call` with the receiver as its sole arg), then
  each iteration `r = iter.next()`, exit on `r.done`, bind `r.value` to `var`. The
  iterator is loop-invariant (dominates the header); body-modified vars become
  loop phis via Braun (header unsealed during the body); `continue` re-runs the
  header. `for-await-of` falls back. Verified: sum/count over an array and over a
  `0..n` range → `60`/`4`/`10` identical with/without `VN_OPT_SSA`.
- **`match` expr** — a chain of pattern tests; each matching arm evaluates its
  body + result and jumps to a merge block whose single block param is the match
  value (a result phi). Supports wildcard / literal (`Eq`) / bind / enum-variant
  (`__variant_name__` compare + `value{i}` payload binds); record patterns and
  guards fall back. Verified: enum-variant matching (`Circle(r)=>…`) + literal/
  bind (`0=>…, x=>…`) → `300 16 0 zero one many` identical with/without `VN_OPT_SSA`.
- **Super** — `super` / `super.name` (`GetSuper`), `super(args)` (constructor:
  `GetSuper "constructor"` + `Call` with `this` as receiver), `super.name(args)`
  (bound method: `GetSuper name` + `Call` over args, no receiver). Verified:
  `Dog extends Animal` with `super(name)` in the ctor + `super.describe()` in an
  override → `Rex / Lab / Animal:Rex:Lab` identical with/without `VN_OPT_SSA`.
- **Optional chaining** — `object?.…` → a null-check branch + result phi
  (reuses `lower_branch_value`): `IsNull(object)` true short-circuits to the
  object, else the property is applied. Member (`GetPropertyMaybe`), index
  (`GetIndex`), module-slot, plain call (`obj(args)`), and method call
  (`obj.m(args)`) all map to existing instructions; extension calls (which pass
  `obj` as the receiver, not a plain-call arg) fall back. Verified: `b?.v ?? -1`
  and `b?.get() ?? -1` on a `Box?` → `42 -1 42 -1` identical with/without
  `VN_OPT_SSA`.
- **Upvalue reads** — `HirBinding::Upvalue` read → `LoadUpvalue` (a leaf, like
  `LoadGlobal`). A closure *body* can now SSA-compile even while the parent that
  *builds* the closure still falls back, because the upvalue array is populated by
  the parent's `MakeClosure` regardless. Verified: `x => x + n` SSA-compiles
  (parent `makeAdder` falls back on closure creation) → `15 25` identical
  with/without `VN_OPT_SSA`.
- **Binding writes + default params** — a unified `load_binding`/`store_binding`
  pair lowers every `HirBinding`: params/locals stay SSA-tracked, while globals
  and upvalues become `LoadGlobal`/`LoadUpvalue` reads and `StoreGlobal`
  (`DefineGlobal`) / `StoreUpvalue` side-effect writes (dest-less, treated as
  memory — no reorder/CSE until the optimizer models them). So mutating an
  upvalue (`count++`) and assigning a global now SSA-compile. Default-valued
  params get an entry prologue: an `IsNull`-guarded `branch_value` per param that
  rewrites it to the default when the arg was omitted. Verified: `greet(name,
  greeting="Hello")` and a `makeCounter` whose arrow does `count = count + 1`
  → `Hello, World / Hi, World / 1 2 3` identical with/without `VN_OPT_SSA`.
- **Trivial-phi removal** (`simplify_phis`): Braun's `tryRemoveTrivialPhi` as a
  fixpoint post-pass.
- Tests (`ssa/tests.rs`, 42 — golden dumps + verifier): identity, const+binary,
  reassign, one-/two-sided `if` phi, no-phi trivial removal, `while`/`for`/
  `do-while` carry, `break`/`continue`, nested-`if` merge, global call, self-call,
  member/index read, member/index write, method call, ternary, array/object
  literal, template, capture-free closure, intrinsic, non-null, sequence, decimal,
  try-op, type-test, this, throw, range, for-in, switch, for-of, match.

**Measured coverage** (`VN_OPT_SSA=1 VN_OPT_TRACE=1` over the full suite, after
binding writes + default params): **542 functions SSA-compile, 44 fall back
(≈ 92.5%)**; suite **728/728**. Remaining fallback reasons, by frequency: closure
**creation** with upvalues (22), `await`/`spawn`/`yield` + generators
(`expression kind`, 12), `try`/`using`/catch-destructure (`statement kind`, 8),
one `update target`, one `for-await-of`.

**Pending** (the rest of §2's instruction set): closure **creation with
upvalues**, enum-construction / the `MakeClass`/enum *value* expressions, modules
(import/export), await/spawn/yield + generators — plus `try`/`using` control flow.
Until then `build_function` returns `Err(Unsupported)` and that function uses the
`lower/` path. (Done: scalar exprs, control flow, loops, plain/self/method calls,
member/index read+write, logical + conditional, array/object literals, templates,
capture-free closures, **upvalue reads + writes**, **global writes**,
**default params**, intrinsics, `!`/sequence/`?.`-member/module-slot/decimal/
bigint/regex, `expr?`/`is`, `this`, `throw`, `range`, `for-in`, `switch`,
`for-of`, `match`, **super calls**, **optional chaining**; class **method bodies**
SSA-compile.)

> Closure *creation* with upvalues needs captured locals to keep a stable register
> across the function (the VM upvalue points at the slot). SSA renaming spreads a
> local across values/regs, so capture-by-slot needs either pinning captured
> locals to a fixed slot (don't SSA-rename them) or a dedicated upvalue cell.
> Deferred — but reading/using an existing upvalue (`LoadUpvalue`) is done, so a
> closure *body* SSA-compiles independently of its parent.

> **`regalloc_post` callee-frame constraint ✅ RESOLVED.** SSA calls were correct
> pre-regalloc but `regalloc_post` miscompiled multi-call expressions: a call
> result live across a later call coloured *above* that call's `arg_start`, so the
> callee frame (`[arg_start, arg_start+callee_register_count)`, which extends past
> `arg_count`) overwrote it (`addOne(x)+addOne(x)` → `22` instead of `42`).
> `regalloc_post` only enforced call-arg **contiguity**, not the callee-frame
> footprint. **Fix:** `verify_callee_frame_constraints` (bail-only) — for each
> call, if any register live across it (`def < call_idx` and a `use > call_idx`,
> excluding the call's own arg block) maps to `>= arg_start`, skip the remap and
> keep the already-correct emitter layout. Cannot miscompile (bail = keep correct
> original); never triggers on the legacy-shaped `lower/` emission (results sit
> below `arg_start`). Validated: default suite **728/728** + `bench` unchanged
> (p50 ~16 ms); SSA suite **728/728**; the `addOne(x)+addOne(x)` probe → `42`.
> Refinement (later): emit SSA call results below `arg_start` so regalloc can
> compress them instead of bailing.

### 2.3 Verifier ✅ DONE
`ssa/verify.rs`: (1) each `Value` defined once; (2) every predecessor edge carries
exactly as many block-args as the target has params; (3) terminator targets in
range; (4) every use is defined and **dominated** by its def (phi operands checked
on the predecessor edge they flow in on). Dominators via Cooper-Harvey-Kennedy
over reverse-postorder. Runs inside `try_compile_function` — a violation forces
fallback rather than emitting wrong code.

### 2.4 Out-of-SSA → bytecode ✅ DONE (for the §2.1 subset)
`ssa/emit.rs`: **critical/phi-edge splitting** (pad block on every branch edge
whose target has params → phis fed only from single-successor `Jump` edges) →
**register-per-value** assignment (entry params `r1..=P`, receiver `r0`, +scratch
+null; `Unsupported` past 255) → block emission in index order. Phi operands
become **parallel `Move`s** (cycles broken via the scratch reg). Jumps:
fall-through to the next block, `Loop` for back-edges, forward `Jump`/`JumpIf*`
with deferred offset fixups; a conditional with a backward target is reshaped so
the forward exit is the conditional and the back-edge an unconditional `Loop`.
`regalloc_post` + `slot_kinds` run downstream, so this emits naive one-reg-per-
value bytecode and lets them compress + type it. Calls use a contiguous
`call_base` block (receiver + args) above all value registers; `regalloc_post`'s
callee-frame bail (§2.1) keeps that correct after compression.

Verified end-to-end: `VN_OPT_SSA=1` runs the suite **728/728** + `bench` clean,
with SSA actually compiling every function it covers (rest fall back);
`tests/scratch_ssa.vn` (while/for/do/if/break/continue functions) prints
identically with and without the SSA gate.

### §2 exit criteria
1. SSA covers every construct the suite uses (instruction set above).
2. `VN_OPT_SSA` becomes the default; the `lower/` naive emitter is retired.
3. Suite 728/728 + bench parity with the `lower/` path (SSA with zero opts must
   match) before any §3 pass is enabled.

---

## 3. Stage 3 — opt passes (on SSA) (0% — THE GOAL)

The thesis value: optimize *better than JS* using static types, no speculation.
Each pass independently toggleable; record suite 728/728 + bench delta
(`bench_loop`, `bench_fib`, callbench-style) per pass.

1. **Const folding** — fold constant ops.
2. **Copy propagation** — eliminate `Move`/copy chains.
3. **Dead code elimination** — drop unused values + unreachable blocks.
4. **CFG simplification** — merge straight-line blocks, fold constant branches,
   drop empty blocks.
5. **Global value numbering** — dedupe equivalent expressions.
6. **Inline-at-scale** — inline multi-statement callees (what an AST inliner
   can't), then re-optimize the merged SSA. Heuristics: size, call-count,
   recursion guard. **Flagship** — call overhead is ~4× the work.
7. **Escape analysis / scalar replacement** — kill non-escaping allocations
   (Closure/Object/BoundMethod per run).
8. **Peephole / strength reduction** — `*2`→shift, `AddImm`, etc.
9. **Type-driven unboxing region** (stretch) — keep ints/floats untagged across a
   block, box only at boundaries.

Order: const-fold → copy-prop → DCE → CFG-simplify to fixpoint; then GVN; then
inline + re-run; then escape.

### 3.1 High-Impact Performance Optimizations

The following optimizations are expected to yield the most significant, order-of-magnitude improvements in execution times, directly addressing current bottlenecks:

1. **Compile-time Devirtualization & IC Bypass (Leveraging Static Types)**
   - **Mechanism**: Using exact class types from `varn-checker`, lower virtual property accesses and method calls on statically known types directly to static offset accesses (`OpGetFixedField` / `OpSetFixedField`) or direct static calls.
   - **Impact**: Completely bypasses runtime Inline Cache (IC) check overhead and enables subsequent **Inlining** of class methods.

2. **SSA-based Loop-Invariant Code Motion (LICM)**
   - **Mechanism**: Identify SSA expressions (including constant loads, global/module slot reads, and arithmetic) in loops whose inputs are defined outside the loop, and hoist them into a newly created loop pre-header block.
   - **Impact**: Accelerates computational loops by removing redundant calculations from the critical execution path.

3. **SSA Register Coalescing & Linear Scan Register Allocation (LSRA)**
   - **Mechanism**: Move register allocation to the SSA IR phase using Linear Scan Register Allocation. Aggressively merge variables connected by `Move` instructions (from phi/block param lowering or variable assignments) if their live ranges do not interfere.
   - **Impact**: Eliminates most `Move` opcodes in the emitted bytecode, reducing bytecode footprint and register-shuffling overhead in the VM loop.

4. **Type-Driven Scalar Replacement of Aggregates (SROA)**
   - **Mechanism**: Perform escape analysis on the SSA IR. If an allocated object, closure, or array literal does not escape the current function boundary, decompose its fields into individual scalar SSA variables.
   - **Impact**: Eliminates heap allocation and GC pressure for short-lived temporary objects, closures (e.g., in higher-order function chains), or intermediate data structures.

5. **VM Dispatch Optimization (Direct Threading / Tail Calls)**
   - **Mechanism**: The interpreter loop uses a `match` dispatch that can suffer from branch mispredictions. Optimize via direct threaded dispatch using Clang/GCC tail-call optimization (`[[musttail]]`) or labels-as-values (computed goto).
   - **Impact**: Significantly reduces branch mispredictions and dispatch overhead, which is the primary interpreter bottleneck.

---

## 4. Stage 4 — Hito A: delete legacy ✅ DONE

`varn-opt` is the **sole backend**. (Commits: `c591931` SSA stage 2 + residual
parity, `1b24361` legacy deletion.)

- `compile_direct` routes through `varn_opt::compile` unconditionally and
  propagates `Err` (no fallback). `VN_OPT` env gate removed; `.opt` cache-tier tag
  removed (`pipeline/cache.rs`).
- **Deleted** (~5.7k lines): `codegen/{compiler,class,function,stmt}.rs`,
  `codegen/expr/` (8 files), `analysis/{escape,inline}.rs`, dead `codegen/scope.rs`,
  `pub use codegen::Compiler`.
- **Kept**: `codegen/{ir,liveness,regalloc_post}` + `analysis/slot_kinds` (run by
  `run_backend_post_passes`).
- Before deletion, all reachable HIR-lowering `panic!`s were converted to
  `Err(Unsupported)`, and the residual feature gaps were closed (match guards —
  *correctly*, vs legacy which silently ignored them; async/gen extension methods;
  anonymous class decls; optional assignment targets; bigint overflow → 0).
- **Validation**: suite **728/728** with no env var; `bench` clean (execute p50
  ~16.8 ms); `varn-opt` unit tests 13/13; workspace builds + tests clean.

### Deferred cleanup (optional, non-blocking)
- Extract `regalloc_post`/`liveness`/`ir`/`slot_kinds` into a shared
  `varn-backend` crate so `varn-opt` runs them itself (drops the
  `run_backend_post_passes` shim in `varn-compiler`).
- Refresh `docs/COMPILER_ARCHITECTURE.md` (still describes the deleted legacy
  codegen) and `CLAUDE.md`.

---

## 5. Objective & roadmap forward

**End state:** one backend (`varn-opt`), SSA-based, that produces bytecode the
existing register VM + JIT run *faster than equivalent JS* by exploiting static
types — no speculation, no deopt.

| Phase | What | Status |
|---|---|---|
| A | `varn-opt` is the sole backend; legacy deleted | ✅ done (§4) |
| B | SSA covers the full instruction set → SSA becomes default → retire `lower/` | 🟡 in progress (§2, 60%) |
| C | Opt passes on SSA → beat-JS perf | 🔲 next (§3) |
| D | Backend crate extraction + doc refresh | 🔲 optional cleanup |

**Recommended order from here:**
1. **§2 instruction set** — grow `ssa/build.rs` + `ssa/emit.rs` to cover calls,
   member/index, closures, classes, enums, `match`, `try`, modules, async; add
   `switch`/`for-of`/`for-in`. Validate each addition: suite 728/728 under
   `VN_OPT_SSA`, golden SSA dumps, verifier clean.
2. Flip `VN_OPT_SSA` to default; confirm SSA-with-zero-opts == `lower/` output
   (suite + bench parity); retire `lower/`.
3. **§3 opt passes** in order; record bench deltas. Inline-at-scale + escape are
   where the "beat JS" payoff lands.
4. §4-D cleanup (backend crate, docs).

---

## 6. Cross-cutting

### Verification (every step)
- `vn run tests/main.vn` → **728/728** (no env var = production path).
- `VN_OPT_SSA=1 vn run tests/main.vn` → **728/728** (SSA path, with fallback).
- `vn bench tests/main.vn` runs clean — the regression trap (regalloc/JIT bite
  under `bench`, not `run`).
- Benchmarks: `tests/bench_loop.vn`, `tests/bench_fib.vn`, callbench-style.
- Dumps: `vn debug -p hir`; SSA dump (`ssa/dump.rs`); SSA verifier.
- Same `FunctionProto` ⇒ VM/JIT correctness inherited.

### Risk areas
- **Register allocation** — regalloc interactions bite under `bench`; always
  validate there.
- IC cache sizing (`cache_count` must match emitted IC sites).
- Upvalue capture chains; `this`/receiver reg-0 invariant.
- Out-of-SSA parallel-copy correctness (swaps via the scratch reg).
- With legacy gone, an `Err` from `varn-opt` is now a hard compile error — keep
  the unreachable defensive `Err` sites genuinely unreachable.

### Caching note
The `.vn` bytecode cache is co-located per source dir; `vn cache clean` only
clears the invoked project's tree. During bring-up, clear `tests/.vn` or compare
against fresh compiles.
