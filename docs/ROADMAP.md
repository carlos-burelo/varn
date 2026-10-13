# Roadmap — Varn

> Estado parcial. Código manda (AGENTS.md Ley 12). Última sync: 2026-10-13 desde código.

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
