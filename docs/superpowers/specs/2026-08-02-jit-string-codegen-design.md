# JIT String Codegen — Design

**Status:** approved 2026-08-02, awaiting implementation plan.

**Goal.** Make `"prefix" + <int>` — the shape every string benchmark is built
from — cost what node charges for it. Today Varn spends ~37 ns per concat where
node spends ~16 ns and its own already-inline string+string path spends 2.5 ns.

This design supersedes §7 of `docs/superpowers/plans/2026-08-01-string-concat-codegen.md`,
which named ropes as node's advantage and deferred the codegen work. The
measurements below say the advantage is not ropes: it is that we leave compiled
code, and that once outside we copy the payload three times.

---

## 1. Measurements this design rests on

Host: i7-1355U, Windows, `--release`. 400k iterations per row, best of 6.
Workload sources in §7.

### External comparison

| workload | varn | node | bun |
|---|---|---|---|
| `j.push("gc_" + i)` | 34 ms | 9.2 ms | 19.1 ms |
| `("gc_" + i).length` | 23 ms | 6.3 ms | 17.1 ms |
| object allocation | 0 ms | 0 ms | 0.1 ms |

The object row is already at zero from the escape-analysis work and is the
control: nothing in this design may move it.

### Cost decomposition

Measured with a cheap sink (`s = <expr>` into a loop-carried variable, one
`.length` at the end) so the per-iteration `.length` call does not contaminate
the rows — an earlier decomposition did exactly that and inflated every row by
~17 ns.

| variant | ms/400k | ns/iter | marginal |
|---|---|---|---|
| loop baseline | 2 | 5 | — |
| `parts[i & 3]` — array index alone | 3 | 7.5 | +2.5 |
| `"g" + parts[i & 3]` — **SSO inline path, no call** | 4 | 10 | **+2.5** |
| `s.length` per iteration, constant receiver | 9 | 22 | +17 |
| `"g" + (i % 1000)` — helper, SSO result, no heap | 11 | 27 | +22 |
| `"" + i` — helper, itoa only | 14 | 35 | +30 |
| `"gc_" + i` — helper + `HeapStr::Inline` | 15 | 37 | +32 |
| `"…37+ bytes…" + i` — helper + `Rc::from` | 30 | 75 | +70 |

### What the numbers establish

1. **The inline concat path from the previous plan's Task 3 is genuinely
   free — 2.5 ns.** Concat is not expensive. Leaving compiled code is.
2. **`"" + i` ≈ `"gc_" + i`.** The cost is the int, not the concatenation and
   not the allocation. `HeapStr::Inline` allocation is ~2 ns on top of a
   no-allocation concat.
3. **`Rc::from` costs 40 ns.** Out of scope here — `INLINE_STR_CAP = 37`
   already covers the band this design targets — but it explains why the
   previous plan's Task 2 mattered even though it measured at zero: it is what
   makes an inline-emittable allocation possible at all.
4. **`.length` is a 17 ns native-op call.** Orthogonal to concat, present in
   every string benchmark.

### What the generated code shows

`vn debug -p clif:asm --fn` on a `"gc_" + i` loop:

- The SSO fast-path guard is **constant-folded to `xor r11d, r11d`**. Cranelift
  proves the operand is `K::Int` and therefore never SSO, so the guard costs
  nothing and the inline block is emitted dead. The kind is available
  statically at the emit site — a specialized `str + int` lowering needs **no
  runtime type guard**.
- The safepoint around the call is 2 stores and 3 loads. The boundary is
  already thin; Task 1 of the previous plan did its job.
- `.length` compiles to a native-op dispatch with a four-slot stack argument
  window, a second call per iteration.

Since the boundary is thin and the body is ~30 ns, the body is where the time
is. Reading `strings::str_concat`, the payload bytes are copied **three times**:

1. into `StrBuf` (`str_repr_into` for each operand),
2. into a freshly zeroed `[u8; 37]` inside `HeapStr::inline`,
3. as part of moving the 48-byte `HeapObj` into the nursery Vec slot.

Plus a `try_from_sso` scan of the result, two `Vec::push` calls with capacity
checks, and an `alloc_count` increment.

---

## 2. Architecture

Three stages, each independently revertible and independently measured.
**Stage A gates Stage B**: it is both a win and the probe that says how much of
the ~30 ns was copies rather than the call. If A recovers most of the gap,
Stage B narrows to the SSO case and the nursery emitter is not built.

```
Stage A   varn-vm       helper writes the payload once
   │
   ├─ gate: how much gap remains?
   ▼
Stage B   varn-jit      emit `str + int` in CLIF, no call at all
Stage C   varn-jit      `.length` as an inline load
```

Stage C is independent of both and can land in any order.

### Stage A — the helper writes once

