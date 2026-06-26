# Native ABI & Intrinsic Dispatch Spec

Authoritative contract for how Varn bytecode reaches native (Rust) builtins. It
covers op-id identity, the `NativeOpEntry` table, the intrinsic wire encoding,
the two dispatch tiers, the JIT calling convention, value marshalling, and the
per-op semantic guarantees.

Scope note: this is the **dispatch/ABI** boundary (compiler ↔ VM ↔ native
builtins). The process/host boundary (capabilities surface, isolates, I/O) is
`HOST_BOUNDARY_SPEC.md`. Authoring builtins (`.vn` + `varn_contract!`) is in
`LBI_ARCHITECTURE.md`.

---

## 1. Three layers

| Layer | Concern | Surface |
|---|---|---|
| **Authoring** | declare + implement a builtin | `.vn` contract + `varn_contract!` typed bodies |
| **Binding** | verify + assign identity | macro emits `trait` (drift = E0046/E0053) + `NativeOpEntry` with op-id |
| **Dispatch** | route a call at runtime | non-uniform, type-driven (op-id tier vs intrinsic tier) |

Authoring is uniform; dispatch is intentionally *not* — hot core ops bypass the
generic path. Keeping these separate is the design invariant the perf work
(typed wrappers, intrinsic inline) must preserve.

---

## 2. Op-id identity

Source: `crates/varn-core/src/op_id.rs`. An op-id is a build-stable FNV-1a hash
over fixed identity strings, so it is identical across builds/platforms and safe
to serialize into cached `.vnc` bytecode.

- Module-level symbol: `compound_op_id(module, symbol)` → hash of `module::symbol`.
- Class-qualified member: `compound_op_id3(module, class, symbol)` → hash of
  `module::class::symbol`. The extra segment guarantees these never collide with
  the 2-segment space.
- Core-type methods: `core_method_op_id(class, method) = compound_op_id3("globals", class, method)`.
  `CORE_MODULE = "globals"` — every core primitive class is registered under it.

