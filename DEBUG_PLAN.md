# DEBUG_PLAN.md — Refactor de `crates/varn-debug` a registro de fases declarativo

> Especificación de implementación, verificada contra el código el 2026-09-19 (toda afirmación tiene evidencia `archivo:línea`).
> Objetivo: arquitectura modular y funcional, sin boilerplate por fase, con modelo de datos intermedio y tests golden.
> **Restricción inviolable: el modo `plain` de cada fase debe quedar byte a byte idéntico** (salvo las fases eliminadas en §7).
> Este documento se autoconsume: el agente que lo ejecute no necesita contexto previo más allá del repo.

---

## 1. Estado actual (inventario)

Crate `crates/varn-debug` — 3.649 líneas, 22 archivos, 0 tests.
Consumidores: `varn-pipeline` (orquesta las fases), `varn-cli/src/commands/debug.rs`, `varn-cli/src/inspect_lsp` (vistas LSP: `types` y `lsp:*`), `varn-cli/src/bench/source.rs:62` (`DebugFlags::default()`).

| Archivo | Líneas | Rol | Fase(s) |
|---|---|---|---|
| `flags.rs` | 369 | struct-god de 41 bools + parser + `PHASES` + `print_phases` | todas |
| `ast.rs` | 916 | render de árbol AST (`print_stmt/print_expr/print_decl`) | ast |
| `bytecode.rs` | 795 | desensamblado de bytecode + `op_color` + `const_hint` | bytecode |
| `tiers.rs` | 255 | clasificación clif/gate/bail (`debug_tiers`, `debug_bails`) | tiers, bails |
| `roots.rs` | 238 | raíces GC por safepoint | roots |
| `clif.rs` | 196 | vistas route/kinds/ir/asm/check + disasm x86 | clif |
| `typeloss.rs` | 179 | opcodes genéricos vs tipados | typeloss |
| `loop_diagnostics.rs` | 137 | loops hoisteables (colgado de bytecode) | bytecode |
| `expr.rs` | 86 | `check:types` diffeable (devuelve `String`) | check:types |
| `summary.rs` | 84 | tamaños y top-10 | summary |
| `symbols.rs` | 75 | tabla de símbolos | symbols |
| `tir.rs` | 66 | dump TIR + verify + cobertura | tir, tir:check |
| `scope.rs` | 63 | árbol de scopes | scope |
| `modules.rs` | 43 | imports/exports | modules |
| `tokens.rs` | 37 | tabla de tokens | tokens |
| `cap_trace.rs` | 14 | capabilities requeridas | caps |
| `binds.rs`, `consts.rs` | 15+14 | stubs "(not implemented)" | (eliminar) |
| `colors.rs` | 1 | re-export de `varn_core::term::colors` | (fusionar en render.rs) |
| `error.rs`, `lib.rs` | 46 | `CliError`, `resolved_copy` | — |

---

## 2. Problemas confirmados (con evidencia)

1. **5 sitios por fase nueva**: campo en `DebugFlags` + match en `DebugFlags::parse` + entrada en `PHASES` + línea en `print_phases` + `if debug.x` en `varn-pipeline/src/{lex,parse,check,compile,execute}.rs`.
2. **`-p all` con lista manual desactualizada** (`flags.rs`, rama `"all"` del parse): omite `typeloss`, `roots`, `gc`, `tir`, `clif:check`, `check_types`; incluye flags muertos (`expr`, `info`).
3. **Flags muertos** (parseados, jamás leídos fuera de `flags.rs`, verificado con búsqueda global): `expr`, `expr_range`, `symbols_all` (solo lo escribe el parser), `errors`, `calls`, `info`. Además `ReplArgs.debug_bytecode` (`cli.rs:143-146`) es ignorado por `repl.rs:6`.
4. **Stubs**: `binds.rs` y `consts.rs` imprimen "(not implemented)".
5. **`build_jit_helpers()` repetido**: `pipeline/compile.rs:60,73,118` — hasta 3 veces por corrida.
6. **Encabezados `=== MODULE ===` triplicados**: `compile.rs:132,141,150`.
7. **Constantes ANSI copiadas a mano** (`BOLD/DIM/R/...`) en 7 archivos: bytecode, summary, tiers, roots, clif, typeloss, loop_diagnostics. `colors.rs` existe y solo re-exporta.
8. **Walk de `FunctionProto` anidadas copiado 6 veces**: tiers, roots, clif, bytecode, summary, typeloss.
9. **`constants_for_inspect` duplicado (2)**: `clif.rs:88-105` y `tiers.rs:107-125`; `roots.rs` reusa el de tiers.
10. **Render del grafo vive en el pipeline**: `print_module_graph`/`print_graph_node`/`shorten_path` en `pipeline/compile.rs:202-253`.
11. **Sin tests ni goldens** (0 resultados en `crates/*/tests` y `tests/`), pese a que `expr.rs` demuestra el patrón correcto: computar → `String` → imprimir.
12. **`any()` por comparación de struct** (`*self != Self::default()`): el propio comentario admite que la lista manual de campos fue un bug (`pipeline::run:72` toma el camino cacheado sin flags y la fase desaparece en silencio).

