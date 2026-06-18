# PLAN — `varn-opt`: AST → HIR → SSA → Opt → Bytecode

Ultra-detailed roadmap to finish the optimizing backend and **replace** the
legacy direct codegen. Living document; update as stages land.

---

## 0. Context & current state

### Why
Measured this session:
- A function call costs **~16 ns ≈ 4× the work** of a leaf like `add(a,b)=a+b`
  (callbench: 101 ms vs 21 ms hand-inlined).
- Native builtins are **~10%** of execute (native-time profiler) — *not* the
  bottleneck; VM execution is.
- The AST-level leaf inliner gave **~8×** on call-bound code, but only for
  single-`return`-expr functions — because there is **no optimizer IR**.

The thesis: V8 must *speculate* to optimize untyped JS; Varn's checker *knows*
the types, so an SSA optimizer can specialize with **no speculation/deopt**. The
ceiling is set by the IR, not the JIT. So: build `AST → HIR → SSA → opt →
bytecode` and **delete** the legacy `AST → bytecode` codegen.

### Architecture
```
AST ──► HIR ──► SSA (CFG) ──► opt passes ──► bytecode (FunctionProto) ──► regalloc_post ──► VM ──► JIT
        typed   basic blocks   const-fold,      (existing Chunk format)     (existing)       (existing)
        desugared + phi nodes   copy-prop, DCE,
                                GVN, inline,
                                escape
```
Backend (`regalloc_post`, register VM `varn-vm`, JIT `varn-jit`) is **reused
unchanged** — `varn-opt` lowers to the same `FunctionProto`.

### Integration contract (preserve)
- Entry: `varn_compiler::codegen::compile_direct(program, annotations,
  extension_calls, extension_members, extension_set_members, export_names) ->
  Result<FunctionProto, Rc<str>>` (3 call sites in `crates/varn-compiler/src/lib.rs`).
- Output: `varn_types::{Chunk, FunctionProto}` (`crates/varn-types/src/chunk.rs`).
- Reused inputs: `varn_core::TypeAnnotations` (`get_numeric`, `get_intrinsic`,
  `get_call_mapping`, `is_reassigned_name`, `module_caps`…), `EscapeAnalysis`.
- Crate deps: `varn-opt` → `varn-core`, `varn-types` only (no cycle).

### Temporary dev gate
`VN_OPT` env in `compile_direct` routes to `varn_opt::compile`; on
`Err(Unsupported)` it falls back to legacy. `VN_OPT_TRACE` prints which modules
varn-opt compiled. Both removed at Stage 4.

### DONE (commits `15b2b19`, `70cf60b`, + stage 1.0)
- Stage 0: crate scaffold, workspace/dep wiring, `VN_OPT` gate + fallback.
- HIR core types (`crates/varn-opt/src/hir/mod.rs`):
  `HirModule/HirFunction/HirStmt/HirExpr/HirType/HirBinding/LocalId`.
- AST→HIR (`hir/lower.rs`) + naive HIR→bytecode (`lower/mod.rs`) for the
  **imperative core**: top-level fn decls→globals, params, locals,
  `let`/assign/`return`/`if`/`while`/`for`(→while)/`break`/`continue`, simple
  positional calls, typed binary+unary, all scalar literals, param/local/global
  identifier resolution. Register model mirrors legacy (every identifier read →
  fresh temp); plain-call ABI; typed opcodes (`AddInt`/`AddFloat`/…) from
  `get_numeric`; jumps/loops; implicit `return null` epilogue.
- Validated `VN_OPT=1`: callbench (5M) → `12499997500000`, fib(30) → `832040`,
  **full suite 686/686** fresh compile (core via varn-opt, rest fall back).
- **Stage 1.0 (post-passes + `CallSelf`):** the `VN_OPT` gate in
  `compile_direct` now walks the whole proto tree (top level + nested protos in
  the constant pools) and runs `regalloc_post::optimize_function` +
  `slot_kinds::infer` on each — option (a), no dep cycle. Added
  `HirExpr::SelfCall` (decided at HIR lowering from fn-name + `is_reassigned_name`
  + scope) → emits `CallSelf` so self-recursion stays inside JIT machine code
  instead of re-entering the VM. Results: fib(30) at call-overhead **parity**
  (14.1 ms vs legacy 13.3 ms; `calls vm-fast` 1, not 2.69 M); suite **bench**
  parity (regalloc on) **41.5 ms vs 41.6 ms** execute p50; suite **686/686**.

