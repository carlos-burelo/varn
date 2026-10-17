# Roadmap — Varn

> Estado parcial. Código manda (AGENTS.md Ley 12). Última sync: 2026-10-16 desde código.

Trabajo futuro verificado, no intenciones. Cada entrada cita su evidencia y
su puerta de validación. Nada aquí se implementa sin pasar por `AGENTS.md`
(checklist de diseño + matriz de validación).

## LSP: paralelismo real del análisis (futuro)

**Hipótesis 2026-10 (verificar en código antes de usar).** Contenido abajo
es hipótesis histórica, no estado. Rutas citadas sin verificar en árbol
actual: `varn-lsp/src/analysis` existe, pero `varn-lsp/src/workspace/resolver.rs`
no existe (`workspace/` = `mod.rs`, `revision.rs`, `std_sources.rs`);
`crates/varn-lsp/tests/design_audit_test.rs` existe (casos h1-h10).
Nada usar sin re-verificar.
Hipótesis: el análisis vive en un solo hilo (`varn-lsp/src/analysis`):
el index bulk es secuencial (~1.1s en este repo tras la serie de
optimizaciones de memoria/throughput) y cada archivo paga el checker
completo (30–600ms). El index IO ya es concurrente; el cómputo no.

**Verificado (no supuesto).**
- `DocumentState: Send` y `DiskResolver: Send + Sync` (sonda de compilación;
  el comentario del `Rc` en `analysis/mod.rs` está obsoleto: cero `Rc` en
  `varn-lsp`). `ModuleLoader: Send + Sync` (`varn-modules/src/loader.rs`).
- Lexer/parser/checker sin `thread_local` (solo comentarios históricos);
  builtins solo `OnceLock`. `thread_local` de vm/jit/compiler no toca esta
  ruta (el LSP no compila en bulk).
- Único bloqueador real: `thread_local! RESOLVER`
  (`varn-lsp/src/workspace/resolver.rs`). Benigno: memo per-thread en
  `varn-modules/src/std_root.rs` (solo perf).
- `Checker::check_with` toma `&dyn ImportResolver`: un `Arc<DiskResolver>`
  entra sin cambiar el checker.

**Fases.**
1. Dueño único, mismo hilo (Ley 3, riesgo ~0): `Analyzer` posee
   `Arc<DiskResolver>`; `with_resolver` lo presta. Sin concurrencia nueva.
2. Pool para bulk: workers acotados corren `run_pipeline` concurrente; el
   hilo dueño solo publica (`index_file` partido en compute/publish, orden
   canónico). Objetivo: index <0.5s en este repo.
3. Protocolo de invalidación (obligatorio antes de mezclar edición+pool):
   epoch por job — `invalidate` en vuelo puede resucitar binds evictados
   (`insert_bind` first-wins); revisar escritura concurrente del caché en
   disco y la raza check-then-insert de `in_flight`.

**No hacer.** Resolvers por worker (reintroduce grafos incoherentes) ni
mergear tablas divergentes (viola Ley 2/3).

**Determinismo (Ley 4).** El orden de `absorb` puede permutar índices
internos entre corridas; las respuestas a nivel texto son idénticas y la
ruta CLI single-thread queda intacta.

**Puerta.** Test de equivalencia paralelo-vs-serial (mismos
exports/definiciones), suite `varn-lsp`, puerta canónica `tests/main.vn` +
`.\scripts\verify.ps1 -Fast`. Sin `memoryStats` suelto como puerta.
Evidencia base: `crates/varn-lsp/tests/design_audit_test.rs` (H1–H10).

## Diferenciadores del lenguaje (futuro, no priorizado)

Registrado 2026-10-16. Eje común: el tipo estático se cumple de punta a
punta (checker → TIR → JIT → caché → arranque), cosa que un runtime de JS
no puede prometer. Orden sugerido: 1, 3, 2, 4. Ninguno empieza antes de
cerrar lo pendiente en `docs/plans/2026-10-16-plan-unificado.md`.

