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

### Fase A — regiones de módulo — HECHA (`6ff813d1`, `<linker fix>`)

- `FunctionProto.global_count: u32` (append, `#[serde(default)]`).
- `VmClosure.module_base: u32` + `varn_types::Closure.module_base`. Fijado al
  evaluar el módulo (`eval_module_proto`, `vm.rs`), heredado por cada closure
  anidada (`MakeClosure`, `LoadStaticFn`, fork de generador y de task).
- `GlobalStore::reserve_region(count) -> base`.
- Intérprete `LoadGlobalIdx`/`StoreGlobalIdx`/`DefineGlobalIdx`: `+ module_base`.
- JIT `clif/globals.rs`: carga `module_base` del parámetro closure y lo suma.
  El acceso a global ya fuerza el lowering frame-aware, así que la closure
  siempre está en scope.
- `from_tir`: `Resolution::GlobalSlot(k)` → `InstKind::LoadGlobalIdx(k)` /
  `StoreGlobalIdx { slot: k }`; `global_load` / `global_store` con mapa
  nombre→slot de `tir.global_names` para imports / clases / exports / free-fns.
- `K::Global` (link estático de llamadas): `CtxLinker` lleva el `module_base`
  del proto que compila (`CtxLinker::for_module`) y lo suma antes de indexar
  el store. Restaurado.
- El pase (`globals/resolve.rs`) queda reducido a una cosa: `LoadGlobal` de un
  símbolo de prelude por nombre → nuevo opcode `LoadNativeGlobalIdx` (absoluto,
  sin base). Un nombre genuinamente dinámico se queda name-keyed.

Esto es la parte del objetivo inviolable que tocaba a los globales: la
re-derivación en runtime de los globales de MÓDULO — el grueso — desapareció.

### Fase B — el pase muere del todo — HECHA (`bf550a5d`)

- `varn_builtins::native_global_layout() -> &[&'static str]` — el orden exacto
  que construye `with_native_layout` (`["print","assert"]` ++ resto ordenado,
  del set `{isIsolate, core} ∪ campos del módulo globals`).
  `with_native_layout` lo consume para el ORDEN y `register_globals_vm` sólo
  para los valores — misma fuente, imposible que difieran.
- `Resolution::NativeGlobal(u32)` + `InstKind::LoadNativeGlobalIdx(u32)`.
- checker `resolve_name`: antes de `ByName`, `native_global_index(name)` →
  `Resolution::NativeGlobal(idx)`. `Call` con callee `NativeGlobal` propaga la
  resolución.
- Miembros de `extension` (`__ext*`) numerados como globales de módulo
  (`collect_extension_names`) → `InstKind::ExtensionCall` gana `slot: Option<u32>`
  → `LoadGlobalIdx` en vez de nombre. Una `x.extMethod()` caliente ya compila
  a CLIF (antes bailaba).
- BORRADOS: `globals/resolve.rs` + sus 3 call sites (`Vm::resolve_globals`,
  `resolve_shared` en `eval_module_proto`, `resolve_in_proto` en el fork de
  task), `FunctionProto.globals_id`, `GlobalStore::id` + `next_store_id` + el
  baile de pre-resolución del bench, la dep `varn-vm` de `varn-debug`
  (`resolved_copy` ahora es clon a secas).
- `varn-checker` gana dep de `varn-builtins` (feature-unificada bajo
  `--workspace`).

### Fase C — `CallIntrinsic` — HECHA (`b60b88e7`)

`import { abs, sqrt, floor, ceil } from "std:math"` → `IntrinsicCall` /
`IntrinsicDirect` (una instrucción ISA en el JIT, sin cruzar la frontera FFI).
`math_intrinsic_imports` mapea nombre local → wire byte; la ligadura del
import descarta un shadow del usuario.

## Control (cada fase)

- 321/321 `tests/*.vn` byte-idénticos ×3 tiers (JIT / no-JIT × @embedded / std-dev)
- `tests/main.vn` → PASSED 1180 ×3
- `run --compare-tiers` sin desacuerdos sobre el corpus
- isolates: `tests/33-globals-async-coherence.vn`, `tests/main.vn` (spawnIsolate)
- `.vnc`: los índices ahora son estables por-módulo → mejora la caché;
  verificar que un `vn cache clean` + re-run coincide
- `cargo test --workspace`