`CORE_CLASSES` (12) is the single source of truth for "is this a core type with
a native method table": `Array, Str, Map, Set, Range, Symbol, Int, Float, Bool,
Char, Decimal, BigInt`. `core_class_name(tag)` / `core_class_tag(name)` round-trip
the canonical registration name (e.g. `"Symbol"`, not surface `"symbol"`); the
checker only emits a direct `CallNativeOp` when the receiver resolves through
this table, so an emitted op-id is guaranteed to resolve at runtime.

---

## 3. `NativeOpEntry` table

Source: `crates/varn-builtins/src/dispatch.rs`. Each `varn_contract!` member
emits one linker-section `NativeOpEntry` with: `id` (op-id), `name`,
`func: NativeFn`, `entry_kind: u8`, `namespace_path` (the `class` for class
members; empty for module-level ops), `module_id`, `capability`.

`NativeFn = fn(&mut dyn NativeCtx, &[VmValue]) -> Result<VmValue, String>`.

### entry_kind

| kind | meaning | placed on module object? |
|---|---|---|
| `0x01` | standalone function-module op | yes (`alloc_fn`) |
| `0x03` | instance method | no — op-id-addressable only |
| `0x04` | static method | no |
| `0x05` | getter | no |
| `0x06` | setter | no |
| `0x09` | static call (built into class) | resolved via `call_static` |
| `0x10` | class builder / module getter | invoked with `[]` |
| `0x11`–`0x15` | static-variant members (incl. `0x14` static-getter) | no |

`build_module` skips `0x03|0x04|0x05|0x06|0x11..=0x15` — those belong to a class
built by its `ClassDef`, not to the module object; they exist purely to be
op-id-addressable for direct dispatch.

### Resolution entry points

- `native_op_fn(id) -> Option<NativeFn>` — raw fn pointer; callers invoke it
  through their own native-call path (preserving their error + profiling
  semantics). This is what the VM/JIT `CallNativeOp` path uses.
- `dispatch_runtime_op(id, ctx, args)` — wrapped path: checks `capability` then
  calls `func`, wrapping failures as `E_RUNTIME_PERMISSION_DENIED` /
  `E_RUNTIME_FAILURE` / `E_RUNTIME_UNKNOWN_WIRE`.

---

## 4. Intrinsic wire encoding (hot tier)

Source: `crates/varn-core/src/intrinsic_ops/`. A wire byte is `DDDD_OOOO`:
high nibble = domain, low nibble = op. **Cap: 16 domains × 16 ops.**

```
encode(domain, op) = (domain << 4) | (op & 0x0F)
```

| Domain | id | ops |
|---|---|---|
| Math | `0x0` | Abs Sqrt Floor Ceil Round Sin Cos Tan Log Exp Pow Min Max (`0x0..0xC`) |
| String | `0x1` | Len Contains StartsWith EndsWith ToUpperCase ToLowerCase Trim |
| Array | `0x2` | Len Push Pop Contains |
| TypeCheck | `0x3` | (type predicates) |

Lookup: `intrinsic_ops::map::lookup(binding_key)` over `MAP_ENTRIES` keyed by
`"{origin}/{method}"` (e.g. `"std:math/pow"`). The 16-op cap is why long-tail
ops (e.g. `acos/asin/atan/atan2`) can NOT be intrinsics and must route through
op-id / a native — see `docs/superpowers/plans/2026-06-26-modules-remaining-gaps.md`.

---

## 5. The two dispatch tiers

### 5a. op-id (general) — `OpCode::CallNativeOp`

- Operands: `[op_id_const_idx: u16][arg_count: u16]`. The op-id is stored as a
  full i64 `PoolEntry` in `proto.chunk.constants` (the NaN-boxed
  `closure.constants` cache truncates u64, so the VM reads from
  `proto.chunk.constants`).
- Layout: receiver at the packed call_base, args above, result back to call_base.
- Lowered by the checker for statically-typed core instance-method calls
  (`arr.push(x)`, `s.slice(...)`) — bypasses string-name + inline-cache.
- VM: resolves `native_op_fn(id)` → calls via `call_native_with_receiver` (the
  exact IC native-call path; identical error + hotspot semantics).
- JIT: `emit_call_native_op` builds the `[receiver,args]` slice and calls the
  `jit_call_native_op` helper. (Typed register-passing is NOT yet implemented —
  the deep perf gap.)

### 5b. intrinsic (hot) — `OpCode::Intrinsic`

- Operand: `[wire_byte << 8 | arg_count]` (single u16 word).
- Annotated by the checker only when the receiver type is `Named(_, Some(origin))`
  (true for `Math`; false for structural `arr: T[]`).
- VM: `intrinsics::dispatch(wire, args, heap)` → per-domain dispatch.
- JIT (`emit_intrinsic`): **inlines** Math `abs`/`sqrt`/`floor` for float-typed
  args (`sqrtsd`, `roundsd`, sign-bit `and`); everything else falls through to
  the `dispatch_intrinsic` helper call.

---

## 6. JIT calling convention (native call)

Source: `crates/varn-jit/src/registers.rs`, `codegen/calls.rs`.

| Role | Windows | SysV (Linux/macOS) |
|---|---|---|
| `ARG_CTX` (arg0) | `Rcx` | `Rdi` |
| `ARG_CLOSURE` (arg1) | `Rdx` | `Rsi` |
| `ARG_BASE` (arg2) | `R8` | `Rdx` |
| `ARG_EXEC_CTX` (arg3) | `R9` | `Rcx` |
| `REG_FRAME_BASE` | `Rbp` | `Rbp` |

Sequence (both `emit_intrinsic` fallthrough and `emit_call_native_op`):
`emit_flush_all` → push `ARG_CTX/ARG_EXEC_CTX/ARG_BASE` → 16-byte stack align
(`need_align` push if `(used_phys.len()+4)` is odd) → load args
(`ARG_CTX=exec_ctx`, `ARG_CLOSURE=op_id/wire imm64`, `ARG_BASE+=dest`,
`ARG_EXEC_CTX=count`) → **Windows: reserve 32-byte shadow space** → call via
`R10` → restore shadow + pops → reload `REG_FRAME_BASE` → store `Rax` to dest →
`emit_reload_all_except(dest)`.

---

## 7. Value marshalling

Source: `crates/varn-types/src/marshal.rs`.

- `FromVm` (native arg): `VmValue, i64, f64, bool, String, char, Option<T>, VnArray`.
  `i64` rejects non-int; `f64` accepts int|float; `String`/`char` go through
  `ctx`.
- `IntoVm` (native return): `VmValue, i64, i32, usize, f64, bool, String, &str,
  char, (), Option<T>, Vec<VmValue>, VnArray`. `()` → `null`.
- `VnArray(VmValue)` is the zero-copy array handle (`len/get/set/push/pop/to_vec`
  via `ctx`), used so array params/returns avoid materializing a `Vec`.
- Class receiver type mapping (`receiver_mapped`): `str→&str`, `int→i64`,
  `Array→VnArray`, others→`VmValue`.

---

## 8. Signature & semantic contract

- First param is always `ctx: &mut dyn NativeCtx`. Then the declared params as
  marshalled types.
- **Fallibility:** function-module ops are fallible (`Result<T, String>`); class
  methods are infallible (`T`). The macro enforces this shape.
- **Receiver:** instance methods (`entry_kind 0x03`) take the receiver at
  `args[0]`. **Static** methods (`str.from`, `Array.isArray`) take NO receiver —
  `args[0]` is their first real param; the checker therefore excludes statics
  (and async/generator/getter/ctor) from op-id receiver-prepend
  (`core_has_method`).
- **Constructor semantics:** a native constructor returns `VmValue` — returning
  `null` keeps the VM-built instance; returning a fresh value replaces it (how
  `Set`/`Map` become their backing value).
- **Errors:** out-of-range/invalid input → `Err(String)`, surfaced as a Varn
  runtime error. `dispatch_runtime_op` prefixes `E_RUNTIME_FAILURE:id=…`.
- **Capabilities:** gated by `entry.capability` in `dispatch_runtime_op`.
  ⚠️ KNOWN BUG: the capability gate is currently effectively a no-op for runtime
  ops (cap strings don't match `cap_to_mask`; `build_table` sets
  `capability: None`). Tracked in memory, not yet fixed.
- **GC:** natives must respect GC roots when returning heap values; do not apply
  incorrect write barriers. Value core types (int/float/bool/null) need no GC.

---

## 9. Adding a builtin (checklist)

1. Declare in the module's `.vn` contract.
2. Implement the typed body in `varn_contract!` (`--features runtime` to compile).
3. op-id is auto-assigned; no manual wiring.
4. Hot core method? It is op-id-dispatched automatically once the class is in
   `CORE_CLASSES` and `core_has_method` passes. Promote to an intrinsic ONLY if
   it fits the 16-op domain cap AND the JIT can inline it.
5. Validate: `cargo check -p varn-builtins --features runtime`, then
   `vn run tests/main.vn` + `vn bench tests/main.vn`.
