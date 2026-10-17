# LBI: nativos con contrato `.vn`

Código manda. Fuentes: `crates/varn-macros/src/varn_contract/`, `crates/varn-macros/src/contract_parse.rs`, `crates/varn-builtins/build.rs`, `crates/varn-builtins/src/modules/mod.rs`, `crates/varn-builtins/src/dispatch/mod.rs`, `crates/varn-core/src/op_id.rs`, `crates/varn-core/src/abi.rs`.

## Piezas reales

1. **Contrato = un `.vn` por módulo**, junto a su `.rs` en `varn-builtins/src/modules/{core,runtime}/<mod>/`. Un directorio con más de un `.vn` aborta el build.
2. **`varn_contract!`** (`varn-macros/src/varn_contract/mod.rs`): lee el `.vn` en tiempo de macro y lo parsea con el parser real (`contract_parse.rs`); no hay tabla intermedia. Contratos inline se rechazan. Por cada `declare function` (y miembros de clase) genera wrappers y un `NativeOpEntry`; exporta `NATIVE_ENTRIES: &[&NativeOpEntry]`. Un `include_bytes!` del `.vn` lo hace dependencia de compilación.
3. **`build.rs` de `varn-builtins`**: recorre `src/modules/{core,runtime}`, parsea cada contrato con `varn-parser` y genera `MODULE_REGISTRY` (`registry.generated.rs`): id `core:<mod>`/`runtime:<mod>`, fuente embebida, exports y si el contrato trae código además de declaraciones.
4. **Registro estático único**: `native_modules!` (`modules/mod.rs`) enumera los módulos y concatena sus `NATIVE_ENTRIES` en `NATIVE_MODULES`. Sin secciones del linker ni marcadores.
5. **`varn-macros`** también define `#[jit_slow]` y `jit_helper_table!` (helpers JIT; no registran módulos).

## Módulos nativos (`modules/mod.rs`)

`buffer compress crypto csv errors ffi fs globals gpu io json math net path process reflect regex sqlite sys task testing text time ui ws` + `types` (`array bigint bool bytes char decimal float int iteration map range set string`). `testing` no aporta entradas nativas.

Entrada: `build_module(id, ctx)` → `dispatch::build_module`: crea el objeto módulo y cuelga cada entrada en su ruta de namespace.

## Dispatch (`dispatch/mod.rs`)

Ids de op **densos**: `registry()` asigna índices en orden de registro, empezando en `FIRST_REGISTRY_ID` (`op_id.rs`). Las ops del módulo núcleo (`CORE_MODULE = "globals"`) que son `KnownOp` toman su id reservado, de modo que el compilador las emite sin buscar por nombre. Búsquedas: `native_op_id(module, ns, symbol)`, `find_native_op_entry(id)` (índice en `Vec`), `native_op_fn(id)`.

`NATIVE_GLOBALS` (`varn-core/src/abi.rs`) es la única tabla ABI de globales nativos; la VM la recorre en ese orden (`varn-vm/src/globals/mod.rs`).

## Capabilities

Declaradas en el contrato con `@permission("<dominio>")` sobre la función; el macro las guarda en el `NativeOpEntry` (argumento distinto de un string → error de compilación). Dominios en uso: `sys.ffi` (12), `ui` (13), `sys.env` (2), `sys.exec` (2).

Chequeo en `check_native_fn_capability` (`dispatch/mod.rs`), llamado por la VM antes de invocar el nativo (`varn-vm/src/exec/ctx_gc.rs`): sin la capability → `E_HOST_PERMISSION_DENIED:id=<mod>:<símbolo>:capability=<dominio>`. Nativos sin `@permission` no se filtran individualmente; el resto del gate es la máscara del CLI (ver `HOST_BOUNDARY_SPEC.md`).