**Where:** `crates/varn-vm/src/nursery.rs`, `crates/varn-vm/src/heap.rs`,
`crates/varn-vm/src/exec/strings.rs`.

Two changes:

1. **Pre-reserve both nursery Vecs to `NURSERY_CAPACITY`.** `objects` starts at
   capacity 4096 and reallocs its way to 16384, and `try_alloc` pushes to
   `objects` *and* `forwarding`, which must stay in lockstep. Reserving both up
   front removes the realloc from every allocation path in the VM and is a
   precondition for Stage B, where an inline bump cannot tolerate a `push` that
   might move the backing store. Cost: ~900 KB resident for a fixed-size
   nursery (16384 × 48 B + 16384 × 8 B), paid once.

2. **`Heap::alloc_str_concat_inline(a, b) -> Option<VmValue>`** — reserve the
   nursery slot first, then format both operands directly into that slot's
   `HeapStr::Inline` byte array. Returns `None` when the result cannot take
   this shape (nursery full, combined length > `INLINE_STR_CAP`, or the result
   is short enough to be SSO), and `str_concat` falls through to what it does
   today. One copy replaces three.

The `Ext` accumulation path in `str_concat` is checked first and is unchanged —
`s = s + x` accumulation must keep its O(1) amortized behaviour.

**Interface:** `str_concat` gains one early-out. Nothing outside `varn-vm`
changes. No JIT changes.

### Stage B — emit `str + int` in CLIF

**Where:** new `crates/varn-jit/src/clif/nursery.rs`, new
`crates/varn-jit/src/clif/itoa.rs`, modified
`crates/varn-jit/src/clif/strconcat.rs` (117 lines today, room to grow),
`crates/varn-jit/src/lib.rs` (layout export), `crates/varn-vm/src/heap.rs`
(layout probe).

Three pieces, composed. Each is a separate module because each is a distinct
piece of knowledge: nursery mechanics, decimal formatting, and the concat
lowering that uses both.

**`clif/nursery.rs` — `emit_nursery_alloc`**

```
load    len   = [rcbox + nursery_len_off]
icmp    len >= NURSERY_CAPACITY  -> slow block
store   [rcbox + nursery_len_off]     = len + 1
store   [rcbox + forwarding_len_off]  = len + 1
store   [forwarding_ptr + len*4]      = None
lea     slot  = nursery_ptr + len * slot_size
returns (slot_addr, heap_idx = len)
```

The `alloc_count` field is bumped too; it feeds GC statistics and the
`bench -v` report, so skipping it would make the JIT's allocations invisible.

This mirrors `emit_gc_safepoint_check` (`clif/alloc.rs:270`), which already
loads the nursery length inline, and `fields.rs`'s `nursery_only` slot
addressing, which already computes `nursery_ptr + idx * slot_size`. Reading a
nursery slot inline is established; this adds writing one.

**Layout discipline.** `Option<HeapObj>` layout is not guaranteed by Rust, and
`HeapStr::Inline`'s field offsets inside it are not either. Following the
`JitArrayLayout` / `JitObjectLayout` precedent — *"every offset here is PROBED
against a real object at startup rather than hardcoded"* — a new `JitStrLayout`
carries:

- a **slot-sized template** (`size_of::<Option<HeapObj>>()`, the same
  `slot_size` `JitArrayLayout` already carries) of
  `Some(HeapObj::Str(HeapStr::Inline { len: 0, ascii: UNKNOWN, bytes: [0; 37] }))`,
  captured by transmuting a real value, so emitted code writes the
  discriminant and `ascii` field as opaque bytes it never has to understand;
- the byte offsets of `len` and `bytes[0]` within the slot, obtained by taking
  references into that same probe value and subtracting the base address.

Emitted code stores the template words, then `len`, then the payload. Nothing
about the enum encoding is duplicated in `varn-jit`.

**`clif/itoa.rs` — decimal digits in machine code**

A loop block emitting digits least-significant-first into a Cranelift stack
slot, then a reversing copy into the destination. `int` is i48 and can be
negative, so the sign is handled before the digit loop.

Division by 10: emit `udiv_imm`/`urem_imm` and inspect the resulting assembly.
If Cranelift's egraph already lowers division by a constant to a magic
multiply, that is the implementation; if it emits a hardware `div`, replace it
with an explicit `umulhi` by `0xCCCC_CCCC_CCCC_CCCD` and a shift. **This is a
verification step in the plan, not an assumption** — a hardware `div` is
~20 cycles and would erase the win on its own.

**`clif/strconcat.rs` — the lowering**

Two different kinds of knowledge are in play here, and conflating them is the
easiest way to get this wrong:

- **`b` is an int — statically.** `state[b_r] == K::Int` is decided at emit
  time, which is why today's SSO guard folds to `xor r11d, r11d`. The
  specialized lowering is emitted only in that case, and it needs no runtime
  type test on `b`.
