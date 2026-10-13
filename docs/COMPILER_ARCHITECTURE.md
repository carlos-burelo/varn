# COMPILER_ARCHITECTURE — TIR → SSA → bytecode (sync código: 2026-10-13)

## TIR (contrato backend)

- `TirModule` en `crates/varn-tir/src/module.rs:119-135`: `source_file`, `imports`,
  `exports`, `types: TyTable`, `classes`, `class_origins`, `enums`, `signatures`,
  `functions`, `globals`, `global_names`, `class_defs`, `top_level: TirFunction`.
- `TirStmt` en `crates/varn-tir/src/node.rs:248-274`: `Expr`, `Let`, `Return`, `If`,
  `Loop`, `Break`, `Continue`, `Throw(TirExpr)`, `Try { body, catch_local, catch_body }`,
  `BuildClass`.
- `BackendTy` en `crates/varn-tir/src/ty.rs:57-80`: `Int/Float/Bool/Char/Str/Bytes/
  Decimal/BigInt`, `Array/Map/Dict/Set/Tuple/Class/Enum/Fn`, `Nullable/Void/Never/
  Dynamic(DynReason)`.

## from_tir (TIR → SSA interno)

`crates/varn-compiler/src/from_tir/build/` tiene 18 ficheros: assign, call,
class_def, classdef_methods, conform, construct, context, exports, expr_access,
expr_arith, expr_collect, expr_invoke, expr_lit, functions, globals, infra, ops,
stmt. Entrada: `compile_module` (`from_tir/compile.rs:112-117`), salida
`FunctionProto`, con `run_post_passes` de regalloc al final.

`try_inline_direct_call` (`from_tir/build/call.rs:62-138`) solo inserta funciones de
un único `return`: rechaza recursión, `is_async`/`is_generator`/`has_rest`/`has_this`,
`locals` no vacíos, cuerpo distinto de `[Return(Some(_))]`, aridad distinta o `spread`,
y retorno que sea `Closure`.

## Pases SSA (`crates/varn-compiler/src/passes/mod.rs:16-51`)

`optimize` itera hasta punto fijo (máx 100) en este orden: `const_fold`,
`redundant_guards`, `monomorphize`, `algebraic`, `cse`, `fixed_fields`, `escape`,
`licm`, `dce`, `cfg::simplify_and_compact`; luego `recompute_preds` y un
`cse::run_global` final seguido de `dce`. No existe pase de inlining a nivel SSA
(el único inlining es el de `from_tir` anterior).

`escape` (`passes/escape.rs:11-60`) es solo scalar-replacement de `AllocInstance`:
registra el bloque de cada sitio y sus `SetFixedField`; si todos los stores están en
el mismo bloque, en orden y sin lecturas previas, y el objeto nunca escapa (usos,
retornos, throws, args de salto), reescribe los `GetFixedField` por los valores
guardados y borra alloc + accesos.

## SSA portable (`varn-types/src/ssa/`)

- `SsaProto` (`mod.rs:58-71`): `name`, `nparams`, `entry`, `blocks`, `values`, `regs`,
  `register_count`, `has_this`, `captured`, `secondary_entries`.
- `SsaOp::Try { catch_ip, catch_value, live }`, `SsaOp::PopTry`,
  `SsaOp::CatchParam { try_val }` (`op.rs:70-80`).
- `SsaTerm::Throw(u32)` junto a `Return/Jump/Branch/Unreachable` (`mod.rs:40-55`).
- `SsaEntryKind::Loop/Resume/Catch` + `SsaSecondaryEntry { block, ip, live, kind }`
  (`mod.rs:74-88`); `catch_at`/`secondary_at` resuelven entradas por `ip`.
- SSA interno (`crates/varn-compiler/src/ssa/ir/ops.rs:232-238`):
  `InstKind::Try { handler: BlockId }`, `PopTry`, `CatchParam { try_val: Value }`;
  terminador `Throw(Value)` (`ssa/ir/control.rs:7`).

## Tipos HIR y rangos de excepción

- `HirType` (`crates/varn-compiler/src/hir/mod.rs:10-26`): tipo interno del lowering
  (`Int/Float/Bool/Str/Ref/Dynamic/Array/Map/Set/Tuple/Class/Nullable`), distinto de
  `BackendTy` del TIR.
- `ExceptionRange { try_start_ip, try_end_ip, catch_ip, err_reg }`
  (`crates/varn-types/src/chunk/proto/records.rs:8-13`): rango serializado en el chunk
  que el emisor deriva de `Try`/`PopTry` y el VM usa en `Throw`.
