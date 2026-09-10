# Etapa 4 — `LoadGlobalIdx` directo, sin pase de reescritura

## El problema

`globals/resolve.rs` recorre el bytecode de cada proto, decodifica cada
instrucción y reescribe `LoadGlobal name` → `LoadGlobalIdx slot`, definiendo
slots en el `GlobalStore` según los ve. Es una re-derivación en runtime de lo
que el checker ya probó (`Resolution::GlobalSlot(k)`). Va contra el objetivo
inviolable: sintaxis de alto nivel, rendimiento de bajo nivel — el compilador
no debe salir ignorante de algo que sabía.

Por qué no se podía emitir el índice directo: el `GlobalStore` es uno por VM,
plano, y arranca con ~cientos de globales nativos (`print`, `assert`, builtins
ordenados) antes de los del módulo, más los de cada import, más `isIsolate`
primero en workers de isolate. El slot absoluto no es conocible en compilación.

## La solución: regiones de globales por módulo

Los nombres de globales de módulo van calificados por archivo (`m.vn::f`), así
que los módulos ya no colisionan en el store plano. En vez de descubrir el
slot recorriendo bytecode:

- **`eval_module_proto`** reserva la región del módulo de una:
  `let base = globals.len(); globals.resize(base + proto.global_count, null)`.
  O(1), determinista, sin conocer otros módulos.
- El módulo emite `LoadGlobalIdx(k)` / `StoreGlobalIdx(k)` con el `k` del
  checker (`GlobalSlot(k)`, 0-based dentro del módulo).
- **`VmClosure.module_base`**: capturado al construir la closure. Las closures
  anidadas lo heredan del frame creador en `MakeClosure`. El `CallFrame` lo lee
  de su closure; el intérprete hace `values[module_base + k]`.
- **JIT**: `module_base` es constante en tiempo de compilación del proto (el
  módulo ya cargó cuando el proto se vuelve caliente) → se hornea
  `(module_base + k) * 16`. Cero overhead. Re-JIT si el proto se re-liga a
  otro store — la infra ya existe (`resolve_in_proto` limpiaba `jit_entry`).
- **Nativos / prelude** (`print`, `isIsolate`): región fija al frente del
  store. Manifiesto `NATIVE_GLOBALS` compartido en `varn-core` (como `op_id`).
  El compilador conoce esos índices → opcode propio `LoadNativeGlobalIdx` que
  NO suma base.

## Fases

### Fase A — regiones de módulo (mata el pase para el caso caliente)

1. `FunctionProto.global_count: u32` (append, `#[serde(default)]`; bumpea
   `BUILD_FINGERPRINT`).
2. `VmClosure.module_base: u32`. `build_closure` lo fija para el top-level;
   `MakeClosure` lo copia del frame creador.
3. `CallFrame` lo lee de la closure. `ExecCtx` cachea `cur_module_base` por
   frame activo (para el JIT).
4. `eval_module_proto` / `ctx_tasks` / `vm.rs`: `globals.reserve_region(n)`.
5. `globals/resolve.rs`: emitir índices RELATIVOS (`slot - base`) mientras siga
   existiendo — deja ambos caminos (pase viejo, compilador nuevo) coherentes.
6. Intérprete `*Idx` (`ops_literals_vars.rs`): `base + idx`.
7. JIT `clif/globals.rs` + `GblCtx`: sumar `module_base` (hornear constante).
8. `from_tir` + `ssa/ir.rs` + `ssa/emit`: `Resolution::GlobalSlot(k)` →
   `InstKind::LoadGlobalIdx { slot: k }` directo; mapa nombre→slot desde
   `tir.global_names` para `build_imports` / `build_class_def` /
   `build_exports` / registro de free-fns.
9. El pase deja de tocar `LoadGlobal` de módulo (ya llegan `*Idx`); sólo
   quedan los `ByName` de prelude → siguen name-keyed (hash, correcto; el JIT
   baila hasta la Fase B).

### Fase B — manifiesto nativo, borrar el pase

10. `varn-core::NATIVE_GLOBALS` generado de la misma fuente que
    `varn-builtins` registra. `with_native_layout` lo sigue exacto.
    `isIsolate` en un slot fijo del manifiesto.
11. `LoadNativeGlobalIdx` / `StoreNativeGlobalIdx` (sin base).
12. checker: `ByName` de prelude conocido → índice de región nativa.
13. Borrar `globals/resolve.rs`, sus 3 call sites, `FunctionProto.globals_id`,
    el arm `LoadGlobal | StoreGlobal | DefineGlobal` de `exec_variable_op`
    (o dejarlo sólo para `dynamic` de verdad).

## Control (cada fase)

- 321/321 `tests/*.vn` byte-idénticos ×3 tiers (JIT / no-JIT × @embedded / std-dev)
- `tests/main.vn` → PASSED 1180 ×3
- `run --compare-tiers` sin desacuerdos sobre el corpus
- isolates: `tests/33-globals-async-coherence.vn`, `tests/main.vn` (spawnIsolate)
- `.vnc`: los índices ahora son estables por-módulo → mejora la caché;
  verificar que un `vn cache clean` + re-run coincide
- `cargo test --workspace`
