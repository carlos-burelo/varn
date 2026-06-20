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
- Lowering of Await, Spawn, and Yield expressions, async and generator functions, and async/generator class methods.
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

## 1. Stage 1 completion — full AST→HIR→bytecode parity (95% Complete)

**Goal:** every construct in the 686-suite lowers through varn-opt; zero
fallback. This is ~reimplementing `crates/varn-compiler/src/codegen/`
(`compiler.rs` + `expr/`×8 + `stmt.rs` + `class.rs` + `function.rs`). Mirror its
exact emission (the VM contract). Each item: AST node → HIR addition → bytecode/
opcodes → legacy reference → verification.

### Stage-1 completion drive (in progress)
Closing every remaining fallback so legacy can be deleted (§4). Validation:
per-feature differential (`diffcheck` — legacy vs `VN_OPT`, both run + bytecode
fallback count) + suite 668/668 both tiers + bench clean. **Caches are
content-keyed and now tier-tagged (`.opt`) so the two codegen paths don't poison
each other — clear `tests/.vn`/`.vn` when validating.**

Landed this drive:
- **Namespaces** (`namespace`/`export namespace`, top-level + inline) — members
  as globals + an object of `Var(Global)` (mirrors `compile_namespace_decl`).
  Unblocked ~10 std modules.
- **Classes — full** (minus decorators): static field/method/block, getters/
  setters (instance + static), destructor (`dispose`), abstract (erased),
  **inheritance** (`extends` → `MakeClass` super-reg) + **super** (ctor call,
  method call, member read). `HirClass` extended; `bind_member` unifies the
  member opcodes.
- **Extensions** (decl + call + member + setter): each member → mangled global
  closure (`__ext*_T_k`, `has_this`); uses lower to `HirExpr::ExtensionCall`
  (receiver in the **receiver slot** = `this`, not arg 0).
- **Bugs fixed (root-caused, not worked around):**
  1. **Checker WR3001 false positive** — `return this` in an extension on a
     primitive: `this` inferred as `Type::named("int")` but the declared return
     is `Type::Intrinsic(Int)`. `ExprKind::This` now maps a primitive
     `current_class` to its intrinsic type; `ext_class_name` covers all scalar
     primitives (`infer_impl.rs`, `checker/decls.rs`).
  2. **slot_kinds soundnessness** — a register the optimizer reuses for an int
     (`i`) and a heap value (`result: str`) was tagged `Int` (first-typed-write
     wins), so the JIT returned the string pointer as an unboxed int. Untyped
     arithmetic/`StrConcat` writers now **taint** the slot → `Dynamic`
     (`analysis/slot_kinds.rs`). Shared fix; benefits legacy. Float/int
     fast-path kinds (set by typed opcodes) untouched.
  3. **Cache tier collision** — bytecode cache keyed by path+source hash only,
     so `VN_OPT` and legacy artifacts overwrote each other (silent mis-runs
     during differential testing). Cache filename now tagged `.opt` under the
     gate (`pipeline/cache.rs`); removed with the gate at Stage 4.
- **Extension-call ABI bug** — initially passed the receiver as arg 0 (callee
  saw `this = null`); fixed to the receiver slot via the dedicated
  `HirExpr::ExtensionCall` + `lower_ext_call`.

Remaining corpus fallbacks (to close): TaggedTemplate, pipeline `_` placeholders, intrinsics, and bigint/decimal/regex literals.

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

### 1.1 Member / index / method access + Inline Caches ✅ DONE
- **Done:** `HirExpr::Member`/`Index`/`MethodCall`; lowering emits `GetProperty`
  (`emit_rrc_ic` + IC slot), `GetIndex`, `CallMethod` (IC slot + name const).
  Per-function `cache_count` threaded → `ic_cache`/`feedback` sized in `finish`.
  Method-call detection (non-computed `.name(args)`, not `super`/extension/
  intrinsic). Module-slot reads, extension members/calls, optional chaining, and
  non-identity call mappings are fully supported via lowering & desugaring.