**Conservar tal cual** (verificado; NO eliminar):
- `resolved_copy` (`lib.rs:32`): 3 consumidores (`tiers.rs:75`, `roots.rs:101`, `clif.rs:33`) y documentado en `docs/VM_ARCHITECTURE.md:196` y `docs/TIR_ETAPA_4_GLOBALS.md:85`.
- `trace`: vivo (`pipeline/lib.rs:71-94`, `execute.rs:44-83`, `ExecSettings::from_env`). No es una fase del registro.
- `gc` + `needs_execution()`: `commands/debug.rs:26`, `execute.rs:166-169`.
- `types`, `types_all`, `types_range` y los 7 `lsp_*`: los consume `varn-cli/inspect_lsp/{types.rs,dashboard.rs}`, NO el pipeline.
- `fn_filter`: lo leen `bytecode.rs:125`, `tiers.rs:141`, `roots.rs:104`, `clif.rs:46`, `typeloss.rs:112`.

---

## 3. Arquitectura destino

```
crates/varn-debug/src/
  phase.rs       trait Phase + Stage + PerModule (solo metadatos)
  registry.rs    tabla estática de fases; parser de -p; print_phases; grupos
  selection.rs   PhaseSel (bitflags u64) + SubModes + Filters
  flags.rs       DebugFlags = API de compatibilidad sobre selection.rs
  report.rs      Report {Tree, Rows, Text, None}
  fmt.rs         Format {Plain, Text}
  render.rs      banners (Section) + paleta ANSI única (absorbe colors.rs)
  walk.rs        for_each_fn + constants_for_inspect (únicos)
  error.rs       CliError (igual)
  lib.rs         re-exports + resolved_copy()
  phases/
    tokens.rs  ast/{mod,stmt,expr,decl}.rs  modules.rs  symbols.rs
    check_types.rs (hoy expr.rs)  bytecode/{mod,ops,loops}.rs
    scope.rs  cap_trace.rs  summary.rs  typeloss.rs  tir.rs
    tiers.rs (tiers+bails comparten classify)  roots.rs  clif.rs

crates/varn-pipeline/src/debug/          (opción A: orquestación, no varn-debug)
  ctx.rs         DebugCtx (datos por etapa) + memoización JitHelpers/ISA
  run_stage.rs   itera registry::ALL; itera el grafo por path ordenado
  graph.rs       print_module_graph / print_graph_node / shorten_path
  emit_debug.rs  helper + sink
```

Principio: **una fase nueva después del refactor = 1 archivo + 1 línea en el registro**. Todo lo demás (parser, `--list-phases`, `-p all`, orquestación) se genera del registro.

Costo de una fase nueva, antes vs después (el argumento anti-boilerplate medido):

| | Hoy | Después |
|---|---|---|
| Sitios a editar | 5 (flags.rs ×3, pipeline, a veces CLI) | 2 (archivo de la fase + `registry::ALL`) |
| Pegamento | 15–25 líneas (campo, match, entradas de PHASES y print_phases) | 5 one-liners de metadatos del trait |
| Riesgo de drift | alto (`-p all` ya se desincronizó) | cero (grupos derivados) |

### 3.1 Trait `Phase`

