# JIT ABI v2 — spec vigente

Contrato entre `varn-jit` (Cranelift) y `varn-vm`. Código manda; rutas abajo
verificadas contra fuentes.

## 1. Versión y helpers

- `JIT_HELPERS_VERSION: u32 = 3` (`crates/varn-jit-abi/src/lib.rs:3`).
- `JitHelpers` (`lib.rs:118-246`) es tabla de direcciones de helpers + layouts +
  offsets al `ExecCtx`: `jit_native_result_offset`, `jit_error_flag_offset`,
  `jit_exit_offset`, `globals_offset`, `globals_store_offset`,
  `young_len_offset`, `young_threshold`, más `frame_layout: JitFrameLayout`
  (`gpr/fpr/refs/dyn/allocs_ptr_offset`, `alloc_bases_offset`) y
  `call_layout: JitCallLayout` (offsets de `CallFrame`: `closure_ptr`, `ip`,
  `base`, `return_reg`, `NO_ACTIVATION`, `MAX_CALL_DEPTH`).

## 2. Activaciones: Native vs Framed

`Activation { Native, Framed }` (`varn-jit/src/clif/abi.rs:16-19`).

- `Native`: firma por `NativeShape::signature()` con `CALL_CONV = CallConv::Tail`
  (`clif/native_abi.rs:41`); args/valores nativos por clase
  (`NativeClass`: Int|Bool→Word `i64`, Float→`f64`, Str|Ref|Dynamic→Boxed par `i64×2`).
  Entrada nativa registrada en `ClifArtifact::native_entry` salvo `has_rest`,
  `is_async` o `is_generator` (`clif/lower.rs:21-40`).
- `Framed`: firma `raw_signature` (4×`i64` de contexto + params; `f64` si el slot
  es Float) (`clif/abi.rs:21-41`); en Windows la envoltura retorna por `sret`
  (`wrapper_returns_via_sret`).

Heurística frameless (`clif/from_ssa/drive.rs:52-62`): sin valores heap, params y
retorno escalares (`Int|Float|Bool`), sin `this`, sin upvalues, sin bloque que
necesite frame ni `SsaTerm::Throw`. `frame_aware = Framed || !frameless`.
`NATIVE_FRAMELESS: u64 = 1 << 63` marca el bit 63 del id de firma nativa
(`clif/native_abi.rs:76`).

Orden de intento (`clif/lower.rs:158-186`): primero `try_lower(Native)`; si falla
con `NEEDS_ACTIVATION` (`from_ssa/context.rs:63`, también cuando hay OSR o
`resumes_in_interpreter`), reintenta con `Framed`. Cualquier otro error es bail.

## 3. Gates y tiering

- `SIZE_GATE_WORDS: usize = 8192` (`varn-jit/src/lib.rs:163`). `gate_reason`
  rechaza protos de más de 8192 words salvo leaf-safe (sin alloc, sync, no
  generador) (`clif/lower.rs:93-106`).
- Umbrales (`varn-vm/src/jit/tiering.rs:7-11`): `JIT_TIER_THRESHOLD = 1` (con
  backedge usa `VARN_JIT_TIER`), `JIT_TIER_THRESHOLD_STRAIGHT = 128` (sin
  backedge), `JIT_OSR_BACKEDGES = 1000` (`VARN_JIT_OSR`).

## 4. Try nativo (sin longjmp/personality en varn-jit)

`grep longjmp|personality crates/varn-jit` vacío; `riscv` ausente (solo host
vía `cranelift_native`: `x86_64`/`aarch64` en `mem/buffer.rs`, `clif/mod.rs:50-53`).

- `emit_try` (`from_ssa/exceptions.rs:33-64`): vuelca vivos a homes, registra el
  landing en `pure_try.stack`. Puro intra-función: `emit_pop_try` lo desapila.
- `emit_throw`/`emit_throw_boxed` (`:75-114`): con landing salta directo con el
  boxed; sin landing guarda en `jit_native_result_offset`, pone
  `jit_error_flag = 1` y salta a `err_ret`.
- `check_error_branch` (`:138-184`) tras cada helper: si hay landing carga el
  boxed de `jit_native_result`, limpia el flag y salta; si solo hay `err_ret`,
  bifurca a salida de error. `call_checked`/`call_checked_value` (`:186-207`)
  = limpiar flag + llamar helper + chequear.

Bails reales de try en `drive.rs`: handler compartido por dos valores
(`:105`, también `exceptions.rs:54-57`), catch block con params (`:131`),
try-stacks inconsistentes (`:156`).

## 5. AOT stub (`varn-jit/src/aot/mod.rs`)

Solo prototipado, no backend general. Emite (`:137-421`): `LoadInt`/`Zero`/`One`,
`LoadConst` (str/int/bool), `LoadTrue/False/Null`, `Move`, `Add/Sub/Mul/Mod`
(`iadd/isub/imul/srem`), `AddImm` (`iadd_imm`), `StrConcat` (helper
`varn_rt_str_concat`), `Call` solo con `argc == 2` redirigido a `varn_rt_print`,
`Return` termina el cuerpo. Todo lo demás se ignora (`=> {}`) o es
`unreachable!` en el brazo exhaustivo (`:423-551`).

## 6. Archivos que mandan

`crates/varn-jit-abi/src/lib.rs` (versión + `JitHelpers` + layouts),
`varn-jit/src/clif/abi.rs`, `clif/native_abi.rs`, `clif/from_ssa/drive.rs`,
`clif/from_ssa/context.rs`, `clif/from_ssa/exceptions.rs`, `clif/lower.rs`,
`varn-jit/src/lib.rs:163`, `varn-vm/src/jit/tiering.rs`,
`varn-vm/src/exec/dispatch/jit_frame.rs`.
