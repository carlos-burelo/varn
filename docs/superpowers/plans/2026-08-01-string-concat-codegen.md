# String Concat Codegen — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use `superpowers:executing-plans` (or `superpowers:subagent-driven-development`) to work this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Cut the per-iteration cost of string building in compiled code, where Varn is ~4x behind node. The three levers are, in order of measured leverage: stop spilling registers that cannot be GC roots, stop paying a `malloc` for every short heap string, and stop calling a helper at all when the result fits inline.

**Architecture:** All three are backend changes. `varn-jit/src/clif/alloc.rs` decides what a safepoint flushes; `varn-vm/src/heap.rs` decides how a dynamic string is stored; `varn-jit/src/clif/alloc.rs` also owns the `StrConcat` lowering that will grow an inline fast path. Nothing in the checker, parser or SSA changes.

**Tech Stack:** Rust, cranelift-codegen (the existing `clif` lowering), `varn-vm`'s NaN-boxed `VmValue` and generational heap.

---

## 1. Why — measured, not assumed

Three string micro-optimizations were tried on 2026-08-01 and **all three measured as noise**. They are recorded here so nobody re-tries them:

| Attempt | Paired result |
|---|---|
| Skip the content-interner probe on dynamic strings | 38 ms vs 39 ms |
| Stack-first `StrBuf` instead of scratch `String` in `BuildStr`/`jit_build_str` | 57 ms vs 55 ms |
| Emit typed `StrConcat` instead of generic `Add` (skips `arith::add` dispatch) | 39 ms vs 38 ms |

(Those changes were kept for other reasons — DRY, and the `<backend_principle>` — but none of them is a speedup. See commits `67ae1df` and `75fe5da`.)

**Where the time actually is**, by decomposition on a 400k-iteration loop (`vn run`, best of several, this host):

| Loop body | ms / 400k | ns / iter |
|---|---|---|
| `j.push(i)` — no string at all | 6 | 15 |
| `j.push("x")` — constant string | 7 | 18 |
| `("g" + (i % 1000)).length` — concat, result ≤5 bytes so **no allocation** | 24–29 | ~65 |
| `("gcgc_" + (i % 1000)).length` — concat, result 6–8 bytes so heap-allocated | 27–37 | ~80 |
| `j.push("gc_" + i)` — the real benchmark | 35–40 | ~95 |

The decisive row is the third: **~50 ns of concat cost with zero allocation.** So the cost is neither the allocator nor the type dispatch. Reading the generated machine code (`vn debug -p clif:asm --fn junk`) shows what it is — around every `call` to the concat helper:

```asm
mov [r11+rsi*8],   r10      ; flush
mov [r11+rsi*8+8], r9
mov [r11+rsi*8+10h], r8
mov [rsp+20h], rcx          ; caller-saved spills
...
call rax
mov r8,[rdi+8]              ; recompute the frame base
mov r9,[r8+rsi*8]           ; reload
mov r9,[r8+rsi*8+8]         ; ← note: same register, three times
mov r9,[r8+rsi*8+10h]
```

Roughly ten memory operations per concat, plus a frame-base recompute, plus the call. For reference, node does the whole iteration in ~21 ns.

**Target:** the `alloc` row of the external comparison, whose string half is currently ~50 ms against node's ~13 ms and bun's ~24 ms. Beating **bun** is the realistic goal of this plan; matching node on string churn is not (see §7).

---

## 2. Global constraints

- **Validation gold** (run after every behaviour-affecting task; all must print `PASSED: 899 / FAILED: 0`):
  - `./target/release/vn.exe run ./tests/main.vn`
  - `VARN_NO_JIT=1 ./target/release/vn.exe run ./tests/main.vn`
  - `VARN_JIT_TIER=999999 ./target/release/vn.exe run ./tests/main.vn`
  - The JIT and no-JIT runs must be **byte-identical**, not merely both passing.
- Purge the compile cache before validating — it is keyed by source hash only and will hide codegen changes:
  `Remove-Item -Recurse -Force $env:LOCALAPPDATA\varn\cache -ErrorAction SilentlyContinue`
- **Measurement protocol (non-negotiable).** This host inverted a 40% effect during the OSR work, and during THIS investigation a batch read as a 50% regression that was pure thermal drift. Always:
  - compare **two binaries alternately in one loop**, never sequentially;
  - take the **median of ≥7 alternating rounds**;
  - keep a control workload in the mix that the change cannot affect (e.g. the object-allocation phase for a string change); if the control moves, discard the batch.
