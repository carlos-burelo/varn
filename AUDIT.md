# Auditoría del Workspace Varn

**Base auditada:** `HEAD` = `fb9c48d` (árbol limpio; las copias sin commitear `crates/varn-compiler/`, `crates/varn-regalloc/` y `crates/varn-term/` fueron eliminadas antes de auditar — no eran miembros del workspace y no tenían ninguna referencia).

**Escala:** 21 crates, 106 787 líneas de Rust, 568 archivos `.rs`.

## 0. Estado funcional verificado

La matriz de validación de `CLAUDE.md` está **verde en las 4 combinaciones**, con `vn cache clean` entre procedencias:

| Procedencia std | JIT | Resultado |
|---|---|---|
| árbol `std/` (dev-checkout) | sí | 991 PASSED / 0 FAILED |
| árbol `std/` | `VARN_NO_JIT=1` | 991 PASSED / 0 FAILED |
| `VARN_STD=@embedded` | sí | 991 PASSED / 0 FAILED |
| `VARN_STD=@embedded` | `VARN_NO_JIT=1` | 991 PASSED / 0 FAILED |

Todo lo que sigue son defectos de **estructura, deuda y documentación** sobre un sistema que funciona. No hay ningún hallazgo de corrección funcional.

## 1. Correcciones a la auditoría anterior

La versión previa de este documento leyó en parte las copias sin commitear, y lanzó dos alarmas que la medición no sostiene:

- **`codegen-units` NO está roto.** Ver §5.1: medido, no inferido.
- **"Borrar `varn-lexer/src/ffi.rs`" habría borrado la API viva del lexer.** El archivo tiene 168 líneas: `varn_scan` (6-66) y `varn_free` (69-75) son FFI muerta, pero `scan` (77) y `scan_with_config` (116) son el punto de entrada que usa todo el workspace. Lo correcto es extraer las 68 líneas de FFI y renombrar el archivo, no eliminarlo.

## 2. Código muerto (medido)

Método: para cada `pub fn` del workspace (1232 en total), se contó toda aparición del identificador en cualquier `.rs` del repo fuera de su archivo de definición, más las apariciones dentro del propio archivo. Cero externas + una interna (la declaración) = sin uso. Verificado también contra `docs/`, `std/`, `benchmarks/` y `scripts/`.

**Resultado: 101 funciones públicas sin ningún consumidor** (99 `pub fn` + `varn_scan`/`varn_free`).

