# Arquitectura de la Stdlib como Paquete

Cómo `std:*` sale del binario `vn` y se distribuye como artefacto versionado
independiente. Diseño completo: [superpowers/specs/2026-07-09-stdlib-package-system-design.md](superpowers/specs/2026-07-09-stdlib-package-system-design.md).
Este documento describe el estado **implementado**, no el aspiracional.

> **✅ Bug de bundle mode corregido.** `std.vnb` tuvo un bug de correctitud
> intermitente (llamadas a `std:math` fallando en runtime) entre el
> aterrizaje de este documento y su corrección. Root cause: puntero JIT
> obsoleto (`REG_GLOBALS`) tras crecer la tabla de globals tras cargar un
> módulo. Corregido — detalle completo en
> [§8 — Known Issue (resuelto)](#known-issue-resuelto--bundle-mode--intrinsics-de-stdmath)
> más abajo.

## 1. Tres niveles

| Nivel | Dónde vive | Ejemplos | Empaquetado |
|-------|-----------|----------|-------------|
| **Core embebido** | `varn-builtins` (Rust, `MODULE_REGISTRY`) | `core:global`, `core:int`, `core:array`, `core:intrinsics` | Siempre en el binario `vn` |
| **Host API** | `varn-builtins/src/modules/host/<m>/` | `runtime:math`, `runtime:fs`, … | Siempre en el binario `vn` — es la frontera con Rust |
| **Std packages** | `std/*.vn` (top-level, fuera de builtins) | `std:math`, `std:fs`, `std:json`, … | Compilado a `std.vnb`, distribuido junto a `vn` |

`core:`/`runtime:`/globals siguen embebidos porque son el host: viajan con el
binario por definición. Los 16 módulos std migrados (`collections, crypto, dispose, fs,
http, io, json, math, net, path, reflect, sys, task, test, time, types`) viven en `std/`.

## 2. `std/` tree y `std.json`

```
std/
  std.json          ← manifest único del bundle
  math.vn crypto.vn fs.vn http.vn io.vn json.vn net.vn
  path.vn sys.vn test.vn time.vn dispose.vn
```

`std.json`:

```json
{
  "version": "0.1.0",
  "hostApi": 1,
  "modules": [
    { "id": "std:math", "pure": true },
    { "id": "std:fs" }
  ]
}
```

`pure: true` marca módulos sin efectos secundarios observables (elegible
para folding/caching agresivo aguas abajo). `hostApi` debe igualar
`varn_core::HOST_API_VERSION` del build que corre `cargo xtask build-std`.

## 3. Formato `.vnb`

Reusa el envelope existente (`varn-modules::artifact`): `MAGIC_VNB` (`b"VNB\0"`)
+ `VNB_FORMAT_VERSION: u32` LE, payload postcard de `StdBundle`.

| Campo | Tipo | Rol |
|-------|------|-----|
| `std_version` | `String` | Versión semver del `std.json` que produjo el bundle |
| `build_fingerprint` | `u32` | `BUILD_FINGERPRINT` del compilador que compiló el bundle |
| `host_api_version` | `u32` | `HOST_API_VERSION` que el bundle espera del host |
| `modules[].id` | `String` | `"std:math"`, etc. |
| `modules[].pure` | `bool` | Copiado de `std.json` |
| `modules[].interface` | `Vec<u8>` | Postcard de `CachedModule` (checker) — exports + bind |
| `modules[].bytecode` | `Vec<u8>` | Postcard de `FunctionProto` — bytecode ya compilado |

Sin tabla de offsets manual — postcard serializa `Vec<BundleModule>` directo;
el índice por-módulo es búsqueda lineal en memoria (12 módulos, costo
irrelevante). `StdBundle::validate_compat_with(host_api_expected)` rechaza el
bundle si `build_fingerprint` u `host_api_version` no calzan exactamente —
**sin fallback silencioso**: error inmediato sugiriendo `cargo xtask build-std`.
Caso residual no cubierto por este check: un símbolo `runtime:*` nuevo
(aditivo, sin bump de versión) sobre un binario viejo falla al resolver ese
import puntual, con error de módulo claro — el check de versión cubre
breaking changes, los aditivos los cubre la resolución de imports.

## 4. Resolución y carga

Orden (`varn_modules::std_root::resolve`), primer hit gana:

1. **Project override**: `varn.json` → `"std": "<ruta>"` (a un `.vnb` o a un
   árbol con `std.json`).
2. **Env**: `VARN_STD` (dev/CI) — mismo formato de ruta.
3. **Dev checkout**: sube por los ancestros del propio ejecutable buscando un
   `std/std.json` hermano — el layout de `target/<profile>/` de este repo.
   Cubre cualquier lanzador (editor, debugger, exe directo) sin depender de
   que `VARN_STD` viaje con el proceso.
4. **Toolchain default**: `<exe_dir>/std.vnb`.

`classify()` decide bundle vs árbol: directorio con `std.json` → `SourceTree`;
archivo → `Bundle`. `StdProvenance` (`ProjectOverride` / `Env` / `DevCheckout`
/ `Toolchain`) viaja hasta `vn doctor` para diagnóstico.

- **Modo bundle**: `StdlibLoader::load` consulta `provider.bytecode_blob(id)`
  primero — deserializa `FunctionProto` directo del blob, sin parse/check/compile.
  El checker resuelve exports vía `provider.interface_blob(id)` (bundle-first,
  antes de `embedded_source`/`source_path`). Módulos blob-backed no entran al
  grafo de recompilación (`module_precompile.rs`) — no tienen `.vnc` propio;
  el bundle es su propia unidad de verificación (fingerprint + host API).
- **Modo árbol fuente**: mismo resolver, compila on-demand con el pipeline
  actual + cache `.vnc` existente — misma relación compile/cache-hit de
  siempre, sin ruta dual nueva.

## 5. Dev workflow

`.cargo/config.toml` en la raíz del repo:

```toml
[env]
VARN_STD = { value = "std", relative = true }
```

`cargo build`/`run`/`test` ven automáticamente el árbol `std/` del repo vía
esa env. Invocar el binario compilado directamente (`./target/debug/vn`,
`vn-lsp` lanzado por un editor, un debugger) no hereda esa env — pero como
todo binario de este repo vive bajo `target/<profile>/`, el tier **dev
checkout** (§4.3) lo encuentra igual subiendo por los ancestros del exe hasta
dar con `std/`. `VARN_STD` sigue siendo útil para apuntar a un árbol/bundle
distinto al del propio checkout (p. ej. probar modo bundle sin salir del
repo). Editar `std/*.vn` invalida el cache `.vnc`
del módulo afectado por hash de contenido (sin purga manual salvo al cambiar
de modo bundle↔árbol).

## 6. `cargo xtask build-std`

```powershell
cargo xtask build-std                                     # std/ → target/std.vnb
cargo xtask build-std --std-dir std --out target/std.vnb   # explícito
```

Sirve la resolución `std:` desde `--std-dir` durante la build (vía
`VARN_STD` interno), valida que cada módulo solo importe `runtime:*`/`std:*`,
tipa completo con el checker, serializa interface + bytecode, y escribe el
`.vnb`. Cero comandos nuevos en `vn` — es tooling de repositorio, no del
lenguaje. CI/release lo invoca al empaquetar.

## 7. Packaging

Un release distribuye `vn` (binario) + `std.vnb` (mismo `build_fingerprint`
y `HOST_API_VERSION`) en el mismo directorio. `vn doctor` reporta el std
activo y su procedencia.

## 8. Startup medido

Workload: `import { Math } from "std:math"; print(Math.abs(-1));`, build
`--release`, `.vn/cache` purgado antes de cada serie, Windows/PowerShell
`Measure-Command`, promedio de 10 corridas en caliente (cache de la config
tibia tras 1 corrida fría de descarte). Ver metodología completa y datos
crudos en el mensaje del commit que introduce este documento.

| Modo | Binario/std | Frío (1ra corrida) | Caliente (avg de 10) |
|------|-------------|---------------------|------------------------|
| Embedded (baseline, pre-Task-4, commit `d8d6a77`) | std embebido en `vn` | ~50.6 ms | **12.03 ms** |
| Árbol fuente (`VARN_STD=std/`) | compila on-demand + `.vnc` | ~41.5 ms | **11.20 ms** |
| Bundle (`VARN_STD=std.vnb`) | carga directa del blob | ~40-43 ms | **10.36 ms**¹ |

¹ Medido con `std:path` (`path.join(...)`), no con `std:math`, porque al
momento de esta medición `std:math` disparaba el bug de bundle mode descrito
(y ya corregido) en §8 más abajo. `std:math` ahora funciona de forma
confiable en modo bundle (180 corridas consecutivas sin fallo tras el fix);
esta tabla no se re-midió con las tres filas bajo condiciones idénticas
tras la corrección, así que se deja como registro histórico de la medición
original en vez de reemplazar una sola fila con un número no comparable.
Bundle mode es marginalmente más rápido que árbol/embedded en
este workload trivial (como es esperable: sin parse/check/compile), pero la
diferencia es pequeña frente al costo fijo de arranque del proceso — no se
reclama una mejora dramática, solo lo medido.

### Known Issue (resuelto) — bundle mode + intrinsics de `std:math`

Al medir con el script exacto del plan (`Math.abs(-1)` en modo bundle), la
ejecución fallaba intermitentemente (~60-90% de las corridas observadas, en
tres series independientes de 10-15 corridas cada una) con
`runtime error: value is not callable: <float basura> (type: float)` al
invocar el `Call` inmediatamente siguiente (típicamente `print`). El mismo
bytecode cacheado producía resultados distintos entre invocaciones de
proceso idénticas.

**Root cause confirmado** (instrumentación directa en el intérprete +
lectura del codegen JIT, `crates/varn-jit/src/codegen/misc/helpers.rs` /
`regalloc.rs`): el prólogo de cada función JIT-compilada cachea, una sola
vez, un puntero crudo (`REG_GLOBALS`, registro `R14`) al buffer interno de
`ExecCtx.globals.values: Vec<VmValue>`. `emit_load_module` (codegen de
`LoadModule`) ya recomputaba `REG_FRAME_BASE` tras la llamada FFI (vía
`recompute_frame: true`, porque cargar un módulo puede crecer el stack de
la VM) pero **no** recomputaba `REG_GLOBALS`. Cargar `std:math` por primera
vez ejecuta el cuerpo entero de `std/math.vn`, que hace `DefineGlobal(Idx)`
~40 veces (E, LN10, …, abs, sqrt, …, el objeto `Math`) — suficiente para
que el `Vec` de globals reasigne su buffer. Toda lectura posterior de un
global vía `LoadGlobalIdx` (p. ej. `print`, resuelto por índice fijo en
tiempo de compilación) leía entonces a través de un puntero colgante hacia
el buffer viejo, ya liberado — lectura de memoria liberada, con contenido
no determinista según qué reutilizó esa página entre la liberación y la
lectura. Esto también explica por qué modo árbol nunca fallaba: compila
`std/math.vn` en el mismo proceso que corre el script, y el `Vec` de
globals típicamente ya tenía suficiente capacidad reservada de antes,
evitando la reasignación exacta en ese punto.

**Fix:** `emit_load_module` ahora re-emite la misma instrucción que usa el
prólogo (`mov REG_GLOBALS, [ExecCtx + globals_offset + 8]`) inmediatamente
después de la llamada FFI que carga el módulo — mismo patrón que
`recompute_frame`, aplicado al puntero de globals. Cambio de ~10 líneas,
un solo call site (`crates/varn-jit/src/codegen/misc/helpers.rs`).

**Verificado:** 180 corridas consecutivas del script del plan en modo
bundle, 0 fallos (vs. ~35-65% de fallo antes del fix); suite completa
(`tests/main.vn`, 674 tests) y `vn bench` verdes en modo árbol y modo
bundle tras el fix; suites de `varn-modules`/`varn-checker`/`varn-builtins`
sin regresión.

Nota para el futuro: otros call sites de FFI en el JIT con `reload: true`
(p. ej. `CallSpread`, que recompone `REG_FRAME_BASE` a mano) no fueron
auditados exhaustivamente por si alguno puede definir globals nuevos de
forma transitiva — `LoadModule` es el único camino confirmado y corregido
aquí.