- Release builds take **3–6 min** on this machine. A slow build is not a hang.
- `int` is i48 wrapping; object identity is the `Rc` address; instruction shapes come only from `varn_types::bytecode::decode`. Do not touch these.
- File size governance: no file over 1000 lines. `crates/varn-jit/src/clif/alloc.rs` is **1606 lines** and is already over — any task that grows it must extract a module instead.
- Work on `main`. Commit per task.

---

## 3. The correctness hazard — read this before Task 1

Task 1 narrows what a GC safepoint spills. Get it wrong and the collector misses a root: an object is freed or moved while a register still refers to it, and the failure is a use-after-free that appears far from the cause and only under allocation pressure.

The rule that makes it safe is already written down in `alloc.rs:279-287`, on `live_boxed` itself:

> a register no reader will touch again does not need to be rooted, and leaving its home slot alone keeps the older (still valid) VmValue the frame's null-fill guarantees, which the collector already scans and tolerates.

So the collector already tolerates a stale-but-valid `VmValue` in a home slot. Task 1 extends that to a second category with the same property: a register the kind flow proves is an **unboxed** `K::Int` or `K::Bool` holds a raw machine integer, not a heap handle. It is not a root, it cannot be rewritten by a move, and its home slot keeps whatever older valid `VmValue` was there.

**What must remain flushed:** anything the flow types `K::Boxed`, `K::Global(..)`, `K::Unset`, or `K::Float`-as-boxed — every kind that could carry a heap index. When in doubt, flush.

`K::Float` registers are already excluded on a different ground (they live in `F64` Variables the collector never looks at); do not disturb that.

---

## 4. Code anchors (verified 2026-08-01)

| What | Where |
|---|---|
| `live_boxed` — the flush set, **ignores `_state` today** | `crates/varn-jit/src/clif/alloc.rs:288` |
| `flush_boxed` / `reload_boxed` | `crates/varn-jit/src/clif/alloc.rs:302` / `:320` |
| `store_home` (boxes via `box_or_pass`) | `crates/varn-jit/src/clif/alloc.rs:183` |
| `frame_base_addr` (recomputed per flush) | `crates/varn-jit/src/clif/alloc.rs:153` |
| `emit_str_concat` — flush / call / reload | `crates/varn-jit/src/clif/alloc.rs:627` |
| `OpCode::StrConcat` CLIF dispatch arm | `crates/varn-jit/src/clif/body.rs:729` |
| `emit_backedge_safepoint` — the pattern for "flush only in the taken arm" | `crates/varn-jit/src/clif/alloc.rs:238` |
| Kind lattice `K` and `is_boxed_kind` | `crates/varn-jit/src/clif/kinds.rs` |
| `jit_str_concat` helper → `strings::str_concat` | `crates/varn-vm/src/exec/ctx_jit_runtime.rs:233` |
| `str_concat` (StrBuf + Ext accumulation) | `crates/varn-vm/src/exec/strings.rs:58` |
| `Heap::alloc_str_dynamic` (`Rc::from` = the malloc) | `crates/varn-vm/src/heap.rs:904` |
| `HeapStr` variants (`Shared` / `Slice` / `Ext`) | `crates/varn-vm/src/heap.rs` (search `enum HeapStr`) |
| `HeapObj` enum (sets the slot stride) | `crates/varn-vm/src/heap.rs:224` |
| `VmValue::try_from_sso` — ≤5 bytes, ASCII only | `crates/varn-types/src/vm_value.rs:129` |
| `QNAN` / `TAG_SSO` constants | `crates/varn-types/src/vm_value.rs:20` / `:30` |
| Binary → opcode specialization (emits `StrConcat`) | `crates/varn-opt/src/ssa/emit.rs:907` |

SSO encoding, for Task 3:

```rust
// varn-types/src/vm_value.rs:129
let mut v: u64 = (b.len() as u64) << 45;
for (i, &byte) in b.iter().enumerate() {
    v |= (byte as u64) << (37 - i as u32 * 8);
}
VmValue(QNAN | TAG_SSO | v)
```

---

## Task 1: Narrow the safepoint flush set by kind state

