# Compilador: TIR → SSA → bytecode

Entrada: `TirModule` (contrato en [`TIR_CONTRATO_TIPADO.md`](TIR_CONTRATO_TIPADO.md)).
Salida: `FunctionProto` con bytecode de registros, metadatos de registros y SSA
portable para el JIT. Todo vive en `crates/vn-compiler/src/`.

## Flujo (`from_tir/compile.rs`)

`compile_module(tir, export_names, source, measure)`:

1. `vn_tir::verify_module` — rechaza TIR inválido (`OptError::InvalidTir`) antes de bajar nada.
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

## Inlining

Tres mecanismos dentro de un módulo, todos en `from_tir/build/`:

- `try_inline_direct_call` (`call.rs`): funciones cuyo cuerpo es un único `return`;
  rechaza recursión, async/generador, rest, `this`, locales, aridad distinta, spread y
  retorno de closure.
- `try_inline_body_call` (`inline_body.rs`): cuerpos generales de funciones libres,
  hasta 96 nodos (`@inline` fuerza). Rechaza closures, `try`, `await`/`yield`, `super`,
  upvalues y parámetros con valor por defecto.
- `try_inline_method_call` (`inline_method.rs`): métodos cuyo receptor tiene clase
  estática conocida (`devirtualized_method`); profundidad máxima 4.

El predicado de elegibilidad es único y vive en `vn-tir` (`inlinable::body_inlinable`):
el compilador y la extracción de plantillas entre módulos lo comparten.

## Enlace entre módulos (`vn-tir`)

Antes de `compile_module`, `lower_module` (`vn-pipeline/src/lower.rs`) ejecuta
`vn_tir::link::link` sobre el TIR del importador:

1. `vn_tir::package::extract` produce, por módulo ya bajado, `ModuleTemplates`: un
   `InlinePackage` por función exportada elegible (la función y las hermanas que llama
   directamente, con sus tipos re-internados en una `TyTable` portable), las
   constantes exportadas con valor literal y la bandera `inert`.
2. `link` instala cada paquete usado en el módulo importador (funciones con global
   `__inl<huella>::nombre`, tipos y firmas remapeados con `TyTable::import_ty`),
   reescribe la llamada a `DirectFn` y deja la decisión de inlinar al compilador.
3. Una lectura de constante importada se reemplaza por su literal.
4. Si el módulo es inerte (su nivel superior solo asigna literales; sin clases, imports
   relativos ni reexports) y todos sus especificadores quedaron enlazados y sin
   referencias, el import se elimina y el módulo no se carga.

Un paquete solo contiene tipos sin ids locales (escalares, `str`, `Bytes`, decimal,
bigint, arrays, mapas, sets, tuplas y nulables de ellos), resoluciones `Local`, `Param`,
`Intrinsic`, `NativeOp` (con al menos un argumento), `NativeDecode` y llamadas directas a
hermanas. Clases, enums, tipos función, genéricos, `this`, closures y estado de módulo no
constante quedan fuera. Los módulos de `std` publican sus plantillas en `std.vnb`
(`MODULES_SYSTEM.md`).

## Pases SSA (`passes/mod.rs::optimize`)

Hasta punto fijo (máx. 100 iteraciones), en este orden: `const_fold`,
`redundant_guards`, `monomorphize`, `algebraic`, `cse`, `fixed_fields`, `escape`,
`licm`, `dce`, `cfg::simplify_and_compact`. Después: `recompute_preds` y un
`cse::run_global` final seguido de `dce` si cambió algo.

`escape` es scalar-replacement de `AllocInstance`: si todos los stores están en el
mismo bloque, en orden, sin lecturas previas, y el objeto no escapa, reescribe los
`GetFixedField` por los valores guardados y borra la asignación.

## SSA portable (`vn-types/src/ssa/`)

Lo que el JIT consume. `SsaProto`: `name`, `nparams`, `entry`, `blocks`, `values`,
`regs`, `register_count`, `has_this`, `captured`, `secondary_entries`.

- Excepciones: `SsaOp::Try { catch_ip, catch_value, live }`, `SsaOp::PopTry`,
  `SsaOp::CatchParam { try_val }`, terminador `SsaTerm::Throw`.
- Entradas secundarias: `SsaEntryKind::{Loop, Resume, Catch}` con
  `SsaSecondaryEntry { block, ip, live, kind }`; `catch_at` / `secondary_at` las
  buscan por `ip` (OSR en bucles, reanudación tras `await`/`yield`, landing de catch).

En el bytecode, `ExceptionRange { try_start_ip, try_end_ip, catch_ip, err_reg }`
(`vn-types/src/chunk/proto/records.rs`) describe cada `try` para la VM.