### Known gaps in the core already shipped
1. ~~Post-passes skipped.~~ **Resolved in stage 1.0** — varn-opt protos now run
   `regalloc_post` + `slot_kinds::infer`, so registers are compressed and
   `register_meta` is populated (JIT typed fast-paths ON).
2. Everything outside the imperative core falls back (see §1).
3. **No generalized inlining.** Legacy's AST leaf-inliner (`analysis/inline.rs`,
   commit `96cce1d`) inlines trivial callees; varn-opt does not, so call-bound
   code without recursion (e.g. callbench: 5 M `add()` calls) is slower until
   §3's inline-at-scale lands. Recursion is unaffected (`CallSelf`, stage 1.0).

---

## 1. Stage 1 completion — full AST→HIR→bytecode parity

**Goal:** every construct in the 686-suite lowers through varn-opt; zero
fallback. This is ~reimplementing `crates/varn-compiler/src/codegen/`
(`compiler.rs` + `expr/`×8 + `stmt.rs` + `class.rs` + `function.rs`). Mirror its
exact emission (the VM contract). Each item: AST node → HIR addition → bytecode/
opcodes → legacy reference → verification.

### 1.0 Post-passes integration ✅ DONE (perf-parity for the core)
- After `lower_module`/`lower_function` build each `FunctionProto`, run
  `regalloc_post::optimize_function(&mut proto)` then `slot_kinds::infer(&mut
  proto)` on **every** proto (module + nested), exactly as `finish_module`/
  `finish_function` do (`compiler.rs:696/736`).
- Problem: those fns live in `varn-compiler` (cycle if `varn-opt` calls them).
  Options: (a) the `VN_OPT` gate in `compile_direct` runs them on varn-opt's
  output (it already lives in varn-compiler) — walk the proto tree and apply;
  (b) move `regalloc_post`/`slot_kinds`/`liveness`/`ir` into a shared crate
  `varn-codegen-backend` that both depend on. **Pick (a)** first (no move),
  refactor to (b) at Stage 4 when legacy is deleted.
- `cache_count`/`ic_cache`/`feedback` must be correctly sized once §1.1 adds IC
  sites (currently 0).
- Verify: `VN_OPT=1 bench tests/main.vn` parity-or-better vs legacy; callbench/
  fib bench within noise of legacy; suite 686/686.

### 1.1 Member / index / method access + Inline Caches 🟡 PARTIAL
- **Done:** `HirExpr::Member`/`Index`/`MethodCall`; lowering emits `GetProperty`
  (`emit_rrc_ic` + IC slot), `GetIndex`, `CallMethod` (IC slot + name const).
  Per-function `cache_count` threaded → `ic_cache`/`feedback` sized in `finish`.
  Method-call detection (non-computed `.name(args)`, not `super`/extension/
  intrinsic). Module-slot reads, extension members/calls, optional chaining, and
  non-identity call mappings fall back. `compile_direct` now traces fallback
  reasons under `VN_OPT_TRACE`. **GetIndex verified end-to-end** (`s[i]` via
  varn-opt); coverage preserved (callbench/fib still route through varn-opt).
- **Pending exercise:** `GetProperty`/`CallMethod` are correct (mirror legacy)
  but can't be hit by a pure-core module — nothing constructs an object/array to
  access until §1.3 literals land. Validate them as part of §1.3.
- **Deferred:** optional chaining (`GetPropertyMaybe`), module slots (§1.8),
  extensions, super (§1.6).

  Original notes:
- AST: `ExprKind::Member { object, property, computed, optional }`,
  `ExprKind::Call` whose callee is a non-computed `Member` (method call).
- HIR: re-enable `HirExpr::Member`/`Index`; add `HirExpr::MethodCall { recv,
  name, args }`.