- **`a` is a string — but its representation is not static.** `K` has no string
  kind (`Unset`, `Int`, `Float`, `Bool`, `Boxed`, `Global`, `Poison`, `Mixed`);
  `a` is known to be a string only because `varn-opt` emits `StrConcat` at all,
  and it may be SSO, `Inline`, `Shared`, `Slice` or `Ext` at runtime. So `a`
  takes a **runtime** representation test, and everything the fast arms cannot
  read falls to the helper.

```
;; emitted only when state[b_r] == K::Int
digits, ndigits = itoa(b)              ; in CLIF, into a stack slot
a_len, a_bytes  = runtime test on a:
                    SSO            -> length and bytes from the value (shifts)
                    heap + Inline  -> length and bytes from the slot (loads)
                    anything else  -> helper arm
total           = a_len + ndigits
  total <= 5           -> assemble an SSO VmValue in registers   (no allocation)
  total <= 37          -> emit_nursery_alloc, write bytes, tag the index
  otherwise            -> existing helper call, with its flush/reload
```

The flush/reload stays in the helper arm only, as it does today. Both fast arms
allocate at most a nursery slot, which cannot trigger a collection — the
back-edge safepoint is what collects — so neither needs rooting.

Of the representations that fall to the helper arm, `Ext` is not merely an
omission — it **must** go there, or `str_concat`'s accumulation path is
bypassed and `s = s + x` goes quadratic.

### Stage C — `.length` as an inline load

**Where:** `crates/varn-jit/src/clif/fields.rs`.

`fast_length` in `varn-vm/src/exec/strings.rs` already has the logic: SSO reads
the length out of the value; a heap string or array reads it from the slot.
Port it to an inline lowering guarded on the object tag, with the generic
native-op call as the slow arm. `HeapStr` variants keep their length in
different places, so the inline arm covers `Inline` and `Shared`, and anything
else falls through.

---

## 3. Correctness

**The GC hazard.** Stage B writes a heap object from generated code. Three
properties must hold:

1. **The slot is fully initialized before it is reachable.** The template write
   happens before the index is tagged into a `VmValue`. A collection cannot
   observe a half-written slot because nothing between the bump and the last
   store can collect.
2. **The bump is not a safepoint.** `emit_nursery_alloc` allocates without
   collecting; when the nursery is full it takes the slow block. This preserves
   today's invariant that collection happens only at back-edge safepoints and
   inside helpers.
3. **`forwarding` stays the same length as `objects`.** The minor collector
   indexes both by nursery index. Bumping one without the other is a
   silent out-of-bounds on the next collection.

**The `Ext` hazard.** `str_concat`'s accumulation path makes `s = s + x`
linear. Any new fast path that fires on an `Ext` left operand would make it
quadratic — a correctness-grade performance bug that no assertion catches.
Both Stage A and Stage B check for it first and decline.

**Testing.** Every stage validates against the repo gold: `tests/main.vn` under
`vn run`, `VARN_NO_JIT=1`, and `VARN_JIT_TIER=999999`, with the JIT and no-JIT
outputs byte-identical, and the compile cache purged first. Stage B adds
`tests/67-jit-str-int.vn` pinning the boundaries the new lowering introduces:
each length from 0 to 40 bytes across the SSO/inline/helper transitions,
negative ints, `int` at its i48 extremes, non-ASCII left operands, `Ext`
accumulation staying linear, and results held live across forced collections.

A Rust unit test asserts the probed `JitStrLayout` round-trips: build a value
through the layout's own offsets, read it back through `HeapStr::as_str`, and
compare. That is what catches a representation change before it becomes a
segfault.

---

## 4. Measurement protocol

Inherited unchanged from the previous plan, because this host has inverted a
40% effect before and once read pure thermal drift as a 50% regression:

- compare two binaries **alternately in one loop**, never sequentially;
- median of **≥7 alternating rounds**;
- keep a control workload the change cannot affect in the mix — the object
  allocation row — and discard the batch if the control moves.

Every stage reports its own numbers in its own commit message. Stage B
additionally reports a counter proving the fast path is taken: instrument
`jit_str_concat` and confirm the call count drops to zero on a workload whose
results are all ≤37 bytes. Without that, a bug in the guard chain is
indistinguishable from "no speedup" — which is how the three attempts recorded
in the previous plan's §1 read.

---

## 5. Scope

**In:** `str + int` lowering, inline nursery allocation, in-CLIF itoa,
`.length` inline load, the nursery pre-reservation that enables them.

**Out:**

- **Ropes / cons strings.** V8 does use them, contrary to the previous plan's
  §7, and they are plausibly part of node's 16 ns. They change what every
  string consumer must handle. Not a performance patch.