- **Done:** `GetProperty`/`CallMethod` are fully verified end-to-end.
- **Done:** optional chaining (`GetPropertyMaybe`), module slots (§1.8),
  extensions, super (§1.6) are completed.

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
  `Pipeline` non-placeholder (`x |> f` desugars to a plain `Call f(x)`);
  `Try`-expr `expr?` (`HirExpr::TryOp` → `GetEnumTag` + `JumpIfTrue` + early `Return`,
  mirroring `compile_try_expr`); `Is` type-test `expr is T` (`HirExpr::TypeTest` +
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

### 1.4 Assignment targets (full) 🟡 PARTIAL
- **Done:** `obj.prop = v` → `SetProperty`, `arr[i] = v` → `SetIndex`, destructuring assignment. Reassign-global → confirm `DefineGlobal` vs `StoreGlobal` semantics (legacy `expr/assignment.rs`).
- **Deferred:** optional assignment targets, compound member assignment (e.g., `x.y += 1`), module-slot assignment, super assignment, non-identifier property assignments.

### 1.5 Remaining statements 🟡 PARTIAL
- **Done (all mirror legacy `stmt.rs` byte-behaviourally):**
  - `throw` → `Throw`; `try/catch/finally` → `Try`/`PopTry`/`Throw` with the
    nested-`Try`-around-`catch` finally dance. A per-function `finally_stack`
    threads pending `finally` bodies into `return`/`break`/`continue` (re-lowered
    + `PopTry` before the transfer). Catch param = a local copied from `err_reg`.
  - `for-of` → iterator protocol (`Symbol.iterator`/`next`/`done`/`value`);
    `for-in` → `ObjectKeys` + index loop.
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
    proper labeled `break`/`continue`; anonymous nested classes; nested namespace declarations.

### 1.6 Classes 🟡 PARTIAL
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
- **Done (inheritance + static/method/class decorators + static blocks/fields/methods):** inheritance (`extends`/`super`/`GetSuper`), static members, getters/setters, class/method decorators, destructor, static blocks, `instanceof`, compound member assignment, module-slot/extension setters.
- **Deferred → fallback:** anonymous classes (top-level, exported, or nested).

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

### 1.7 Enums 🟡 PARTIAL
- **Done:** `Decl::Enum` → `HirExpr::Enum` (`MakeClass` + per-variant
  `MakeEnumVariant`/`DefineStatic` with the `Enum.Variant[:fields]` meta string,
  incrementing tags, integer discriminants) + instance fields/methods (class
  core). `match` expr → `HirExpr::Match` (legacy `compile_match` branch chain):
  wildcard, literal (`Eq`), identifier-bind, and enum-variant patterns
  (`__variant_name__` compare + `value{i}` payload binds). Verified end-to-end
  via varn-opt: enum decls, payload variant construction (`Shape.Circle(10)`),
  and variant matching (`Circle(r) => …`) match legacy exactly (`300 20 red
  red`). Suite 686/686.
- **Deferred → fallback:** match guards, non-integer enum discriminants, static enum fields, enum field initializers, static enum methods, record/sequence/type patterns.

  Original notes:
- `Decl::Enum`: variants (tags), payloads, `MakeEnumVariant`, `GetEnumTag`,
  pattern matching over variants (`match` expr). Ref legacy enum codegen +
  `tests/41-advanced-enums.vn`.

### 1.8 Modules (import/export) 🟡 PARTIAL
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
- **Deferred → fallback:** export default/named/all, re-exports, nested namespaces.

  Original notes:
- `import`/`export` decls → `LoadModule`/`LoadModuleSlot`/`StoreModuleSlot`,
  module object, export ordering. `export_names`. Ref VM `exec/modules.rs`,
  `ctx_modules.rs`; `varn-modules` resolver. Needed for multi-module programs
  (the suite imports stdlib + cross-module).

### 1.9 Async / generators 🟡 PARTIAL
- **Done:** `is_async`/`is_generator` functions; `await`/`yield`/`spawn` expressions lowerer and bytecode generator; Task/Isolate runtime integration; async and generator class methods support. Corrected VM suspend dest-reg alignment.
- **Deferred:** async/generator extension methods.

### 1.10 Patterns / params / generics 🟡 PARTIAL
- **Done (simple default/optional/rest params):**
  - **Default** `x = expr`: `HirParam.default` (lowered in the function's own
    scope, two-pass so defaults can see earlier params) → emitter prologue
    `IsNull`/`JumpIfFalse`/`Move` before the body (legacy `function.rs` loop).
  - **Optional** `x?`: no codegen — the VM passes null when the arg is absent.
  - **Rest** `...args`: `HirFunction.has_rest` → `FunctionProto.has_rest`; the VM
    collects surplus args into the slot's array. `arity` already counts it.
    - Verified differential vs legacy (default/rest identical); suite 668/668;
      `VN_OPT bench` (regalloc ON) clean.
- **Deferred → fallback:** generic functions (generic methods/classes remain deferred); destructuring params (array/object/rest/assignment patterns) in params/`let`/for; named/spread args + non-identity `get_call_mapping` (arg reordering/defaults). Generics monomorphization is type-erased at codegen.

### 1.11 Differential testing harness ✅ DONE
- Add `vn debug -p hir` (dump HIR) and a **bytecode differ**: compile a program
  both ways (legacy vs varn-opt, pre-regalloc) and diff opcode streams to catch
  emission mismatches early. Per-module suite under `VN_OPT=1` must stay
  686/686 at every sub-step (the regression trap lives in `bench`, regalloc on).

---

## 2. Stage 2 — SSA (60% Complete)

**Backbone complete + wired end-to-end.** SSA construction (CFG + loops),
verifier, and out-of-SSA → bytecode all land for the scalar / control-flow
subset, gated behind `VN_OPT_SSA` (per-function: try SSA, fall back to the §1
emitter on `Unsupported`). Validated: `VN_OPT=1 VN_OPT_SSA=1` runs the suite
**728/728** and `bench` clean (regalloc/JIT trap), with SSA actually compiling
every function in the suite it covers (the rest fall back). Remaining for 100%
is the **effectful instruction set** in SSA (calls, member/index, closures,
classes, enums, match, try) + `switch`/`for-of`/`for-in` control flow — the bulk
of re-expressing §1 through SSA so the path can be made unconditional (§4).

Build the real optimizer IR from HIR. Per-function.

### Architecture decision (locked) — block params + Braun on-the-fly
Chosen over the textbook phi-nodes + Cytron dominance-frontier approach the
original notes sketched. Rationale (perf + KISS):
- **Block parameters** (Cranelift/MLIR style) instead of phi nodes: merge
  operands live on the predecessor edges as block arguments, isomorphic to phis
  but with no in-block phi placement to keep consistent.
- **Braun et al. 2013 on-the-fly construction**: SSA is built in a single pass
  during HIR lowering (per-block `write`/`read` def maps; a sealed-block read
  with >1 pred spawns a block param and recurses into preds for operands; an
  unsealed block — a loop header whose back-edge isn't built yet — records an
  incomplete phi filled at seal time). No separate dominance-frontier pass →
  faster compilation; produces minimal SSA → same opt power as Cytron.
- A **dominator tree is computed separately** only where needed (the §2.3
  verifier, and any §3 pass that wants it), not for construction.

### 2.1 CFG construction 🟡 PARTIAL (straight-line, if/else & loops done)
- SSA IR in `crates/varn-opt/src/ssa/`: `ir.rs` (`SsaFunc`/`Block`/`BlockId`/
  `Value`/`ValueDef`/`Inst`/`InstKind`/`Terminator`); `build.rs` (HIR→SSA);
  `dump.rs` (text form for tests + future `vn debug -p ssa`).
- Scalar dataflow (params/locals → `Value`s, each carrying its `HirType`) is in
  SSA; effectful/heap ops will be ordinary instructions threaded in program
  order. `Terminator` = `Return`/`Jump{args}`/`Branch{cond,then/else+args}`/
  `Unreachable` (construction placeholder).
- Coverage so far: straight-line bodies (scalar literals, param/local read &
  reassign, typed `Binary`/`Unary`, assignment-as-expression + `++`/`--` on
  scalar bindings, `Return`), **`if`/`else`** (branch + merge block params), and
  **loops** — `while`, C-style `for` (`update` in its own block so `continue`
  runs it), `do-while`, plus `break`/`continue` via a `LoopCtx` target stack.
  Anything else → `Err(Unsupported)`.
- **Bug fixed:** the `if` merge recorded `then_b`/`else_b` as its predecessor
  instead of the *current* block after the body — wrong (and a panic at
  seal/phi time) once a branch body contains nested control flow. Now uses the
  actual current block. Only reachable from SSA construction (not the run/bench
  path), so it never affected the suite; guarded by a new nested-`if` test.

### 2.2 SSA form 🟡 PARTIAL (Braun construction core done)
- `write_var`/`read_var`/`read_var_recursive` + `seal_block`/`add_phi_operands`
  implement Braun construction with block params. Entry-block params = the
  function params.
- **Trivial-phi removal**: a `simplify_phis` fixpoint post-pass over the finished
  CFG removes block params with ≤1 distinct non-self operand, replacing all uses
  with the unique operand (Braun's `tryRemoveTrivialPhi`, run as a post-pass
  rather than interleaved — uniform use-replacement, no spurious phis for vars
  unmodified across a merge). Skips the entry block (its params are real args).
- Tests (`ssa/tests.rs`, 12): identity, const+binary, local reassign, one-sided
  `if` phi, two-sided `if/else` phi, unmodified-var-no-phi (trivial removal),
  `while` header phi, `for` update-block increment, `do-while` latch, `break`
  to exit, `continue` to update, nested-`if` fall-through merge. The
  incomplete-phi / unsealed-header seal path is now exercised by the loop tests.
- **Pending in §2.1/2.2**: `switch`/`match`/`try` control flow, and the full
  effectful instruction set (calls, member/index, closures, classes, etc.) so
  every §1-covered construct lowers to SSA. Until then `build_function` returns
  `Err(Unsupported)` for those and the pipeline keeps using the §1 HIR→bytecode
  path.

### 2.3 Verifier ✅ DONE
- `ssa/verify.rs`: (1) each `Value` defined once (param or inst dest); (2) every
  predecessor edge carries exactly as many block-args as the target has params;
  (3) terminator targets in range; (4) every use is defined and **dominated** by
  its def (phi operands checked against the predecessor edge they flow in on).
  Dominators via Cooper-Harvey-Kennedy over reverse-postorder. Runs inside
  `try_compile_function` as a safety net — a violation forces fallback to the §1
  path rather than emitting wrong code. Tested (`verifier_accepts_constructed_ssa`).

### 2.4 Out-of-SSA + lowering to bytecode ✅ DONE (for the §2.1 subset)
- `ssa/emit.rs`: **critical/phi-edge splitting** (a pad block on every branch
  edge whose target has params, so phis are fed only from single-successor
  `Jump` edges) → **register-per-value** assignment (entry params = `r1..=P`,
  receiver `r0`; +scratch +null reg; `Unsupported` past 255) → block emission in
  index order. Phi operands become **parallel `Move`s** on each edge (cycles
  broken via the scratch reg). Jumps: fall-through when the target is the next
  block, `Loop` for back-edges, forward `Jump`/`JumpIf*` with deferred offset
  fixups; a conditional with a backward target is reshaped so the forward exit is
  the conditional and the back-edge an unconditional `Loop`. `regalloc_post` +
  `slot_kinds::infer` run downstream via the `VN_OPT` gate, so this emits naive
  one-reg-per-value bytecode and lets them compress + type it.
- Wired: `ssa::try_compile_function` (build → verify → emit), called from
  `lower::lower_function` under `VN_OPT_SSA` with per-function fallback
  (`VN_OPT_TRACE` reports which functions SSA compiled vs fell back).
- Verified: `VN_OPT=1 VN_OPT_SSA=1` → suite **728/728** + `bench` clean (execute
  p50 ~20 ms, no regression vs the no-SSA path); `tests/scratch_ssa.vn` (while/
  for/do/if/break/continue functions) prints identically under legacy, `VN_OPT`,
  and `VN_OPT+VN_OPT_SSA`.
- **Pending (the §2.1/2.2 instruction-set gap):** once calls/heap/closures/
  classes/etc. lower to SSA, the gate can be made unconditional and the no-SSA
  path retired (§4). Until then SSA compiles the subset it covers and the rest
  fall back, both paths green.

---

## 3. Stage 3 — opt passes (on SSA) (0% Complete)

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
   508, Object 351, BoundMethod 155 / run). EscapeAnalysis ideas.