The highest-leverage change in this plan, and the only one that is **not** string-specific: it makes every allocating opcode in compiled code cheaper (`StrConcat`, `BuildStr`, `BuildArray`, `ArrayPush`, property stores, calls, the back-edge safepoint).

**Files:**
- Modify: `crates/varn-jit/src/clif/alloc.rs:288` (`live_boxed`)
- Test: `tests/65-safepoint-roots.vn` (new)

**Interfaces:**
- Produces: `live_boxed` keeps its signature `fn live_boxed(actx: &AllocCtx, state: &[K]) -> Vec<usize>` — the `_state` parameter loses its underscore and starts being read. Every existing caller already passes the flow state, so no call site changes.

- [ ] **Step 1: Write the failing test first.** This test must fail if the narrowed flush set drops a real root. It needs live heap references held across an allocating call, plus enough allocation to force several minor collections, so a missed root is actually collected.

Create `tests/65-safepoint-roots.vn`:

```vn
// A GC safepoint spills live registers to their ctx.stack home slots so the
// collector can root and rewrite them. `live_boxed` decides that set. This
// file pins the set being narrow enough to be fast and wide enough to be
// correct: every assert here holds a heap reference ACROSS an allocating
// call, under enough pressure to force real collections.
//
// A missed root shows up here as a wrong value or a crash, not as a warning.

// Heap refs (strings) live across a concat that allocates on every iteration.
function spRefsSurviveConcat(n: int): int {
  const keep = "anchor-string-long-enough-to-be-heap"
  let last = ""
  let hits = 0
  for (let i = 0; i < n; i = i + 1) {
    last = "item-" + i
    if (keep === "anchor-string-long-enough-to-be-heap") { hits = hits + 1 }
  }
  if (last === "item-" + (n - 1)) { hits = hits + 1 }
  return hits
}
assert("safepoint: heap refs survive allocating concats", spRefsSurviveConcat(50000) === 50001)

// Ints interleaved with heap refs: the ints are what Task 1 stops flushing,
// so their values must still be exact after collections move things around.
function spIntsExactAcrossGc(n: int): int {
  let acc = 0
  let s = ""
  for (let i = 0; i < n; i = i + 1) {
    s = "x" + i
    acc = acc + (i % 7)
  }
  if (s === "") { return -1 }
  return acc
}
assert("safepoint: ints exact across collections", spIntsExactAcrossGc(70000) === 209997)

// An array of heap objects built under pressure: the array reference itself
// is live across every push, and every element must survive.
class SpBox { v: int
  constructor(v: int) { this.v = v } }
function spArraySurvives(n: int): int {
  const arr = []
  for (let i = 0; i < n; i = i + 1) { arr.push(new SpBox(i)); arr.push("s" + i) }
  return arr.length + arr[0].v + arr[(n - 1) * 2].v
}
assert("safepoint: array and elements survive", spArraySurvives(20000) === 60000 + 0 + 19999)

// Nested: an outer heap ref live across an inner allocating loop.
function spNested(outer: int, inner: int): int {
  let total = 0
  const tag = "outer-anchor-string"
  for (let i = 0; i < outer; i = i + 1) {
    let local = ""
    for (let j = 0; j < inner; j = j + 1) { local = tag + j }
    if (local === tag + (inner - 1)) { total = total + 1 }
  }
  return total
}
assert("safepoint: outer ref survives inner allocating loop", spNested(500, 100) === 500)

print("[PASSED] 65. Safepoint roots ")
```

Add `import "./65-safepoint-roots.vn"` to `tests/main.vn` and bump the module-count line to 65.

- [ ] **Step 2: Run it and confirm it PASSES on today's code.** This test is a regression net, not a red test — the current behaviour is correct, just slow. Run `./target/release/vn.exe run ./tests/65-safepoint-roots.vn` under both `vn run` and `VARN_NO_JIT=1`. Expected: `[PASSED] 65. Safepoint roots`. If it fails now, stop — something else is wrong and this plan's premise is void.

- [ ] **Step 3: Commit the test on its own**, so the narrowing lands against a net that is already in the tree.

```bash
git add tests/65-safepoint-roots.vn tests/main.vn
git commit -m "test: pin GC safepoint root coverage before narrowing the flush set"
```

- [ ] **Step 4: Narrow `live_boxed`.** Replace `crates/varn-jit/src/clif/alloc.rs:288`:

```rust
pub(super) fn live_boxed(actx: &AllocCtx, state: &[K]) -> Vec<usize> {
    let ip = actx.cur_ip.get();
    let regs: Vec<usize> = (0..actx.nregs)
        .filter(|&r| !meta_is_float(actx.register_meta, r))
        // A register the flow proved UNBOXED holds a raw machine integer, not
        // a heap handle: the collector has nothing to root and nothing to
        // rewrite, and its home slot keeps whatever older valid `VmValue` was
        // there — the same tolerance the liveness filter below already relies
        // on. `is_boxed_kind` is the authority; anything it does not vouch for
        // stays in the set.
        .filter(|&r| state.get(r).copied().map_or(true, is_boxed_kind_or_unknown))
        .filter(|&r| actx.live.is_live_after(ip, r))
        .collect();
    if let Some(rec) = &actx.safepoints {
        rec.borrow_mut().push((ip, regs.clone()));
    }
    regs
}

/// Conservative complement of "provably not a root": `true` for every kind
/// that could carry a heap index, including the ones the flow has not decided.
fn is_boxed_kind_or_unknown(k: K) -> bool {
    !matches!(k, K::Int | K::Bool)
}
```

Note the import: `is_boxed_kind` already exists in `super::kinds` but means something narrower; this helper is deliberately its conservative complement and lives next to `live_boxed`. Do NOT reuse `is_boxed_kind` directly — it returns `false` for kinds like `K::Unset` that must still be flushed.

- [ ] **Step 5: `cargo check --workspace`.** Expected: clean, and no call-site changes (every caller already passes `state`).

- [ ] **Step 6: Validation gold**, all three configurations, byte-identical. Plus `tests/65-safepoint-roots.vn` under both tiers.

- [ ] **Step 7: Verify the flush set actually shrank.** `vn debug -p roots --fn junk <file>` reports the root set per safepoint against Cranelift's own liveness. Compare before/after on the `junk` benchmark from §6: the int loop counter and bound must disappear from the flushed set while the array and string registers remain. If nothing shrank, the kind flow is not proving `K::Int` where you expected and the rest of this task is a no-op — investigate before measuring.

- [ ] **Step 8: Measure**, per the §2 protocol, on the §6 workloads. Record the medians in the commit message. Use the object-allocation phase as the control: this change should move it too (it also flushes), so if you want a true control use `bench_fib` or another non-allocating benchmark.

- [ ] **Step 9: Commit.**

---

## Task 2: Inline-capacity `HeapStr` for short dynamic strings

Every dynamic string over the 5-byte SSO limit pays a `malloc` and a copy in `Rc::from(s_ref)`, then a second store into the heap slot. The benchmark's strings are 6–9 bytes — just over the line. Giving `HeapStr` an inline variant removes the malloc for exactly that band.

**Files:**
- Modify: `crates/varn-vm/src/heap.rs` (the `HeapStr` enum, `alloc_str_dynamic`, and every `match` over `HeapStr`)
- Test: `tests/66-inline-strings.vn` (new)

**Interfaces:**
- Produces: `HeapStr::Inline { len: u8, bytes: [u8; N] }`, with `HeapStr::as_str` / `as_ref` / `len` / `is_ascii_cached` handling it like any other variant. No caller outside `heap.rs` learns it exists.

- [ ] **Step 1: Measure the available headroom FIRST.** The inline capacity must not grow `HeapObj`, because that enum's size is the heap slot stride — widening it makes every allocation of every type more expensive and would trade a string win for a global loss.

Add a temporary test in `crates/varn-vm/src/heap.rs`:

```rust
#[test]
fn heap_obj_size_headroom() {
    // Printed so the plan's Task 2 can pick an inline capacity that does not
    // widen the slot stride.
    eprintln!("size_of::<HeapObj>() = {}", std::mem::size_of::<HeapObj>());
    eprintln!("size_of::<HeapStr>() = {}", std::mem::size_of::<HeapStr>());
}
```

Run `cargo test -p varn-vm heap_obj_size_headroom -- --nocapture`. Pick `N` as the largest value where `size_of::<HeapObj>()` is unchanged. **If the headroom is under 8 bytes, stop and skip to Task 3** — an inline variant that only covers 6–7 byte strings is not worth the churn across every `HeapStr` match arm. Record the measured numbers in the commit message either way.