### 1. Rendimiento garantizado por el compilador

- **Idea.** Un atributo `@static` en una función o un modo
  `--strict-static`, que hagan **fallar la compilación** si esa ruta cae a
  opcodes genéricos o el JIT hace bail. En el LSP, marcar en línea las
  líneas dinámicas con su causa.
- **Evidencia (2026-10-16).** El diagnóstico ya existe como herramienta
  de auditoría: `vn debug -p typeloss`, `tiers` y `bails`
  (`crates/varn-debug`). Falta convertirlo en regla del lenguaje.
- **Primer paso.** Fijar la semántica (qué cuenta como degradación) y la
  fase que lo verifica: TIR (`Coverage`/`DynReason`) para la forma y el
  gate de lowering del JIT para el bail.
- **Puerta.** Corpus de casos que deben compilar y fallar; suite canónica
  sin regresiones.

### 2. Ejecutables que arrancan calientes (PGO + código incrustado)

- **Idea.** `vn build` incrusta en el ejecutable el bytecode y el código
  máquina de las funciones calientes, obtenidos de una corrida de
  entrenamiento. Cero calentamiento en la primera ejecución.
- **Evidencia (2026-10-16).** Arranque vacío ~13 ms; la std ya es imagen
  (`varn-modules/src/std_image.rs`); el JIT produce `CodeImage`
  relocalizables con caché persistente (`varn-jit/src/clif/lower.rs`,
  `varn-vm/src/jit/code_cache.rs`); ya existe el sobre `VEXE` de
  ejecutable standalone (`varn-cli/src/main.rs`).
- **Primer paso.** Sección de imágenes JIT dentro del `VEXE`, servida
  por el mismo `CodeStore`; el perfil decide qué funciones entran.
- **Puerta.** `VARN_JIT_CACHE=verify` sobre el ejecutable; tiempo de
  primera corrida medido contra la segunda.

### 3. Capacidades inferidas estáticamente, aplicadas por dependencia

- **Idea.** El compilador deduce qué toca cada módulo (red, disco, FFI,
  procesos) y el gestor de paquetes lo aplica **por dependencia**;
  `vn audit` muestra el reporte antes de ejecutar.
- **Evidencia (2026-10-16).** Existen los flags `--allow-*` y
  `--sandbox` (`vn run --help`) y el campo `FunctionProto.required_caps`
  (`varn-types/src/chunk/proto/definition.rs`), pero el backend lo emite
  vacío (`required_caps: Vec::new()` en
  `varn-compiler/src/ssa/emit/mod.rs`). Posible pérdida de la migración
  HIR→TIR, como la promoción de módulo (`5e652ef0`): verificar antes de
  diseñar.
- **Hallazgo (2026-10-16).** El checker ya propaga `@capability`
  estáticamente: anotar un contrato con `@capability("ui")` exige que todo
  llamador lo declare. Los contratos usan `@permission` (solo runtime,
  `9394b438`) para no activar esa propagación todavía.
- **Primer paso.** Decidir si los contratos pasan a `@capability` y cómo
  se cierra la propagación en la frontera de paquete (por dependencia).
- **Puerta.** Paquete de prueba que pide red sin declararla y debe ser
  rechazado; suite canónica.

### 4. Builds reproducibles y direccionados por contenido

- **Idea.** Mismo código ⇒ mismo artefacto, byte a byte, para paquetes,
  grafos de módulos e imágenes; eso permite caché remota y verificación
  entre máquinas.
- **Evidencia (2026-10-16).** La Ley 4 ya obliga al determinismo; la
  imagen std es idéntica byte a byte con caché fría y tibia
  (`1f264fde`); la huella de contenido es única (`varn_core::fingerprint`).
- **Primer paso.** Sonda que compile el mismo programa en dos procesos
  y dos directorios y compare todos los artefactos.
- **Puerta.** Esa sonda en `verify.ps1`.