8. **Peephole / strength reduction** — `*2`→shift, `AddImm`, etc.
9. **Type-driven unboxing region** (stretch) — keep ints/floats untagged across
   a block, box only at boundaries (the per-op box/unbox cost noted earlier).

Order matters: const-fold → copy-prop → DCE → CFG-simplify iterate to fixpoint;
then GVN; then inline + re-run; then escape.

---

## 4. Stage 4 — replace legacy (Hito A) (70% Complete)

**Goal: delete legacy codegen and make varn-opt's §1 HIR→bytecode path the only
backend.** SSA (§2/§3) is *not* required for this — it's an optimization layer
that grows on top afterwards.

### Audit (2026-06-20): varn-opt §1 is ~99% complete
The full suite — **51 comprehensive feature files** (destructuring, generics,
named args, match, decorators, enums, modules, classes, async, exports …) all
imported by `tests/main.vn` — compiles through varn-opt with **zero fallback**
(`VN_OPT=1 VN_OPT_TRACE=1` shows no `fallback` lines). The plan's old
"deferred → fallback" notes were stale: those features are implemented. Suite
**728/728** on the varn-opt path.

### Crash-safety fix ✅ DONE
The HIR lowering previously **`panic!`'d** on unsupported constructs instead of
returning `Err`, so anything varn-opt couldn't lower *crashed* `vn` under
`VN_OPT` rather than falling back. All ~18 such panics across
`hir/lower/{expr,stmt,decl,mod}.rs` are now `Err(OptError::Unsupported(..))`, so
unsupported constructs degrade to legacy gracefully. Verified: a `match` guard
now prints the correct result via fallback (`Unsupported("hir: match guard")`)
instead of aborting; suite still 728/728.