```rust
pub enum Stage { Lex, Parse, Check, Compile, Exec }
pub enum PerModule { No, Graph }   // Graph = iterar graph.modules por path ordenado

pub trait Phase: Sync {
    fn id(&self) -> &'static str;                    // "bytecode"
    fn aliases(&self) -> &[&'static str] { &[] }     // caps => ["cap-trace", "cap"]
    fn title(&self) -> &'static str;                 // única fuente de --list-phases
    fn stage(&self) -> Stage;
    fn per_module(&self) -> PerModule { PerModule::No }
    /// false para las sweep/exec-only: tir, tir:check, check:types, roots,
    /// typeloss, gc, clif:check (miembros de vistas, no de "all"). La lista
    /// exacta de pertenencia está en §3.3.
    fn in_all(&self) -> bool { true }
    fn groups(&self) -> &[&'static str] { &[] }      // "check" => symbols, ...
    fn collect(&self, ctx: &DebugCtx, sel: &Selection) -> Report;
    fn render(&self, rep: &Report, fmt: Format, w: &mut dyn Write);
}
```

> **Nota (opción A):** `collect`/`render` toman `DebugCtx`, que vive en
> `varn-pipeline`. Por el ciclo de dependencias, el trait que implementa
> `phases/` en `varn-debug` es **solo metadatos** (lo implementado hoy:
> `id`/`aliases`/`title`/`stage`/`per_module`/`in_all`/`groups`). El despacho
> `collect`→`render` vive en `varn-pipeline::debug::run_stage`, que conoce el
> `DebugCtx` y llama a las funciones de render de cada fase (hoy ya reciben
> datos de crates bajos). Pendiente de confirmar en el Paso 3 si conviene
> partir el trait en `PhaseMeta` (varn-debug) + despacho (pipeline).

`registry::ALL: &[&dyn Phase]`, en orden de `Stage`. De ahí se generan (nunca a mano): `PHASES`, `print_phases`, `DebugFlags::parse`, el error de fase desconocida con lista de válidos, `-p all` y los grupos (`check`, `clif:all`, `roots:all`, `lsp:all`).

### 3.2 `DebugCtx` (datos por etapa, sin lógica)

> **Decisión (opción A, 2026-09-19):** `DebugCtx` y `run_stage` viven en
> **`varn-pipeline`**, no en `varn-debug`. Motivo: `varn-pipeline` ya depende de
> `varn-debug` (`crates/varn-pipeline/Cargo.toml`), así que la dirección inversa
> sería un ciclo. `varn-debug` conserva `Phase` (solo metadatos), `registry`,
> `selection`, `render`, `report`, `walk`, y las funciones de render por fase
> (que reciben datos de crates bajos: `varn-core`/`varn-checker`/`varn-tir`/
> `varn-types`/`varn-jit`). El grafo (`ModuleGraphBuild`) y el resultado del
> checker nunca cruzan a `varn-debug`; el pipeline los tipea y despacha.

```rust
pub struct DebugCtx {
    pub path: String,
    pub source: String,
    pub tokens: Option<&[Token]>,
    pub lexeme_buf: Option<&[u8]>,
    pub program: Option<&Program>,
    pub check: Option<&varn_checker::CheckResult>,
    pub proto: Option<&FunctionProto>,        // entry
    pub graph: Option<&ModuleGraphBuild>,     // para PerModule::Graph
    pub tir: Option<&TirModule>,
    pub gc_report: Option<String>,            // Stage::Exec
    // memoización: 1 sola construcción por corrida (corrige problema 5)
    jit: OnceCell<JitHelpers>,
    isa: OnceCell<Option<varn_jit::OwnedTargetIsa>>,
}
```

- Constructores por etapa (`DebugCtx::lex(...)`, `::parse(...)`, `::check(...)`, `::compile(...)`, `::exec(...)`): el pipeline solo ensambla datos, no conoce fases.
- `jit()` / `isa()` devuelven lo memoizado. Las fases JIT (tiers, bails, roots, clif) lo obtienen de aquí; **`pipeline/compile.rs` deja de llamar `build_jit_helpers`**.
- `ModuleGraphBuild` viene de `varn-pipeline::module_precompile` (el módulo ya es `pub`): verificar visibilidad de los campos que `phases/graph.rs` necesita (`entry_path`, `deps`, `modules`, `source_hashes`, `package_nodes`) y exponer accesores si hace falta.
- **Invariante de dependencias**: `varn-debug` NO debe depender de `varn-vm` — la arista se cortó deliberadamente para romper un ciclo (`crates/varn-vm/src/gc_report.rs:1-6`). Por eso `gc_report` llega como `String` ya renderizado por la VM, y ninguna fase del registro toca tipos de `varn-vm`. Esta restricción es la que permite que `vn debug` siga compilando sin el backend de ejecución.

### 3.3 Selección y compatibilidad