- [ ] **Step 2: Write the failing test.** Create `tests/66-inline-strings.vn`, covering both sides of the new boundary and the operations that read a string's bytes:

```vn
// Dynamic strings up to the SSO limit live inside the VmValue; longer ones
// become heap strings. Task 2 adds a third representation between them —
// short heap strings stored inline in the heap object rather than behind an
// Rc. It must be invisible: same contents, same comparisons, same slicing,
// same length, whichever representation a given string happens to take.

function isStr(n: int): string { return "ab" + n }

// Straddle the boundaries: 3 bytes (SSO), 8 bytes (inline candidate),
// 40 bytes (definitely out of line).
assert("inline: sso-sized",    isStr(1) === "ab1")
assert("inline: inline-sized", isStr(123456) === "ab123456")
assert("inline: long", ("ab" + "cdefghijklmnopqrstuvwxyz0123456789ABCD") === "abcdefghijklmnopqrstuvwxyz0123456789ABCD")

// Length must agree across representations.
function lenOf(s: string): int { const d = s; return d.length }
assert("inline: length sso",    lenOf(isStr(1)) === 3)
assert("inline: length inline", lenOf(isStr(123456)) === 8)

// Equality between two independently built strings of the same contents.
assert("inline: equality of equal contents", isStr(4242) === ("ab" + 4242))

// Concatenating an inline string as the LEFT operand must still work — this
// is the path that reads the bytes back out.
assert("inline: inline string concatenated again", (isStr(123456) + "!") === "ab123456!")

// Slicing reads the bytes through a different route than concat.
function sliceOf(n: int): string { const d = isStr(n); return d.slice(2) }
assert("inline: slice of an inline string", sliceOf(123456) === "123456")

// Used as a map key, which canonicalizes through the interner.
function mapRoundTrip(n: int): int {
  const m = new Map()
  m.set(isStr(n), n)
  return m.get("ab" + n)
}
assert("inline: works as a map key", mapRoundTrip(123456) === 123456)

// Survives a collection: built, held, then heavy allocation, then read.
function inlineSurvivesGc(n: int): bool {
  const held = isStr(999999)
  let junk = ""
  for (let i = 0; i < n; i = i + 1) { junk = "g" + i }
  if (junk === "") { return false }
  return held === "ab999999"
}
assert("inline: survives a collection", inlineSurvivesGc(60000) === true)

print("[PASSED] 66. Inline strings ")
```

Add `import "./66-inline-strings.vn"` to `tests/main.vn` and bump the module-count line.

- [ ] **Step 3: Run it on today's code.** Expected: PASSES (these are all existing semantics). It is the net for Step 4, not a red test.

- [ ] **Step 4: Add the variant.** In `crates/varn-vm/src/heap.rs`, first declare the capacity next to the `HeapStr` definition, with the number Step 1 measured:

```rust
/// Bytes a short dynamic string keeps inside its heap object instead of
/// behind an `Rc`. Chosen in Task 2 Step 1 as the largest value that leaves
/// `size_of::<HeapObj>()` unchanged — that size is the slot stride shared by
/// every heap type, so widening it to help strings would tax every other
/// allocation. Pinned by `heap_obj_slot_stride_is_unchanged`.
const INLINE_STR_CAP: usize = /* the value measured in Step 1 */;
```

Then add to `enum HeapStr`:

```rust
    /// A short dynamic string stored IN the heap object, with no `Rc` behind
    /// it. `alloc_str_dynamic`'s `Rc::from` is a malloc plus a copy for every
    /// string over the 5-byte SSO limit, and the common
    /// `"prefix" + <small int>` result lands just past it. Capacity is chosen
    /// so `size_of::<HeapObj>()` does not change — the slot stride is shared
    /// with every other heap type.
    Inline { len: u8, bytes: [u8; INLINE_STR_CAP] },
```

Then extend every `match` over `HeapStr` in `heap.rs`. Compile errors enumerate them exhaustively — work through the list rather than searching by hand, and do NOT add a `_ =>` arm anywhere, since that is what would silently miss one.

- [ ] **Step 5: Route allocation through it.** In `alloc_str_dynamic` (`heap.rs:904`), after the SSO attempt and before `Rc::from`:

```rust
        if s_ref.len() <= INLINE_STR_CAP {
            let mut bytes = [0u8; INLINE_STR_CAP];
            bytes[..s_ref.len()].copy_from_slice(s_ref.as_bytes());
            let hs = HeapStr::Inline { len: s_ref.len() as u8, bytes };
            return self.alloc_str_view(hs);
        }
```

