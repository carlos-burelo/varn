# Sistema de paquetes para la stdlib de Varn

**Fecha:** 2026-07-09
**Estado:** Aprobado (diseño). Pendiente: plan de implementación.

## Problema

La stdlib vive entera dentro de `crates/varn-builtins` y se compila en el binario `vn`.
Cuatro dimensiones escalan mal cuando la stdlib crezca:

1. **Fricción al autorar** — un módulo nuevo requiere: `.vn` contract + `module.json` +
   Rust + registro en `mod.rs` + capa espejo `runtime:` (`Math.sqrt` → `mathSqrt` es puro
   boilerplate).
2. **Monolito compilado** — binario y compile times crecen con cada módulo; no hay carga
   bajo demanda ni actualización independiente.
3. **Organización** — el layout `primitives/std/globals` + `runtime/` anidado +
   `module.json` sidecars se vuelve difícil de navegar con 100+ módulos.
4. **Versionado/distribución** — stdlib atada a la versión del compilador; sin historia
   de evolución independiente.

Dos rediseños previos ya resolvieron autoría (`varn_contract!`, drift = error de
compilación) y dispatch (op-id / `CallNativeOp`). Este diseño ataca el eje restante:
**empaquetado, distribución y frontera host**.

## Decisiones (con alternativas descartadas)

| # | Decisión | Descartado |
|---|----------|------------|
| 1 | Core embebido + std como paquetes precompilados | Binario único estilo Go; cdylibs dinámicos (Fase 4, sigue pendiente aparte) |
| 2 | `runtime:*` = Host API formal versionada | Matar la capa runtime; mantener wrappers siempre |
| 3 | Adquisición: std instalada con el toolchain + override local por proyecto | Registry con descarga (red/lockfiles/cache: infraestructura futura); vendoring por proyecto |
| 4 | std se versiona como **una unidad** (un artefacto por release) | Semver por módulo (requiere resolución de compatibilidad interna) |
| 5 | Artefacto = **bundle único `.vnb`** | Directorio de `.vnc` sueltos (sin atomicidad/integridad); híbrido fuente-embebida + override (sistema dual permanente, prohibido por `<evolution_strategy>`) |
| 6 | Host API expone **clases-recurso + funciones sueltas** | Solo funciones planas (sopa de verbos, handles opacos); objeto host único (god object); syscall table mínima (máxima obra, capa extra en ops calientes) |
| 7 | Bundle se construye con **`cargo xtask build-std`** (herramienta del repo, estilo `x.py` de Rust) | Subcomando `vn std` (el usuario final nunca construye la std; ningún lenguaje expone eso); plegarlo en `vn build` |

## 1. Layout del repo

```
std/                          ← NUEVO árbol top-level, 100 % Varn
  std.json                    ← manifest único del bundle
  math.vn                     ← std:math
  json.vn  fs.vn  http.vn  io.vn  net.vn  path.vn  crypto.vn
  time.vn  task.vn  reflect.vn  testing.vn  sys.vn  dispose.vn  collections.vn

crates/varn-builtins/src/
  primitives/                 ← core types (Rust + varn_contract!), sin cambio
  globals/                    ← sin cambio
  host/                       ← runtime:* natives, subidos desde modules/std/*/runtime/
    math/  fs/  http/ …       ← cada uno: contract .vn + Rust + module.json (como hoy)
```

`std.json` (manifest):

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

**Muere:** `modules/std/` dentro de builtins; los `module.json` per-module de std
(metadata → `std.json`); el anidado `x/runtime/x_runtime.vn`; las fuentes std embebidas
en el binario.

**Se mantiene:** `module.json` como sidecar de los módulos *nativos* (`host/`,
`primitives/`, `globals/`) — decisión previa ya validada.

## 2. Host API formal

- `runtime:*` es **la frontera** entre std (Varn) y host (Rust). Superficie pequeña,
  estable, documentada en `docs/HOST_BOUNDARY_SPEC.md`.