```rust
pub struct Selection {
    sel: PhaseSel,        // u64 bitflags, un bit por fase
    sub: SubModes,
    filters: Filters,
}
pub struct Filters { pub fn_filter: Option<String>, pub line_range: Option<(u32, u32)> }
pub struct SubModes {
    clif: ClifView,       // Route | Kinds | Ir | Asm | Check
    roots: RootsView,     // Diff | Summary
    lsp: LspView,         // Hovers|Semantic|Types|Completions|Symbols|Colorize|Hints
    tir_check: bool, check_types: bool, symbols_all: bool, types_all: bool,
}
```

- `DebugFlags` se conserva como **wrapper** con la MISMA API pública que consumen hoy `RunOpts` (`opts.rs`), `inspect_lsp` y `commands/debug.rs`: `parse(&str) -> Result<Self, CliError>`, `any() -> bool` (equivale a `!sel.is_empty()`), `needs_execution() -> bool` (equivale a gc), y campos accesibles `fn_filter`, `trace`, `types`, `types_all`, `types_range`, `lsp`, `lsp_hovers`, ..., `lsp_hints`. Recomendado: mantener esos campos pub en el wrapper, sincronizados dentro de `parse`, para minimizar el diff en `varn-cli/inspect_lsp` (que lee `flags.types_range`, `flags.types_all` y los 7 `lsp_*`).
- `trace` queda FUERA del registro: campo del wrapper (lo consume `execute.rs`; `opts.trace` lo inyecta en `pipeline::run`).
- `format: Format` es campo nuevo del wrapper (default `Plain`).

**Semántica de `parse()` a replicar exactamente** (fuente: `flags.rs:143-311`):
- split por `,`, trim, partes vacías ignoradas.
- Prefijos con sub-modos: `types:`, `symbols:`, `expr:` (muere, ver §7), `lsp:`, `roots:`, `clif:`, `tir:`, `check:`; sub-modos unidos por `+`.
- Alias `cap-trace|cap|caps` → caps. `symbols:all` → symbols + symbols_all. `types:all` / `types:N` / `types:N-M` (rango con `parse_line_range`, conservar la función y sus errores). `expr:N` muere.
- Errores de sub-fase desconocida con lista de sub-fases válidas; error de fase desconocida con lista de ids + hint `--list-phases`.

**Correcciones respecto a hoy**:
- Fases muertas ya no parsean: `expr`, `errors`, `calls`, `info`, `binds`, `consts` (ver §7).
- `check` = `symbols` + `symbols_all` + `types` + `types_all` (hoy además seteaba `binds` y `expr`, muertos).
- `all` = iterar el registro activando cada fase con `in_all()==true`; para las vistas LSP setea `types`/`lsp` en el wrapper. **Pertenencia exacta tras el refactor**: tokens, ast, bytecode(+loop diagnostics), symbols:all, modules, types:all, scope, graph, caps, lsp:all, tiers, bails, summary, clif(route+kinds+ir+asm). Excluidos por diseño (sweep/exec-only): tir, tir:check, check:types, roots, typeloss, gc, clif:check.
- `clif:all` activa route/kinds/ir/asm pero NO `clif:check` (igual que hoy). `roots:all` no activa diff ni summary (igual que hoy).

### 3.4 Formatos

