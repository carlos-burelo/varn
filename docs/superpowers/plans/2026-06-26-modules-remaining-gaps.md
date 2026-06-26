# MODULES.md Remaining Gaps — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the four genuinely-unimplemented items that `MODULES.md`'s design intent points at, without re-deriving or regressing the already-merged core-types / op-id-dispatch / `varn_contract!` architecture.

**Architecture:** `MODULES.md` is a research report. ~90% of it is already built on `main` via two approved redesigns: `varn_contract!` makes the `.vn` the single source of truth for builtins (drift = compile error), and a build-stable **op-id** + `OpCode::CallNativeOp` gives type-driven native dispatch (JIT-supported). This plan targets ONLY what those redesigns left undone, verified against live code, not the doc's literal (and regressive) crate-split / per-method-opcode proposals.

**Tech Stack:** Rust workspace (`crates/*`), Varn stdlib in `.vn`, `varn_contract!` proc-macro, x86-64 JIT (`varn-jit`), register VM (`varn-vm`).

## Global Constraints

- Validation baseline: build `vn` with `cargo build -p varn-cli --bin vn`; then `target/release/vn.exe run tests/main.vn` must pass the full suite + print the `assertSummary()` line with zero failures.
- JIT/regalloc trap: ALSO run `target/release/vn.exe bench tests/main.vn` — `run` alone has historically passed code that `bench` (force-JIT) breaks.
- Builtins isolation check: `cargo check -p varn-builtins --features runtime` (without `--features runtime` the modules are excluded and edits silently "pass").
- Varn binding keyword is `let`/`var`, NOT `val`. `assert(label, condition)` and `print(...)` are globals (no import).
- CLAUDE.md rules apply: no dual/legacy paths, no dead code, no unnecessary breaking churn, measure before claiming perf wins (`<performance_rules>`).
- The doc's crate-split (`varn-core-types`/`varn-intrinsics`/`varn-std`) and per-method string opcodes are explicitly OUT OF SCOPE — they duplicate `varn-core`/`varn-types`/`varn-builtins` and the op-id mechanism, violating `<replacement_over_extension>`.

---

## File Structure

| File | Phase | Responsibility |
|---|---|---|
| `crates/varn-builtins/src/modules/std/math/runtime/math_runtime.vn` | 1 | Declare the missing host math primitives |
| `crates/varn-builtins/src/modules/std/math/math.rs` | 1 | `varn_contract!` native bodies for those primitives |
| `crates/varn-builtins/src/modules/std/math/math.vn` | 1 | `Math.acos/asin/atan/atan2` route to natives instead of `0.0` stubs |
| `tests/52-math-trig.vn` (new) + `tests/main.vn` | 1 | Regression coverage in the suite |
| `crates/varn-op-macros/src/varn_contract.rs` | 2 | Emit typed monomorphic wrapper alongside the `&[VmValue]` wrapper |
| `crates/varn-types/src/marshal.rs` | 2 | Typed-ABI marshalling support |
| `crates/varn-jit/src/codegen/calls.rs` | 2,3 | Typed register-passing for `CallNativeOp`; inline codegen for hot core ops |
| `crates/varn-vm/src/exec/ctx_jit_values.rs` | 2 | `jit_call_native_op` typed fast path |
| `crates/varn-core/src/intrinsic_ops/{array,string}.rs` + `crates/varn-vm/src/exec/intrinsics/{array,string}.rs` | 3 | Either inline the hottest or delete dead scaffold |
| `crates/varn-checker/src/checker_annotations.rs` | 3 | Route hot core ops to the chosen path |
| `docs/NATIVE_ABI_SPEC.md` (new) | 4 | Formal per-op signatures, ABI, pre/postconditions |

Phases are independently shippable. Recommended order: **1 → 4 → 3 → 2** (correctness first, then the doc that pins the contract, then the lateral/perf work last so it can be measured against a documented baseline).

---

## Phase 1 — Fix Math inverse-trig stubs (real correctness bug)