`alloc_str_view` already takes a built `HeapStr` and handles the nursery/old-gen split, so nothing else changes.

- [ ] **Step 6: `cargo test --workspace`**, then validation gold (all three configurations, byte-identical), then `tests/66-inline-strings.vn` under both tiers.

- [ ] **Step 7: Confirm the slot stride did not change.** Re-run the size test from Step 1. If `size_of::<HeapObj>()` grew, the capacity is too large — reduce `INLINE_STR_CAP` and repeat. This is not optional: a wider stride is a global regression that this benchmark would not show.

- [ ] **Step 8: Measure** per §2, and **also** measure a non-string allocating benchmark (`benchmarks/bench_gc_alloc.vn`'s object phase, or `bench_dto.vn`) to prove the wider `HeapStr` did not cost anything elsewhere.

- [ ] **Step 9: Commit**, with both the string numbers and the no-regression numbers in the message. Delete the temporary size test, or keep it as a real assertion:

```rust
#[test]
fn heap_obj_slot_stride_is_unchanged() {
    // The inline string capacity was chosen against this number; if the enum
    // grows, every heap allocation of every type pays for it. Substitute the
    // value Step 1 printed BEFORE the variant was added.
    assert_eq!(std::mem::size_of::<HeapObj>(), /* pre-Task-2 size_of::<HeapObj>() */);
}
```

Both blanks above are deliberate: they are measurements this task takes, not decisions the plan can make in advance. Every other value in this plan is exact.

---

## Task 3: Inline SSO fast path for `StrConcat`

When the result fits in a `VmValue` there is no allocation, so there is no reason to leave compiled code at all — no call, no flush, no reload. This is the `emit_backedge_safepoint` pattern: test inline, do the cheap thing on the common arm, and keep the existing helper on the other.

**Files:**
- Create: `crates/varn-jit/src/clif/strconcat.rs` (`alloc.rs` is 1606 lines and must not grow)
- Modify: `crates/varn-jit/src/clif/mod.rs` (register the module), `crates/varn-jit/src/clif/alloc.rs:627` (`emit_str_concat` delegates)
- Test: `tests/64-str-concat-typed.vn` already exists and covers the semantics; extend it rather than adding a file.

**Interfaces:**
- Consumes: `AllocCtx`, `flush_boxed` / `reload_boxed` / `live_boxed` from `super::alloc` (all `pub(super)`, visible to a sibling module).
- Produces: `pub(super) fn emit_str_concat(b: &mut FunctionBuilder, actx: &AllocCtx, state: &[K], code: &[u16], ip: usize)` — same signature `alloc::emit_str_concat` has today, so `body.rs:729` does not change beyond the module path.

- [ ] **Step 1: Extend the existing test with the boundary cases the fast path introduces.** Append to `tests/64-str-concat-typed.vn`, before the final `print`:

```vn
// The Cranelift fast path builds a small-string-optimized result inline and
// falls back to the helper otherwise, so every boundary of "fits inline"
// needs pinning: exactly at the limit, one past it, and non-ASCII (which the
// SSO encoding refuses regardless of length).
assert("strconcat: result exactly at the sso limit", ("ab" + 123) === "ab123")
assert("strconcat: result one past the sso limit", ("ab" + 1234) === "ab1234")
assert("strconcat: empty + empty", ("" + "") === "")
assert("strconcat: non-ascii refuses the inline path", ("é" + 1) === "é1")
assert("strconcat: non-ascii on the right", ("a" + "ñ") === "añ")
assert("strconcat: multibyte stays correct", ("日本" + 5) === "日本5")
```

- [ ] **Step 2: Run the extended file** under both tiers. Expected: PASSES today (all existing semantics).

- [ ] **Step 3: Commit the test extension on its own.**

- [ ] **Step 4: Move `emit_str_concat` into the new module, unchanged.** Create `crates/varn-jit/src/clif/strconcat.rs` with the current body from `alloc.rs:627-650` verbatim, add `pub(crate) mod strconcat;` to `clif/mod.rs`, delete the old function, and point `body.rs:729` at `strconcat::emit_str_concat`. Run the validation gold. **This step must be a pure move with no behaviour change** — verify by checking that `vn bench ./tests/main.vn -v` reports the same total code size (`compilar ... KB`) as before.

- [ ] **Step 5: Commit the move separately** from the behaviour change, so a bisect can tell them apart.

- [ ] **Step 6: Add the fast path.** The shape, inside `emit_str_concat`:

```rust
// Both operands must be SSO for the inline path: an SSO value carries its
// bytes and length in the value itself, so the result can be assembled with
// shifts. Anything heap-tagged needs a dereference the slow path already
// does properly.
//
// Guard chain, all inline:
//   a.is_sso() && b.is_sso() && (a.sso_len() + b.sso_len()) <= 5
// SSO already refuses non-ASCII at construction, so a value that IS sso is
// ASCII by induction and no byte test is needed here.
//
// Result assembly mirrors `VmValue::try_from_sso`:
//   v = ((la + lb) << 45)
//     | (a_bytes_shifted)
//     | (b_bytes_shifted >> (la * 8))
//   out = QNAN | TAG_SSO | v
// where the byte fields already sit at bits 37, 29, 21, 13, 5 — so `b`'s
// payload only needs a right shift by `la * 8` to land after `a`'s.
```

Emit it as a `brif` into a fast block and a slow block joining at a merge block with one `I64` block param, exactly as `emit_backedge_safepoint` does. **The flush/reload must live in the SLOW arm only** — that is the entire point of the task; a flush before the branch throws the win away.

- [ ] **Step 7: `cargo check --workspace`**, then validation gold (all three configurations, byte-identical), plus the extended `tests/64-str-concat-typed.vn` under both tiers.

- [ ] **Step 8: Verify the fast path is actually taken.** Add a temporary `eprintln!` in `jit_str_concat` (the helper) and run a loop whose results are all ≤5 bytes (`"a" + (i % 100)`); the helper must print far less often than the iteration count — ideally never. Remove the print before committing. Without this check a bug in the guard chain shows up only as "no speedup", which is indistinguishable from the last three attempts.

- [ ] **Step 9: Measure** per §2 on a short-result workload (`("a" + (i % 100)).length`, 400k iterations) AND on the §6 benchmark, which this task should NOT change (its results are 6–9 bytes and take the slow arm). Report both.

- [ ] **Step 10: Commit.**

---

## Task 4: Re-measure the external comparison and document

- [ ] **Step 1: Run the three-way comparison** with the final binary:

```bash
node   <scratch>/gc_split.js
bun    <scratch>/gc_split.js
./target/release/vn.exe run <scratch>/gc_fn.vn
```

The JS and Varn sources for this comparison are in §6. Report the string phase and the object phase separately — the object phase should already read ~0–1 ms from the escape-analysis work and must not have regressed.

- [ ] **Step 2: Update `docs/VM_ARCHITECTURE.md`.** The §7 JIT section gained a "Tiering y OSR" subsection in the OSR work; add a sibling "Strings en código compilado" covering: the three representations (SSO / inline / `Rc`), which one `alloc_str_dynamic` picks and why, the safepoint narrowing rule from Task 1 with its soundness argument, and the `StrConcat` fast-path guard chain.

- [ ] **Step 3: Record the negative results.** In the same doc section, state plainly that the interner probe, the scratch-`String` removal, and the typed-opcode specialization were each measured at zero. This repo's convention is that a perf conclusion carries its evidence; a *disproved* hypothesis is worth as much as a confirmed one and costs a day to re-derive.

- [ ] **Step 4: Commit.**

---

## 5. Definition of done

- [ ] `tests/main.vn` passes under `vn run`, `VARN_NO_JIT=1`, and `VARN_JIT_TIER=999999`, with byte-identical output between the JIT and no-JIT runs.
- [ ] `cargo test --workspace` green.
- [ ] `tests/65-safepoint-roots.vn` and `tests/66-inline-strings.vn` pass under both tiers.
- [ ] `size_of::<HeapObj>()` is unchanged from its pre-Task-2 value, asserted by a test.
- [ ] The string phase of the gc benchmark improves, measured by the §2 protocol, with the numbers in the commit messages.
- [ ] A non-string allocating benchmark shows no regression.
- [ ] No file over 1000 lines among those this plan touches; `clif/alloc.rs` must not have grown.
- [ ] `docs/VM_ARCHITECTURE.md` documents the string representations, the safepoint rule, and the three measured-at-zero attempts.

## 6. Reproducing the measurements

Write these outside the repo.

```js
// gc_split.js — the external comparison, phases separated
class GcVtA{ constructor(x){ this.x=x; } }
class GcVtB{ constructor(y){ this.y=y; } }
function junk(){ const j=[]; for(let i=0;i<400000;i++) j.push("gc_"+i); return j.length; }
function alloc(){ let aa=0,bb=0; for(let i=0;i<100000;i++){ const a=new GcVtA(i); aa+=a.x; const b=new GcVtB(i); bb+=b.y; } return aa+bb; }
let bj=Infinity, ba=Infinity, c1, c2;
for(let r=0;r<12;r++){
  let t=performance.now(); c1=junk();  const mj=performance.now()-t;
  t=performance.now();     c2=alloc(); const ma=performance.now()-t;
  if(r>=2){ if(mj<bj)bj=mj; if(ma<ba)ba=ma; }
}
console.log("junk_ms="+bj.toFixed(1)+"  alloc_ms="+ba.toFixed(1)+"  chk="+c1+"/"+c2);
```

```vn
// gc_fn.vn — the same workload, in functions so it is JIT-compiled
import { now } from "std:time"

class GcVtA { x: int
    constructor(x: int) { this.x = x } }
class GcVtB { y: int
    constructor(y: int) { this.y = y } }

function junk(): int {
    let j = []
    for (let i = 0; i < 400000; i = i + 1) { j.push("gc_" + i) }
    return j.length
}
function alloc(): int {
    let aa = 0
    let bb = 0
    for (let i = 0; i < 100000; i = i + 1) {
        let a = new GcVtA(i)
        aa = aa + a.x
        let b = new GcVtB(i)
        bb = bb + b.y
    }
    return aa + bb
}

let t0 = now()
let c1 = junk()
let t1 = now()
let c2 = alloc()
let t2 = now()
print("junk_ms=" + (t1 - t0))
print("alloc_ms=" + (t2 - t1))
print("chk=" + c1 + "/" + c2)
```

**The workload must live in functions.** A top-level loop is never compiled (module top-levels are excluded), so measuring one compares Varn's interpreter against node's JIT — which is how the original benchmark was accidentally written.

## 7. Explicitly out of scope

- **Matching node on string churn.** node builds 400k strings in ~13 ms against our ~50 ms. After this plan the gap narrows but does not close: the remaining cost is an out-of-line helper doing work node emits inline. Closing it means generating the whole concat — `itoa`, the copy, and the nursery bump — as machine code, which needs inline heap allocation in CLIF. That is its own plan.
- **Ropes / lazy concatenation.** JSC uses them (measured: bun defers 12.7 ms of flattening in the same workload), node apparently does not and still wins. A rope representation changes what every string consumer must handle; do not start it as a performance patch.
- **The `Ext` accumulation path.** `str_concat` already makes `s = s + x` linear. It is not on this benchmark's path and needs nothing.

## 8. Rollback

Tasks 1, 2 and 3 are independent and each is revertible on its own.

Task 1 is the one to watch: if a GC bug appears after it, revert **it first** regardless of what else landed, because a missed root is the only change here that can corrupt memory rather than merely compute a wrong string. `tests/65-safepoint-roots.vn` is the file that should have caught it — if it did not, widen that test before re-attempting the narrowing.

## 9. Known follow-ups (do not do here)

- Array element representation — the `arr` row of the external comparison (1.53x vs node). NaN-boxed elements against packed SMI.
- The suspected hole in `hir::inline::collect_mutated_globals` (`crates/varn-opt/src/hir/inline.rs:437`): `scan_stmts` never descends into nested statement bodies via `push_child_stmts`, so a global reassigned inside an `if` or loop body may be invisible to the inliner's safety check. **Unconfirmed** — an attempt to reproduce it did not fire the inliner at all, and `if (true)` was folded before the scan. Needs a callee the inliner actually inlines plus a non-foldable condition. `hir::ctor_summary` deliberately does its own complete scan rather than reuse this one.
- `a + b` with both operands declared `string` is rejected by the checker (`WR3010: invalid binary operation '+' between 'string' and 'string'`), while `"a" + b` is fine. Looks unintentional.
- `.length` does not exist on the static `string` type, only on `dyn`. `let s: string = ""; s.length` fails to compile.
