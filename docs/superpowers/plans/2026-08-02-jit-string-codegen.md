# JIT String Codegen Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `"prefix" + <int>` cost what node charges for it — from ~37 ns per concat to under 15 ns — by writing the result once instead of three times, and then by never leaving compiled code for it at all.

**Architecture:** Three stages. **Stage A** (`varn-vm`) makes the concat helper build its result directly instead of staging it through a `StrBuf` and a zeroed `[u8; 37]`. **Stage B** (`varn-jit`) emits `str + int` in CLIF over an inline nursery bump, so the common shape never calls out. **Stage C** (`varn-jit`) lowers `.length` to an inline load. Stage A gates Stage B: it is both a win and the probe that says how much of the gap was copies rather than the call.

**Tech Stack:** Rust, cranelift-codegen (the existing `clif` lowering), `varn-vm`'s NaN-boxed `VmValue`, generational heap and nursery.

**Design:** `docs/superpowers/specs/2026-08-02-jit-string-codegen-design.md` — read §1 before starting; it carries the measurements every decision here rests on.

---

## Global Constraints

- **Validation gold.** Run after every behaviour-affecting task. All must print `PASSED: 899 / FAILED: 0` (the count rises as this plan adds test modules — match whatever the tree reports before your change):
  - `./target/release/vn.exe run ./tests/main.vn`
  - `VARN_NO_JIT=1 ./target/release/vn.exe run ./tests/main.vn`
  - `VARN_JIT_TIER=999999 ./target/release/vn.exe run ./tests/main.vn`
  - The JIT and no-JIT runs must be **byte-identical**, not merely both passing. Diff them.
- **Purge the compile cache before validating.** It is keyed by source hash only and will happily serve you bytecode compiled by the previous binary:
  `Remove-Item -Recurse -Force $env:LOCALAPPDATA\varn\cache -ErrorAction SilentlyContinue`
- **Measurement protocol (non-negotiable).** This host has inverted a 40% effect and once read pure thermal drift as a 50% regression. Always: compare **two binaries alternately in one loop**, never sequentially; take the **median of ≥7 alternating rounds**; keep the object-allocation row in the mix as a control and **discard the batch if the control moves**.
- Release builds take **3–6 min** on this machine. A slow build is not a hang. Use `--profile quick` for iteration, `--release` for every number you report.
- `int` is i48 wrapping; object identity is the `Rc` address; instruction shapes come only from `varn_types::bytecode::decode`. Do not touch these.
- **File size governance:** no file over 1000 lines. `crates/varn-jit/src/clif/alloc.rs` is **1669 lines** and already over — it must not grow by a single line. New work goes in new modules.
- **Never hardcode a Rust enum or `Vec` layout in `varn-jit`.** Offsets are probed in `varn-vm` and passed across, the way `JitArrayLayout` and `JitObjectLayout` already are.
- Work on `main`. Commit per task.

---

## File Structure

| File | Responsibility | Stage |
|---|---|---|
| `crates/varn-vm/src/nursery.rs` (modify) | Pre-reserve both Vecs to `NURSERY_CAPACITY` so no allocation can realloc | A |
| `crates/varn-vm/src/heap.rs` (modify) | `alloc_str_concat_inline`; later the `JitStrLayout` probe | A, B |
| `crates/varn-vm/src/exec/strings.rs` (modify) | `str_concat` gains one early-out | A |
| `crates/varn-jit/src/lib.rs` (modify) | `JitStrLayout` declaration + `JitHelpers` field | B |
| `crates/varn-jit/src/clif/nursery.rs` (create) | `emit_nursery_alloc` — inline bump, returns slot address + heap index | B |
| `crates/varn-jit/src/clif/itoa.rs` (create) | Decimal digits in machine code | B |
| `crates/varn-jit/src/clif/strconcat.rs` (modify, 117 lines) | Compose itoa + nursery into the `str + int` lowering | B |
| `crates/varn-jit/src/clif/strlen.rs` (create) | `.length` inline load | C |
| `tests/67-jit-str-int.vn` (create) | Boundaries the new lowering introduces | B |
| `tests/68-str-length.vn` (create) | `.length` across representations | C |
| `docs/VM_ARCHITECTURE.md` (modify) | Document the three stages | D |

---

## Task 1: Pre-reserve the nursery so no allocation can realloc

`Nursery::try_alloc` pushes to `objects` **and** `forwarding`, which the minor collector indexes in lockstep. `objects` starts at capacity 4096 and reallocs its way to 16384. Stage B's inline bump cannot tolerate a `push` that might move the backing store — and Task 2's fast path holds a raw pointer across the reservation for the same reason. Reserving both up front removes the realloc from every allocation path in the VM.

Cost: ~900 KB resident (16384 × 48 B for `objects`, 16384 × 8 B for `forwarding`), paid once at startup for a fixed-size nursery.

**Files:**
- Modify: `crates/varn-vm/src/nursery.rs:45-54` (`Nursery::new`)
- Test: `crates/varn-vm/src/nursery.rs` (unit test, same file — this is a `varn-vm` internal invariant, not a language-level behaviour)

**Interfaces:**
- Produces: `Nursery::objects_capacity() -> usize` and `Nursery::forwarding_capacity() -> usize`, both `pub`, so the invariant test and Task 5's emitted bounds check can assert against the same number.

- [ ] **Step 1: Write the failing test.** Add at the bottom of `crates/varn-vm/src/nursery.rs`, inside a `#[cfg(test)] mod` (create one if the file has none):

```rust
#[cfg(test)]
mod capacity_invariant {
    use super::*;
    use crate::heap::{HeapObj, HeapStr};

    /// Stage B emits a nursery bump inline, which is only sound if a `push`
    /// can never move the backing store. That makes "capacity is
    /// NURSERY_CAPACITY from birth and never changes" a load-bearing
    /// invariant rather than a tuning detail.
    #[test]
    fn nursery_capacity_is_fixed_from_birth() {
        let mut n = Nursery::new();
        assert_eq!(n.objects_capacity(), NURSERY_CAPACITY);
        assert_eq!(n.forwarding_capacity(), NURSERY_CAPACITY);

        let objects_ptr = n.objects_data_ptr();
        for i in 0..NURSERY_CAPACITY {
            let obj = HeapObj::Str(HeapStr::inline("x"));
            assert!(n.try_alloc(obj).is_ok(), "alloc {i} should fit");
        }
        assert_eq!(n.objects_capacity(), NURSERY_CAPACITY, "capacity grew");
        assert_eq!(
            n.objects_data_ptr(),
            objects_ptr,
            "backing store moved — an inline bump would write into freed memory"
        );

        // One past capacity must decline, not grow.
        assert!(n.try_alloc(HeapObj::Str(HeapStr::inline("y"))).is_err());
        assert_eq!(n.objects_capacity(), NURSERY_CAPACITY);
    }
}
```

- [ ] **Step 2: Run it and watch it fail.**

Run: `cargo test -p varn-vm nursery_capacity_is_fixed_from_birth`
Expected: FAIL — `objects_capacity` and `objects_data_ptr` do not exist yet.

- [ ] **Step 3: Add the accessors and the reservation.** In `crates/varn-vm/src/nursery.rs`, replace `Nursery::new` (currently at `:45-54`):

```rust
    pub fn new() -> Self {
        // Full capacity from birth, not grown into. `try_alloc` pushes to
        // `objects` and `forwarding` together and the minor collector indexes
        // both by nursery index, so a realloc in either is a moving backing
        // store — which the JIT's inline bump (clif/nursery.rs) and
        // `Heap::alloc_str_concat_inline` both assume cannot happen. Fixed
        // size, so this is ~900 KB paid once rather than a growth curve.
        Self {
            objects: Vec::with_capacity(NURSERY_CAPACITY),
            forwarding: Vec::with_capacity(NURSERY_CAPACITY),
            remembered: Vec::new(),
            alloc_count: 0,
            minor_gc_count: 0,
            minor_gc_promoted: 0,
        }
    }

    /// Capacity of the object slots. Constant for the nursery's lifetime —
    /// see `new`. Exposed so the invariant test and the JIT's emitted bounds
    /// check assert against one number.
    pub fn objects_capacity(&self) -> usize {
        self.objects.capacity()
    }

    /// Capacity of the forwarding slots, which must track `objects`.
    pub fn forwarding_capacity(&self) -> usize {
        self.forwarding.capacity()
    }

    /// Backing-store address of the object slots, for the invariant test.
    pub fn objects_data_ptr(&self) -> *const Option<HeapObj> {
        self.objects.as_ptr()
    }
```

- [ ] **Step 4: Run the test.**

Run: `cargo test -p varn-vm nursery_capacity_is_fixed_from_birth`
Expected: PASS.