- Bytecode: `GetProperty` (emit_rrc_ic with a **cache slot**), `GetIndex`
  (emit_rrr), `CallMethod` (cache slot + name const). Optional chaining →
  `IsNull` + `JumpIfTrue` (legacy `expr/calls.rs`, `expr/member.rs`).
- **Cache slots:** each GetProperty/SetProperty/CallMethod/GetPropertyMaybe
  consumes a cache index (`alloc_cache` in legacy). Track a per-function
  `cache_count`; build `ic_cache`/`feedback` of that size in `finish`.
- Setter side: `SetProperty`, `SetIndex` for assignment targets (§1.4).
- Ref: `codegen/expr/member.rs`, `expr/calls.rs::emit_method_call`,
  `compiler.rs::alloc_cache`.
- Verify: a module using objects/arrays/method calls compiles via varn-opt and
  passes (e.g. `tests/` collection/string modules).

### 1.2 Closures + upvalues + nested functions ✅ DONE
- Replaced the single-function `Scope` with a **frame stack**: name resolution
  walks outward across frames, capturing enclosing bindings as upvalues
  (`HirUpvalueSrc::ParentLocal/ParentParam/ParentUpvalue`, dedup) — the legacy
  `resolve_upvalue`/`add_upvalue` chain, but symbolic since registers aren't
  assigned until lowering. Function exprs, arrows (expr + block body), and
  nested function decls lower to `HirExpr::Closure { func, upvalues }` →
  `MakeClosure` (or `LoadStaticFn` when capture-free), resolving upvalue sources
  to `(is_local, index)` against the parent frame's registers. `LoadUpvalue`/
  `StoreUpvalue` for upvalue read/write; `CloseUpvalue` (`HirStmt::CloseUpvalues`)
  on inner-block pop (function-level captures are closed by the VM's `Return`).
- **Nested self-recursion uses `CallSelf`**, not self-capture: `is_self_call`
  checks only the current frame (legacy `name_resolves_locally`), so a recursive
  nested function stays capture-free (`LoadStaticFn`) and avoids a use-before-def
  on its own slot that the register allocator can't model.
- Verified end-to-end via varn-opt: higher-order calls, escaping closures
  capturing a param, a mutating counter (independent instances), and nested
  recursion all match (`42 15 1 2 1 3 120`). Suite 686/686.

  Original notes:
- AST: `ExprKind::Function`/`Arrow`; function decls **inside** functions.
- HIR: `HirExpr::Closure { fn_index, captures: Vec<Upvalue> }`;
  `HirBinding::Upvalue`. Capture analysis: a name resolved in an enclosing
  function scope → upvalue (local-in-parent vs upvalue-in-parent chain).
- Bytecode: `MakeClosure` (dest, uv_count, proto_idx, then `(is_local, index)`
  per upvalue) — or `LoadStaticFn` when no captures (legacy `function.rs::
  emit_closure`). `LoadUpvalue`/`StoreUpvalue`/`CloseUpvalue`.
- HIR lowering must thread a parent-scope chain (mirror `Compiler.parent`,
  `resolve_upvalue`, `UpvalueDesc`).
- Ref: `codegen/function.rs`, `compiler.rs` upvalue machinery, `scope.rs`.
- Verify: closures/arrows in suite modules (map/filter callbacks, etc.).

### 1.3 Remaining expressions 🟡 PARTIAL
- **Done:** `Logical` (`&&`/`||`/`??`), `Conditional` ternary, `Update`
  (`++`/`--` on identifier bindings, prefix/postfix), simple `Array` literals
  (no spread/holes), fixed-shape `Object` literals (`BuildObjectWithShape`, all
  static keys, value props only). Also: type-only top-level decls (`interface`,
  `type`, `struct`) are now erased instead of forcing fallback (mirrors legacy
  `stmt.rs`). Verified end-to-end via varn-opt (array/object/property/logical/
  ternary/update/nullish → correct), which also exercises §1.1 `GetProperty`.
  Suite 686/686.
- **Done (batch 2 — expression-kind fallback 12→5):** `Template` (`BuildStr`
  chain + `ToString`), `Range` (`InvokeRuntimeStatic __range__`), `Sequence`
  (eval-all, yield-last), `As`/`Satisfies` (transparent), `NonNull` (`!` →
  `AssertNotNull`), char literals, and **assignment-as-expression** (identifier/
  member/index + compound-identifier, yields the value). Verified differential vs
  legacy (template/char/`as`/`!`/sequence/range/assign-expr all identical); suite
  668/668 both paths.
- **Three cross-cutting bugs found + fixed** (each pre-existing, exposed once more
  modules routed through varn-opt — all verified differential-clean):
  1. **C-style `for` + `continue`:** `for` desugared to `while(test){body;update}`
     made `continue` skip `update` → infinite loop. Now a dedicated
     `HirStmt::ForClassic` whose `continue` (`Forward`) lands on the `update`.
  2. **`AssertNotNull` operand byte:** the VM reads the reg from the *high* byte;
     legacy emits `pack(0,r)` (low) but survives via its IR-regalloc. varn-opt has
     no IR, so it now emits `pack(r,0)`.
  3. **`regalloc_post` captured-local liveness + `BuildObjectWithShape`:** (a) a
     `MakeClosure`-captured *local*'s slot was reused as the closure's own dest
     (the open upvalue then read the closure) — fixed by extending captured
     locals' liveness to function end in `scan_bytecode`; (b) `BuildObjectWithShape`'s
     value block must stay register-contiguous but its count lives in the shape
     constant (invisible to the bytecode-only `regalloc_post`), so varn-opt now
     emits `BuildObject` (explicit key/value pairs, each reg tracked/remapped
     independently). Both fixes are correct for legacy too (legacy rarely
     compresses, so it never hit them). Shape-sharing via `BuildObjectWithShape`
     is a deferred optimisation.
- **Done (batch 3 — expression/binary-op cleanup, 30→33 modules via varn-opt):**
  `>>>`/`instanceof`/`in` binary ops (`HirBinOp::Ushr/Instanceof/In` → same
  opcodes as legacy, numeric-kind-independent); **empty object literal** `{}`
  (`lower_object` already emits `BuildObject` count-0 — dropped the guard);
  `Pipeline` non-placeholder (`x |> f` desugars to a plain `Call f(x)`;
  placeholder form `f(_)` still falls back); `Try`-expr `expr?`
  (`HirExpr::TryOp` → `GetEnumTag` + `JumpIfTrue` + early `Return`, mirroring
  `compile_try_expr`); `Is` type-test `expr is T` (`HirExpr::TypeTest` +
  `HirTypeTest` resolved at lowering to `IsNull`/`IsArray`/`Typeof`-compare/
  `Instanceof`/const-false, mirroring `member::compile_is`). Verified
  differential vs legacy (instanceof/in/ushr/empty-obj/pipeline/try/is all
  identical); suite 668/668; `VN_OPT bench` (regalloc ON) clean.
- **Deferred:** `New` (§1.6), `TaggedTemplate`, pipeline `_` placeholder,
  intrinsics, bigint/decimal/regex literals (decimal/bigint need `rust_decimal`),
  array spread/holes, computed/method/getter/setter object props, member/index
  update targets (§1.4), `Await`/`Spawn`/`Yield` (§1.9).

  Original notes:
- `Logical` (`&&`/`||`/`??`) → branch + `Move` (legacy `operators.rs::
  compile_logical`).
- `Conditional` ternary → branch + `Move` (`compile_conditional`).
- `Update` (`++`/`--`) prefix/postfix → load/AddImm/store (`assignment.rs`).
- `Array`/`Object` literals → `MakeArray`/object build opcodes + element/prop
  emission (`expr/collections.rs`, `expr/fields.rs`).
- `New` (constructor) → class instantiation path (§1.6).
- `Template`/`TaggedTemplate` → `StrConcat`/`ToString` chains (`templates.rs`).
- `Range` → range object; `Pipeline` `|>` → desugar to call; `Sequence` →
  eval-discard-last; `Spread` (§1.10); `Await`/`Spawn`/`Yield` (§1.9).
- `As`/`Satisfies`/`NonNull`/`Paren` → mostly transparent (`AssertNotNull` for
  `!`); `Try`-expr → `GetEnumTag` early-return (`operators.rs::compile_try_expr`).
- **Intrinsics:** `get_intrinsic(offset)` Some → emit `Intrinsic` opcode (wire
  byte) instead of a call (Math.*, etc.) — see `try_emit_intrinsic` in legacy
  `expr/calls.rs`; the VM decodes via `varn_core::intrinsic_ops`.
- char/bigint/decimal/regex literals → their `LoadConst`/builder forms.

### 1.4 Assignment targets (full)
- Currently only identifier targets. Add: `obj.prop = v` → `SetProperty`,
  `arr[i] = v` → `SetIndex`, compound (`+=` on member/index), destructuring
  assignment. Reassign-global → confirm `DefineGlobal` vs `StoreGlobal`
  semantics (legacy `expr/assignment.rs`).

### 1.5 Remaining statements ✅ DONE
- **Done (all mirror legacy `stmt.rs` byte-behaviourally):**
  - `throw` → `Throw`; `try/catch/finally` → `Try`/`PopTry`/`Throw` with the
    nested-`Try`-around-`catch` finally dance. A per-function `finally_stack`
    threads pending `finally` bodies into `return`/`break`/`continue` (re-lowered
    + `PopTry` before the transfer). Catch param = a local copied from `err_reg`.
  - `for-of` → iterator protocol (`Symbol.iterator`/`next`/`done`/`value`);
    `for-in` → `ObjectKeys` + index loop. (`for-await-of` → fallback, async only.)
  - `switch` → sequential `Eq` chain with fall-through; a `break`-only scope so
    `continue` targets the enclosing loop.
  - `do-while`.
  - `using`/disposables → `dispose`/`disposeAsync` (no-arg `CallMethod`) at block
    exit (reverse decl order), tracked per block in the HIR `Scope` and emitted
    via `block_epilogue` (alongside `CloseUpvalues`); also at frame exit.
  - Labels are **ignored** (matches legacy: `break`/`continue` always target the
    innermost loop; `Labeled` just lowers its body). Proper label resolution is
    deferred to §4 when legacy is replaced.
- **Emitter infra added** (`lower/`): `LoopCtx` now carries a `ContinueMode`
  (`Backward(off)` for while/for-of, `Forward` for for-in/do-while increment,
  `Skip` for switch) + `continue_jumps` + `finally_depth`; `FnLower.finally_stack`.
- **Bug fixed:** `lower_module` passed `nlocals = 0`, so module-level block locals
  (`using`/block `let`) collided with temporaries and got clobbered by intervening
  calls. Now passes `module.top_level.locals` (mirroring `lower_function`).
- **Verified:** suite **668/668** via varn-opt (the `"statement kind"` fallback is
  gone); focused differential tests for try/catch/finally (incl. `break`/`continue`
  running `finally`), for-of/for-in, switch/do-while, and `using` all match legacy
  exactly. `VN_OPT bench` (regalloc/JIT) exits clean. Module `45-simple-file-test`
  now compiles via varn-opt.
- **Four pre-existing legacy control-flow bugs fixed (in BOTH legacy `stmt.rs`
  and varn-opt — surfaced by §1.5; verified on both paths):**
  1. **`switch` fall-through:** a matched empty/grouped case fell into the next
     *test* instead of the next *body* (`case 2: case 3: …` mis-classified `2`).
     Both emitters now emit all case tests first (`Eq`→`JumpIfTrue` to the body),
     then the bodies in order, so they fall through correctly.
  2. **`switch` + `continue` (a crash):** legacy dropped the switch's
     `continue_jumps`, leaving an unpatched jump (0xFFFF…) → VM/JIT crash
     (`varn-jit/compiler.rs:403` out-of-bounds). `continue` in a `switch` now
     targets the enclosing loop (varn-opt: `ContinueMode::Skip`; legacy: forward
     the jumps to the outer loop ctx). Fixed at the source, so no JIT change needed.
  3. **`finally` skipped on `return` inside `catch`:** the `finally` was popped
     from `finally_stack` before the `catch` body. `try/catch/finally` is now
     lowered in three explicit shapes so `finally` runs on every exit path.
  4. **`finally`-only `try` swallowed exceptions:** `try { throw } finally {}`
     ran `finally` but never rethrew. The handler now rethrows after `finally`.
- **Refactor (file-size governance):** the two god-files were split by domain —
  `hir/lower.rs` (1597) → `hir/lower/{mod,decl,stmt,expr}.rs`; the emitter
  `lower/mod.rs` (988) → `lower/{mod,stmt,expr,class,control}.rs`. All ≤ ~490 lines.
- **Deferred → fallback:** `for-await-of`; destructuring `using`/catch params;
  proper labeled `break`/`continue`.

### 1.6 Classes 🟡 CORE DONE
- **Done (core):** `HirExpr::This` (reg 0), `HirExpr::Class` (+`HirClass`/
  `HirMethod`), `HirFunction.has_this`. A class lowers to `MakeClass` +
  `DeclareField`(s) + `Method`(s); constructor and methods are `has_this`
  closures (`LoadStaticFn`/`MakeClosure` then `Method`). Constructor is
  synthesised when absent; field initializers run after the body as
  `this.name = expr` (legacy order — verified to match, incl. the field-reset
  quirk). `new C(args)` reuses the `Call` path (VM constructs on a class
  callee). Member/index assignment targets landed too (`SetProperty`/`SetIndex`
  = §1.4 partial). Verified end-to-end via varn-opt: fields, constructor, `this`,
  methods, `new`, field read/write match legacy (`7 70 30`). Suite 686/686.
- **Deferred → fallback:** inheritance (`extends`/`super`/`GetSuper`), static
  members, getters/setters, decorators, abstract, destructor, static blocks,
  `instanceof`, compound member assignment, module-slot/extension setters.

  Original notes:
- `Decl::Class`: fields (init exprs run in constructor), methods (→ protos in a
  vtable), constructor (receiver = `this` at reg 0; `pending_field_inits`),
  inheritance (`extends`, `super` calls, `GetSuper`), static members, getters/
  setters (vtable + getter/setter vtables → IC versions), `instanceof`.
- Bytecode: `MakeClass`/`DeclareField`/`Inherit`/`ClassMemberOp`/`BindMethod`/
  `InvokeVirtual`/`GetSuper`/`GetFixedField`/`SetFixedField` (legacy
  `codegen/class.rs`, VM `exec/class.rs`, `ctx_jit_runtime.rs`).
- Largest single chunk. `this` handling, field offsets, vtable versions.
- Verify: class-heavy suite modules.

### 1.7 Enums ✅ CORE DONE
- **Done:** `Decl::Enum` → `HirExpr::Enum` (`MakeClass` + per-variant
  `MakeEnumVariant`/`DefineStatic` with the `Enum.Variant[:fields]` meta string,
  incrementing tags, integer discriminants) + instance fields/methods (class
  core). `match` expr → `HirExpr::Match` (legacy `compile_match` branch chain):
  wildcard, literal (`Eq`), identifier-bind, and enum-variant patterns
  (`__variant_name__` compare + `value{i}` payload binds). Verified end-to-end
  via varn-opt: enum decls, payload variant construction (`Shape.Circle(10)`),
  and variant matching (`Circle(r) => …`) match legacy exactly (`300 20 red
  red`). Suite 686/686.
- **Deferred → fallback:** match guards, record/sequence/type patterns,
  static/non-int-discriminant enum members, enum field initializers.

  Original notes:
- `Decl::Enum`: variants (tags), payloads, `MakeEnumVariant`, `GetEnumTag`,
  pattern matching over variants (`match` expr). Ref legacy enum codegen +
  `tests/41-advanced-enums.vn`.

### 1.8 Modules (import/export) ✅ CORE DONE
- **Done:** `import` → `HirStmt::Import` (`LoadModule`; per specifier
  `LoadModuleSlot`/`GetProperty` + `DefineGlobal`; bare/type-only = side-effect
  load). `export <decl>` → lower the inner decl as a module global (`Closure`/
  `Class`/`Enum`/`Assign`) then `StoreModuleSlot` per export name (slot =
  `export_names` position or `get_slot_idx`). `export_names` threaded into the
  lowerer. Verified end-to-end: a two-module program (bare + named import,
  `export function`, cross-module call) compiles **both** modules via varn-opt
  and matches legacy (`21`). Suite 686/686; the gate has moved past imports —
  remaining suite fallbacks are now `~`/`typeof` unary (§1.3-rest), async/
  generator (§1.9), and generic classes (§1.10).
- **Deferred → fallback:** `export default`/`export { … }`/`export *`,
  nested namespaces (`namespace` decl), re-exports.

  Original notes:
- `import`/`export` decls → `LoadModule`/`LoadModuleSlot`/`StoreModuleSlot`,
  module object, export ordering. `export_names`. Ref VM `exec/modules.rs`,
  `ctx_modules.rs`; `varn-modules` resolver. Needed for multi-module programs
  (the suite imports stdlib + cross-module).

### 1.9 Async / generators
- `is_async`/`is_generator` functions; `await`/`yield`/`spawn`; Task/Isolate
  runtime. Defer (lowest frequency, highest complexity). Until done → fallback.

### 1.10 Patterns / params / generics
- Destructuring (array/object/rest/default/assignment patterns) in
  params/`let`/for — mirror `function.rs::declare_pattern_into`/`_global`.
- Default/rest/optional params (`IsNull` + default expr; rest collection).
- Generics: type-erased at codegen (monomorphization is a later opt); ensure
  `type_params` don't block lowering once bodies are type-erased.
- Named/spread args + `get_call_mapping` (arg reordering/defaults) — replicate
  `compile_args_contiguous`'s mapping path.

### 1.11 Differential testing harness (build alongside §1)
- Add `vn debug -p hir` (dump HIR) and a **bytecode differ**: compile a program
  both ways (legacy vs varn-opt, pre-regalloc) and diff opcode streams to catch
  emission mismatches early. Per-module suite under `VN_OPT=1` must stay
  686/686 at every sub-step (the regression trap lives in `bench`, regalloc on).

---

## 2. Stage 2 — SSA

Build the real optimizer IR from HIR. Per-function.

### 2.1 CFG construction
- HIR (already control-flow-explicit) → basic blocks split at branch/jump/loop
  targets. Block = list of instructions + terminator (`Jump`/`CondJump`/`Return`/
  `Loop`). Edges. Entry block.

### 2.2 SSA form
- Compute dominator tree (Lengauer–Tarjan or iterative) + dominance frontiers.
- Phi insertion at frontiers for each variable assigned in >1 block.
- Variable renaming (per-variable version stacks) → SSA values.
- SSA IR types in `crates/varn-opt/src/ssa/` (`SsaFunc`, `Block`, `Inst`,
  `Value`, `Phi`, `Terminator`). Values carry `HirType`.

### 2.3 Verifier
- Each value defined once; uses dominated by defs; phi arg count = pred count;
  types consistent. Run in debug builds + a `vn debug -p ssa` dump.

### 2.4 Out-of-SSA + lowering to bytecode
- Phi elimination → parallel copies on edges (handle swaps via temp).
- Instruction selection SSA → `Chunk` (reuse the §1 emitter as the target, or a
  dedicated `ssa->bytecode`). Then `regalloc_post` as today.
- Verify: suite 686/686 + bench parity with the no-SSA path (SSA with zero opts
  must match).

---

## 3. Stage 3 — opt passes (on SSA)

Each: independently toggleable, suite 686/686 + bench delta (callbench,
bench_math, fib) recorded. Static types drive specialization (no speculation).

1. **Const folding** — fold constant ops (extends legacy `try_fold_binary`).
2. **Copy propagation** — eliminate `Move`/copy chains.
3. **Dead code elimination** — drop values with no uses + unreachable blocks.
4. **CFG simplification** — merge straight-line blocks, fold constant branches,
   remove empty blocks.
5. **Global value numbering** — dedupe equivalent expressions.
6. **Inline-at-scale** — inline multi-statement callees (what the AST inliner
   can't), then re-optimize the merged SSA. Heuristics: size, call-count,
   recursion guard. This is the flagship (call overhead = 4× the work).
7. **Escape analysis / scalar replacement** — kill non-escaping allocs (Closure
   508, Object 351, BoundMethod 155 / run). Reuse `EscapeAnalysis` ideas.
8. **Peephole / strength reduction** — `*2`→shift, `AddImm`, etc.
9. **Type-driven unboxing region** (stretch) — keep ints/floats untagged across
   a block, box only at boundaries (the per-op box/unbox cost noted earlier).

Order matters: const-fold → copy-prop → DCE → CFG-simplify iterate to fixpoint;
then GVN; then inline + re-run; then escape.

---

## 4. Stage 4 — replace legacy

- Route `compile_direct` through `varn-opt` unconditionally (all 3 `lib.rs`
  entries).
- Move `regalloc_post`/`liveness`/`ir`/`slot_kinds` into a shared backend crate
  (or keep in varn-compiler and have varn-opt depend on a thin `varn-backend`).
- **Delete** legacy AST→bytecode: `codegen/compiler.rs`, `codegen/expr/`,
  `codegen/stmt.rs`, `codegen/function.rs`, `codegen/class.rs`,
  `analysis/inline.rs` (superseded by SSA inlining). Keep `regalloc_post`,
  `liveness`, `ir`.
- Remove `VN_OPT`/`VN_OPT_TRACE`.
- Full suite + bench parity-or-better; update docs
  (`docs/COMPILER_ARCHITECTURE.md`, `CLAUDE.md`).

---

## 5. Cross-cutting

### Verification (every step)
- `vn run tests/main.vn` → **686/686**.
- `vn bench tests/main.vn` (regalloc ON) runs clean — this caught the LoadConst
  regression; it's the trap.
- Benchmarks: `/tmp/callbench.vn`, `bench_math.vn`, fib — record deltas.
- Dump modes: `vn debug -p hir|ssa`; SSA verifier; bytecode differ (§1.11).
- Backend untouched ⇒ same `FunctionProto` ⇒ VM/JIT correctness inherited.

### Risk areas
- **Register allocation correctness** — the LoadConst regression showed regalloc
  interactions bite under `bench` (not `run`). Always validate with `bench`.
- IC cache sizing (`cache_count` must match emitted IC sites or VM mis-indexes).
- Upvalue capture chains; `this`/receiver reg 0 invariant.
- Module/import ordering and global resolution.
- Out-of-SSA parallel-copy correctness (swaps).

### Caching note
The `.vn` bytecode cache is co-located per source dir; `vn cache clean` only
clears the invoked project's tree. During bring-up, clear caches (incl. `/tmp`)
or compare against fresh compiles.

---

## 6. Magnitude & recommended order

| Block | Size | Value |
|---|---|---|
| §1.0 post-passes | small | perf-parity for the core already shipped — **do first** |
| §1.1 member/method+IC | medium | unblocks most of the suite (objects everywhere) |
| §1.2 closures/upvalues | medium | callbacks (map/filter) everywhere |
| §1.3–1.5 exprs/stmts | medium | broad coverage |
| §1.6 classes | **large** | class-heavy modules |
| §1.7–1.8 enums/modules | medium-large | cross-module suite |
| §1.9 async/gen | large | defer (low frequency) |
| §2 SSA | large | the optimizer foundation |
| §3 opt passes | large | **the thesis value** (beat-JS) |
| §4 replace | medium | cleanup |

**Recommended path:**
1. §1.0 (post-passes) — measurable perf-parity now.
2. §1.1 + §1.2 (member/method + closures) — biggest suite-coverage unlock.
3. §2 SSA on the stable core **in parallel** — lets §3 opt passes start showing
   wins early on callbench/fib without waiting for full §1.
4. Grow §1.3–1.8 by suite frequency; §1.9 last.
5. §3 opt passes (const-fold→…→inline-at-scale→escape).
6. §4 replace + delete legacy.

Reality: full parity (§1) ≈ reimplementing the whole codegen — weeks. SSA + opts
(§2–3) are where the "optimize much better than JS" payoff lands, on top of a
stable subset.