- Nueva constante `HOST_API_VERSION: u32` en `varn-core`. Bump obligatorio en cada
  breaking change de la superficie `runtime:*` (firma cambiada, símbolo eliminado).
  Añadir símbolos no rompe.
- **Forma de la superficie** — criterio por semántica:
  - Recurso con identidad y lifecycle → **clase nativa**: `File`, `Socket`, `Dir`,
    `Process`, `TcpServer`. Estado vive en la instancia; sin handles opacos.
    `varn_contract!` ya soporta clases (precedente: `IsolatePort`).
  - Operación stateless → **función**: `hash`, `now`, `random`, trigonometría.
- Natives con **nombre público** (`runtime:fs` exporta `readFile`, no `fsReadFile`) —
  ya namespaceados por módulo, sin riesgo de colisión.
- **Regla de wrappers en std:**
  - Passthrough 1:1 → re-export directo: `export { File } from "runtime:fs"`
    (el AST ya soporta `ExportDecl::Named { source }` y `ExportDecl::All`).
  - Wrapper `.vn` solo donde agrega semántica: namespaces con constantes (`Math`),
    composición, sugar puro en Varn:

    ```vn
    // std/fs.vn
    export { File } from "runtime:fs";

    export function readFile(path: str): str {
        // compone File.open + read + cierre (integra std:dispose)
    }
    ```

  - El costo runtime de los wrappers restantes ya es ~0 (inlining HIR de
    single-expression, cache v18).
- **Restricción compilada:** los módulos std solo pueden importar `runtime:*` y
  `std:*`. `build-std` rechaza cualquier otro import.

## 3. Artefacto `.vnb` (bundle)

Reusa el envelope existente de `varn-modules/src/artifact.rs` con nuevo `MAGIC_VNB`.

```
header:  magic + format_version
         std_version        (semver, str)
         build_fingerprint  (u32) — debe ser == BUILD_FINGERPRINT del vn que carga
         host_api_version   (u32) — debe ser == HOST_API_VERSION del binario
index:   por módulo: id, offset/len del interface-blob, offset/len del bytecode-blob,
         hash fnv1a64 del contenido
blobs:   interface = tabla de exports serializada para el checker
                     (reusa la serialización del types-cache .vnm)
         bytecode  = FunctionProto serializado (reusa el payload .vnc)
```

**`BUILD_FINGERPRINT` (decisión 2026-07-09, reemplaza versiones manuales):** los
consts manuales `COMPILER_CACHE_VERSION`/`TYPE_CACHE_VERSION` fueron eliminados. El
build.rs de `varn-modules` hashea las fuentes de los 8 crates con semántica de
compilación (core, lexer, parser, checker, types, opt, backend, modules) y genera
`BUILD_FINGERPRINT: u32`. Cualquier rebuild que pueda cambiar bytecode/codegen o la
serialización de interfaces del checker invalida automáticamente `.vnc`/`.vnm`/`.vnb`
— sin disciplina de bump manual, sin purgas manuales de cache. No captura cambios de
toolchain (rustc, versiones de deps): subaproximación aceptada, las fuentes dominan
el drift.

**Regla de compatibilidad (honesta):** el bytecode está atado al build exacto del
compilador. Mismatch de `format_version`, `build_fingerprint` o `host_api_version` =
error claro e inmediato; **sin fallback silencioso**. En la práctica la std viaja con
el toolchain (match por construcción); un override local se regenera con el
`build-std` del mismo workspace.

Caso residual: un cambio *aditivo* al host (símbolo nuevo, sin bump de
`HOST_API_VERSION`) no lo detecta el check de versión — un bundle que use el símbolo
nuevo sobre un binario viejo falla al resolver el import `runtime:*`, con error de
módulo claro. Aceptado: el check de versión cubre breaking changes; los aditivos los
cubre la resolución de imports.

## 4. Resolución y carga

Orden de resolución para `std:x` — un solo mecanismo, dos formas de almacenamiento:

1. **Override de proyecto:** `varn.json` → `"std": "<ruta>"`. La ruta apunta a un
   `.vnb` **o** a un árbol fuente con `std.json`.
2. **Default del toolchain:** `std.vnb` junto al ejecutable (`<exe_dir>/std.vnb`).

- **Modo bundle:** `ModuleId::Std` → lookup en índice → deserializa `FunctionProto`
  lazy, por módulo, al primer import. Sin parse, sin compile, sin cache `.vnc`.
- **Modo árbol fuente** (dev en el repo): mismo resolver; compila on-demand con el
  pipeline actual + cache `.vnc` existente. Relación compile / cache-hit, no ruta dual.
- El trait `provider` actual (`embedded_source` / `source_path`) se reemplaza por
  `StdProvider` con esta resolución.
- `core:` y `runtime:` siguen embebidos — son el host, viajan con el binario por
  definición.

## 5. Checker

Hoy `resolve_stdlib_module_exports_ref` parsea la fuente `.vn` embebida.

- **Modo bundle:** exports deserializados del interface-blob. Cero parse de std en el
  arranque. Semántica idéntica garantizada: `build-std` genera el blob con el mismo
  checker.
- **Modo fuente:** parsea como hoy.
- **Intrinsics** (`Math.pow` → opcode `Intrinsic`): sin cambio. La anotación ocurre al
  checkear el programa del usuario contra los tipos del módulo; no depende de dónde
  vive la fuente.

## 6. Tooling

- **Cero comandos nuevos en `vn`.**
- `cargo xtask build-std` — workspace crate `xtask/` que linkea `varn-pipeline`
  directamente: compila `std/` → `std.vnb`, valida imports (solo `runtime:`/`std:`),
  typecheck completo, embebe las tres versiones. CI/release la invoca al empaquetar.
- `vn inspect` gana: origen de la std activa (toolchain/override), versión, host API
  requerida, lista de módulos.
- `varn.json` gana la clave opcional `"std"`.
- Release empaqueta `vn` + `std.vnb`.

## 7. Migración (orden, suite verde en cada paso)

1. **Host:** renombrar natives a nombres públicos + convertir recursos a clases
   nativas (`File`, `Socket`, …), módulo por módulo.
2. **Layout:** mover los `.vn` de std a `std/` top-level, crear `std.json`, eliminar
   los `module.json` de std y el anidado `runtime/`.
3. **Carga:** `StdProvider` + resolución (modo árbol fuente primero — el repo sigue
   funcionando sin bundle).
4. **Bundle:** formato `.vnb` + `cargo xtask build-std` + carga bundle + interface-blob
   del checker.
5. **Purga:** eliminar fuentes std embebidas de `varn-builtins` (build.rs queda solo
   para `core:`/`runtime:`/`globals`); packaging de release.

## 8. Validación

- `vn run tests/main.vn` **y** `vn bench tests/main.vn` verdes en cada fase (bench
  atrapa bugs de JIT/regalloc que run no detecta).
- Benchmark de arranque antes/después (embedded-compile vs bundle-load) — medido,
  no asumido (`<performance_rules>`).
- Test de rechazo: bundle con `build_fingerprint` o `host_api_version` distinto →
  error claro.
- Builtins se validan con `cargo check -p varn-builtins --features runtime`.

## Documentación a actualizar

- Nuevo `docs/STDLIB_ARCHITECTURE.md` (este sistema).
- `ARCHITECTURE.md`, `CRATES_STATE.md`, `LBI_ARCHITECTURE.md`,
  `HOST_BOUNDARY_SPEC.md`, `CLI_REFERENCE.md` (inspect).

## Fuera de alcance

- Registry con descarga por red (decisión 3 lo pospone; el formato `.vnb` + versionado
  ya deja el terreno preparado).
- Cdylibs nativos de terceros (`varn_register_v1`, Fase 4 del contract-redesign).
- Semver por módulo std.