- [ ] **Step 5: Verify a collection preserves capacity.** `collect` calls `self.objects.clear()`, which keeps capacity — but `forwarding` is handled separately and must not be replaced with a fresh `Vec`. Read `Nursery::collect` in full and confirm neither Vec is reassigned; if either is, change it to `.clear()`. Then add to the same test module:

```rust
    /// A collection must not hand back a smaller (or relocated) nursery.
    #[test]
    fn nursery_capacity_survives_a_collection() {
        let heap = crate::heap::Heap::new();
        let mut stack: Vec<VmValue> = Vec::new();
        for i in 0..1000 {
            let v = unsafe { heap.inner_mut() }
                .alloc_str_dynamic(format!("keep-{i}-padding-to-exceed-sso"));
            stack.push(v);
        }
        let inner = unsafe { heap.inner_mut() };
        let before = inner.nursery.objects_data_ptr();
        inner.collect_minor(&mut stack, &[]);
        assert_eq!(inner.nursery.objects_capacity(), NURSERY_CAPACITY);
        assert_eq!(
            inner.nursery.objects_data_ptr(),
            before,
            "collection relocated the nursery backing store"
        );
    }
```

If the exact `Heap` construction/collection entry points differ from the names above, adapt them to what `heap.rs` actually exposes — the assertion (capacity and pointer unchanged across a collection) is the point, not the scaffolding.

Run: `cargo test -p varn-vm nursery_capacity`
Expected: both PASS.

- [ ] **Step 6: Full workspace test + validation gold.**

```bash
cargo test --workspace
cargo build --release
```
```powershell
Remove-Item -Recurse -Force $env:LOCALAPPDATA\varn\cache -ErrorAction SilentlyContinue
```
```bash
./target/release/vn.exe run ./tests/main.vn > /tmp/jit.txt
VARN_NO_JIT=1 ./target/release/vn.exe run ./tests/main.vn > /tmp/nojit.txt
diff /tmp/jit.txt /tmp/nojit.txt && echo IDENTICAL
VARN_JIT_TIER=999999 ./target/release/vn.exe run ./tests/main.vn
```
Expected: all three pass, `IDENTICAL` printed.

- [ ] **Step 7: Measure.** Per the §Global measurement protocol, on `decomp.vn` (design §7). This change is expected to be **neutral to slightly positive** — it removes reallocs from every allocation path. Record the medians. If any row regresses beyond noise, the ~900 KB is touching a cache effect and the reservation should be reconsidered before Stage B depends on it.

- [ ] **Step 8: Commit.**

```bash
git add crates/varn-vm/src/nursery.rs
git commit -m "perf(vm): the nursery is full-capacity from birth, never grown into"
```

---

## Task 2: The concat helper builds its result once

`str_concat` stages a short result through a `StrBuf`, then `HeapStr::inline` zeroes a `[u8; 37]` and copies into it, then the 48-byte `HeapObj` moves into the nursery slot. Three copies plus a `try_from_sso` scan of the assembled string. This builds the byte array directly and hands it over once.

Deliberately **safe** — no raw pointers. It removes the `StrBuf` and the double-copy through `HeapStr::inline`, which is most of the win at a fraction of the risk. A true single-copy version (reserve the slot, then write through a raw pointer into it) is the follow-up **only if** Task 3's measurement shows the remaining cost is still in the copies.

**Files:**
- Modify: `crates/varn-vm/src/heap.rs` (add `alloc_str_concat_inline` next to `alloc_str_dynamic:953`)
- Modify: `crates/varn-vm/src/exec/strings.rs:58` (`str_concat` gains one early-out)
- Test: `crates/varn-vm/src/heap.rs` (unit test), plus the existing `tests/64-str-concat-typed.vn` and `tests/66-inline-strings.vn` as the behavioural net

**Interfaces:**
- Consumes: `Nursery::objects_capacity` from Task 1 (via the unchanged `try_alloc`, which now never reallocs).
- Produces: `Heap::alloc_str_concat_inline(&mut self, a: VmValue, b: VmValue) -> Option<VmValue>` — `Some(value)` when the result was built inline, `None` when the caller must fall through to the general path.

- [ ] **Step 1: Write the failing test.** Add to `crates/varn-vm/src/heap.rs`'s test module:

```rust
    /// The inline concat path must agree with the general path on every input
    /// it accepts, and must decline (rather than produce a wrong answer) on
    /// everything else. The `None` cases are as load-bearing as the `Some`
    /// ones: an `Ext` left operand taken here would bypass `str_concat`'s
    /// accumulation path and make `s = s + x` quadratic.
    #[test]
    fn inline_concat_agrees_with_the_general_path() {
        let heap = Heap::new();
        let h = unsafe { heap.inner_mut() };

        // (left, right) pairs that must round-trip to the expected string.
        let cases: &[(&str, i64, &str)] = &[
            ("", 0, "0"),
            ("gc_", 1, "gc_1"),
            ("gc_", 400000, "gc_400000"),
            ("ab", -7, "ab-7"),
            ("", -1, "-1"),
            // Exactly INLINE_STR_CAP (37): 34 chars + "123".
            ("abcdefghijklmnopqrstuvwxyz01234567", 123, "abcdefghijklmnopqrstuvwxyz01234567123"),
        ];
        for (l, r, want) in cases {
            let a = h.alloc_str_dynamic(l);
            let b = VmValue::from_int(*r);
            let got = h
                .alloc_str_concat_inline(a, b)
                .expect("should have been built inline");
            assert_eq!(h.str_repr(got), *want, "inline concat of {l:?} + {r}");
        }

        // One past INLINE_STR_CAP must decline.
        let long = h.alloc_str_dynamic("abcdefghijklmnopqrstuvwxyz012345678");
        assert!(
            h.alloc_str_concat_inline(long, VmValue::from_int(123)).is_none(),
            "38-byte result must decline"
        );

        // A non-int right operand must decline.
        let s = h.alloc_str_dynamic("x");
        assert!(h.alloc_str_concat_inline(s, s).is_none(), "non-int rhs must decline");

        // Non-ASCII left operand still round-trips (bytes are copied whole).
        let uni = h.alloc_str_dynamic("日本語のプレフィックス");
        let got = h
            .alloc_str_concat_inline(uni, VmValue::from_int(5))
            .expect("multibyte prefix fits in 37 bytes");
        assert_eq!(h.str_repr(got), "日本語のプレフィックス5");
    }
```

- [ ] **Step 2: Run it and watch it fail.**

Run: `cargo test -p varn-vm inline_concat_agrees_with_the_general_path`
Expected: FAIL — `alloc_str_concat_inline` does not exist.

- [ ] **Step 3: Implement it.** Add to `crates/varn-vm/src/heap.rs`, immediately after `alloc_str_dynamic` (which ends at `:979`):

```rust
    /// Concatenate string `a` and integer `b` straight into one byte array,
    /// handed to the nursery once.
    ///
    /// `str_concat`'s general path copies the payload three times — into a
    /// `StrBuf`, into the zeroed `[u8; INLINE_STR_CAP]` that
    /// `HeapStr::inline` builds, and again moving the `HeapObj` into the
    /// slot — plus a `try_from_sso` scan of the assembled result. This writes
    /// the bytes once and knows the length before it starts.
    ///
    /// `None` means "not my shape, use the general path":
    ///
    /// * `b` is not an int — the digit fast path is the whole point;
    /// * `a` is `Ext` — it **must** fall through, or `str_concat`'s
    ///   accumulation path is bypassed and `s = s + x` goes quadratic;
    /// * the result exceeds `INLINE_STR_CAP` — it needs an `Rc`;
    /// * the result fits SSO — cheaper still, and `alloc_str_dynamic` already
    ///   does it with no heap slot at all.
    pub fn alloc_str_concat_inline(&mut self, a: VmValue, b: VmValue) -> Option<VmValue> {
        use crate::strbuf::{itoa, INT_MAX_DIGITS};

        if !b.is_int() {
            return None;
        }

        // Resolve `a`'s bytes. SSO materializes into a local; a heap string
        // borrows. `Ext` declines here, before anything is written.
        let mut sso_buf = [0u8; 5];
        let a_bytes: &[u8] = if a.is_sso() {
            a.sso_as_str(&mut sso_buf).as_bytes()
        } else if a.is_heap() {
            match self.get(a.as_heap_idx()) {
                Some(HeapObj::Str(HeapStr::Ext { .. })) => return None,
                Some(HeapObj::Str(hs)) => hs.as_str().as_bytes(),
                _ => return None,
            }
        } else {
            return None;
        };

        let mut digits = [0u8; INT_MAX_DIGITS];
        let digits = itoa(b.as_int(), &mut digits).as_bytes();
        let total = a_bytes.len() + digits.len();

        // Below the SSO limit there is no reason to touch the heap at all;
        // above INLINE_STR_CAP the bytes cannot live in the slot.
        if total <= 5 || total > INLINE_STR_CAP {
            return None;
        }

        let mut bytes = [0u8; INLINE_STR_CAP];
        bytes[..a_bytes.len()].copy_from_slice(a_bytes);
        bytes[a_bytes.len()..total].copy_from_slice(digits);

        // ASCII is decided here for free: the digits always are, so the
        // answer is `a`'s. Recording it saves the first `.length` a scan.
        let ascii = if a_bytes.is_ascii() {
            ascii_flag::YES
        } else {
            ascii_flag::NO
        };

        Some(self.alloc_str_view(HeapStr::Inline {
            len: total as u8,
            ascii: std::cell::Cell::new(ascii),
            bytes,
        }))
    }
```