- **`Rc::from`'s 40 ns** for strings over 37 bytes. Real, measured, and a
  different band than this design targets.
- **`int + int` or float formatting** in CLIF. Same machinery would serve them;
  neither is on a measured hot path.
- **Generalizing `emit_nursery_alloc` to `BuildArray` / class construction.**
  The primitive is designed so they *can* adopt it, but doing so here would
  make the string win land last and blur attribution.

---

## 6. Success criteria

- `("gc_" + i)` drops from ~37 ns to under 15 ns.
- `("gc_" + i).length` — the external comparison row — beats bun's 17.1 ms and
  closes most of the distance to node's 6.3 ms.
- The object-allocation control row stays at 0 ms.
- `s = s + x` accumulation stays linear, proven by a timed test rather than by
  inspection.
- `tests/main.vn` passes under all three configurations, JIT and no-JIT output
  byte-identical.
- No file over 1000 lines. `clif/alloc.rs` is 1669 lines and already over — it
  must not grow; the new work goes in new modules.

---

## 7. Reproducing the measurements

Write these outside the repo. **The workload must live in functions** — module
top-level is never compiled, so a top-level loop measures our interpreter
against node's JIT.

```vn
// decomp.vn — the cost decomposition
import { now } from "std:time"
const N = 400000

function fA(): int { let n = 0; for (let i = 0; i < N; i = i + 1) { n = n + (i & 3) } return n }
function fB(parts: dyn): int { let s = ""; for (let i = 0; i < N; i = i + 1) { s = "g" + parts[i & 3] } return s.length }
function fI(parts: dyn): int { let s = ""; for (let i = 0; i < N; i = i + 1) { s = parts[i & 3] } return s.length }
function fC(): int { let s = ""; for (let i = 0; i < N; i = i + 1) { s = "g" + (i % 1000) } return s.length }
function fD(): int { let s = ""; for (let i = 0; i < N; i = i + 1) { s = "gc_" + i } return s.length }
function fE(): int { let s = ""; for (let i = 0; i < N; i = i + 1) { s = "gc_defghijklmnopqrstuvwxyz0123456789ABC" + i } return s.length }
function fF(): int { let s = ""; for (let i = 0; i < N; i = i + 1) { s = "" + i } return s.length }
function fL(): int { let n = 0; let s = "gc_abc"; for (let i = 0; i < N; i = i + 1) { n = n + s.length } return n }

let parts = ["a", "bb", "ccc", "d"]
for (let r = 0; r < 6; r = r + 1) {
  let t0=now(); let a=fA()
  let t1=now(); let b=fB(parts)
  let t2=now(); let c=fC()
  let t3=now(); let d=fD()
  let t4=now(); let e=fE()
  let t5=now(); let f=fF()
  let t6=now(); let g=fI(parts)
  let t7=now(); let h=fL()
  let t8=now()
  print("A_loop="+(t1-t0)+" B_sso_fast="+(t2-t1)+" C_sso_res="+(t3-t2)+" D_inline="+(t4-t3)+" E_rc="+(t5-t4)+" F_itoa="+(t6-t5)+" I_index="+(t7-t6)+" L_length="+(t8-t7)+" chk="+(a+b+c+d+e+f+g+h))
}
```

```js
// gc_split.js — the external comparison
class GcVtA{ constructor(x){ this.x=x; } }
function junk(){ const j=[]; for(let i=0;i<400000;i++) j.push("gc_"+i); return j.length; }
function concatOnly(){ let n=0; for(let i=0;i<400000;i++) n+=("gc_"+i).length; return n; }
function pushOnly(){ const j=[]; for(let i=0;i<400000;i++) j.push(i); return j.length; }
function alloc(){ let aa=0; for(let i=0;i<100000;i++){ const a=new GcVtA(i); aa+=a.x; } return aa; }
const names=["junk","concat","push","alloc"], fns=[junk,concatOnly,pushOnly,alloc];
const best=fns.map(()=>Infinity);
for(let r=0;r<12;r++){ fns.forEach((f,k)=>{ const t=performance.now(); f(); const m=performance.now()-t; if(r>=2&&m<best[k])best[k]=m; }); }
console.log(names.map((n,k)=>n+"="+best[k].toFixed(1)).join("  "));
```

Purge the compile cache before every validation run — it is keyed by source
hash only and will hide codegen changes:

```powershell
Remove-Item -Recurse -Force $env:LOCALAPPDATA\varn\cache -ErrorAction SilentlyContinue
```

---

## 8. Rollback

Stage A is a `varn-vm` change with no representation consequences; reverting it
restores `str_concat`'s current body. Stage B is the one to watch: it writes
heap objects from generated code, so a GC bug appearing after it should be
answered by reverting it first, regardless of what else landed. Stage C touches
one lowering and is independent of both.
