# LBI: nativos linker-bound

Código manda. Fuentes: `crates/varn-contract/src/lib.rs:8`, `crates/varn-contract/src/varn_contract/mod.rs:16-184`, `crates/varn-builtins/src/dispatch/mod.rs:10-97`, `crates/varn-builtins/src/modules/mod.rs:1-113`, `crates/varn-op-macros/src/lib.rs:6-13`.

## Piezas reales

1. `varn_contract` proc-macro (`varn-contract/src/lib.rs:8`). Expande (`varn_contract/mod.rs:16-37`): contrato debe ser path `.vn` listado en `contracts.json`; inline rechazado con error. Lee miembros vía `tables::lookup` (error pide `vn gen-contract-tables` si falta, `mod.rs:40-62`).
2. Genera trait `__VarnContract_<mod>` + wrappers + tabla `__VARN_LINK_MARKER_<MOD>` (`mod.rs:160-181`), más `include_bytes!` del contrato para trazabilidad.
3. Cada entry `NativeOpEntry` en sección linker (`mod.rs:125-144`, `dispatch/mod.rs:26-34`): Windows `.varn_ops$B` (marcadores `$A`/`$C`), macOS `__DATA,varn_ops`, resto `varn_ops`.
4. Boot: `iter_native_ops()` escanea sección y filtra `func_ptr` nulo (`dispatch/mod.rs:36-60`); `all_native_ops()` suma fallback registrados (`62-97`).
5. Fallback link: `force_link_builtins()` (`modules/mod.rs:60-113`) registra marcadores por módulo para que linker no descarte.
6. `varn-op-macros` (`op-macros/src/lib.rs:6-13`): solo `#[jit_slow]` y `jit_helper_table!` (helpers JIT, no registro módulos).

## Módulos nativos (`modules/mod.rs:1-47`)

23 `pub mod`: `buffer compress crypto csv errors ffi fs globals io json math net path process reflect regex sqlite sys task testing time types ws` (+`core/types/*`: array bigint bytes bool char decimal float int map range set string).

Entrada: `build_module(id, ctx)` → `dispatch::build_module` (`mod.rs:51-53`); `has_native_builder` chequea id.

## Dispatch (`dispatch/mod.rs:99-153`)

`entry_op_id` = hash compuesto (módulo, namespace, símbolo). `TABLE: OnceLock<FxHashMap<u64, DispatchEntry>>`, `native_op_fn(id)`, `dispatch_runtime_op` envuelve error como `E_RUNTIME_*`.

## Capabilities

Fijas en código, no en proc-macro (`dispatch/mod.rs:122-131`): solo 3 mapeos existen:

- `runtime:ffi:*` → `sys.ffi`
- `runtime:sys env|setEnv` → `sys.env`
- `runtime:process exec|spawnProcess` → `sys.exec`

Resto nativos sin capability individual: gate es máscara CLI (ver `HOST_BOUNDARY_SPEC.md`). Chequeo en `check_native_op_capability` (`203-221`) y en `dispatch_runtime_op` (`228-245`): sin capability → `E_HOST_PERMISSION_DENIED` / `E_RUNTIME_PERMISSION_DENIED`.