Note the borrow shape: `a_bytes` borrows `self` immutably and must be copied into the local `bytes` array before `alloc_str_view` takes `&mut self`. Written as above, `bytes` is filled while the borrow is live and the borrow ends at the `Some(...)` — if the borrow checker disagrees, copy `a_bytes` into `bytes` and rebind lengths into locals before the final statement rather than reaching for `unsafe`.

- [ ] **Step 4: Route `str_concat` through it.** In `crates/varn-vm/src/exec/strings.rs`, insert immediately after the `Ext` accumulation block closes (after line `87`'s `}`) and before the `StrBuf` comment:

```rust
    // The `"prefix" + <int>` shape, built once instead of staged through a
    // `StrBuf` and a zeroed `[u8; INLINE_STR_CAP]`. Declines to anything it
    // cannot serve, including an `Ext` left operand — but the accumulation
    // path above has already claimed those.
    if let Some(v) = heap.alloc_str_concat_inline(a, b) {
        return v;
    }
```

- [ ] **Step 5: Run the tests.**

Run: `cargo test -p varn-vm` then `cargo test --workspace`
Expected: PASS, including the existing `heap_obj_slot_stride_is_unchanged`.

- [ ] **Step 6: Validation gold.** Build release, purge the cache, run all three configurations, diff JIT against no-JIT. Then specifically:

```bash
./target/release/vn.exe run ./tests/64-str-concat-typed.vn
./target/release/vn.exe run ./tests/66-inline-strings.vn
VARN_NO_JIT=1 ./target/release/vn.exe run ./tests/66-inline-strings.vn
```
Expected: `[PASSED] 64. ...` and `[PASSED] 66. Inline strings`.

- [ ] **Step 7: Prove accumulation stayed linear.** The `Ext` decline is the one thing no assertion above catches at scale. Write this outside the repo and run it:

```vn
import { now } from "std:time"
function acc(n: int): int {
  let s = ""
  for (let i = 0; i < n; i = i + 1) { s = s + "abcdefgh" }
  return s.length
}
let t0 = now(); let a = acc(20000)
let t1 = now(); let b = acc(40000)
let t2 = now()
print("20k=" + (t1 - t0) + " 40k=" + (t2 - t1) + " chk=" + (a + b))
```
Expected: the 40k time is roughly **2x** the 20k time. If it is ~4x, an `Ext` operand is reaching the new path — fix before continuing, this is the quadratic regression the design warns about.

- [ ] **Step 8: Measure.** Per the protocol, on `decomp.vn`. The rows that must move are `D_inline` (`"gc_" + i`) and `F_itoa` (`"" + i`). `B_sso_fast` and the object-allocation control must not.

- [ ] **Step 9: Commit**, with the medians and the accumulation-linearity numbers in the message.

```bash
git add crates/varn-vm/src/heap.rs crates/varn-vm/src/exec/strings.rs
git commit -m "perf(vm): build a short str+int concat once instead of three times"
```

---

## Task 3: The gate — decide whether Stage B is worth building

Stage A was chosen as both a win and a probe. This task spends fifteen minutes reading its result before committing to the largest piece of codegen this JIT has taken on.

**Files:** none. This task produces a decision and a paragraph.

- [ ] **Step 1: Re-run the decomposition** on the Task 2 binary, per the protocol, and write the table down next to the design's §1 numbers.

- [ ] **Step 2: Compute the remaining gap.** The reference: `"gc_" + i` was 37 ns/iter, of which the loop baseline is 5 ns and the already-inline concat path (`B_sso_fast`) proves a call-free concat costs 2.5 ns. So the floor for a fully-inline `str + int` is roughly `5 + 2.5 + itoa`.

- [ ] **Step 3: Decide, and record the decision in the commit message.**
  - **`"gc_" + i` still above ~20 ns** → the boundary and the remaining copies dominate. **Proceed to Task 4.** This is the expected outcome.
  - **`"gc_" + i` at or below ~15 ns** → Stage A got there on its own. **Stop after Task 8** (`.length`), skip Tasks 4–7, and record in `docs/VM_ARCHITECTURE.md` that the inline nursery emitter was measured unnecessary. That is a result worth as much as the code.
  - **Between the two** → build Task 4–6 (layout probe, nursery emitter, itoa) but land only the **SSO arm** of Task 7, whose payoff is largest per line. Note the nursery arm as a follow-up with its measured expected value.

- [ ] **Step 4: Commit the decision** as a docs-only change to this plan file, checking the boxes above and adding a `**Gate result:**` line under this task.

---

## Task 4: Probe the string slot layout and hand it to the JIT

Stage B writes a `HeapObj` from generated code. `Option<HeapObj>`'s encoding and `HeapStr::Inline`'s field offsets inside it are not guaranteed by Rust, so the JIT must never spell them out. This follows the established precedent verbatim: *"every offset here is PROBED against a real object at startup rather than hardcoded"* (`JitObjectLayout`, `varn-jit/src/lib.rs:80-83`).

**Files:**
- Modify: `crates/varn-jit/src/lib.rs` (add `JitStrLayout`, add a `str_layout` field to `JitHelpers` near `object_layout:224`)
- Modify: `crates/varn-vm/src/heap.rs` (add `jit_str_layout()` next to `jit_object_layout():1760`; wire it into wherever `jit_object_layout()` is passed into `JitHelpers`)
- Test: `crates/varn-vm/src/heap.rs` (round-trip unit test)

**Interfaces:**
- Produces:
```rust
pub const STR_TEMPLATE_MAX: usize = 64;

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct JitStrLayout {
    /// Discriminant byte value of `HeapObj::Str` (niche-shared with `Option`).
    pub str_tag: usize,
    /// A ready-made `Some(HeapObj::Str(HeapStr::Inline { len: 0, ascii:
    /// UNKNOWN, bytes: [0; INLINE_STR_CAP] }))`, captured as raw bytes.
    /// Emitted code stores `slot_size` bytes of this and then overwrites
    /// `len_off` and the payload — so it never has to understand the
    /// discriminant or the `ascii` cell.
    pub template: [u8; STR_TEMPLATE_MAX],
    /// `size_of::<Option<HeapObj>>()` — how much of `template` is live.
    pub slot_size: usize,
    /// Slot base → the `Inline` variant's `len: u8`.
    pub len_off: usize,
    /// Slot base → the `Inline` variant's `bytes[0]`.
    pub bytes_off: usize,
    /// `INLINE_STR_CAP` — the largest result the inline arm may build.
    pub inline_cap: usize,
    /// RcBox base → the nursery `forwarding` Vec's three words.
    pub nursery_fwd_vec_off: usize,
    /// RcBox base → `Nursery::alloc_count`.
    pub alloc_count_off: usize,
    /// `NURSERY_CAPACITY` — the bound the emitted bump checks against.
    pub nursery_capacity: usize,
}
```

- [ ] **Step 1: Write the failing test.** Add to `crates/varn-vm/src/heap.rs`'s test module:

```rust
    /// The layout the JIT will write through must reconstruct a string the
    /// VM can read back. This is the test that catches a representation
    /// change before it becomes a segfault in generated code.
    #[test]
    fn jit_str_layout_round_trips() {
        let lay = Heap::jit_str_layout();
        assert!(lay.slot_size <= varn_jit::STR_TEMPLATE_MAX);
        assert_eq!(lay.inline_cap, INLINE_STR_CAP);
        assert_eq!(lay.nursery_capacity, crate::nursery::NURSERY_CAPACITY);

        // Build a slot the way emitted code will: template, then len, then
        // bytes — and nothing else.
        let mut raw = [0u8; varn_jit::STR_TEMPLATE_MAX];
        raw[..lay.slot_size].copy_from_slice(&lay.template[..lay.slot_size]);
        let payload = b"gc_400000";
        raw[lay.len_off] = payload.len() as u8;
        raw[lay.bytes_off..lay.bytes_off + payload.len()].copy_from_slice(payload);

        let slot: Option<HeapObj> =
            unsafe { std::ptr::read(raw.as_ptr() as *const Option<HeapObj>) };
        match &slot {
            Some(HeapObj::Str(hs)) => {
                assert_eq!(hs.as_str(), "gc_400000");
                assert_eq!(hs.len(), 9);
            }
            _ => panic!("layout-built slot did not read back as a string"),
        }
        std::mem::forget(slot);

        // The tag byte the emitted guard reads must distinguish it from None.
        let none: Option<HeapObj> = None;
        let none_tag = unsafe { *(&none as *const _ as *const u8) } as usize;
        assert_ne!(lay.str_tag, none_tag, "Option<HeapObj> niche probe failed");
    }
```

- [ ] **Step 2: Run it and watch it fail.**

Run: `cargo test -p varn-vm jit_str_layout_round_trips`
Expected: FAIL — `JitStrLayout` / `jit_str_layout` do not exist.

- [ ] **Step 3: Declare the struct** in `crates/varn-jit/src/lib.rs`, immediately after `JitObjectLayout`'s definition, using the `Produces` block above verbatim. Then add to `JitHelpers`, next to `object_layout` at `:224`:

```rust
    /// Probed string-slot layout for the inline concat allocation path.
    pub str_layout: JitStrLayout,
```

- [ ] **Step 4: Write the probe** in `crates/varn-vm/src/heap.rs`, next to `jit_object_layout` (`:1760`):

```rust
    /// Probed layout facts for the JIT's inline string allocation (see
    /// [`varn_jit::JitStrLayout`]).
    ///
    /// Nothing here is hardcoded. The template is a real
    /// `Some(HeapObj::Str(HeapStr::Inline { .. }))` captured as bytes, and the
    /// field offsets come from taking references into that same value — so a
    /// change to `HeapObj`, to `HeapStr`, or to the compiler's niche
    /// placement moves the emitted code with it instead of silently
    /// invalidating it.
    pub fn jit_str_layout() -> varn_jit::JitStrLayout {
        let slot: Option<HeapObj> = Some(HeapObj::Str(HeapStr::Inline {
            len: 0,
            ascii: std::cell::Cell::new(ascii_flag::UNKNOWN),
            bytes: [0u8; INLINE_STR_CAP],
        }));
        let size = std::mem::size_of::<Option<HeapObj>>();
        assert!(
            size <= varn_jit::STR_TEMPLATE_MAX,
            "Option<HeapObj> ({size} B) outgrew the JIT template buffer"
        );

        let base = &slot as *const _ as usize;
        let (len_off, bytes_off) = match &slot {
            Some(HeapObj::Str(HeapStr::Inline { len, bytes, .. })) => (
                (len as *const u8 as usize) - base,
                (bytes.as_ptr() as usize) - base,
            ),
            _ => unreachable!("just built as Inline"),
        };

        let mut template = [0u8; varn_jit::STR_TEMPLATE_MAX];
        let raw = unsafe { std::slice::from_raw_parts(base as *const u8, size) };
        template[..size].copy_from_slice(raw);
        let str_tag = raw[0] as usize;

        varn_jit::JitStrLayout {
            str_tag,
            template,
            slot_size: size,
            len_off,
            bytes_off,
            inline_cap: INLINE_STR_CAP,
            nursery_fwd_vec_off: 2 * std::mem::size_of::<usize>()
                + std::mem::offset_of!(HeapInner, nursery)
                + crate::nursery::Nursery::forwarding_vec_byte_offset(),
            alloc_count_off: 2 * std::mem::size_of::<usize>()
                + std::mem::offset_of!(HeapInner, nursery)
                + std::mem::offset_of!(crate::nursery::Nursery, alloc_count),
            nursery_capacity: crate::nursery::NURSERY_CAPACITY,
        }
    }
```

Add the missing accessor to `crates/varn-vm/src/nursery.rs`, next to `objects_vec_byte_offset` (`:97`):

```rust
    /// Byte offset of the `forwarding` Vec's three words within `Nursery`,
    /// for the JIT's inline allocation — which must bump both Vecs, since the
    /// minor collector indexes them together.
    pub fn forwarding_vec_byte_offset() -> usize {
        std::mem::offset_of!(Nursery, forwarding)
    }
```

`alloc_count` is `pub` already; if `offset_of!` on it fails because the field is private in some build, add a `pub fn alloc_count_byte_offset()` beside the others rather than making the field public.

- [ ] **Step 5: Wire it into `JitHelpers`.** Find where `jit_object_layout()` is passed in (grep `object_layout:` across the workspace) and add `str_layout: Heap::jit_str_layout(),` beside it.

- [ ] **Step 6: Run the tests.**

Run: `cargo test -p varn-vm jit_str_layout_round_trips` then `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Validation gold.** No behaviour changed, but the probe runs at startup and its asserts are new. All three configurations, byte-identical.

- [ ] **Step 8: Commit.**

```bash
git add crates/varn-jit/src/lib.rs crates/varn-vm/src/heap.rs crates/varn-vm/src/nursery.rs
git commit -m "feat(jit): probe the string slot layout instead of spelling it out"
```

---

## Task 5: `emit_nursery_alloc` — the inline bump

**Files:**
- Create: `crates/varn-jit/src/clif/nursery.rs`
- Modify: `crates/varn-jit/src/clif/mod.rs` (register the module)
- Test: exercised by Task 7; this task's own check is that it compiles and that a hand-written call site produces the expected asm.

**Interfaces:**
- Consumes: `JitStrLayout` (Task 4), `JitArrayLayout::{nursery_slots_vec_off, slots_ptr_off, slot_size}`, `JitHelpers::heap_field_offset` and `nursery_len_offset` (all existing).
- Produces:
```rust
pub(super) struct NurserySlot {
    /// Machine address of the freshly reserved `Option<HeapObj>` slot.
    pub addr: Value,
    /// Its nursery index, ready to be tagged into a heap `VmValue`.
    pub idx: Value,
}

pub(super) fn emit_nursery_alloc(
    b: &mut FunctionBuilder,
    helpers: &JitHelpers,
    exec_ctx: Value,
    slow: Block,
) -> NurserySlot;
```
On return the builder sits in a block where `addr` and `idx` are valid; a full nursery has branched to `slow`.

- [ ] **Step 1: Write the module.** Create `crates/varn-jit/src/clif/nursery.rs`:

```rust
//! Inline nursery allocation for CLIF.
//!
//! Reading a nursery slot inline is already established — `clif/fields.rs`
//! and `clif/emit.rs` address `nursery_ptr + idx * slot_size` for property and
//! array fast paths, and `alloc::emit_gc_safepoint_check` already loads the
//! live-object count. This adds *writing* a fresh one.
//!
//! Sound because of three properties, all of which must hold:
//!
//! 1. **The backing store never moves.** `Nursery::new` reserves both Vecs to
//!    `NURSERY_CAPACITY` from birth, so no `push` can realloc under emitted
//!    code holding a slot address.
//! 2. **The bump is not a safepoint.** It cannot collect; a full nursery takes
//!    the `slow` block. Collection still happens only at back-edge safepoints
//!    and inside helpers.
//! 3. **`forwarding` tracks `objects`.** The minor collector indexes both by
//!    nursery index, so both lengths are bumped here. Bumping one is a silent
//!    out-of-bounds on the next collection.

use cranelift_codegen::ir::{condcodes::IntCC, types, Block, InstBuilder, MemFlags, Value};
use cranelift_frontend::FunctionBuilder;

use crate::JitHelpers;

pub(super) struct NurserySlot {
    pub addr: Value,
    pub idx: Value,
}

/// Reserve one nursery slot, or branch to `slow` if the nursery is full.
pub(super) fn emit_nursery_alloc(
    b: &mut FunctionBuilder,
    helpers: &JitHelpers,
    exec_ctx: Value,
    slow: Block,
) -> NurserySlot {
    let alay = &helpers.array_layout;
    let slay = &helpers.str_layout;
    let m = MemFlags::trusted();

    // ExecCtx -> heap Rc -> RcBox base, the same walk the field fast paths do.
    let rcbox = b
        .ins()
        .load(types::I64, m, exec_ctx, helpers.heap_field_offset as i32);

    // 1. Capacity check. NOT the GC threshold: this is the hard bound past
    //    which `try_alloc` itself declines. Crossing the softer
    //    `nursery_threshold` is the back-edge safepoint's business.
    //    The length offset already lives on `JitHelpers` (the back-edge
    //    safepoint reads it); it is deliberately NOT duplicated onto
    //    `JitStrLayout`.
    let len = b
        .ins()
        .load(types::I64, m, rcbox, helpers.nursery_len_offset as i32);
    let has_room = b.ins().icmp_imm(
        IntCC::UnsignedLessThan,
        len,
        slay.nursery_capacity as i64,
    );
    let ok = b.create_block();
    b.ins().brif(has_room, ok, &[], slow, &[]);
    b.switch_to_block(ok);

    // 2. Bump both lengths. `forwarding`'s new entry must read as `None`;
    //    `Option<u32>` is 8 bytes with a niche, so its `None` pattern is
    //    whatever the probe captured — see step 3.
    let next = b.ins().iadd_imm(len, 1);
    b.ins()
        .store(m, next, rcbox, helpers.nursery_len_offset as i32);
    let fwd_len_off = slay.nursery_fwd_vec_off + 2 * std::mem::size_of::<usize>();
    b.ins().store(m, next, rcbox, fwd_len_off as i32);

    // 3. alloc_count, so JIT allocations are visible to `bench -v` and to the
    //    GC statistics rather than silently uncounted.
    let ac = b
        .ins()
        .load(types::I64, m, rcbox, slay.alloc_count_off as i32);
    let ac = b.ins().iadd_imm(ac, 1);
    b.ins().store(m, ac, rcbox, slay.alloc_count_off as i32);

    // 4. Slot address: nursery objects data pointer + len * slot_size.
    let data = b.ins().load(
        types::I64,
        m,
        rcbox,
        (alay.nursery_slots_vec_off + alay.slots_ptr_off) as i32,
    );
    let off = b.ins().imul_imm(len, alay.slot_size as i64);
    let addr = b.ins().iadd(data, off);

    NurserySlot { addr, idx: len }
}
```

- [ ] **Step 2: Handle the `forwarding` `None` write.** Step 1 bumps `forwarding.len()` without writing the new element. `Vec<Option<u32>>` slots past the old length hold whatever was there before — for a nursery that has been collected, a stale `Some(idx)`, which would make the collector treat a fresh object as already-forwarded. Two options; take the first:

  **(a) Zero it on the inline path.** Extend `JitStrLayout` with `fwd_none_pattern: u64` and `fwd_elem_size: usize`, probed in `jit_str_layout` as:

```rust
        let none_fwd: Option<u32> = None;
        let fwd_elem_size = std::mem::size_of::<Option<u32>>();
        let mut pat = [0u8; 8];
        let raw_fwd = unsafe {
            std::slice::from_raw_parts(&none_fwd as *const _ as *const u8, fwd_elem_size)
        };
        pat[..fwd_elem_size].copy_from_slice(raw_fwd);
        let fwd_none_pattern = u64::from_ne_bytes(pat);
```

  then in `emit_nursery_alloc`, after bumping:

```rust
    let fwd_data = b.ins().load(
        types::I64,
        m,
        rcbox,
        (slay.nursery_fwd_vec_off + alay.slots_ptr_off) as i32,
    );
    let fwd_off = b.ins().imul_imm(len, slay.fwd_elem_size as i64);
    let fwd_addr = b.ins().iadd(fwd_data, fwd_off);
    let none = b.ins().iconst(types::I64, slay.fwd_none_pattern as i64);
    b.ins().store(m, none, fwd_addr, 0);
```

  **(b)** Confirm by reading `Nursery::collect` that `forwarding` is cleared *and* zero-filled every collection, making the stale-entry case impossible. If and only if you verify that, skip (a) and record the reasoning in the module doc.

- [ ] **Step 3: Register the module.** Add `pub(crate) mod nursery;` to `crates/varn-jit/src/clif/mod.rs` beside the other `clif` submodules.

- [ ] **Step 4: `cargo check --workspace`.**

Expected: clean. Nothing calls `emit_nursery_alloc` yet, so expect a dead-code warning; leave it — Task 7 is the consumer, and silencing it with `#[allow]` would outlive its reason.

- [ ] **Step 5: Commit.**

```bash
git add crates/varn-jit/src/clif/nursery.rs crates/varn-jit/src/clif/mod.rs crates/varn-jit/src/lib.rs crates/varn-vm/src/heap.rs
git commit -m "feat(jit): reserve a nursery slot inline, without leaving compiled code"
```

---

## Task 6: `emit_itoa` — decimal digits in machine code

**Files:**
- Create: `crates/varn-jit/src/clif/itoa.rs`
- Modify: `crates/varn-jit/src/clif/mod.rs`
- Test: Task 7's `tests/67-jit-str-int.vn` is the behavioural net; this task's own check is the assembly inspection in Step 3, which is load-bearing.

**Interfaces:**
- Produces:
```rust
pub(super) struct Itoa {
    /// Number of digit bytes written, including a leading `-`.
    pub ndigits: Value,
    /// The digits packed as an SSO payload: byte 0 of the number at bit 37,
    /// byte 1 at bit 29, … — the placement `VmValue::try_from_sso` uses. Only
    /// meaningful when `ndigits <= 5`.
    pub packed: Value,
    /// Address of the first digit byte in a stack slot, for the memory arm.
    pub buf: Value,
}

pub(super) fn emit_itoa(b: &mut FunctionBuilder, n: Value) -> Itoa;
```
One emitter serves both arms: the SSO arm wants a `u64`, the nursery arm wants bytes. Producing both in the same digit loop costs one extra store per digit and avoids two loops that could disagree.

- [ ] **Step 1: Write the module.** Create `crates/varn-jit/src/clif/itoa.rs`:

```rust
//! Decimal formatting of an `int` in generated code.
//!
//! `int` is i48, so at most 15 digits plus a sign. Digits come out
//! least-significant first, which is exactly the order both consumers want:
//!
//! * the **packed** accumulator places each new digit at bit 37 and shifts the
//!   previous ones down by 8, so after the loop byte 0 of the number sits at
//!   bit 37 — the placement `VmValue::try_from_sso` uses, ready to be OR-ed
//!   under a prefix;
//! * the **buffer** is filled backwards from its end, so the digits end up
//!   contiguous and in reading order.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags, StackSlotData,
    StackSlotKind, Value};
use cranelift_frontend::FunctionBuilder;

/// i48 min is -140737488355328: 15 digits plus a sign.
const MAX_DIGITS: u32 = 16;

/// Bit position of SSO byte 0. `try_from_sso` writes byte `i` at `37 - i * 8`.
const SSO_BYTE0_SHIFT: i64 = 37;

pub(super) struct Itoa {
    pub ndigits: Value,
    pub packed: Value,
    pub buf: Value,
}

pub(super) fn emit_itoa(b: &mut FunctionBuilder, n: Value) -> Itoa {
    let slot = b.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        MAX_DIGITS,
        0,
    ));
    let base = b.ins().stack_addr(types::I64, slot, 0);
    let m = MemFlags::trusted();

    let zero = b.ins().iconst(types::I64, 0);
    let is_neg = b.ins().icmp(IntCC::SignedLessThan, n, zero);
    let neg = b.ins().ineg(n);
    let mag = b.ins().select(is_neg, neg, n);

    // loop(mag, pos, packed) -> emits one digit per iteration.
    let loop_b = b.create_block();
    let done_b = b.create_block();
    b.append_block_param(loop_b, types::I64); // remaining magnitude
    b.append_block_param(loop_b, types::I64); // write cursor (from the end)
    b.append_block_param(loop_b, types::I64); // packed accumulator
    b.append_block_param(done_b, types::I64); // final cursor
    b.append_block_param(done_b, types::I64); // final packed

    let start_pos = b.ins().iconst(types::I64, MAX_DIGITS as i64);
    b.ins().jump(loop_b, &[mag.into(), start_pos.into(), zero.into()]);

    b.switch_to_block(loop_b);
    let cur = b.block_params(loop_b)[0];
    let pos = b.block_params(loop_b)[1];
    let acc = b.block_params(loop_b)[2];

    let q = b.ins().udiv_imm(cur, 10);
    let q10 = b.ins().imul_imm(q, 10);
    let r = b.ins().isub(cur, q10);
    let ch = b.ins().iadd_imm(r, b'0' as i64);

    let pos1 = b.ins().iadd_imm(pos, -1);
    let addr = b.ins().iadd(base, pos1);
    let ch8 = b.ins().ireduce(types::I8, ch);
    b.ins().store(m, ch8, addr, 0);

    let shifted = b.ins().ushr_imm(acc, 8);
    let placed = b.ins().ishl_imm(ch, SSO_BYTE0_SHIFT);
    let acc1 = b.ins().bor(shifted, placed);

    let more = b.ins().icmp_imm(IntCC::NotEqual, q, 0);
    b.ins().brif(
        more,
        loop_b,
        &[q.into(), pos1.into(), acc1.into()],
        done_b,
        &[pos1.into(), acc1.into()],
    );

    b.switch_to_block(done_b);
    let end_pos = b.block_params(done_b)[0];
    let end_acc = b.block_params(done_b)[1];

    // Prepend '-' by stepping the cursor back one more and storing it. The
    // packed accumulator gets the same treatment: shift the digits down and
    // drop the sign into byte 0.
    let neg_pos = b.ins().iadd_imm(end_pos, -1);
    let sign_pos = b.ins().select(is_neg, neg_pos, end_pos);
    let sign_addr = b.ins().iadd(base, sign_pos);
    let minus = b.ins().iconst(types::I8, b'-' as i64);
    let existing = b.ins().load(types::I8, m, sign_addr, 0);
    let to_store = b.ins().select(is_neg, minus, existing);
    b.ins().store(m, to_store, sign_addr, 0);

    let acc_shift = b.ins().ushr_imm(end_acc, 8);
    let minus_placed = b
        .ins()
        .iconst(types::I64, (b'-' as i64) << SSO_BYTE0_SHIFT);
    let acc_neg = b.ins().bor(acc_shift, minus_placed);
    let packed = b.ins().select(is_neg, acc_neg, end_acc);

    let total = b.ins().iconst(types::I64, MAX_DIGITS as i64);
    let ndigits = b.ins().isub(total, sign_pos);
    let buf = b.ins().iadd(base, sign_pos);

    Itoa { ndigits, packed, buf }
}
```

- [ ] **Step 2: Register the module.** Add `pub(crate) mod itoa;` to `crates/varn-jit/src/clif/mod.rs`.

- [ ] **Step 3: Verify the division lowering — this step is load-bearing.** Cranelift may lower `udiv_imm` by 10 to a hardware `div` (~20 cycles on this core), which would erase the entire win on its own. Build, then dump the asm of a function that uses the new path and look at the digit loop:

```bash
cargo build --release
./target/release/vn.exe debug -p clif:asm --fn fD <scratch>/decomp.vn | grep -iE "\bdiv\b|mul|shr"
```

Expected: a `mul`/`mulx` by a magic constant (`0xCCCC…CD`) plus a shift. **If you see a bare `div`**, replace the `udiv_imm`/`imul_imm` pair in Step 1's loop with an explicit magic multiply:

```rust
    // q = cur / 10, via the standard unsigned reciprocal: multiply-high by
    // 0xCCCCCCCCCCCCCCCD, then shift right by 3. Cranelift did not do this
    // for us; a hardware `div` here is ~20 cycles and costs more than the
    // helper call this whole path exists to avoid.
    let magic = b.ins().iconst(types::I64, 0xCCCC_CCCC_CCCC_CCCDu64 as i64);
    let hi = b.ins().umulhi(cur, magic);
    let q = b.ins().ushr_imm(hi, 3);
```

Record which form you shipped, and the evidence, in the commit message.

- [ ] **Step 4: `cargo check --workspace`.** Expected: clean, with a dead-code warning until Task 7.

- [ ] **Step 5: Commit.**

```bash
git add crates/varn-jit/src/clif/itoa.rs crates/varn-jit/src/clif/mod.rs
git commit -m "feat(jit): format an int as decimal digits in generated code"
```

---

## Task 7: Lower `str + int` without leaving compiled code

**Files:**
- Modify: `crates/varn-jit/src/clif/strconcat.rs` (117 lines; the existing string+string fast path stays exactly as it is)
- Test: `tests/67-jit-str-int.vn` (new), plus `tests/main.vn`

**Interfaces:**
- Consumes: `nursery::emit_nursery_alloc` and `nursery::NurserySlot` (Task 5), `itoa::emit_itoa` and `itoa::Itoa` (Task 6), `JitStrLayout` (Task 4), and the existing `alloc::{live_boxed, flush_boxed, reload_boxed, def_result}`.
- Produces: no signature change. `emit_str_concat` keeps `(b, actx, state, code, ip)` and `body.rs:729` does not move.

**The two kinds of knowledge in play** — conflating them is the easiest way to get this wrong:
- **`b` is an int, statically.** `state[b_r] == K::Int` is decided at emit time. That is why today's SSO guard folds to `xor r11d, r11d`. The specialized lowering is emitted **only** in that case and needs no runtime test on `b`.
- **`a` is a string, but its representation is not static.** `K` has no string kind — `Unset`, `Int`, `Float`, `Bool`, `Boxed`, `Global`, `Poison`, `Mixed`. `a` is known to be a string only because `varn-opt` emitted `StrConcat` at all. It may be SSO, `Inline`, `Shared`, `Slice` or `Ext` at runtime, so `a` takes a **runtime** representation test.

- [ ] **Step 1: Write the test first.** Create `tests/67-jit-str-int.vn`:

```vn
// `"prefix" + <int>` is lowered in CLIF: the digits are formatted in machine
// code and the result is either assembled as an SSO value or written into a
// nursery slot reserved inline. Three arms, so three sets of boundaries — and
// the arms are chosen by RESULT LENGTH, which is why every length near a
// transition is pinned here rather than a representative sample.
//
// A bug in the guard chain shows up as a wrong string, a wrong length, or a
// crash under allocation pressure. None of it shows up as a warning.

function cat(p: dyn, n: int): dyn { return p + n }

// --- SSO arm: total <= 5 bytes -----------------------------------------
assert("strint: empty + 0", cat("", 0) === "0")
assert("strint: 1 + 1 digit", cat("a", 7) === "a7")
assert("strint: exactly 5", cat("ab", 123) === "ab123")
assert("strint: 5 all digits", cat("", 12345) === "12345")

// --- inline arm: 6..37 bytes -------------------------------------------
assert("strint: 6 bytes", cat("ab", 1234) === "ab1234")
assert("strint: the benchmark shape", cat("gc_", 400000) === "gc_400000")
assert("strint: exactly 37", cat("abcdefghijklmnopqrstuvwxyz01234", 567890) === "abcdefghijklmnopqrstuvwxyz01234567890")

// --- helper arm: 38+ bytes ---------------------------------------------
assert("strint: 38 bytes falls back", cat("abcdefghijklmnopqrstuvwxyz012345", 678901) === "abcdefghijklmnopqrstuvwxyz012345678901")
assert("strint: long prefix", cat("abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGH", 1) === "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGH1")

// --- negatives: the sign is a byte the digit loop prepends --------------
assert("strint: negative small", cat("v", -1) === "v-1")
assert("strint: negative at the sso limit", cat("v", -123) === "v-123")
assert("strint: negative into the inline arm", cat("gc_", -400000) === "gc_-400000")
assert("strint: zero is one digit, not none", cat("n", 0) === "n0")

// --- i48 extremes ------------------------------------------------------
assert("strint: i48 max", cat("m", 140737488355327) === "m140737488355327")
assert("strint: i48 min", cat("m", -140737488355328) === "m-140737488355328")

// --- non-ASCII left operand: bytes are copied whole, length is chars ----
assert("strint: multibyte prefix", cat("日本", 5) === "日本5")
assert("strint: accent prefix", cat("é", 12) === "é12")
function multiLen(): int { const s = cat("日本", 5); return s.length }
assert("strint: multibyte length is chars not bytes", multiLen() === 3)

// --- the left operand's own representation varies ----------------------
// A slice, a shared string and a concat result each reach the lowering as a
// different HeapStr variant.
function fromSlice(): dyn { const long = "abcdefghijklmnopqrstuvwxyz"; return long.slice(0, 3) + 9 }
assert("strint: sliced left operand", fromSlice() === "abc9")
function fromConcat(): dyn { const c = "ab" + "cd"; return c + 9 }
assert("strint: concat-built left operand", fromConcat() === "abcd9")

// --- accumulation must stay linear -------------------------------------
// `s = s + x` seeds an Ext buffer, which MUST take the helper arm. If the
// inline path claims it, this goes quadratic — 200k appends would not finish.
function accumulate(n: int): int {
  let s = ""
  for (let i = 0; i < n; i = i + 1) { s = s + i }
  return s.length
}
assert("strint: accumulation stays linear", accumulate(200000) === 1088890)

// --- survives collections ----------------------------------------------
// Results held live across enough allocation to force several minor GCs.
function survivesGc(n: int): int {
  const held = cat("keep_", 987654)
  let hits = 0
  for (let i = 0; i < n; i = i + 1) {
    const t = cat("gc_", i)
    if (t === "gc_" + i) { hits = hits + 1 }
  }
  if (held === "keep_987654") { hits = hits + 1 }
  return hits
}
assert("strint: results survive collections", survivesGc(60000) === 60001)

print("[PASSED] 67. JIT str+int ")
```

Add `import "./67-jit-str-int.vn"` to `tests/main.vn` after line 66, and bump the count line at `:68` from 66 to 67.

- [ ] **Step 2: Verify `accumulate(200000) === 1088890` before relying on it.** That constant is the total digit count of `0..199999` and must be confirmed, not assumed:

```bash
node -e "let n=0;for(let i=0;i<200000;i++)n+=String(i).length;console.log(n)"
```
Substitute whatever it prints. A wrong constant here turns the linearity guard into a false failure that will be "fixed" by deleting it.

- [ ] **Step 3: Run the test on today's binary.**

Run: `./target/release/vn.exe run ./tests/67-jit-str-int.vn` and again with `VARN_NO_JIT=1`.
Expected: `[PASSED] 67. JIT str+int` under both. These are all existing semantics — this is a regression net, not a red test. If anything fails now, stop: the lowering would be built against a wrong specification.

- [ ] **Step 4: Commit the test on its own**, so the lowering lands against a net already in the tree.

```bash
git add tests/67-jit-str-int.vn tests/main.vn
git commit -m "test: pin str+int concat boundaries before lowering it in CLIF"
```

- [ ] **Step 5: Add the specialized lowering.** In `crates/varn-jit/src/clif/strconcat.rs`, keep `emit_str_concat` as the entry point and give it an early branch. The existing string+string fast path is untouched.

```rust
pub(super) fn emit_str_concat(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let a_r = (code[ip + 1] >> 8) as usize;
    let b_r = (code[ip + 1] & 0xFF) as usize;

    // `b` being an int is decided HERE, at emit time — which is why the
    // string+string guard below folds to `xor r11d, r11d` in this shape and
    // why this arm needs no runtime test on `b`. `a`'s representation is a
    // different matter: `K` has no string kind, so it is tested at runtime.
    if state.get(b_r).copied() == Some(K::Int) {
        return emit_str_int_concat(b, actx, state, code, ip, dest, a_r, b_r);
    }
    emit_str_str_concat(b, actx, state, code, ip, dest, a_r, b_r)
}
```

Move today's body into `emit_str_str_concat` verbatim, then write `emit_str_int_concat` with this shape:

```
n            = unboxed int value of b (i48 sign-extend, as the typed ops read it)
it           = itoa::emit_itoa(b, n)
a_len, a_pack, a_addr = runtime representation test on `a`:
                 SSO           -> len from bits 45.., payload already placed
                 heap + Str tag + Inline variant
                               -> len from [slot + len_off], bytes at slot + bytes_off
                 anything else -> helper arm
total        = a_len + it.ndigits

  total <= 5            -> SSO arm
  total <= inline_cap   -> nursery arm
  otherwise             -> helper arm
```

**SSO arm** — no allocation, so nothing to root:

```rust
    // `a`'s bytes are already at their final positions; the digits start at
    // result index a_len, which is 8*a_len bits below where emit_itoa placed
    // them. One shift for the whole payload — bytes past a length are zero,
    // so the two payloads simply or together.
    let shift = b.ins().ishl_imm(a_len, 3);
    let digits_shifted = b.ins().ushr(it.packed, shift);
    let len_field = b.ins().ishl_imm(total, SSO_LEN_SHIFT);
    let tagged = b.ins().bor(tag_bits, len_field);
    let with_a = b.ins().bor(tagged, a_pack);
    let res = b.ins().bor(with_a, digits_shifted);
```

Note this arm requires `a` to be SSO — an `Inline` heap `a` whose total is ≤5 is impossible, since `Inline` is only ever built for strings over the SSO limit.

**Nursery arm** — reserve, template, len, bytes:

```rust
    let slay = &actx.helpers.str_layout;
    let m = MemFlags::trusted();
    let slot = nursery::emit_nursery_alloc(b, actx.helpers, actx.exec_ctx, slow_block);

    // Template first: it carries the discriminant and the `ascii` cell, so
    // emitted code never has to understand either. Stored as whole words off
    // the probed byte array, zero-padding the tail — `slot_size` is not
    // required to be a multiple of 8, and the bytes past it are inside the
    // slot stride and are about to be overwritten by the payload anyway.
    let words = slay.slot_size.div_ceil(8);
    for w in 0..words {
        let mut wb = [0u8; 8];
        let start = w * 8;
        let end = (start + 8).min(slay.slot_size);
        wb[..end - start].copy_from_slice(&slay.template[start..end]);
        let v = b.ins().iconst(types::I64, u64::from_ne_bytes(wb) as i64);
        b.ins().store(m, v, slot.addr, (start) as i32);
    }

    // Then the byte length, which the template left at 0.
    let len8 = b.ins().ireduce(types::I8, total);
    b.ins().store(m, len8, slot.addr, slay.len_off as i32);

    // Then the payload: `a`'s bytes at index 0, the digits at index a_len.
    let dst = b.ins().iadd_imm(slot.addr, slay.bytes_off as i64);
    emit_byte_copy(b, dst, a_addr, a_len);              // a  -> [0 .. a_len)
    let dst_digits = b.ins().iadd(dst, a_len);
    emit_byte_copy(b, dst_digits, it.buf, it.ndigits);  // digits -> [a_len ..)

    // Finally tag the nursery index into a heap VmValue, exactly as
    // `VmValue::from_heap_idx` does. Read that function and mirror it — do
    // not re-derive the tag bits here.
    let res = /* QNAN | TAG_HEAP | slot.idx, per VmValue::from_heap_idx */;
```

`emit_byte_copy(b, dst, src, n)` is a local helper in this module: a block with a counter parameter, a `uload8`, a `store`, and a `brif` back — the same shape as `emit_itoa`'s digit loop. Write it once and call it twice rather than unrolling; at 1–37 bytes the loop overhead is noise next to the call it replaces.

The `res` line is the one place where the exact bit pattern must come from `varn_types::vm_value` rather than from this plan: open `VmValue::from_heap_idx` and mirror what it does, the way `strconcat.rs` already imports `QNAN`/`TAG_SSO` for the string+string arm instead of writing constants inline.

**Helper arm** — unchanged from today: `live_boxed`, `flush_boxed`, `call_helper`, `reload_boxed`. All four arms join at one merge block with a single `I64` parameter, and `def_result` runs once on it.

**The flush stays in the helper arm only.** Neither fast arm can collect — `emit_nursery_alloc` declines to `slow` rather than collecting — so neither needs rooting. Hoisting the flush above the branch would pay the safepoint on the paths that exist to avoid it.

- [ ] **Step 6: Check the file size.** `crates/varn-jit/src/clif/strconcat.rs` was 117 lines. If this pushes it past ~400, split the SSO/nursery/helper arms into `clif/strconcat/` submodules now rather than later — `clif/alloc.rs` is what happens when that decision is deferred.

Run: `wc -l crates/varn-jit/src/clif/strconcat.rs`

- [ ] **Step 7: `cargo check --workspace`, then `cargo test --workspace`.**

- [ ] **Step 8: Validation gold**, all three configurations, byte-identical, cache purged. Plus `tests/67-jit-str-int.vn`, `tests/64-str-concat-typed.vn` and `tests/66-inline-strings.vn` under both tiers.

- [ ] **Step 9: Prove the fast path is actually taken.** Without this, a bug in the guard chain is indistinguishable from "no speedup" — which is exactly how the three attempts in the previous plan's §1 read. Add a temporary counter to `jit_str_concat` (`crates/varn-vm/src/exec/ctx_jit_runtime.rs:233`):

```rust
    // TEMPORARY — remove before committing.
    {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let c = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if c % 100_000 == 0 { eprintln!("jit_str_concat calls: {c}"); }
    }
```

Run a 400k-iteration `"gc_" + i` loop. Expected: **zero** calls — every result is 4–9 bytes and takes a fast arm. Then run a 400k loop with a 40-byte prefix and expect ~400k calls, confirming the helper arm still works. Remove the counter.

- [ ] **Step 10: Measure**, per the protocol, on `decomp.vn`. Report `C_sso_res`, `D_inline`, `F_itoa`, and the object-allocation control. `B_sso_fast` must not move — that path was not touched.

- [ ] **Step 11: Commit**, with the call-count evidence and the medians in the message.

---

## Task 8: `.length` as an inline load

Independent of Stage B — it can land before Task 4 or after Task 7. `.length` costs 17 ns/iter as a native-op call with a four-slot stack argument window, and it appears in every string benchmark.

**Files:**
- Create: `crates/varn-jit/src/clif/strlen.rs`
- Modify: `crates/varn-jit/src/clif/body.rs` (the dispatch arm determined in Step 1), `crates/varn-jit/src/clif/mod.rs`
- Test: `tests/68-str-length.vn` (new), `tests/main.vn`

**Interfaces:**
- Consumes: `JitStrLayout::{str_tag, len_off}` (Task 4 — if Task 3's gate sent you past Stage B, do Task 4 first, it is small and self-contained), `JitArrayLayout::{nursery_slots_vec_off, slots_ptr_off, slot_size}`.
- Produces: `pub(super) fn emit_str_length(b, actx, state, code, ip) -> bool` — `true` when it emitted an inline lowering, `false` when the caller must fall through to today's generic path.

- [ ] **Step 1: Determine which opcode `.length` actually reaches.** `body.rs` has arms for both `GetProperty` (`:707`, which routes to `get_property_ic_fast` — and `ctx_jit_values.rs:941` already calls `fast_length` there) and `CallNativeOp` (`:741`). The asm from the design's §1 shows a four-slot argument window, which is `CallNativeOp`'s shape, but the receiver's static type may select between them. Settle it:

```bash
./target/release/vn.exe debug -p clif --fn fL <scratch>/decomp.vn | grep -iE "GetProperty|CallNativeOp"
```

Target whichever arm the benchmark hits. If it is `GetProperty`, the win is smaller than 17 ns (that path already avoids the arg window) — re-measure before building, and if it is under ~5 ns, stop and record that instead.

- [ ] **Step 2: Write the test.** Create `tests/68-str-length.vn`:

```vn
// `.length` is lowered to an inline load off the heap slot. It must agree
// with the generic getter across every string representation and on arrays,
// and it must keep reporting CHARACTERS, not bytes — the inline arm reads a
// byte length, so a non-ASCII receiver has to fall through.

function len(s: dyn): int { return s.length }

assert("len: sso", len("abc") === 3)
assert("len: empty", len("") === 0)
assert("len: inline heap string", len("abcdefghij") === 10)
assert("len: long shared string", len("abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGH") === 44)
assert("len: dynamic concat result", len("gc_" + 400000) === 9)
assert("len: slice view", len("abcdefghijklmnopqrstuvwxyz".slice(2, 8)) === 6)

// Characters, not bytes — the inline arm must decline on non-ASCII.
assert("len: multibyte is chars", len("日本語") === 3)
assert("len: accents are chars", len("éàü") === 3)
assert("len: mixed ascii and multibyte", len("ab日本") === 4)

// Arrays share the getter.
assert("len: array", len([1, 2, 3]) === 3)
assert("len: empty array", len([]) === 0)

// Under allocation pressure, where the receiver's slot can be evacuated.
function lenAcrossGc(n: int): int {
  let hits = 0
  for (let i = 0; i < n; i = i + 1) {
    const s = "gc_" + i
    if (s.length >= 4) { hits = hits + 1 }
  }
  return hits
}
assert("len: correct across collections", lenAcrossGc(60000) === 60000)

print("[PASSED] 68. String length ")
```

Add `import "./68-str-length.vn"` to `tests/main.vn` and bump the count line to 68.

- [ ] **Step 3: Run it on today's binary** under both tiers. Expected: `[PASSED] 68. String length`. Regression net, not a red test.

- [ ] **Step 4: Commit the test on its own.**

```bash
git add tests/68-str-length.vn tests/main.vn
git commit -m "test: pin .length across string representations before lowering it inline"
```

- [ ] **Step 5: Write the lowering.** Create `crates/varn-jit/src/clif/strlen.rs`, porting the logic that `varn-vm/src/exec/strings.rs:35` (`fast_length`) already states:

```
receiver is SSO            -> length is bits 45..48 of the value
receiver is heap:
   tag == str_tag
     variant == Inline     -> load [slot + len_off] as u8
     and ascii_state == YES -> that byte IS the char count
     otherwise             -> slow
   tag == array_tag        -> load the element Vec's len word
   otherwise               -> slow
anything else              -> slow
```

The ASCII guard is the subtle part and must not be skipped: `HeapStr::Inline`'s `len` is a **byte** count, while `.length` is a **character** count. They agree only for ASCII. `is_ascii_cached` computes and memoizes the answer on first use, so the inline arm reads the cached flag and declines on `UNKNOWN` or `NO`, letting the helper resolve and cache it. Task 2's `alloc_str_concat_inline` already records `YES`/`NO` at construction for exactly this reason, so concat results answer on the first query.

Add `pub(crate) mod strlen;` to `clif/mod.rs` and call it from the arm found in Step 1, falling through to today's code when it returns `false`.

- [ ] **Step 6: `cargo check --workspace`, then `cargo test --workspace`.**

- [ ] **Step 7: Validation gold**, all three configurations, byte-identical, plus `tests/68-str-length.vn` under both tiers.

- [ ] **Step 8: Verify the inline arm is taken.** Same technique as Task 7 Step 9: a temporary counter in the generic getter, a 400k `.length` loop on an ASCII string, expecting near-zero calls; then a multibyte receiver, expecting ~400k.

- [ ] **Step 9: Measure** on `decomp.vn`'s `L_length` row and on the external comparison's `("gc_" + i).length`. Report both.

- [ ] **Step 10: Commit.**

---

## Task 9: Re-measure the external comparison and document

**Files:**
- Modify: `docs/VM_ARCHITECTURE.md` (§7's "Strings en código compilado" subsection, added by the previous plan at `:282`)

- [ ] **Step 1: Run the three-way comparison** with the final binary, per the protocol:

`gc_split.js` is in the design's §7. Write its Varn counterpart alongside it —
**the workload must live in functions**, since module top-level is never
compiled and a top-level loop would compare our interpreter against node's JIT:

```vn
// gc_fn.vn
import { now } from "std:time"
class GcVtA { x: int
    constructor(x: int) { this.x = x } }
function junk(): int { let j = []; for (let i = 0; i < 400000; i = i + 1) { j.push("gc_" + i) } return j.length }
function concatOnly(): int { let n = 0; for (let i = 0; i < 400000; i = i + 1) { n = n + ("gc_" + i).length } return n }
function pushOnly(): int { let j = []; for (let i = 0; i < 400000; i = i + 1) { j.push(i) } return j.length }
function alloc(): int { let aa = 0; for (let i = 0; i < 100000; i = i + 1) { let a = new GcVtA(i); aa = aa + a.x } return aa }

for (let r = 0; r < 6; r = r + 1) {
  let t0 = now(); let a = junk()
  let t1 = now(); let b = concatOnly()
  let t2 = now(); let c = pushOnly()
  let t3 = now(); let d = alloc()
  let t4 = now()
  print("junk=" + (t1-t0) + " concat=" + (t2-t1) + " push=" + (t3-t2) + " alloc=" + (t4-t3) + " chk=" + (a+b+c+d))
}
```

```bash
node <scratch>/gc_split.js
bun  <scratch>/gc_split.js
./target/release/vn.exe run <scratch>/gc_fn.vn
```

Report the string phase and the object phase separately. Baselines to beat, from the design's §1: `("gc_"+i).length` at varn 23 ms, bun 17.1 ms, node 6.3 ms; `j.push("gc_"+i)` at varn 34 ms, bun 19.1 ms, node 9.2 ms. The object row must still read 0 ms.

- [ ] **Step 2: Update `docs/VM_ARCHITECTURE.md`.** The existing subsection (in Spanish, matching that file) says the fast path does not fire for `"prefijo" + <entero>` and that reaching it *"significa emitir `itoa` en CLIF; es otro trabajo."* Replace that paragraph with what was built: the `str + int` lowering's three arms and how the arm is chosen; `emit_nursery_alloc`'s three soundness properties (fixed backing store, not a safepoint, `forwarding` tracked); why the layout is probed rather than written down; and the `.length` inline load with its ASCII guard.

- [ ] **Step 3: Record what did not work.** This repo's convention is that a perf conclusion carries its evidence, and a disproved hypothesis is worth as much as a confirmed one. Add whatever this plan produced: Task 3's gate decision with its numbers, the `udiv_imm` lowering finding from Task 6 Step 3, and anything measured at zero. Keep the existing list of five measured zeros — do not overwrite it.

- [ ] **Step 4: Correct the record on ropes.** The previous plan's §7 states node does not use them. V8 does use cons strings, and that is plausibly part of node's remaining advantage. Note the correction where the doc discusses the gap, so the next person does not re-derive it.

- [ ] **Step 5: Commit.**

---

## Definition of Done

- [ ] `tests/main.vn` passes under `vn run`, `VARN_NO_JIT=1`, and `VARN_JIT_TIER=999999`, with byte-identical JIT and no-JIT output.
- [ ] `cargo test --workspace` green.
- [ ] `tests/67-jit-str-int.vn` and `tests/68-str-length.vn` pass under both tiers.
- [ ] `size_of::<HeapObj>()` still 48, asserted by the existing `heap_obj_slot_stride_is_unchanged`.
- [ ] `jit_str_layout_round_trips` passes — the JIT's view of the slot reconstructs a string the VM reads back.
- [ ] `s = s + x` accumulation is linear, proven by the timed test in Task 2 Step 7 and by `tests/67-jit-str-int.vn`'s `accumulate(200000)`.
- [ ] The fast path is proven taken by call-count evidence, not inferred from timing.
- [ ] `("gc_" + i)` is under 15 ns/iter, or the gate decision in Task 3 records why not.
- [ ] The object-allocation control row is still 0 ms.
- [ ] No file over 1000 lines among those touched; `clif/alloc.rs` did not grow.
- [ ] `docs/VM_ARCHITECTURE.md` documents the lowering, the soundness argument, the gate result, and the ropes correction.

## Rollback

Each stage is independently revertible.

Task 1 (nursery pre-reservation) is a precondition for Tasks 2, 5 and 7 — reverting it means reverting them too.

**Task 7 is the one to watch.** It writes heap objects from generated code, so it is the only change here that can corrupt memory rather than merely compute a wrong string. If a GC bug appears after it, revert it first regardless of what else landed. `tests/67-jit-str-int.vn`'s `survivesGc` is what should have caught it; if it did not, widen that test before re-attempting.