- `Format::Plain`: **byte a byte la salida actual** (mismos `Section`, colores, `eprintln!`). Default.
- `Format::Text`: generaliza el contrato de `expr.rs`/`check:types`: sin color, sin box-drawing, una línea por registro con campos `|`-separados, orden determinista (claves ordenadas; grafo por path ordenado), basename en lugar de ruta absoluta (en Windows las rutas traen `\\?\`), sin tiempos. Header `# <phase-id> <basename>`.
- Nuevo flag CLI: `vn debug --format text|plain` (`DebugArgs`, default `plain`).

### 3.5 Reporte intermedio

```rust
pub enum Report { None, Text(String), Rows(Vec<Vec<String>>), Tree(Vec<TreeNode>) }
pub struct TreeNode { label: String, children: Vec<TreeNode> }
```

Regla: `collect()` no imprime. Fases con métricas propias (roots, typeloss) devuelven un struct específico en su archivo y lo convierten a `Report` al final de `collect`.

### 3.6 `run_stage` (orquestación, vive en `varn-pipeline` — opción A)

```rust
pub fn run_stage(stage: Stage, ctx: &DebugCtx, flags: &DebugFlags, out: &mut dyn Write) {
    for phase in registry::ALL {
        if phase.stage() != stage || !flags.contains(phase.id()) { continue; }
        match phase.per_module() {
            PerModule::No => dispatch(phase, ctx, flags, out),
            PerModule::Graph => for path in ctx.graph_paths_sorted() {
                let mctx = ctx.for_module(path);
                dispatch(phase, &mctx, flags, out);   // el banner lo controla la fase
            }
        }
    }
}
```

- `run_stage` SOLO ordena paths y llama; **el banner por módulo lo controla cada fase** (matiz a preservar: `summary` imprime `=== MODULE X ===` siempre que corre por un módulo; `tiers`/`bails`/`roots` imprimen su header solo cuando tienen contenido — ver `compile.rs:150-171` y `roots.rs:148-152`).
- Corrige el problema 6 (encabezados duplicados) y centraliza la iteración del grafo (hoy 3 bucles distintos en `compile.rs:126-171`).

---

## 4. Especificación por fase (contrato a preservar)

Para cada fase: etapa, entrada, origen, invariantes que los goldens deben congelar.

1. **tokens** · Lex · `tokens + lexeme_buf + path` · tabla `Idx|Loc|Kind|Lexeme`, `Section("tokens")` magenta, footer `"{n} tokens scanned"`.
2. **ast** · Parse · `program` · árbol `├──/└──` con indentación `│   `/`    `; footer `"{n} top-level statements"`. Al migrar, dividir en `phases/ast/{mod,stmt,expr,decl}.rs` (916 líneas hoy; el umbral de gobierno del repo es 1000 — `docs/CRATES_STATE.md` §4).
3. **modules** · Parse · `program` · conteo imports/exports con `{:?}` de specifiers/source (preservar formato).
4. **symbols** · Check · `check_result` · tabla Loc|Kind|Name|Type con tags `[core]/[std]/[usr]` (regla de `symbols.rs:29-44`); footer con `arena.len()`. El parámetro `_flags` muere.
5. **check:types** · Check · `program+source+check` · **la fase modelo**: `render_check_types` devuelve `String`, ordenado por id, `line_col` 1-based, basename del filename. Todo el modo Text del resto de fases imita este estilo. PRESERVAR: las dos tablas (checker por `Expr::id()`, anotaciones por offset) siguen en secciones separadas — es un hallazgo documentado en `expr.rs:51-58`, no una decisión de formato.
6. **bytecode** · Compile · `proto + filters` · headers por proto con indentación por profundidad, flags `[async,gen,has_this,rest,state_size]` (`bytecode.rs:127-146`), índice de constantes `[NNN]` con `const_hint`, tabla `Off|Lin|Opcode|Operands/Hint`, `op_color` por clase (control=amarillo, calls=magenta, loads/stores=cian, objects=azul, aritmética=verde), `short_global_key` (`ruta/absoluta::sym` → `file::sym`), `build_fn_index`, decodificación hi/lo u16. **Leer `bytecode.rs:740-795` completo antes de mover**: la llamada a `print_loop_diagnostics` está dentro de un condicional cuyo alcance exacto hay que reproducir (es la condición oscura del plan — riesgo §10).
7. **loop_diagnostics** (sub-vista de bytecode, no fase propia) · veredictos HOISTED/blocked/skipped/eligible + nota `masked_by_resolution` + `alloc_free_ignoring_global_resolution` (`loop_diagnostics.rs:18-55`).
8. **scope** · Compile · `proto + filename` · SOLO proto raíz + strings del pool; **no recursa a anidadas hoy** — preservar ese comportamiento tal cual.
9. **caps** · Compile · `proto + filename` · trace de `required_caps` (aliases `cap-trace|cap|caps`).
10. **graph** · Compile · `graph` · **migra desde `pipeline/compile.rs:202-253`**: `print_module_graph` (entry + total), `print_graph_node` (con detección de ciclo → `(cycle)`), `shorten_path`. Usa los colores `C_MODULES`/`C_ERRORS` del módulo colors de varn-core.
11. **summary** · Compile · `proto` · totales (funcs, words, consts, exports, over-gate con `varn_jit::SIZE_GATE_WORDS`) + top-10 por words con marca `← excede el gate`; `truncate()` con elipsis; `collect()` recursivo sobre `PoolEntry::Function`.
12. **typeloss** · Compile · `proto + filters` · tabla PAIRS (11 pares exactos, `typeloss.rs:28-40`), `Counts{typed, generic, by_op, members}`, orden desc por generic, detalle `op×n` + members leídos del pool, línea verde cuando no hay pérdidas.
13. **tir** · Compile · `ctx.tir` (el módulo de PRODUCCIÓN) · **Corrección de fidelidad incluida**: hoy `tir.rs:20-27` re-emite el módulo con `Default::default()` en extension_calls/members/set_members, mientras que el compilador emite con las tablas reales del checker (`pipeline/compile.rs:35-40`) — es decir, `-p tir` puede mentir sobre las extensiones justo cuando hay extensiones. Tras el refactor el pipeline entrega en `ctx.tir` el MISMO `TirModule` que compiló (una sola llamada a `emit_module`): el dump pasa a ser el módulo real, no una reconstrucción. `tir:check` = `verify_module` + `Coverage::of().report()` + sondas `from_tir` ssa/proto con `catch_unwind(AssertUnwindSafe(...))` — sin cambios.
14. **tiers** · Compile · `proto + ctx.jit()` · `classify()` aplica el size gate ANTES de `inspect` (orden de producción, doc `tiers.rs:1-15`); `TierRow{Clif|Gate|Bail}`; `constants_for_inspect` (heap-free: solo la fidelidad `is_int` importa); header por módulo SOLO con contenido; respeta `fn_filter`.
15. **bails** · Compile · mismas piezas que tiers, agrupado por causa (`debug_bails`).
16. **roots** · Compile · `proto + ctx.jit() + filters` · todo el bloque de métricas `roots.rs:100-238` es el activo más fino del crate: netting de `unboxed`, `lost = cranelift − ours − unboxed` (con el límite documentado del kind mal inferido), `in_reg`, `roots:diff` (solo filas con `in_reg > 0`), `roots:summary` (sin filas), header por módulo solo con contenido, avisos finales (unboxed / reg_roots / unmatched / lost). **Mover los comentarios de invariantes de `roots.rs:1-30` y `roots.rs:111-131` VERBATIM.**
17. **clif** · Compile · `proto + ctx.jit() + filters` · sub-vistas route/kinds/ir/asm/check; contrato de `clif:check` solo → imprime SOLO violaciones (silencio = sano); disasm con `iced-x86` bajo `#[cfg(target_arch = "x86_64")]` en DOS pasadas independientes (raw y wrapper — el padding desincroniza el decoder; `clif.rs:131-139,165-183`) y dump hex portable en otras arquitecturas; `fn_filter` selecciona qué se RENDERIZA, nunca qué se CAMINA (`clif.rs:43-52`); `resolved_copy` antes de inspeccionar.
18. **gc** · Exec · `gc_report` (texto de `machine.gc_report()`) · única fase que requiere ejecución (`needs_execution()`).
19. **(CLI, fuera del registro)** `types` y `lsp:*` viven en `varn-cli/inspect_lsp` — solo migran a leer `Selection`/`Filters` (`filters.line_range`, `sub.types_all`, `sub.lsp`); su pertenencia a `all`/`check` se mantiene vía el wrapper.

---

## 5. Cambios en `varn-pipeline`

- Reemplazar los `if debug.x` por construcción de `DebugCtx` + una llamada:
  - `lex.rs:35-37` → ctx con `tokens/lexeme_buf` y `run_stage(Stage::Lex, ...)`.
  - `parse.rs:39-43` → ctx con `program`, `Stage::Parse`.
  - `check.rs:70-79` → ctx con `check`, `Stage::Check` (symbols + check:types).
  - `compile.rs:56-171` → ctx con `proto/graph/tir`, `Stage::Compile`. El `TirModule` que va al ctx es el de producción (emitido en `compile.rs:35-40`); se elimina la re-emisión de `tir.rs`.
  - `execute.rs:166-169` → ctx con `gc_report`, `Stage::Exec`.
- Firma para tests: `compile_source_with(source, path, verbose, debug, strict, sink: &mut dyn FnMut(Stage, &DebugCtx))`; la `compile_source` actual queda como wrapper con sink no-op. `emit_debug` (helper del pipeline) delega en `varn_debug::run_stage` con stderr como sink.
- Borrar de `compile.rs`: los ~80 ifs de fases, los 3 bucles de módulos, `print_module_graph`/`print_graph_node`/`shorten_path` (→ `phases/graph.rs`), y las llamadas a `build_jit_helpers`.
- `execute.rs` conserva `trace` (no es fase) y entrega `gc_report` en el ctx de `Stage::Exec`.
- `pipeline/src/opts.rs`: `parse_debug_opt` y `RunOpts` SIN cambios de forma.

## 6. Cambios en `varn-cli`

- `commands/debug.rs`: añadir `--format` (clap `ValueEnum`, default `plain`) → `debug.format`; el resto idéntico (`list_phases`, `parse_debug_opt`, `fn_filter`, `inspect_lsp::run_for`, `no_run` desde `needs_execution()`).
- `cli.rs`: eliminar `ReplArgs.debug_bytecode` (muerto; verificar que solo `repl.rs` lo referencia).
- `inspect_lsp/{types,dashboard}.rs`: solo adaptar al nuevo tipo de flags (misma lógica de filtrado).
- `bench/source.rs:62`: sin cambios (el default sigue existiendo).

## 7. Eliminaciones y consolidaciones

- **Eliminar**: `binds.rs`, `consts.rs`, flags `expr/expr_range/errors/calls/info`, `symbols_all` como flag de primer nivel (pasa a `sub.symbols_all`), `ReplArgs.debug_bytecode`, `colors.rs` (paleta → `render.rs`).
- `print_phases` se regenera del registro (el texto literal de hoy muere; mismo contenido, generado).
- `walk.rs`: `for_each_fn(proto, depth, |depth, proto|)` único; `constants_for_inspect` único (definición canónica: la de `clif.rs`, que es la documentada).
- `truncate()` triplicado (`summary.rs:72-79`, `tiers.rs:245-254`, `typeloss.rs:163-171` — misma semántica: elipsis `…` contando chars) → helper único en `render.rs`.
- `varn-core::term::terminal::Table` ya existe (lo usan `inspect_lsp` y los reportes de bench, con `display_width` consciente de ANSI): los renderers en modo `Text` y las vistas nuevas pueden apoyarse en él. `Plain` conserva el formateo manual exacto (restricción byte a byte).
- `render.rs`: paleta única (`BOLD DIM RED GREEN YELLOW BLUE MAGENTA CYAN R` + `Section`), consumida por todas las fases. Actualizar el `use` en `cap_trace.rs` y en `pipeline/compile.rs` mientras exista.
- `lib.rs`: `resolved_copy` queda; re-exports nuevos: `run_stage`, `Phase`, `Stage`, `DebugCtx`, `Format`, `Report`.

---

## 8. Verificación (criterio de hecho del refactor)

### 8.1 Goldens — el pago real

- Ubicación: `crates/varn-pipeline/tests/debug_golden.rs` (el pipeline ya depende de todo; evita un ciclo de dev-deps con varn-debug).
- Fixtures: `crates/varn-pipeline/tests/fixtures/debug/*.vn` — 6–10 programas pequeños: aritmética tipada, loop con array, clase con fields, generics que caen a dynamic, closure, módulo importado, una función > `SIZE_GATE_WORDS` para forzar `gate`.
- **Paso 0 (ANTES de tocar código)**: generar los goldens en modo `plain` desde HEAD sin modificar, para: tokens, ast, bytecode, summary, typeloss, tiers, bails, roots, clif:route+kinds, check:types, tir, tir:check, graph, scope, caps, modules, symbols. Guardar como `fixtures/debug/<caso>.<fase>.golden.txt`. El refactor queda obligado a mantenerlos verdes → ese es el A/B automatizado.
- Tras migrar cada fase, generar su par `<fase>.text.golden.txt` (nuevo contrato diffeable) y congelarlo como baseline.
- Normalización obligatoria: basename-only, sin `\\?\`, EOL LF (añadir a `.gitattributes`: `crates/varn-pipeline/tests/fixtures/**/*.golden.txt text eol=lf`), sin tiempos, módulos en orden de path.
- Las fases JIT dependen del ISA → marcar esos tests `#[cfg(target_arch = "x86_64")]` (el disasm portable cubre el resto).
- Ejecución: vía el sink de §5 (proceso interno, sin binario `vn`) + smoke end-to-end final con `cargo run -p varn-cli -- debug <fixture> -p <fase>`.

### 8.2 Tests unitarios del registro (`#[cfg(test)]` en varn-debug)

- Toda fase del registro aparece en `print_phases`; `title` no vacío; ids únicos; alias sin colisión con ids.
- `parse("all")` activa exactamente la lista de §3.3; `parse("check")` = symbols:all + types:all; `clif:all` activa route/kinds/ir/asm pero NO clif:check; `roots:all` no activa diff/summary.
- Nombres muertos (`errors`, `calls`, `info`, `expr`, `binds`, `consts`) → error de uso; `tir:xyz`/`clif:xyz` → error con sub-fases válidas.
- `parse_line_range`: `N`, `N-M`, `N-`, `all`, inválidos.
- `any()` equivale a `!sel.is_empty()`; `needs_execution()` solo con gc.

### 8.3 Gates de build

- `cargo clippy --workspace` (lints del workspace; `unused_crate_dependencies` vigila deps muertas; `iced-x86` se queda — lo usa clif:asm).
- `cargo build -p varn-debug -p varn-pipeline -p varn-cli` y `cargo test -p varn-debug -p varn-pipeline`.
- Suite del lenguaje intacta: `tests/main.vn` (1.139 aserciones) según CONTRIBUTING.md. El refactor no toca compilación de producción; confirmar que ningún módulo de producción importaba los símbolos eliminados.

---

## 9. Orden de implementación (cada paso con "hecho" verificable)

1. **Paso 0**: goldens plain desde HEAD (§8.1) → commit de fixtures + goldens. *Hecho cuando: pasan contra el código sin tocar.*
2. Esqueleto: `phase.rs`, `registry.rs`, `selection.rs`, `ctx.rs`, `report.rs`, `fmt.rs`, `render.rs`, `walk.rs` + wrapper `DebugFlags` (parse delega al registro; mismos resultados en los casos de §8.2). *Hecho: tests del registro verdes, crate compila, el pipeline aún usa la API vieja.*
3. Fases triviales al registro: tokens, modules, symbols, scope, caps, ast (split en 4), check:types, graph (migra desde el pipeline). *Hecho: goldens de estas fases verdes.*
4. Fases de análisis: bytecode(+ops,+loops), summary, typeloss, tir. *Hecho: goldens plain + text de estas fases verdes.*
5. Fases JIT: tiers, bails, roots, clif. Memoización de helpers/ISA en ctx. *Hecho: goldens JIT verdes; helpers construidos 1 vez por corrida.*
6. Pipeline: `emit_debug` + sink, borrado de ifs y de `print_module_graph`, gc vía Stage::Exec, `--format` en CLI. *Hecho: `pipeline/compile.rs` sin menciones a fases concretas; smoke `vn debug tests/main.vn -p all` con diff vacío vs HEAD (salvo banners de binds/consts).*
7. Limpieza: eliminar archivos/flags muertos, `colors.rs`; actualizar `docs/CRATES_STATE.md` (tamaños) — la referencia de `docs/VM_ARCHITECTURE.md:196` a `resolved_copy` sigue válida. Suite completa verde.

## 10. Riesgos y mitigaciones

- **Salida plain no idéntica** → goldens del paso 0 por fase; migrar de a una fase y correr goldens entre fase y fase.
- **Matiz de banners por módulo** (summary siempre vs tiers/bails/roots solo con contenido) → modelado como comportamiento de cada fase, no de `run_stage`.
- **Condición oscura en bytecode** (`print_loop_diagnostics`, `bytecode.rs:740-795`) → leer el bloque completo antes de mover; el golden lo congela.
- **`DebugFlags` se clona por corrida** (camino del cache, `pipeline::run`) → wrapper sigue siendo `Clone` barato (u64 + Options + String).
- **Orden de HashMap en fases** → Text exige orden explícito (paths ordenados, claves ordenadas); plain conserva el orden actual (donde ya se ordena: `compile.rs:129-131` sort de paths, `expr.rs` sort por id, `typeloss` sort por generic).
- **Worktrees `.claude/`** contienen copias viejas de `pipeline/compile.rs` — ignorarlas, no son miembros del workspace.
- **`inspect_lsp` consume campos `flags.lsp_*`/`flags.types_*`** → mantener esos campos en el wrapper para no tocar su lógica.

## 11. Fuera de alcance

Reportes de `varn-cli/bench` (usan `CompileRecord` del JIT), formato JSON, cambios visuales al plain actual, reestructurar `varn-lsp`, migrar las vistas LSP fuera del CLI, paralelismo, cambios de semántica de fases (solo arquitectura).