| Crate | `pub fn` sin uso | Comentario |
|---|---|---|
| `varn-types` | 29 | `emit2`/`emit3`/`emit4`/`patch_jump` ([chunk.rs:894-984](crates/varn-types/src/chunk.rs#L894)), `num_add`/`num_sub`/`num_mul`/`num_div` ([value/traits.rs:99-178](crates/varn-types/src/value/traits.rs#L99)) — la VM tiene su propia aritmética |
| `varn-lsp` | 18 | capa `index/query.rs` completa (5), `queries/semantics.rs`, `queries/syntax.rs`, `util/converters.rs` (3) |
| `varn-core` | 11 | `time.rs` entero (ver abajo), `module_id` (3), `count_ast_nodes` |
| `varn-backend` | 11 | ver abajo |
| `varn-checker` | 7 | `bind_with_globals`, `resolve_stdlib_module_bind`, `find_module_bind_for_type` |
| `varn-builtins` | 4 | `native_fast_op_fn`, `core_modules`, `stdlib_modules`, `range_op` |
| `varn-modules` | 3 | `runtime_module_ids`, `is_package_module_path`, `docs_error_url` |
| `varn-parser` | 3 | `range_from`, `peek_expect`, `is_arrow_ahead` |
| `varn-runtime` | 3 | ver abajo |
| `varn-base`, `varn-diagnostics`, `varn-pipeline`, `varn-utilities` | 2 c/u | `_fnv1a64_extend`/`_fnv1a64_u64` en pipeline llevan `_` para silenciar el lint |
| `varn-jit`, `varn-opt` | 1 c/u | `fmt_duration_csv`, `use_counts` |
| `varn-vm`, `varn-cli`, `varn-debug`, `varn-pm`, `varn-lexer`, `varn-op-macros` | 0 | limpios |

### 2.1 Bloques muertos completos

**`varn-runtime::Scheduler` — 451 líneas, cero consumidores.** `Scheduler`, `TaskRunner`, `TaskId`, `alloc_task_id`, `SchedulerConfig` y `run_with_metrics` no aparecen en ningún crate fuera de `varn-runtime`. Del crate (869 líneas) solo se consume `channel` (346 líneas, 19 usos), `init_heap` (31) y `Suspend` (8). El único `tokio::task::LocalSet` del repo está en [scheduler.rs:151](crates/varn-runtime/src/scheduler.rs#L151), dentro del código muerto: `tokio` aparece **exclusivamente** en `scheduler.rs` dentro de ese crate.

El `await` real no pasa por ahí. [ctx_tasks.rs:29-37](crates/varn-vm/src/exec/ctx_tasks.rs#L29) espera con `std::sync::mpsc::channel` + `rx.recv()` — bloqueo síncrono — y `suspend_timer` cae a `thread::sleep`. El modelo asíncrono ejecutado es cooperativo-síncrono dentro de la VM; el event loop documentado no se instancia nunca.

**`varn-core/src/time.rs` — 155 líneas, cero consumidores.** Las 10 funciones (`is_leap`, `days_to_ymd`, `ymd_to_days`, `calendar_to_secs`, `calendar_to_millis`, `secs_to_calendar`, `unix_to_iso`, `iso_to_unix`, `now_millis`, `now_secs_f64`) solo se llaman entre sí. El calendario que sí se usa está en `varn-builtins/src/modules/host/time/time.rs` vía `chrono`. Es una implementación a mano superada por la dependencia.

**`varn-backend` — 297 de 1380 líneas muertas (21,5 %).**
- [`ir.rs`](crates/varn-backend/src/ir.rs) completo (144 líneas): `IrBuilder`/`IrModule` nunca se construyen; su único enlace es `use crate::ir::IrModule` en [liveness.rs:3](crates/varn-backend/src/liveness.rs#L3), para `analyze_module`, que nadie llama.
- `liveness.rs`: 153 de 241 líneas muertas — `analyze_module` + `analyze` (28-60), `max_concurrent_live` + `live_ranges` (122-176) y `InterferenceGraph` entero (177-241, con `from_live_ranges`, `chromatic_number_upper_bound`, `find_low_degree_node`).
- Vivo: `LiveRange`, `LivenessAnalyzer::{new, record_def, record_use, analyze_with_back_edges}`, invocados desde [regalloc_post.rs:747](crates/varn-backend/src/regalloc_post.rs#L747), y `run_post_passes`.

### 2.2 Causa raíz

El commit `22d7d7b` ("drop every `#[allow(dead_code)]`") eliminó lo que el lint podía ver. **`rustc` no reporta `dead_code` para items `pub` de un crate `lib`**: son API exportada por definición. Con 21 crates y casi todo `pub`, el lint queda ciego sobre el 100 % de las fronteras.

Arreglo estructural, no cosmético: reducir a `pub(crate)` todo lo que no cruza la frontera del crate (el lint vuelve a ver), y añadir `[workspace.lints]` con `unreachable_pub` y `unused_crate_dependencies`. Repetir esta medición debe ser un paso de CI, no una auditoría manual.

## 3. Dependencias declaradas y no usadas

Doce entradas de `[dependencies]` cuyo nombre no aparece en ningún `.rs` del crate:

| Crate | Dependencias sin uso |
|---|---|
| `varn-builtins` | `tokio`, `parking_lot`, `mio`, `ryu`, `url`, `ureq`, `tiny_http` |
| `varn-cli` | `tower-lsp`, `serde`, `serde_json` (sí se usan en `[build-dependencies]`, no en `src/`) |
| `varn-runtime` | `parking_lot` |
| `varn-types` | `parking_lot` |

Peso real: `ureq` y `tiny_http` están detrás de la feature `runtime`, que `varn-vm`, `varn-lsp` y `varn-cli` activan siempre — es decir, **todo binario distribuido compila un cliente HTTP y un servidor HTTP que nadie importa** (`net.rs` implementa sockets con `std::thread::spawn` directo, sin ureq ni tiny_http). `varn-builtins` declarando `tokio` mientras `varn-runtime` es quien lo necesita invierte la relación real.

## 4. Aristas del grafo

El grafo es acíclico y las fronteras frontend / compilador / VM son reales. Los problemas son tres aristas concretas:

**a) La VM depende del parser, por proc-macro.**
`varn-vm → varn-builtins → varn-op-macros → varn-parser + varn-lexer + varn-core`. Compilar el motor de ejecución exige compilar el frontend y expandir `varn_contract!`, que parsea los `.vn` de contrato en tiempo de expansión. No hay ciclo, pero cualquier edit del parser invalida VM, JIT y todo aguas abajo, y serializa el grafo de build. Destino: mover el parseo de contratos a un build-script que emita un AST preparseado.

**b) `varn-pipeline → varn-lsp`, con la feature siempre encendida.**
`varn-pipeline/Cargo.toml` declara `lsp-debug = ["dep:varn-lsp", ...]` y `varn-cli` la activa incondicionalmente (`varn-pipeline = { …, features = ["lsp-debug"] }`). El orquestador del pipeline consume el servidor de editor en [lsp_debug.rs](crates/varn-pipeline/src/lsp_debug.rs) y [debug/types.rs:11](crates/varn-pipeline/src/debug/types.rs#L11). Es una arista invertida permanente con forma de flag opcional — el patrón que `<evolution_strategy>` prohíbe explícitamente.

**c) Tres implementaciones del driver lex→parse→check.**
Sitios de llamada a `varn_lexer::scan` / `varn_parser::parse` fuera de las fases del pipeline:

| Archivo | Llamadas |
|---|---|
| `varn-checker/src/module_resolver.rs` | 10 |
| `varn-pipeline/src/stdlib_loader.rs` | 4 |
| `varn-lsp/src/pipeline/mod.rs` + `queries/syntax.rs` | 4 |
| `varn-pipeline/src/module_precompile.rs` | 2 |
| `varn-cli/src/{debug_binder.rs,bench/source.rs}` | 4 |

`varn-lsp` **no depende de `varn-pipeline`** y mantiene su propio `pipeline/` (686 líneas). El checker re-corre el frontend por su cuenta en 10 puntos. Aquí está la deuda estructural más cara de las tres.

## 5. Perfil de compilación

### 5.1 `codegen-units`: la nota de `CLAUDE.md` está desactualizada

`CLAUDE.md` afirma que `[profile.release]` debe mantener `codegen-units = 1` porque los marcadores de sección MSVC (`.varn_ops$A` / `$C`) requieren un único CGU final. El workspace tiene `codegen-units = 16`. Medición directa:

| Build | Ops en la sección del linker (`VARN_DEBUG_OPS`) | `tests/main.vn` | Tamaño de `vn.exe` |
|---|---|---|---|
| `codegen-units = 16` (actual) | **313** | 991 PASSED | 14 609 920 B |
| `codegen-units = 1` (override) | **313** | 991 PASSED | 13 087 232 B |

La tabla es idéntica: la agrupación `$A`/`$B`/`$C` sobrevive a 16 CGUs con `lld-link`. Además el riesgo es estructuralmente imposible desde que existe el registro de respaldo: `register_provider()` llama a `force_link_builtins()` ([provider_impl.rs:247](crates/varn-builtins/src/provider_impl.rs#L247)), que registra los 33 arrays `__VARN_LINK_MARKER_*` (módulos host, globals y primitivas); ese array — emitido por el macro en [varn_contract.rs:799](crates/varn-op-macros/src/varn_contract.rs#L799) — apunta a **los mismos statics** que la sección, y `all_native_ops()` deduplica por `ptr::eq`. Sección truncada o no, la tabla queda completa y con la misma firma tipada.

Consecuencias:
1. Corregir la nota de `CLAUDE.md`: `codegen-units = 1` no es un requisito de corrección.
2. Único dato a favor de CU=1 medido aquí: **−10,4 % de tamaño de binario** (1,45 MB). Los tiempos de build no son comparables en esta medición (el build CU=1 recompiló también las dependencias externas), así que la decisión CU=1 vs 16 debe tomarse con un benchmark propio, no con estos números.
3. El comentario de [dispatch.rs:72-77](crates/varn-builtins/src/dispatch.rs#L72) ("un op puede aparecer en ambas listas como dos entradas distintas; la de sección es la autoritativa") es falso: al ser el mismo static, `ptr::eq` lo colapsa. El orden no es load-bearing.

### 5.2 `panic = "abort"` + `catch_unwind`

`[profile.release]` fija `panic = "abort"`, y [host/mod.rs:350](crates/varn-vm/src/exec/host/mod.rs#L350) usa `catch_unwind` para detectar la ausencia de `LocalSet` y caer al `thread::sleep`. Con `abort`, ese `catch_unwind` no puede recuperar: el proceso muere.

Alcance real, para no exagerarlo: la rama solo se alcanza si `Handle::try_current()` es `Ok`, es decir, si hay un runtime Tokio activo sin `LocalSet`. Hoy ningún camino del CLI ejecuta la VM dentro de un runtime Tokio (`varn-lsp` no depende de `varn-vm`), así que la rama está inalcanzable en la práctica. Queda como trampa latente para cualquier host embebido o para el momento en que el LSP evalúe código. Arreglo correcto: sustituir el `catch_unwind` por una comprobación explícita de `LocalSet` (o por `tokio::task::LocalSet::try_id`), no por más `catch_unwind`.

## 6. Divergencia documentación ↔ código

**`docs/CRATES_STATE.md`**
- Dice "16 crates"; hay 21. Faltan `varn-base`, `varn-utilities`, `varn-op-macros`, `varn-lsp`, `varn-pm`.
- El mermaid tiene `Diagnostics --> Core`, **invertido**: `varn-core/Cargo.toml` declara `varn-diagnostics`. Faltan las aristas problemáticas (`VM → Builtins → op-macros → parser`, `pipeline → lsp`).
- La columna "Cobertura de Tests 90-99 %" no tiene respaldo alguno: 19 archivos con `#[cfg(test)]` y 86 `#[test]` sobre 106 787 líneas. Es una métrica inventada; borrarla es mejor que dejarla.

**`docs/ARCHITECTURE.md`**
- "16 crates especializados" (mismo error).
- `varn-core` descrito como "Cero dependencias internas": depende de `varn-diagnostics` y `varn-base`.
- "134 opcodes": el enum `OpCode` tiene **137** variantes.
- El flujo `varn-opt → varn-backend` se dibuja como fases secuenciales, pero `varn-opt` **depende** de `varn-backend` y lo invoca desde dentro ([varn-opt/src/lib.rs:64](crates/varn-opt/src/lib.rs#L64)); no es una etapa posterior.

**`docs/RUNTIME_ARCHITECTURE.md`**
Documenta un event loop Tokio con `LocalSet` por hilo, `Suspend::Task` y un "Runtime Scheduler" como columna vertebral del `await`. Ese scheduler es el código muerto de §2.1. Lo que se ejecuta es espera bloqueante con `mpsc::recv` en la VM. El documento describe un diseño, no el sistema. `README.md` repite la afirmación.

## 7. Nombres y fronteras de crates (juicio, no defecto)

| Crate | Qué contiene | Problema | Propuesta |
|---|---|---|---|
| `varn-opt` (15 714 L) | Compilador completo AST→HIR→SSA→bytecode | El nombre dice "optimizador"; `CLAUDE.md` y `ARCHITECTURE.md` necesitan una nota aclaratoria *porque* el nombre falla | `varn-compiler` |
| `varn-backend` (1380 L) | Liveness + regalloc post-pass sobre bytecode | "Backend" sugiere codegen; y `varn-opt` lo consume por dentro | `varn-regalloc` |
| `varn-types` (6548 L) | `VmValue`, `FunctionProto`, `Chunk`, bytecode | Choca con el sistema de tipos, que vive en `varn-checker/types/` y `varn-core/cg_ty.rs` | `varn-bytecode` |
| `varn-base` (185 L) | `TypeTag` + `TypeFlags` | Dos crates de fundación sin criterio separador; todos sus consumidores (`core`, `types`, `vm`) ya dependen de `varn-core`, que ya depende de `varn-base` | fusionar en `varn-core` |
| `varn-utilities` (366 L) | `chalk.rs`, `colors.rs`, `terminal.rs` | Nombre genérico prohibido por `<domain_modularization>`; el contenido es un dominio real y muy usado (49 archivos) | `varn-term` |
| `varn-diagnostics` (606 L, 0 deps) | Diagnósticos | Solo 4 archivos lo importan directo; `varn-core` re-exporta su API completa en 4 líneas ([error.rs:1](crates/varn-core/src/error.rs#L1)) | fusionar en `varn-core` |

Consolidando `base` + `diagnostics` + `utilities`: 21 crates → 19, sin perder ninguna frontera real.

`varn-core` también deriva a bolsa: `ast/` + `opcode.rs` + `intrinsics.rs` + `paths.rs` + `doc.rs` + `time.rs`. Con `time.rs` muerto (§2.1) el problema se reduce solo.

Además, `varn-debug/src/colors.rs` es un archivo de una línea: `pub use varn_utilities::colors::*;`, y 16 sitios importan `varn_debug::colors`. Cadena de re-export por colores ANSI; deben importar `varn-term` directamente.

## 8. Gobierno de tamaño

Seis archivos cruzan el umbral de refactor obligatorio (1000 líneas):

| Archivo | Líneas |
|---|---|
| `varn-opt/src/ssa/emit.rs` | 1635 |
| `varn-opt/src/ssa/build/expr.rs` | 1513 |
| `varn-opt/src/hir/lower/expr.rs` | 1260 |
| `varn-opt/src/hir/lower/decl.rs` | 1196 |
| `varn-opt/src/ssa/build/stmt.rs` | 1089 |
| `varn-types/src/chunk.rs` | 1052 |

Cinco de seis en `varn-opt`, y quince archivos más entre 700 y 1000. El subárbol `varn-vm/src/exec/` son **12 806 líneas** (67 % del crate) — es el candidato natural a división por dominio (`dispatch/`, `calls/`, `host/`, `tasks/` ya existen como archivos, no como fronteras).

## 9. Higiene de manifiestos y repositorio

- **No hay `[workspace.package]`**: `version = "0.1.0"` y `edition = "2021"` repetidos 21 veces.
- **No hay `[workspace.lints]`** — ver §2.2.
- Dependencias fuera de `[workspace.dependencies]`: `postcard` (4 crates), `tokio` (4, con features divergentes `["rt"]` / `["full"]` / `["time","rt"]` que unifican a `full` de todos modos), `semver` (2), `ureq` (2), `sha2` (2), `hex` (2).
- `varn-debug` declara `rustc-hash = "1"` en vez de `{ workspace = true }`.
- `varn-lexer` tiene un bloque `[lib] name/crate-type` redundante (son los defaults).
- `varn-types/src/bin/sizeof.rs` y el bin `debug_binder` de `varn-cli` se compilan siempre; son herramientas de dev.
- `crates/varn-core/src/diagnostics/` es un **directorio vacío** sin `mod` que lo declare.
- Basura en la raíz: `temp_test_file.txt` (21 B) está **trackeado**; `temp_simple.txt` (19 B) está silenciado con una línea propia en `.gitignore` (`.gitignore:24`) en lugar de un patrón.

## 10. Eficiencia estructural

[`find_native_op_entry`](crates/varn-builtins/src/dispatch.rs#L138) recorre `all_native_ops()`, que en cada llamada construye un `Vec` de 313 entradas deduplicando con `ptr::eq` contra todo lo acumulado — O(n²) ≈ 49 000 comparaciones y una asignación por invocación. Se llama desde [varn-vm/src/jit/helpers.rs:77](crates/varn-vm/src/jit/helpers.rs#L77), una vez por cada `CallNativeOp` que el JIT baja, y desde `describe_op`, `build_module` y `collect_module_fields`. El crate ya tiene la estructura correcta para esto: `TABLE` (`OnceLock<FxHashMap<u64, DispatchEntry>>`), que `dispatch_runtime_op` y `native_op_fn` sí usan.

Arreglo: un segundo `OnceLock<FxHashMap<u64, &'static NativeOpEntry>>` y `all_native_ops()` cacheado en `OnceLock<Vec<_>>`.

**Sin medición.** Es un hallazgo estructural (asignación y O(n²) evitables en un camino que ya tiene índice); el impacto en tiempo de compilación JIT no se benchmarkeó y no debe declararse hasta medirlo.

## Orden recomendado

1. **Borrar el código muerto verificado** — riesgo nulo, gana claridad inmediata: `varn-runtime/src/scheduler.rs` (451 L) + `TaskRunner`/`TaskId`, `varn-core/src/time.rs` (155 L), `varn-backend/src/ir.rs` (144 L) + los 153 L de `liveness.rs`, la FFI de `varn-lexer` (68 L). Con `scheduler.rs` fuera, `tokio` sale de `varn-runtime`.
2. **Corregir `CLAUDE.md` (§5.1) y las tres docs de §6.** La documentación que describe un scheduler inexistente es peor que la ausencia de documentación: dirige el trabajo futuro hacia una arquitectura que no está ahí.
3. **Quitar las 12 dependencias sin uso** y añadir `[workspace.package]` + `[workspace.lints]` con `unreachable_pub` / `unused_crate_dependencies`. Esto convierte §2 y §3 en un check automático.
4. **Bajar a `pub(crate)`** las ~100 funciones sin consumidor externo que se decida conservar, para que el lint vuelva a ver las fronteras.
5. **Fusionar `varn-base` + `varn-diagnostics` en `varn-core`; renombrar `varn-utilities` → `varn-term`** (21 → 19 crates).
6. **Renombrar `varn-opt` → `varn-compiler` y `varn-backend` → `varn-regalloc`**: mecánico, y elimina la necesidad de notas aclaratorias en las instrucciones del agente.
7. **Cortar `varn-pipeline → varn-lsp`**: mover `lsp_debug.rs` y `debug/types.rs` a `varn-cli`, que ya depende de ambos.
8. **Unificar los tres drivers del frontend** (§4c) y **mover el parseo de contratos de `varn-op-macros` a un build-script** (§4a). El trabajo grande, y el que más deuda paga.
9. **Dividir `varn-vm/src/exec/` y los 6 archivos >1000 líneas** por dominio.