### Residual constructs ✅ DONE (0 fallback for valid programs)
All reachable fallbacks closed; each mirrors or improves on legacy:
- **match guards** (`n if cond => …`) — implemented *correctly* (HIR
  `HirMatchCase.guard` + emitter branch). Legacy `compile_match` silently
  **ignored** guards (`_ => None`), so this is a strict improvement; verified
  `match v { n if n>10 => 1, n if n>3 => 2, _ => 0 }` yields `2` (legacy: `1`).
- **match `Sequence` / `Type` patterns** → lowered as wildcard, exactly matching
  legacy's `_ => None` (real nested destructuring is a future feature for both).
- **async/generator extension methods** — `is_async`/`is_generator` threaded
  through `push_global_closure`.
- **anonymous class declarations** — synthesise `"anonymous"` (mirrors top-level).
- **optional assignment targets** (`o?.x = v`) — `optional` ignored = plain member
  store, mirroring legacy `store_to_target` (verified prints `5`, no fallback).
- **bigint literal overflow** (> i128) → defaults to `0`, mirroring legacy
  `codegen/expr/mod.rs` (legacy also caps at i128 and uses `0` on overflow).
- `debugger` is rejected by the checker (not a real construct).

The only remaining `Err` sites in HIR lowering are **parser/checker-unreachable**
defensive cases (non-identifier property/super/assign targets — the parser only
produces identifiers in non-computed members and the checker rejects non-lvalue
assigns; decl/op catch-alls where every variant already has an explicit arm).
Crucially these are now `Err` (graceful fallback) not `panic!` (crash).

