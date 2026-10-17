# Especificación de la ABI JIT/Nativa

Contrato vigente entre código generado (`varn-jit`, Cranelift) y VM (`varn-vm`).
Detalle completo en `docs/JIT_ABI_V2_SPEC.md`; aquí el resumen normativo.

## 1. Versión

`JIT_HELPERS_VERSION = 9` (`crates/varn-jit-abi/src/lib.rs`). `JitHelpers`
(`lib.rs`): direcciones de helpers + layouts + offsets al `ExecCtx`
(`jit_native_result_offset`, `jit_error_flag_offset`, `jit_exit_offset`,
`globals_offset`, `young_len_offset/threshold`, `frame_layout`, `call_layout`).

## 2. Convención de llamada JIT

Dos activaciones (`varn-jit/src/clif/abi.rs`): `Native` (firma
`NativeShape`, `CallConv::Tail`, `Int|Bool→i64`, `Float→f64`, resto boxed
`i64×2`; `NATIVE_FRAMELESS = 1<<63`) y `Framed` (`raw_signature`, `sret` en
Windows). Se intenta `Native` y se cae a `Framed` con `NEEDS_ACTIVATION`
(`clif/lower.rs`). Gate: 8192 words (`SIZE_GATE_WORDS`); tiering
1 / 128 recto / OSR 1000 (`varn-vm/src/jit/tiering.rs`).

## 3. Valores y registros FFI

`VmValue` = 16 B (`tag: u64, payload: u64`); kinds 0..6
(NULL/BOOL/INT/FLOAT/HEAP/SSO/SYMBOL), SSO hasta 8 B. Los frames separan
registros por clase: GPR `i64`, FPR `f64`, Refs `HeapRef`, Dyn `VmValue`
(`docs/VM_ARCHITECTURE.md` §1-2). Las nativas Rust tienen tipo `NativeFn = fn(&mut dyn NativeCtx, &[VmValue]) -> NativeFnResult`
(`varn-types/src/native.rs`); se generan desde el contrato `.vn` con `varn_contract!`
(ver `LBI_ARCHITECTURE.md`).

## 4. Errores y plataforma

Try nativo sin `longjmp`/`personality` en `varn-jit`: flag
`jit_error_flag` + boxed en `jit_native_result` (`from_ssa/exceptions.rs`).
El `setjmp`/`longjmp` vive solo en `varn-vm/src/arch/` para la frontera
JIT↔intérprete. Plataformas: host `x86_64`/`aarch64` vía `cranelift_native`;
no existe backend RISC-V.
