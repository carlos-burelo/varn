# JIT ABI v2 — spec vigente

Contrato entre `varn-jit` (Cranelift) y `varn-vm`. Código manda; rutas abajo
verificadas contra fuentes.

## 1. Versión y helpers

- `JIT_HELPERS_VERSION: u32 = 9` (`crates/varn-jit-abi/src/lib.rs`).
- `JitHelpers` (`lib.rs`) es tabla de direcciones de helpers + layouts +
  offsets al `ExecCtx`: `jit_native_result_offset`, `jit_error_flag_offset`,
  `jit_exit_offset`, `globals_offset`, `globals_store_offset`,
  `young_len_offset`, `young_threshold`, más `frame_layout: JitFrameLayout`
  (`gpr/fpr/refs/dyn/allocs_ptr_offset`, `alloc_bases_offset`) y
  `call_layout: JitCallLayout` (offsets de `CallFrame`: `closure_ptr`, `ip`,
  `base`, `return_reg`, `NO_ACTIVATION`, `MAX_CALL_DEPTH`).

## 2. Activaciones: Native vs Framed

`Activation { Native, Framed }` (`varn-jit/src/clif/abi.rs`).

- `Native`: firma por `NativeShape::signature()` con `CALL_CONV = CallConv::Tail`
  (`clif/native_abi.rs`); args/valores nativos por clase
  (`NativeClass`: Int|Bool→Word `i64`, Float→`f64`, Str|Ref|Dynamic→Boxed par `i64×2`).
  Entrada nativa registrada en `ClifArtifact::native_entry` salvo `has_rest`,
  `is_async` o `is_generator` (`clif/lower.rs`).
- `Framed`: firma `raw_signature` (4×`i64` de contexto + params; `f64` si el slot
  es Float) (`clif/abi.rs`); en Windows la envoltura retorna por `sret`
  (`wrapper_returns_via_sret`).

Heurística frameless (`clif/from_ssa/setup.rs::prepare`): sin valores heap, params y
retorno escalares (`Int|Float|Bool`), sin `this`, sin upvalues, sin bloque que
necesite frame ni `SsaTerm::Throw`. `frame_aware = Framed || !frameless`.
`NATIVE_FRAMELESS: u64 = 1 << 63` marca el bit 63 del id de firma nativa
(`clif/native_abi.rs`).

Orden de intento (`clif/lower.rs`): primero `try_lower(Native)`; si falla
con `NEEDS_ACTIVATION` (`from_ssa/context.rs`: entrada OSR, o un op que necesita
homes —vivos de `try`, `await`, `yield`, upvalues—), reintenta con `Framed`. Cualquier otro error es bail.

Entrada directa JIT→JIT (`varn-vm/src/exec/jit_helpers/`): tras empujar el frame
del callee, `calls_static.rs::direct_entry` devuelve su código compilado salvo que
sea `is_async || is_generator` (la llamada solo crea la tarea/generador); `try`
ya no veta. `await f(x)` fusionado (`suspend.rs::mounted_entry`) entra directo a
todo callee async con código: todo frame nativo entre el `setjmp` del dispatch y
el que suspende está detenido en un punto de suspensión (ip = `resume_ip`, vivos
en homes), así que el `longjmp` deja `frames` coherente.

## 3. Gates y tiering

- `SIZE_GATE_WORDS: usize = 8192` (`varn-jit/src/lib.rs`). `gate_reason`
  rechaza protos de más de 8192 words salvo leaf-safe (sin alloc, sync, no
  generador) (`clif/lower.rs`).
- Umbrales (`varn-vm/src/jit/tiering.rs`): `JIT_TIER_THRESHOLD = 1` (con
  backedge usa `VARN_JIT_TIER`), `JIT_TIER_THRESHOLD_STRAIGHT = 128` (sin
  backedge), `JIT_OSR_BACKEDGES = 1000` (`VARN_JIT_OSR`).

## 4. Try nativo (sin longjmp/personality en varn-jit)

`grep longjmp|personality crates/varn-jit` vacío; `riscv` ausente (solo host
vía `cranelift_native`: `x86_64`/`aarch64` en `mem/buffer.rs`, `clif/mod.rs`).

- `emit_try` (`from_ssa/exceptions.rs`): vuelca vivos a homes (por eso un
  `try` con vivos exige `Framed`; pendiente P2b del plan), registra el
  landing en `pure_try.stack`. Puro intra-función: `emit_pop_try` lo desapila.
- `emit_throw`/`emit_throw_boxed`: con landing salta directo con el
  boxed; sin landing guarda en `jit_native_result_offset`, pone
  `jit_error_flag = 1` y salta a `err_ret`.
- `check_error_branch` tras cada helper: si hay landing carga el
  boxed de `jit_native_result`, limpia el flag y salta; si solo hay `err_ret`,
  bifurca a salida de error. `call_checked`/`call_checked_value`
  = limpiar flag + llamar helper + chequear.

Bails reales de try en `setup.rs`: handler compartido por dos valores
(`landing_maps`, también `exceptions.rs`), catch block con params
(`create_blocks`), try-stacks inconsistentes (`reachability`).

## 5. AOT stub (`varn-jit/src/aot/mod.rs`)

Solo prototipado, no backend general. Emite: `LoadInt`/`Zero`/`One`,
`LoadConst` (str/int/bool), `LoadTrue/False/Null`, `Move`, `Add/Sub/Mul/Mod`
(`iadd/isub/imul/srem`), `AddImm` (`iadd_imm`), `StrConcat` (helper
`varn_rt_str_concat`), `Call` solo con `argc == 2` redirigido a `varn_rt_print`,
`Return` termina el cuerpo. Todo lo demás se ignora (`=> {}`) o es
`unreachable!` en el brazo exhaustivo.

## 6. Archivos que mandan

`crates/varn-jit-abi/src/lib.rs` (versión + `JitHelpers` + layouts),
`varn-jit/src/clif/abi.rs`, `clif/native_abi.rs`, `clif/from_ssa/drive.rs`,
`clif/from_ssa/context.rs`, `clif/from_ssa/exceptions.rs`, `clif/lower.rs`,
`varn-jit/src/lib.rs`, `varn-vm/src/jit/tiering.rs`,
`varn-vm/src/exec/dispatch/jit_frame.rs`.
