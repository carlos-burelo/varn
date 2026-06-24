# Varn Runtime Performance Investigation

Investigation of eight reported performance/correctness anomalies. All numbers
below are **measured on this machine** with the release binary
(`cargo build --release -p varn-cli`), Windows 11, via `vn run` (wall clock) and
`vn bench`. Bytecode dumps are from `vn debug -p bytecode`.

> **Methodology note up front (see Investigation 8):** `vn bench` enables
> hotspot/opcode profiling, which adds a `RefCell::borrow_mut` + hashmap update
> on *every* global access, allocation and call. Bench numbers are therefore
> inflated relative to `vn run`. Where the original spec quoted a number, it was
> a bench number; the matching `vn run` number is given alongside.

---

## Cross-cutting finding: integers are 48-bit

`VmValue::from_int` masks the payload to 48 bits and `as_int` sign-extends from
bit 47 — [`vm_value.rs:63`](../crates/varn-types/src/vm_value.rs#L63),
`MASK_INT48 = 0x0000_FFFF_FFFF_FFFF`.

```
int range = -2^47 .. 2^47-1  ≈ ±1.4 × 10^14
```

Values beyond that silently truncate (wrap) or, for literals, fall back to
`f64`. Probes:

```
2_000_000_000 + 2_000_000_000      -> 4000000000           (ok, > i32)
1_000_000_000 * 1_000_000_000      -> 1000000000000000000  (ok)
9223372036854775807 (i64::MAX)     -> 9223372036854776000  (became f64, rounded)
```

This directly corrupts Investigation 1 and 6 benchmarks (their accumulators
exceed 2^47), so their printed results are meaningless even though the timing
is real. **Recommendation:** either document 48-bit semantics loudly and trap
on overflow, or move to boxed `i64` (heap or a wider tag) for values outside the
inline range.

---

## Investigation 1 — Global variable access cost

### Root cause

Top-level `let`/`const` are compiled as **module globals**, not registers.
HIR lowering keys the decision on scope:
[`decl.rs:303`](../crates/varn-opt/src/hir/lower/decl.rs#L303)

```rust
let is_global = scope.is_global();   // true at module top level
... HirBinding::Global(name) ...     // vs HirBinding::Local(local) when nested
```

So in the benchmark:

* `sum` (top-level) → **global**: every `sum += i` is `LoadGlobal` + `Add` +
  `DefineGlobal` (store), each iteration.
* `i` (declared inside the `for`) → **register** (`AddInt`, `Move`).

Disassembly of the loop body (`vn debug -p bytecode`):

```
0012 LoadGlobal   r2 = global[0]   ; "sum"   <- reload every iter
0014 Add          r3 = r2 op r1            <- generic, type-erased
0016 DefineGlobal def global[0] = r3       ; "sum"  <- store every iter
0019 LoadIntOne   r2 = 1
0020 AddInt       r3 = r1 op r2            <- i++ is specialized
```

Two compounding problems:

1. **Q1/Q2:** `sum` is a global (memory load/store via `GlobalStore.values[idx]`,
   [`globals.rs`](../crates/varn-vm/src/globals.rs)); `i` is a register. Only
   `sum` generates the global traffic — confirmed by the profile showing one
   `LoadGlobalIdx`/`DefineGlobalIdx` pair per iteration.
2. The accumulator uses **generic `Add`** (runtime type dispatch) while `i++`
   uses `AddInt`. Integer specialization reaches the loop counter but not the
   `+=` accumulator.

### Q3 — Is there global→register hoisting?

**No.** There is no LICM / register-promotion of globals. The global is reloaded
and restored every iteration. More fundamentally, the whole loop lives in the
`<module>` function, which is **excluded from the JIT** (see Investigation 7),
so it runs in the interpreter with full global traffic.

### Q4 — Estimated / measured gain

Wrapping the identical loop in a function makes `sum`/`i` locals **and** makes
the function JIT-eligible:

| Variant | `vn run` | `vn bench` p50 | JIT |
|---|---|---|---|
| Top-level (`<module>`, global `sum`) | **3.15 s** | 4.43 s | `JIT runs 0` |
| Wrapped in `fn main()` (local `sum`) | **0.15 s** | 0.10 s | `main` JIT'd, `JIT runs 1` |

**≈ 21× faster** (`vn run`); 44× in bench (profiling-inflated). The speedup is
the combination of (a) register vs global memory traffic and (b) the loop
becoming JIT-compilable.

### Recommended compiler changes

* Treat module-private (non-exported, non-isolate-captured) top-level
  `let`/`const` as **locals of the `<module>` function** (registers). The
  checker already knows the binding is module-private; use it
  (`<backend_principle>`).
* Add integer specialization for compound-assign accumulators
  (`Add` → `AddInt` when both SSA operands are typed `int`).
* For globals that must stay global, add loop-invariant load hoisting:
  load once into a register before a loop with no intervening call/observer,
  store back on exit.

---

## Investigation 2 — Object allocation benchmark

### Answer: **A) Allocations are removed.** (proven)

The object `x` is never used, so DCE eliminates the entire allocation. The loop
body bytecode contains **no `BuildObject`** and no `a/b/c` computation — only the
counter:

```
0008 LoadIntOne  r2 = 1
0009 AddInt      r3 = r1 op r2
0011 Move        r1 = r3
0013 Loop        -> +15
```

"220 ms" is an empty 10M loop. "122 heap allocs" is runtime/stdlib bootstrap,
not the loop. **The profiler is not wrong.**

### Proof it is DCE, not a profiler bug

Add a sink so `x` escapes (`acc += x.a + x.b + x.c`):

```
0019 BuildObject  r4 = {3 pairs}
0029 GetProperty  r4.prop[2] ...
```

`vn bench` then reports `heap allocs = 996,026` for 1,000,000 iterations — one
object per iteration (the rest are nursery/bootstrap). So allocations happen
exactly when the result is live. DCE / const-fold / escape behaviour is correct
here; the dead-store elimination is simply doing its job.

---

## Investigation 3 — Property access cost (100% IC hit)

### Measured cost

The Investigation 3 benchmark is top-level, so it runs in the interpreter.
Measured at 50M iterations × 3 properties = 150M `GetProperty`:

| Variant | `vn run` | ns / property access |
|---|---|---|
| Top-level (interpreter) | 7.43 s | **≈ 49.5 ns** |
| Wrapped in a function (JIT) | 0.64 s | **≈ 4.25 ns** |

(The spec's 16.94 s / 300M ≈ 56 ns matches the interpreter row.)

### Q1/Q2 — Where the time goes on an IC *hit*

Path: [`get_property.rs`](../crates/varn-vm/src/exec/dispatch/reg_ops/get_property.rs).
Even on a guaranteed hit, each access pays:

1. `closure.feedback.borrow()` — a **RefCell borrow** to read the `megamorphic`
   flag (line 25-31), every access.
2. `self.heap.get(obj.as_heap_idx())` to classify the object as `Object` —
   **then a second `heap.get`** inside the entry loop (lines 34 and 57). Double
   heap indirection.
3. Linear scan over `poly_slot.entries` (up to the polymorphic degree).
4. `unsafe &*o.0.as_ptr()` deref + `shape.id == entry.id` compare + bounds check
   `slot < values.len()`.
5. `record_ic_hit_getprop()` (cheap branch when profiling is off).

So a "100% IC hit" is still a RefCell borrow + two heap lookups + a loop + two
derefs. That is why it is ~50 ns in the interpreter and still ~4 ns under the
JIT (which calls `jit_get_property_ic_fast`, the same logic minus dispatch).

### The missed specialization

The object literal's shape is **statically known three lines above the loop**,
yet the compiler emits dynamic `GetProperty cs=N`, not `GetFixedField`
(direct-slot), which exists in the opcode set and JIT helpers
(`jit_get_fixed_field`). Disassembly of the wrapped variant:

```
0005 BuildObject  r4 = {3 pairs}
0028 GetProperty  r4.prop[0] cs=0   ; "a"   <- should be GetFixedField slot 0
```

### Q3 — Cost if fully specialized

`GetFixedField` with a statically known slot needs only: one heap deref + one
indexed load (no RefCell, no IC scan, no shape hashing). Realistically
**~1–2 ns** — i.e. another ~2–4× on top of the JIT row, ~25× over the current
interpreter row.

### Q4 — Comparison

* **V8 hidden classes / JSC:** monomorphic inline cache compiles to a shape
  guard + a fixed offset load, ~1–2 ns; megamorphic falls back to a dictionary.
* **LuaJIT:** trace compiler specializes table access in the trace; the guard is
  hoisted out of the loop entirely.
* **Wren / reference Lua VM:** hash lookup per access, comparable to Varn's
  *slow* path, but without the RefCell/double-lookup overhead.

Varn's IC stores the right metadata but the *hit* path is heavier than peers
because of (a) the per-access RefCell borrow, (b) the duplicated `heap.get`, and
(c) emitting dynamic `GetProperty` where the shape is statically known.

### Recommendation

* Emit `GetFixedField`/`SetFixedField` when the checker/optimizer knows the
  object's shape (object literals, typed class fields).
* Remove the per-access `feedback.borrow()` from the hit path (store the
  megamorphic bit in the IC slot itself, or behind the same pointer already
  dereferenced).
* Resolve the object to its `HeapObj` once and reuse it.

---

## Investigation 4 — Closure call vs direct call

### Answer: **A) Measurement artifact.**

`vn run` (no profiling):

| Benchmark | `vn run` |
|---|---|
| Closure (`counter()` 50M) | **4.11 s** |
| Direct (`add(sum,1)` 50M) | **4.32 s** |

They are equal within noise — **not** 4.92 s vs 8.09 s. The spec's 2× gap comes
from bench-mode profiling: the direct-call benchmark touches the global `sum`
each iteration (a profiled global access) and records a named function call;
`record_hotspot_*` does a `RefCell::borrow_mut` + hashmap update per event, and
the per-call/per-global recording weighs more heavily on the direct path.

### Q2/Q3/Q4

* Both call paths do equivalent work; both target functions are JIT-compiled.
  `vn bench` confirms `add` runs with `JIT runs 50,000,000  jit 100%`.
* The closure's `x` is an upvalue (`LoadUpvalue`/`StoreUpvalue`); the direct
  benchmark's `sum` is a global. In the interpreter that asymmetry would matter,
  but with both functions JIT'd the per-iteration difference collapses to noise.
* No bug; the reported inversion is purely the profiler's per-event cost
  distributed unevenly across the two shapes.

---

## Investigation 5 — Deep recursion / stack overflow

### Q1 — Which stack?

**Both, in layers — and the failure is on the Rust native stack.**

* Frames are VM-managed: `frames: Vec<CallFrame>`, and the interpreter loop
  `run_until_inner_raw`
  ([`dispatch/mod.rs:89`](../crates/varn-vm/src/exec/dispatch/mod.rs#L89)) is
  iterative (`'frame_loop`). Pure-interpreter recursion is bounded by an
  explicit guard: `if self.frames.len() >= 10000 { Err("call depth exceeded
  10000") }` — present on every interpreter call path
  ([`calls.rs:217,261,352`](../crates/varn-vm/src/exec/dispatch/reg_ops/calls.rs#L217)).
* **But `count` gets JIT-compiled** (it is a non-`<module>` function, so it is
  compiled on first call — Investigation 7). JIT'd code that calls back into the
  VM does so via `run_until_inner` recursively
  ([`ctx_jit_runtime.rs:822`](../crates/varn-vm/src/exec/ctx_jit_runtime.rs#L822)),
  which consumes **Rust native stack per Varn frame and does not check the 10000
  guard**.

### Evidence

```
count(5000)   -> 0          (ok)
count(9000)   -> 0          (ok)
count(11000)  -> 0          (ok — note: > 10000 guard, so the guard was NOT hit)
count(20000)  -> thread 'main' has overflowed its stack   (hard crash, exit 127)
count(1000000)-> thread 'main' has overflowed its stack
```

`count(11000)` succeeding past the logical 10000 limit proves the recursion is
not going through the guarded interpreter path; the hard crash between 11k and
20k (~default 8 MB Rust stack, ≈ 500–700 B per level) proves per-frame native
stack consumption via the JIT recursion path. **There is no tail-call
optimization** (`return count(n-1)` retains the frame).

### Q2 — Current architecture

Iterative interpreter over a heap `Vec<CallFrame>` (good), but the JIT↔VM
re-entry path is genuinely recursive in Rust, and the depth guard is inconsistent
(enforced in the interpreter, absent in the JIT re-entry). Result: a graceful
error in pure-interpreter mode, an unrecoverable abort once JIT'd.

### Q3 — Comparison

* **Lua / Wren:** VM frames on a heap-grown stack; deep recursion raises a
  catchable "stack overflow" error, never crashes the host.
* **JVM / CLR:** native frames, fixed `-Xss`, throws `StackOverflowError` (CLR
  can fail-fast, but it is deterministic and documented).
* **V8:** native frames with a stack-limit check that throws `RangeError:
  Maximum call stack size exceeded`.

### Q4 / Deliverable — recommended architecture

1. **Short term:** make the JIT re-entry path honour the same depth guard so the
   failure is the catchable VM error, never a host abort. Cheap, removes the
   crash.
2. **Medium term:** implement **tail-call optimization** for calls in tail
   position (`return f(...)`) — replace the current frame instead of pushing.
   `count` becomes O(1) depth.
3. **Long term:** keep the iterative VM-frame model and make the JIT call other
   Varn functions by **returning to the iterative trampoline** (push frame, let
   `run_until_inner` loop) instead of recursing in Rust. That removes native
   stack growth entirely; depth is then bounded only by heap and the explicit
   guard. Segmented stacks are unnecessary once the trampoline is in place.

---

## Investigation 6 — String concatenation O(n²) / allocation failure

### Root cause — two multiplicative O(n) costs per concat

`s = s + "a"` lowers to `StrConcat`, implemented as
[`strings.rs:17`](../crates/varn-vm/src/exec/strings.rs#L17):

```rust
pub fn str_concat(a, b, heap) -> VmValue {
    let sa = heap.str_repr(a);
    let sb = heap.str_repr(b);
    heap.alloc_str(format!("{}{}", sa, sb))   // copies the whole accumulator
}
```

and `alloc_str` [`heap.rs:463`](../crates/varn-vm/src/heap.rs#L463) **interns by
content**:

```rust
let rs: RuntimeString = Rc::from(s_ref);             // O(n) copy #2
if let Some(&packed) = self.string_interner.get(&rs) // O(n) HASH of full string
```

Per iteration on an n-char accumulator:

1. `format!` — copy n chars (O(n)).
2. `Rc::from` — copy n chars again (O(n)).
3. `string_interner.get` — hash n chars (O(n)).
4. Insert into `string_interner` — which **retains an `Rc<str>` to every
   intermediate string forever** (it is never pruned in the loop).

### Q3 — Is it O(n²)?

**Yes, three times over** (copy, copy, hash), plus O(n²) *retained* bytes because
the interner keeps every intermediate. Sum of lengths `1..n` = `n(n+1)/2` bytes
retained.

### Q1 — Why a ~483 KB allocation "fails"

It is not that 483 KB is too big; it is that by that point cumulative retained
memory is exhausted. At n = 100,000 the interner alone holds ≈ `5 × 10^9` bytes
(~5 GB); at n = 1,000,000 it would need ≈ `5 × 10^11` bytes (~500 GB). The
process hits the OS limit and the next allocation — which happens to be ~483 KB
— is the one that fails.

### Measured scaling

```
n = 100,000 concatenations  ->  94 seconds   (and ~GBs resident)
n = 1,000,000               ->  allocation failure (per spec)
```

94 s for 100k is far worse than even copy-only O(n²) would predict — the extra
cost is the full-string hashing on every `alloc_str` and the growing interner.

### Q2 — allocator / GC

GC cannot rescue this for two reasons:

* The interner holds live `Rc` references, so the intermediate strings are
  **reachable** — not garbage.
* `needs_gc()` / `needs_minor_gc()` are only checked at **frame push**
  ([`frame_ctrl.rs:25`](../crates/varn-vm/src/exec/frame_ctrl.rs#L25)). The
  concat loop makes no calls, so GC is never even consulted inside it.

### Q4 — Growth strategy

None. Every concat allocates an **exact-size** new string (+1 char). No `×1.5`,
no `×2`, no in-place buffer.

### Recommendation

* **Do not intern `alloc_str` results.** Reserve interning for known-constant
  strings (`alloc_str_interned` already exists at
  [`heap.rs:503`](../crates/varn-vm/src/heap.rs#L503)); dynamic concat output
  must never enter the content-keyed interner.
* Give `StrConcat` an in-place fast path: when the left operand is a uniquely
  referenced heap string (`Rc::get_mut`/refcount == 1), `push_str` into it with
  amortized doubling instead of allocating a fresh string.
* Consider a rope / `StringBuilder` type for accumulation patterns.
* Check `needs_gc` on loop back-edges, not only at frame push, so long
  call-free allocating loops can still collect.

---

## Investigation 7 — JIT architecture

### Q1 — Exact triggering rules

The JIT is **eager and per-function**, not tiered/threshold-based:

* `CallFrame::new`/`new_owned` call `compile_jit()` on **first frame creation**
  ([`frame.rs:96,117`](../crates/varn-vm/src/frame.rs#L96)).
* `compile_jit` [`frame.rs:121`](../crates/varn-vm/src/frame.rs#L121):
  * **`<module>` is hard-excluded**: `if name == "<module>" { jit_failed = true;
    return }`.
  * If `proto.jit_entry` is already set → reuse cached native code (`jit_cached`).
  * Else call `varn_jit::compile`; on success cache the entry in
    `proto.jit_entry` (a `Cell`), on failure set `jit_failed` and fall back to
    the interpreter permanently for that proto.
* At frame entry the loop runs native code when `jit_entry.is_some()` and bumps
  `JIT_STATS.jit_runs` once per invocation
  ([`dispatch/mod.rs:98`](../crates/varn-vm/src/exec/dispatch/mod.rs#L98)).

So **"JIT runs" = number of JIT'd-function frame entries**, not a compile count.

### Q2 — Why the ALU/global benchmark shows `JIT runs 0`

Because its hot loop lives in `<module>`, the one function explicitly excluded
from compilation. Confirmed: `vn bench inv1.vn` → `JIT runs 0, interpreted runs
1`. Wrapping the same loop in `fn main()` → `main … jit 100%, JIT runs 1`, and
the 21× speedup from Investigation 1.

### Q3 — Why the function-call benchmark shows `JIT runs 50,000,000`

`add` / `counter` are ordinary functions → compiled on first call, cached, then
entered 50M times. `vn bench inv4_direct.vn` → `add 50 000 000  jit 100%`.

### Q4 — Theoretical ceiling of the current design

The JIT is a **call-threaded Cranelift compiler**: `JitHelpers`
([`frame.rs:139`](../crates/varn-vm/src/frame.rs#L139)) maps essentially every
opcode — `jit_add`, `jit_lt`, `jit_get_property`, `jit_load_global_idx`, … — to a
**Rust helper call**. The generated code mostly removes the bytecode
dispatch/decode loop and register-array indirection, but arithmetic and property
access still call into boxed Rust helpers. There is:

* no cross-call inlining,
* no trace formation,
* no speculative type specialization (an `int` loop still calls `jit_add`, which
  re-checks NaN tags).

**Ceiling:** roughly 2–5× over the interpreter for arithmetic-heavy code (what we
measured: Inv1 ~21× includes the global→register effect; Inv3 property access
~11.6×), and more for call-heavy code (dispatch elimination). It will **not**
approach LuaJIT/TurboFan, which emit native integer ops and inline monomorphic
property loads. The single highest-leverage JIT improvement is **type-specialized
arithmetic** (emit a native `iadd` with a deopt guard instead of `call jit_add`)
and **`<module>` compilation** (so top-level hot loops are not stranded in the
interpreter).

---

## Investigation 8 — Benchmark methodology & suite

### Problems with the current ad-hoc benchmarks

1. **Profiling skews `vn bench`.** Hotspot/opcode counters
   (`RefCell::borrow_mut` + hashmap per global access / alloc / call) inflate
   bench numbers vs `vn run`: Inv1 global 4.43 s (bench) vs 3.15 s (run);
   Inv4 shows a spurious 2× closure-vs-direct gap that vanishes under `vn run`.
2. **They measure the wrong thing.** Inv1/Inv6 overflow 48-bit ints (results are
   garbage); Inv2 measures an empty loop because DCE removed the body; Inv1/Inv3
   conflate "global vs local" and "interpreter vs JIT" because top-level code is
   never JIT'd.
3. **No isolation.** A single benchmark mixes global access + arithmetic +
   loop + JIT-eligibility, so a regression cannot be attributed.

### Principles for the suite

* Run timing with **profiling off** (`vn run` semantics); collect counters in a
  separate profiled pass.
* Put every micro-benchmark **inside a function** so the JIT path is exercised,
  and add a separate explicit `<module>`-level variant to measure the
  interpreter.
* Keep accumulators **within 48-bit int range** (or use a checksum that is
  overflow-stable) so results validate correctness too.
* Make every result **observable** (print/return a checksum) to defeat DCE; also
  keep a paired "dead" variant to measure DCE/escape behaviour deliberately.
* Target **100 ms – 2 s** per iteration; pick N per benchmark accordingly.

### Recommended suite

| # | Name | Workload (inside a fn, checksum-returning) | Targets | Expected signal |
|---|---|---|---|---|
| 1 | `alu_int` | tight `int` loop, accumulate with modulo to stay < 2^47 | VM/JIT arithmetic, type spec. | should be JIT'd; baseline for `jit_add` cost |
| 2 | `alu_float` | same with `float` | float path, NaN-box | compare to int path |
| 3 | `global_vs_local` | identical loop, one module-global accumulator vs one local | global traffic, register promotion | quantifies Inv1 fix |
| 4 | `prop_mono` | read 3 fields of one fixed-shape object N times | IC hit, GetFixedField | quantifies Inv3 fix |
| 5 | `prop_poly` | same over 4 different shapes | polymorphic IC, megamorphic fallback | IC degradation curve |
| 6 | `array_sum` | index-sum over a 10k-element array | bounds checks, `GetIndex` | array fast path |
| 7 | `array_build` | push N elements, then sum | allocation + growth, nursery GC | alloc throughput |
| 8 | `obj_alloc_live` | allocate `{a,b,c}` per iter, checksum a field | escape (live), allocator, minor GC | real alloc rate (vs Inv2 dead) |
| 9 | `call_direct` | `add(a,b)` N times | call dispatch, JIT entry | per-call cost |
| 10 | `call_closure` | captured-counter closure N times | upvalue access | upvalue vs global |
| 11 | `call_method` | virtual method on an instance N times | `CallMethod`, vtable IC | method dispatch cost |
| 12 | `recursion` | `fib(30)` (non-tail) and a tail-recursive `sum` | call depth, future TCO | crash today; TCO target |
| 13 | `string_build` | accumulate N chars via concat **and** via a builder | StrConcat, interner, GC | exposes Inv6; validates fix |
| 14 | `string_ops` | slice/length/index over a long string | string heap path | |
| 15 | `map_set` | insert/lookup N keys in a Map | hashing, heap collections | |
| 16 | `json_roundtrip` | parse + stringify a medium document | real-world stdlib | end-to-end |
| 17 | `gc_churn` | allocate short-lived objects in a loop with calls | minor/major GC pacing | GC pause/throughput |

For each: record `execute` time (profiling off), and in a separate profiled run
record allocs, nursery vs major GC counts, IC hit/miss, and `JIT runs` /
`interpreted runs`. Store a committed baseline so CI can flag regressions.

---

## Fixes applied in this pass

All landed changes were validated against `tests/main.vn` (**728 passed, 0
failed**, no regression).

### Inv6 — string concatenation OOM → fixed

* `Heap::alloc_str_dynamic` ([`heap.rs`](../crates/varn-vm/src/heap.rs)) — a
  non-interning allocation for dynamically produced strings. `str_concat`,
  `to_string` and `str_slice` ([`strings.rs`](../crates/varn-vm/src/exec/strings.rs))
  now use it, so concatenation no longer hashes the full accumulator on every
  step nor retains every intermediate in the content interner.
* GC on loop back-edges
  ([`ops_control_calls.rs`](../crates/varn-vm/src/exec/dispatch/ops_control_calls.rs),
  `OpCode::Loop`) — a long call-free allocating loop now reaches a GC check and
  reclaims dead intermediates instead of growing unbounded.

Result: `s = s + "a"` × 100,000 went from **94 s (then OOM at 1M)** to **2.83 s
with the correct length and bounded memory** (~33×). The remaining cost is the
inherent O(n²) copy of an immutable string type; a builder/rope is the follow-up
for true O(n) (see below).

### Inv5 — recursion host crash → fixed (now catchable)

* Depth guard `jit_guard_call_depth` added to the unguarded JIT native-call
  helpers `jit_call`, `jit_prepare_call`, `jit_push_self_frame`
  ([`ctx_jit_values.rs`](../crates/varn-vm/src/exec/ctx_jit_values.rs)).
* Removed the unguarded `is_pure` self-call fast path in the JIT and added an
  inline call-depth check that diverts to the guarded helper at the limit
  ([`codegen/calls.rs`](../crates/varn-jit/src/codegen/calls.rs), `emit_call_self`).

Result: `count(20000)` / `count(1_000_000)` now raise a catchable
`runtime error: stack overflow: call depth exceeded 10000` (exit 1) instead of
aborting the host (`thread 'main' has overflowed its stack`, exit 127).
`fib(30)` still returns `832040` in ~77 ms — no recursion-perf regression.

### Inv1 + Inv7 — top-level code never JIT'd → fixed

* Removed the `<module>` exclusion in `compile_jit`
  ([`frame.rs`](../crates/varn-vm/src/frame.rs)). Top-level code is now JIT
  compiled like any function (compilation failure still falls back to the
  interpreter, so it is safe).

Result: the Investigation 1 benchmark (`sum += i` at module scope) went from
**3.15 s (interpreter) to 0.229 s (`JIT runs 1`)** — **~13.7×** — with
`tests/main.vn` still 728/728 (the suite imports 30+ relative modules plus
`std:task` / `std:reflect`, so module execution, exports and std-loading under
JIT are exercised). This achieves Inv1's headline win *without* the risky
global→register binding change (semantics, captures and exports are unchanged).
A residual global→register promotion would add only marginal gains over this and
is deferred.

### Still open (larger features — deliberately not rushed)

These each touch broad surface area and carry regression risk; recommended as
focused, separately-validated efforts rather than a single batch:

* **48-bit int → boxed `i64`** (chosen direction). The bug is `from_int` masking
  to 48 bits even when the i64 result is exact. A correct fix needs a boxed
  wide-int representation threaded through every arithmetic/comparison/`to_string`
  path and the JIT helpers, plus type-system alignment — `add`/`sub`/`mul`
  currently have no `BigInt` operand handling and fall through to `f64`.
* **Inv5 (proper) TCO** — tail self-recursion as O(1) stack (and faster). The
  crash is already fixed (catchable guard); this would lift the depth limit for
  tail calls.
* **Inv6 (proper) builder/rope** — a mutable string type so accumulation is true
  O(n). The OOM is already fixed; this addresses the residual O(n²) time
  (1,000,000 concats currently complete in ~8m54s instead of failing).
* **Inv3** `GetFixedField` for statically known shapes + dropping the per-access
  `feedback.borrow()` — optimizer shape tracking + IC hit-path change.
* **Inv7** type-specialized JIT arithmetic (native `iadd` + deopt guard) to raise
  the call-threaded ceiling.

## Summary of actionable fixes (by leverage)

1. **Compile `<module>` / promote module-private top-level vars to registers** —
   unblocks the most common "it's slow at top level" surprise; measured 21×
   on the Inv1 shape. (Inv1, Inv7)
2. **Do not content-intern dynamic strings + in-place `StrConcat` growth** —
   turns an O(n²)/OOM into O(n). (Inv6)
3. **Honour the call-depth guard on the JIT re-entry path + add TCO** — turns a
   host crash into a catchable error and makes deep/tail recursion work. (Inv5)
4. **Emit `GetFixedField` for statically known shapes; drop the per-access
   RefCell borrow** — ~2–4× on property access. (Inv3)
5. **Type-specialized arithmetic in the JIT** (native `iadd` + deopt guard) —
   raises the JIT ceiling. (Inv7)
6. **Decide 48-bit int semantics** (trap on overflow or widen) — correctness.
7. **Benchmark with profiling off; isolate variables; defeat DCE; stay in int
   range** — so future numbers mean something. (Inv8)