**Verified live:** `Math.pow/exp/log` already return correct values (the `Intrinsic` opcode intercepts `std:math/{pow,exp,log}` before the `.vn` body). Only **`acos/asin/atan/atan2`** are broken — they are NOT in the `MathOp` intrinsic enum and their `.vn` bodies are `return 0.0;` ([math.vn:59-70,97-99](../../../crates/varn-builtins/src/modules/std/math/math.vn#L59-L70)). They can NOT be added to `MathOp` (the 4-bit domain caps at 16 ops; Math uses `0x0..0xC`, only 3 slots free). Correct fix = route through `runtime:math` natives exactly like `sqrt`/`sin` already do.

Probe proving the bug (run before fixing):
```
import { Math } from "std:math";
print(Math.acos(0.0));   // prints 0  — WRONG, should be 1.5707963...
print(Math.asin(1.0));   // prints 0  — WRONG, should be 1.5707963...
print(Math.atan(1.0));   // prints 0  — WRONG, should be 0.7853981...
print(Math.atan2(1.0,1.0)); // prints 0 — WRONG, should be 0.7853981...
```

### Task 1: Route acos/asin/atan/atan2 to host natives

**Files:**
- Modify: `crates/varn-builtins/src/modules/std/math/runtime/math_runtime.vn`
- Modify: `crates/varn-builtins/src/modules/std/math/math.rs:18` (inside the `varn_contract!` impl block)
- Modify: `crates/varn-builtins/src/modules/std/math/math.vn:1-11` (import) and `:59-70,97-99` (bodies)
- Test: `tests/52-math-trig.vn` (new), `tests/main.vn`

**Interfaces:**
- Produces native fns `mathAcos(x)`, `mathAsin(x)`, `mathAtan(x)`, `mathAtan2(y,x)` in module `runtime:math`, each `Result<f64, String>` (function-module ops are fallible per `varn_contract!`).

- [ ] **Step 1: Write the failing test** — create `tests/52-math-trig.vn`:

```
import { Math } from "std:math";

const PI: float = 3.141592653589793;
function near(a: float, b: float): bool {
    return a - b < 0.0001 && b - a < 0.0001;
}

assert("acos(0)=pi/2",    near(Math.acos(0.0), PI / 2.0));
assert("acos(1)=0",       near(Math.acos(1.0), 0.0));
assert("asin(1)=pi/2",    near(Math.asin(1.0), PI / 2.0));
assert("asin(0)=0",       near(Math.asin(0.0), 0.0));
assert("atan(1)=pi/4",    near(Math.atan(1.0), PI / 4.0));
assert("atan2(1,1)=pi/4", near(Math.atan2(1.0, 1.0), PI / 4.0));
assert("atan2(0,1)=0",    near(Math.atan2(0.0, 1.0), 0.0));
print("[PASSED] 52. Math inverse trig");
```

- [ ] **Step 2: Wire the test into the suite** — in `tests/main.vn`, add before the `assertSummary()` line:

```
import "./52-math-trig.vn"
```

- [ ] **Step 3: Run it to verify it fails**

Run: `target/release/vn.exe run tests/main.vn`
Expected: assertions `acos(0)=pi/2`, `asin(1)=pi/2`, `atan(1)=pi/4`, `atan2(1,1)=pi/4` FAIL (stub returns 0.0); `assertSummary()` reports failures.

- [ ] **Step 4: Declare the host primitives** — append to `crates/varn-builtins/src/modules/std/math/runtime/math_runtime.vn`:

```
export declare function mathAcos(x: float): float;
export declare function mathAsin(x: float): float;
export declare function mathAtan(x: float): float;
export declare function mathAtan2(y: float, x: float): float;
```

- [ ] **Step 5: Implement the native bodies** — in `crates/varn-builtins/src/modules/std/math/math.rs`, inside `impl MathRuntime { ... }` (after `mathRandom`):

```rust
        fn mathAcos(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.acos()) }
        fn mathAsin(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.asin()) }
        fn mathAtan(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.atan()) }
        fn mathAtan2(_ctx: &mut dyn NativeCtx, y: f64, x: f64) -> Result<f64, String> { Ok(y.atan2(x)) }
```

- [ ] **Step 6: Route the Varn bodies to the natives** — in `crates/varn-builtins/src/modules/std/math/math.vn`, extend the import (lines 1-11) to include the four new names, then replace the four stub bodies (lines 59-70, 97-99):

```
    export function acos(x: float): float {
        return mathAcos(x);
    }
    export function asin(x: float): float {
        return mathAsin(x);
    }
    export function atan(x: float): float {
        return mathAtan(x);
    }
    export function atan2(y: float, x: float): float {
        return mathAtan2(y, x);
    }
```

- [ ] **Step 7: Drift + isolation check**

Run: `cargo check -p varn-builtins --features runtime`
Expected: compiles clean. (A signature mismatch between `math_runtime.vn` and `math.rs` surfaces here as E0046/E0053.)

- [ ] **Step 8: Build and run the suite to verify it passes**

Run: `cargo build -p varn-cli --bin vn` then `target/release/vn.exe run tests/main.vn`
Expected: `[PASSED] 52. Math inverse trig` prints; `assertSummary()` reports zero failures.

- [ ] **Step 9: JIT trap check**

Run: `target/release/vn.exe bench tests/main.vn`
Expected: exit 0, no regression.

- [ ] **Step 10: Commit**

```bash
git add crates/varn-builtins/src/modules/std/math tests/52-math-trig.vn tests/main.vn
git commit -m "fix(math): route acos/asin/atan/atan2 to host natives (were 0.0 stubs)"
```

---

## Phase 4 — Formal native-ABI / intrinsic spec (do early; pins the contract)

The doc (§3.4/§3.5) asks for a documented, per-op ABI with pre/postconditions. None exists; `HOST_BOUNDARY_SPEC.md` covers the host boundary but not the op-id/intrinsic dispatch ABI. Writing this BEFORE the perf work (Phases 2-3) gives those phases a written invariant to preserve.

### Task 2: Author `docs/NATIVE_ABI_SPEC.md`

**Files:**
- Create: `docs/NATIVE_ABI_SPEC.md`
- Modify: `docs/CRATES_STATE.md` (add a one-line pointer to the new doc)

**Interfaces:** None (documentation). Source the facts from verified live code, not `MODULES.md`:
- op-id = FNV-1a over `module::symbol` / `module::class::symbol` — `crates/varn-core/src/op_id.rs`.
- `CORE_MODULE = "globals"`, `CORE_CLASSES` list — `op_id.rs:49,65`.
- `NativeOpEntry` kinds (0x01 standalone, 0x03 method, 0x04 static, 0x05 getter, 0x14 static-getter) — `crates/varn-op-macros/src/varn_contract.rs`, `crates/varn-builtins/.../dispatch/entry.rs`.
- Intrinsic wire byte `DDDD_OOOO` (4-bit domain × 4-bit op), domains Math/String/Array/TypeCheck — `crates/varn-core/src/intrinsic_ops/wire.rs`.
- Two dispatch tiers: `OpCode::CallNativeOp` (op-id, general) vs `OpCode::Intrinsic` (wire byte, capped/hot) — `crates/varn-vm/src/exec/dispatch/mod.rs`, `crates/varn-vm/src/exec/intrinsics/mod.rs`.
- Calling convention regs (`ARG_CTX`, `ARG_BASE`, `ARG_EXEC_CTX`, `ARG_CLOSURE`, Win shadow-space 32) — `crates/varn-jit/src/codegen/calls.rs:778-821`.
- Marshalling (`FromVm`/`IntoVm`/`VnArray`, receiver mapping) — `crates/varn-types/src/marshal.rs`.

- [ ] **Step 1: Write the spec** with these sections: (1) the three layers Authoring/Binding/Dispatch; (2) op-id computation + the `globals::Class::method` scheme + `.vnc` portability guarantee; (3) `NativeOpEntry` kind table; (4) intrinsic wire encoding + domain/op cap (16/domain) and when an op belongs in Intrinsic vs op-id; (5) the runtime calling convention (register table, Windows vs SysV, stack alignment); (6) per-op signature convention (`fn(&mut dyn NativeCtx, args…) -> Result<T,String>` for function-modules, infallible `T` for class methods) with the receiver-at-args[0] rule and the static-method exception; (7) semantic contract: out-of-range → runtime error, GC-root safety, no incorrect write barriers.

- [ ] **Step 2: Cross-check every claim against the cited file** (open each, confirm line still matches). Fix any drift.

- [ ] **Step 3: Add pointer** in `docs/CRATES_STATE.md` and commit:

```bash
git add docs/NATIVE_ABI_SPEC.md docs/CRATES_STATE.md
git commit -m "docs: formal native-ABI + intrinsic dispatch spec"
```

---

## Phase 3 — Resolve the dormant Array/String Intrinsic scaffold

**Verified live:** `intrinsic_ops/array.rs` (Len/Push/Pop/Contains) and `intrinsic_ops/string.rs` (Len/Contains/StartsWith/…) plus their VM dispatch (`exec/intrinsics/array.rs`, `string.rs`) are fully implemented BUT never reached: the checker only emits `record_intrinsic` for receivers typed `Named(_, Some(origin))` (i.e. `Math`), so `arr: int[]` / `s: str` fall through to the op-id `record_native_op` path ([checker_annotations.rs:500-524](../../../crates/varn-checker/src/checker_annotations.rs#L500-L524)). The op-id path already handles these correctly and is JIT-compiled. So the Intrinsic array/string code is currently **dead**.

**Decision gate (first task):** the only payoff from routing array/string through Intrinsic is if the JIT *inlines* them. Today `emit_intrinsic` inlines only Math `abs/sqrt/floor` for float args; everything else calls the `dispatch_intrinsic` helper ([calls.rs:745-822](../../../crates/varn-jit/src/codegen/calls.rs#L745-L822)) — same cost as the op-id path. So:
- **Option A (inline & activate):** add inline JIT codegen for `array.length` (and `string.length`) — a raw heap-length load, no call — then wire the checker to route those to Intrinsic. Real win for length-in-hot-loop.
- **Option B (delete dead code):** remove the array/string `intrinsic_ops` + VM intrinsics + their `MAP_ENTRIES`, keeping only Math (which is genuinely inlined). Honors `<anti_god_object>`/"no código muerto"; zero perf change.

Pick via measurement (Task 3). Default recommendation: **Option B unless Task 3 shows a measurable length-loop win**, because `arr.length` already resolves O(1) (commit `22afc6c`) and Option A adds a second dispatch tier for the same op.

### Task 3: Measure whether inlining array length beats the current path

**Files:** scratch `.vn` only; no source change yet.

- [ ] **Step 1: Write a length-bound hot loop** `tests/_bench_len.vn`:

```
let arr: int[] = [];
for (let i = 0; i < 1000; i = i + 1) { arr.push(i); }
let total: int = 0;
for (let r = 0; r < 50000; r = r + 1) {
    for (let i = 0; i < arr.length; i = i + 1) { total = total + 1; }
}
print(total);
```

- [ ] **Step 2: Baseline-measure** with `target/release/vn.exe bench tests/_bench_len.vn` at array sizes 1k/2k/4k (edit the first bound). Record ns/iter. This is the number Option A must beat.

- [ ] **Step 3: Decide A or B**, record the measurement + decision in the commit message of whichever task you execute next. Delete `tests/_bench_len.vn`.

### Task 4A: (if Option A) Inline `array.length` / `string.length` in the JIT

**Files:**
- Modify: `crates/varn-jit/src/codegen/calls.rs:726-776` (`emit_intrinsic`, add array/string length arms modeled on the `sqrtsd` block)
- Modify: `crates/varn-checker/src/checker_annotations.rs:507-524` (when `(class, method)` maps to an intrinsic wire, prefer `record_intrinsic` over `record_native_op`)
- Modify: `crates/varn-core/src/intrinsic_ops/{array,string}.rs` (trim `MAP_ENTRIES` to only the ops you actually inline; delete the rest per "no dead code")
- Modify: `crates/varn-vm/src/exec/intrinsics/{array,string}.rs` (keep only retained ops)
- Test: extend `tests/05-arrays.vn` / `tests/03-strings.vn` with length-in-loop asserts

**Interfaces:**
- Consumes: `core_class_of_type` + a new `(class,method) -> Option<u8>` intrinsic lookup (add `pub fn core_intrinsic_wire(class: &str, method: &str) -> Option<u8>` in `varn-core/src/intrinsic_ops/mod.rs`, keyed off the existing wire enums — do NOT reuse the `"std:array/len"` string keys; key on `(class,method)`).
- Produces: inline machine code for `length` so a JIT'd length loop performs no native call.

- [ ] **Step 1:** Add `core_intrinsic_wire(class, method)` to `intrinsic_ops/mod.rs` returning the wire byte for `("Array","length")`/`("str","length")`. Unit-test it.
- [ ] **Step 2:** In `checker_annotations.rs`, inside the `core_has_method` branch, first try `core_intrinsic_wire` → `record_intrinsic`; else `record_native_op`. Add a checker unit test asserting `arr.length` annotates intrinsic.
- [ ] **Step 3:** In `emit_intrinsic`, add an arm: when `wire_byte` decodes to Array/String `Len`, load the receiver (`first_reg`), read the heap object length inline, box as int, store. Model the load/box on the existing `sqrtsd` arm + `VmValue::from_int` layout. Guard with `emit_debug_assert` for the receiver being a heap ptr.
- [ ] **Step 4:** Trim the now-unreachable `MAP_ENTRIES`/dispatch arms for any array/string op you did NOT inline (keep behavior via op-id). Confirm no `intrinsic_ops` symbol is dead (`cargo build` warnings).
- [ ] **Step 5:** `vn run tests/main.vn` + `vn bench tests/main.vn` pass; re-run the Task 3 bench → confirm the inlined length loop beats baseline. Commit with the measurement.

### Task 4B: (if Option B) Delete the dead array/string Intrinsic scaffold

**Files:**
- Modify: `crates/varn-core/src/intrinsic_ops/mod.rs` (drop `array`, `string` modules)
- Delete: `crates/varn-core/src/intrinsic_ops/array.rs`, `crates/varn-core/src/intrinsic_ops/string.rs`
- Delete: `crates/varn-vm/src/exec/intrinsics/array.rs`, `crates/varn-vm/src/exec/intrinsics/string.rs`
- Modify: `crates/varn-vm/src/exec/intrinsics/mod.rs:13-19` (remove Array/String domain arms)
- Modify: `crates/varn-core/src/intrinsic_ops/map.rs:11-15` (drop the `string`/`array` chains)
- Modify: `crates/varn-core/src/intrinsic_ops/wire.rs` (remove unused `String`/`Array` domain variants if nothing else references them)

- [ ] **Step 1:** Confirm nothing routes to these (grep `record_intrinsic` callers; the checker only feeds Math today). 
- [ ] **Step 2:** Remove the modules + dispatch arms above.
- [ ] **Step 3:** `cargo build -p varn-cli --bin vn` clean (no unused-warning). `vn run tests/main.vn` + `vn bench tests/main.vn` pass (behavior unchanged — array/string still served by op-id).
- [ ] **Step 4: Commit**

```bash
git commit -am "refactor(core): drop dead Array/String Intrinsic scaffold (served by op-id)"
```

---

## Phase 2 — Typed monomorphic native wrappers (dispatch-abi Phase 3b)

**This is the deep, genuinely-unstarted perf gap.** Today every `CallNativeOp` — including hot `arr.push(i)` / `s.charAt(i)` — boxes args into a `&[VmValue]` slice and runs per-arg `FromVm`/`IntoVm`, even in JIT'd code: `emit_call_native_op` always builds the stack slice and calls `jit_call_native_op` ([calls.rs:824-888](../../../crates/varn-jit/src/codegen/calls.rs#L824-L888)). The checker statically knows the exact arg types but discards them at the ABI boundary (`<backend_principle>` violation). Goal: for ops whose params are all `int`/`float`/`bool`, generate a typed monomorphic wrapper (`fn(i64,i64)->i64`) and have the JIT pass values in registers, skipping the slice + marshalling.

> This phase is large and inherently measured. Treat Task 5 as a spike that must show a win before Tasks 6-7 land.

### Task 5: Spike + benchmark the typed-wrapper hypothesis

**Files:** scratch only.

- [ ] **Step 1:** Pick the hottest all-scalar core op (e.g. `array.push(int)`). Write `tests/_bench_push.vn` (a tight push loop) and `bench`-measure baseline at 3 sizes (`<performance_rules>`).
- [ ] **Step 2:** Hand-write (throwaway branch) a single typed wrapper for `push` and a direct register call path in `emit_call_native_op`, bypassing the slice. Measure. 
- [ ] **Step 3:** Record the delta. If <~5% on the hot loop, STOP — document the negative result in the plan and skip Tasks 6-7 (the `&[VmValue]` path is good enough; do not add a second ABI for no gain, per `<implementation_preferences>`). If meaningful, proceed.

### Task 6: Emit typed wrappers from `varn_contract!`

**Files:**
- Modify: `crates/varn-op-macros/src/varn_contract.rs` (alongside the `&[VmValue]` dispatch wrapper + `NativeOpEntry`, emit a typed wrapper + a new entry kind, e.g. `0x06 typed-method`, carrying a scalar-signature descriptor — only when every param + return is `int`/`float`/`bool`)
- Modify: `crates/varn-types/src/marshal.rs` (helpers to read/write scalar `VmValue` payloads without the generic `FromVm` path)
- Modify: `crates/varn-builtins/.../dispatch/entry.rs` + `native.rs` (expose `typed_op_fn(op_id) -> Option<TypedFn>` next to `native_op_fn`)

**Interfaces:**
- Produces: `pub fn typed_op_fn(op_id: u64) -> Option<TypedNativeFn>` where `TypedNativeFn` is an enum/union over the supported scalar arities (document exact variants in `NATIVE_ABI_SPEC.md` from Phase 4).

- [ ] **Step 1:** Define `TypedNativeFn` variants for the arities the spike proved worthwhile (start with arity-1 and arity-2 over `i64`/`f64`/`bool`-as-i64).
- [ ] **Step 2:** Extend the macro to detect all-scalar signatures and emit the typed wrapper + register it. Add a macro-expansion unit test (`cargo expand` or a compile-test) proving a scalar method gets both wrappers and a non-scalar method gets only the slice wrapper.
- [ ] **Step 3:** `cargo check -p varn-builtins --features runtime` clean; `vn run tests/main.vn` unchanged (typed wrappers not yet called).
- [ ] **Step 4: Commit.**

### Task 7: JIT register-passing path for typed ops

**Files:**
- Modify: `crates/varn-vm/src/exec/ctx_jit_values.rs` (`jit_call_native_op` gains a typed fast branch: if `typed_op_fn(op_id)` exists, read the scalar operands directly and call it, skipping slice construction)
- Modify: `crates/varn-jit/src/codegen/calls.rs:824-888` (`emit_call_native_op`: when the op-id is typed-eligible at JIT-compile time, emit the register-passing sequence instead of slice-build)

- [ ] **Step 1:** Add the typed branch in `jit_call_native_op`; gate behind `typed_op_fn`. Interpreter-path test: a typed op still returns identical results (`vn run`).
- [ ] **Step 2:** In `emit_call_native_op`, resolve typed-eligibility from `proto.chunk.constants` at compile time (same way the op-id is already read at line 837); emit register loads for the scalar args + direct call; fall back to the existing slice path otherwise.
- [ ] **Step 3:** `vn run tests/main.vn` + `vn bench tests/main.vn` pass (correctness identical). Re-run Task 5 bench → confirm the win holds end-to-end.
- [ ] **Step 4: Commit** with the before/after numbers in the message.

---

## Self-Review

- **Spec coverage vs MODULES.md intent:** §3.2 Math contract → Phase 1. §3.4 formal ABI/signatures → Phase 4. §3.3/§3.5 intrinsic table + JIT inline → Phase 3. §3.4 "values in registers, skip `&[VmValue]`" → Phase 2. Doc §4.1 crate-split + §3.3 per-method opcodes → intentionally excluded (already superseded; documented in Global Constraints).
- **Already-done items NOT re-planned:** `.vn`-as-source-of-truth (`varn_contract!`), op-id dispatch, `CallNativeOp` + JIT Phase 3a, NaN-box/SSO core types, `NativeCtx` — all live on `main`.
- **Type consistency:** `core_intrinsic_wire(class,method)->Option<u8>` (Phase 3) and `typed_op_fn(op_id)->Option<TypedNativeFn>` (Phase 2) are named identically wherever referenced. op-id facts sourced from `op_id.rs` (`core_method_op_id`, `CORE_MODULE="globals"`).
- **Measurement gates:** Phases 2 and 3 each open with a benchmark task that can veto the rest — honoring `<performance_rules>` and avoiding a second ABI / dual path for no gain.