**Validation:** suite **728/728** on both `VN_OPT=1` and `VN_OPT=1 VN_OPT_SSA=1`;
varn-opt unit tests 13/13; targeted probes (guards, `o?.x=`) correct with no
fallback trace.

### Then the mechanical replacement
- Route `compile_direct` through `varn-opt` unconditionally (all 3 `lib.rs`
  entries).
- Move `regalloc_post`/`liveness`/`ir`/`slot_kinds` into a shared backend crate
  (or keep in varn-compiler and have varn-opt depend on a thin `varn-backend`).
- **Delete** legacy AST→bytecode: `codegen/compiler.rs`, `codegen/expr/`,
  `codegen/stmt.rs`, `codegen/function.rs`, `codegen/class.rs`,
  `analysis/inline.rs` (superseded by SSA inlining). Keep `regalloc_post`,
  `liveness`, `ir`.
- Remove `VN_OPT`/`VN_OPT_TRACE` (keep `VN_OPT_SSA` for the §2 layer until §2 is
  complete, then remove it too).
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

---

## 7. Remaining Stage 1 Parity Tasks (Added 2026-06-19)

The following tasks are required to achieve 100% parity with the legacy compiler for Stage 1:

### Phase 2: Core Object Literals, New, & Intrinsics (Current Target)
* **New Expression**: Instantiating classes via `new` keyword.
* **Intrinsics**: Direct opcode emission for VM built-ins (e.g. `Math.*`).
* **Object Literals (Advanced)**: Computed keys, method declarations, getters, and setters.
* **Array Literals (Advanced)**: Spreads (`...`) and holes (empty elements).
* **Updates on Members/Indices**: Prefix/postfix `++`/`--` on member and index targets (e.g., `obj.x++`, `arr[0]--`).

### Phase 3: Patterns, Destructuring, & Control Flow
* **Destructuring Declarations & Assignments**: Destructuring array and object patterns in `let`/`const` declarations, variable assignments, function parameters, and loops.
* **for-await-of Bucles**: Asynchronous iteration.
* **using & catch Destructuring**: Destructuring catch parameters and `using` disposables.
* **Labeled break/continue**: Non-innermost loop transfers.
* **Nested Namespaces**: Declarations of spaces of names inside other namespaces.

### Phase 4: Enums, Matching, Modules, & Generics
* **Anonymous Classes**: Declarations of anonymous classes.
* **Advanced Match Patterns**: Guards (`if` in match), `Record` patterns, sequence patterns, and type patterns.
* **Enum Extensions**: Non-integer enums, static enum members, and enum field initializers.
* **Module Exports**: Default exports, named exports, wildcard exports, and re-exports.
* **Generics**: Generic classes, generic functions, and generic methods.
* **Call Mappings**: Named arguments, spread arguments, and reordered/default argument mapping.

