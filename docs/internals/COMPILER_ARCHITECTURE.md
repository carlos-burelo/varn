# Compilador: TIR → SSA → bytecode

Entrada: `TirModule` (contrato en [`TIR_CONTRATO_TIPADO.md`](TIR_CONTRATO_TIPADO.md)).
Salida: `FunctionProto` con bytecode de registros, metadatos de registros y SSA
portable para el JIT. Todo vive en `crates/varn-compiler/src/`.

## Flujo (`from_tir/compile.rs`)

`compile_module(tir, export_names, source, measure)`:

1. `varn_tir::verify_module` — rechaza TIR inválido (`OptError::InvalidTir`) antes de bajar nada.
2. Por función (`compile_one`, top-level y cada closure vía `compile_closure`):
   1. `build_top_level` / `build_function` (`from_tir/build/`) — TIR → SSA interno.
   2. `passes::optimize` — pases a punto fijo.
   3. `passes::state_machine::run` — solo async/generadores: analiza puntos de suspensión (`ssa/suspend.rs`) y transforma la función; devuelve `state_size`.
   4. `ssa::verify::verify` — SSA inválido es error interno, no se emite.
   5. `emit_function_meta` (`ssa/emit/`) — SSA → bytecode + `FunctionProto` (contrato de parámetros para funciones no top-level).
3. `regalloc::run_post_passes` sobre el proto final.

## from_tir: TIR → SSA interno

`from_tir/build/`, un archivo por dominio: `assign`, `call`, `class_def`,
`classdef_methods`, `conform`, `construct`, `context`, `exports`, `expr_access`,
`expr_arith`, `expr_collect`, `expr_invoke`, `expr_lit`, `functions`, `globals`,
`infra`, `ops`, `stmt`.

Tipo interno del lowering: `HirType` (`hir/mod.rs`: `Int/Float/Bool/Str/Ref/Dynamic/
Array/Map/Set/Tuple/Class/Nullable`), distinto de `BackendTy`.

Único inlining: `try_inline_direct_call` (`from_tir/build/call.rs`). Solo inserta
funciones cuyo cuerpo es un único `return`; rechaza recursión, async/generador,
rest, `this`, locales, aridad distinta, spread y retorno de closure.

## Pases SSA (`passes/mod.rs::optimize`)

Hasta punto fijo (máx. 100 iteraciones), en este orden: `const_fold`,
`redundant_guards`, `monomorphize`, `algebraic`, `cse`, `fixed_fields`, `escape`,
`licm`, `dce`, `cfg::simplify_and_compact`. Después: `recompute_preds` y un
`cse::run_global` final seguido de `dce` si cambió algo.

`escape` es scalar-replacement de `AllocInstance`: si todos los stores están en el
mismo bloque, en orden, sin lecturas previas, y el objeto no escapa, reescribe los
`GetFixedField` por los valores guardados y borra la asignación.

## SSA portable (`varn-types/src/ssa/`)

Lo que el JIT consume. `SsaProto`: `name`, `nparams`, `entry`, `blocks`, `values`,
`regs`, `register_count`, `has_this`, `captured`, `secondary_entries`.

- Excepciones: `SsaOp::Try { catch_ip, catch_value, live }`, `SsaOp::PopTry`,
  `SsaOp::CatchParam { try_val }`, terminador `SsaTerm::Throw`.
- Entradas secundarias: `SsaEntryKind::{Loop, Resume, Catch}` con
  `SsaSecondaryEntry { block, ip, live, kind }`; `catch_at` / `secondary_at` las
  buscan por `ip` (OSR en bucles, reanudación tras `await`/`yield`, landing de catch).

En el bytecode, `ExceptionRange { try_start_ip, try_end_ip, catch_ip, err_reg }`
(`varn-types/src/chunk/proto/records.rs`) describe cada `try` para la VM.
