# Arquitectura de la Stdlib como Paquete

Cómo `std:*` sale del binario `vn` y se distribuye como artefacto versionado
independiente. Diseño completo: [superpowers/specs/2026-07-09-stdlib-package-system-design.md](superpowers/specs/2026-07-09-stdlib-package-system-design.md).
Este documento describe el estado **implementado**, no el aspiracional.

## 1. Tres niveles

| Nivel | Dónde vive | Ejemplos | Empaquetado |
|-------|-----------|----------|-------------|
| **Core embebido** | `varn-builtins` (Rust, `MODULE_REGISTRY`) | `core:*`, `globals` | Siempre en el binario `vn` |
| **Host API** | `varn-builtins/src/modules/host/<m>/` | `runtime:math`, `runtime:fs`, … | Siempre en el binario `vn` — es la frontera con Rust |
| **Std packages** | `std/*.vn` (top-level, fuera de builtins) | `std:math`, `std:fs`, `std:json`, … | Compilado a `std.vnb`, distribuido junto a `vn` |

`core:`/`runtime:`/globals siguen embebidos porque son el host: viajan con el
binario por definición. Los 12 módulos std migrados (`crypto, dispose, fs,
http, io, json, math, net, path, sys, test, time`) viven en `std/`.

**Deferred (no migrados en este plan):** `std:collections`, `std:reflect`,
`std:task`, `std:types` — comparten fuente con ids `core:*` o requieren
manejo especial del checker. Siguen embebidos en `MODULE_REGISTRY`; el
provider compuesto cae de vuelta al registro para estos 4. Migran en el plan
de seguimiento **Host API reshape** (renombrar natives, clases-recurso,
migración de estos 4 fuera del registro).

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
3. **Toolchain default**: `<exe_dir>/std.vnb`.

`classify()` decide bundle vs árbol: directorio con `std.json` → `SourceTree`;
archivo → `Bundle`. `StdProvenance` (`ProjectOverride` / `Env` / `Toolchain`)
viaja hasta `vn doctor` para diagnóstico.

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

`cargo build`/`run`/`test` ven automáticamente el árbol `std/` del repo.
Invocar el binario compilado directamente (`./target/debug/vn`, fuera de
`cargo`) no hereda esa env — exportar `VARN_STD` manualmente o depender del
fallback a `<exe_dir>/std.vnb`. Editar `std/*.vn` invalida el cache `.vnc`
del módulo afectado por hash de contenido (sin purga manual salvo al cambiar
de modo bundle↔árbol).

## 6. `cargo xtask build-std`

```powershell
cargo xtask build-std                              # std/ → ./std.vnb
cargo xtask build-std --std-dir std --out std.vnb   # explícito
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

¹ Medido con `std:path` (`path.join(...)`), no con `std:math` — ver Known
Issue abajo. Bundle mode es marginalmente más rápido que árbol/embedded en
este workload trivial (como es esperable: sin parse/check/compile), pero la
diferencia es pequeña frente al costo fijo de arranque del proceso — no se
reclama una mejora dramática, solo lo medido.

### Known Issue — bundle mode + intrinsics de `std:math` (bloqueante)

Al medir con el script exacto del plan (`Math.abs(-1)` en modo bundle), la
ejecución falla intermitentemente (~60-90% de las corridas observadas, en
tres series independientes de 10-15 corridas cada una) con
`runtime error: value is not callable: <float basura> (type: float)` al
invocar `Math.abs`. El mismo bytecode cacheado produce resultados distintos
entre invocaciones de proceso idénticas — indica un bug de estado en tiempo
de ejecución (no un error de compilación determinista).

Diagnóstico realizado (sin fix — fuera de alcance de esta tarea):
- **No** reproduce en modo árbol (15/15 corridas OK) ni en el baseline
  embebido pre-Task-4 (15/15 OK) con el script idéntico → regresión nueva,
  no preexistente.
- **No** reproduce con otro módulo std que usa el mismo patrón de namespace
  (`std:path`, `path.join(...)`, 10/10 OK en modo bundle).
- **No** reproduce corriendo `tests/main.vn` completo en modo bundle (10/10
  OK) — y `tests/52-math-trig.vn` (parte de esa suite) importa `std:math` y
  llama `Math.acos/asin/atan/atan2` con el mismo patrón. La falla solo se
  observó en scripts aislados donde el import+llamada a `Math.*` ocurre
  entre las primeras operaciones del proceso — sugiere una dependencia de
  estado/timing (heap, shape cache o GC temprano) más que un error
  determinista de compilación.
- El loader (`StdlibLoader::load`) es puramente síncrono — se descarta una
  condición de carrera en la carga del blob en sí.
- Hipótesis más probable: interacción entre el opcode `Intrinsic` que el
  compilador emite para llamadas reconocidas a `Math.*` (spec §5) y algo del
  ciclo de vida runtime específico de bytecode cargado desde blob (shapes /
  inline cache / GC) en las primeras ejecuciones del proceso. Sin confirmar
  — requiere debugging dedicado de VM (no cubierto por esta tarea).

**Esto bloquea el uso en producción de `std.vnb` para módulos con intrinsics
reconocidos por el compilador hasta que se investigue y corrija.** Se deja
registrado aquí en vez de ocultarlo; ver mensaje del commit para el detalle
completo de las corridas.
