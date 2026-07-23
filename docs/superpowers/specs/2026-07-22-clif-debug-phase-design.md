# Diseño — `vn debug -p clif`: introspección del backend Cranelift

**Fecha:** 2026-07-22
**Estado:** aprobado (pendiente de plan de implementación)
**Alcance:** añadir una fase de inspección del JIT/CLIF al comando `vn debug`. No es un comando nuevo; es una fase más (`-p clif`) del dashboard existente.

## Contexto y motivación

`vn debug -p <phase>` ya expone casi todo el pipeline: `tokens`, `ast`, `check`,
`bytecode`, `hir`, `ssa`, `symbols`, `types`, `graph`, `caps`, `lsp`, etc.
Cubre desde el lexer hasta la SSA del compilador. **Hay un hueco: la capa JIT no
tiene ninguna vista.** No existe forma de inspeccionar, sin correr el programa,
qué hace el backend Cranelift con una función.

Durante la campaña de ensanchado de opcodes CLIF (Fases 5a/5b) el único
diagnóstico disponible fue `VARN_CLIF_TRACE=1`, que imprime `CLIF ROUTE <fn>` /
`CLIF BAIL <fn>: <razón>` durante la ejecución — sin IR, sin lattice de kinds,
sin código generado. Cazar un miscompile (p.ej. el bug "Duration hours", donde
`DivInt` producía un float mal-interpretado como int) requirió bisección manual
de opcodes. Esta fase da la visibilidad estructurada que faltaba.

## No-objetivos

- No ejecuta el programa (es inspección estática, `no_run`, como el resto de
  `vn debug`).
- No es un tier-differential (comparar interp vs clif en runtime) — descartado
  explícitamente por el owner; es otra herramienta.
- No cubre el codegen del template JIT (posible fase futura `-p tjit`, fuera de
  alcance).
- No simboliza las llamadas a helpers en el disasm en v1 (ver Limitaciones).

## UX / invocación

Sigue el idiom de sub-fases existente (`lsp:hovers`, `types:N`):

```bash
vn debug -p clif        file.vn      # las 4 vistas
vn debug -p clif:ir     file.vn      # solo CLIF IR
vn debug -p clif:asm    file.vn      # solo disasm x86-64
vn debug -p clif:kinds,clif:route    # mezcla de vistas
vn debug -e "function f(a:int):int = a*2 + 1"   # inline, como el resto
```

- **Recursivo por función**: recorre el proto top-level y todos los protos
  anidados vía `chunk.constants` (reusa el patrón de
  `disasm_impl::disassemble_recursive`).
- **Incluido en `-p all`**. Si resulta ruidoso en la práctica se puede excluir;
  decisión diferida a la validación.

Sub-fases de `clif:`

| token | vista |
|-------|-------|
| `route` | decisión ROUTE/BAIL + razón |
| `kinds` | lattice de kinds por registro/bloque |
| `ir`    | CLIF IR textual |
| `asm`   | disasm x86-64 del código generado |
| `all`   | las 4 (equivale a `clif` a secas) |

`clif` sin sufijo = las 4 vistas.

## Las cuatro vistas (por función)

1. **ROUTE / BAIL + razón.** Llama a `try_compile`. Imprime `ROUTE` o
   `BAIL: <razón>` (p.ej. `unsupported opcode DivInt`). Es `VARN_CLIF_TRACE`
   pero estructurado y sin ejecutar. La vista más barata.
2. **Lattice de kinds.** El resultado de `kind_flow`: el `K`
   (`Int`/`Bool`/`Boxed`/`Global`/`Poison`/`Mixed`/`Unset`) de cada registro en
   la entrada de cada bloque. Explica POR QUÉ un registro bailea ("boxed use of
   Int register") o por qué un valor recibe cierta representación.
3. **CLIF IR textual.** `ctx.func.display().to_string()` — bloques,
   instrucciones clif, params. Lo que Cranelift realmente va a compilar.
4. **Disasm x86-64.** Los bytes finalizados del `JitBuffer` decodificados. v1:
   crudo (las llamadas muestran direcciones centinela porque los helpers son
   dummy). Muestra la forma del codegen y bugs de regalloc/isel a nivel
   instrucción.

**Comportamiento en BAIL:** una función que bailea muestra `route` + `kinds`
solamente (no llegó a haber IR ni buffer). ROUTE muestra las 4.

## Arquitectura

`varn-jit` produce los datos, `varn-debug` los renderiza, `varn-pipeline`
orquesta (ya depende de ambos + `varn-vm`). La inspección **no ejecuta**, pero sí
usa los helpers **reales** (direcciones de función + layouts probados) para que
el IR/disasm sean fieles a producción. Esos helpers son estáticos —no requieren
un `ExecCtx` vivo—, así que se construyen con un builder libre extraído del
bloque ya existente en `varn-vm::frame::compile_jit` (mejora DRY: hoy ese literal
está inline). Actualización sobre la decisión inicial de "helpers dummy": se
descartó porque un `dummy()` de ~110 campos es frágil y volvería el codegen de
ops de array/objeto/native-op no-fiel (offsets a cero); el builder real es más
simple y fiel, y `varn-pipeline` ya enlaza `varn-vm`.

### Componente A — `varn-jit/src/clif/debug.rs` (nuevo)

```rust
pub struct ClifInspection {
    pub route: Result<(), String>,       // Ok = ROUTE; Err = razón del BAIL
    pub kinds: KindReport,               // lattice por bloque/registro
    pub clif_ir: Option<String>,         // IR textual (None si bailó antes)
    pub code: Option<CodeBytes>,         // bytes del buffer (None si bailó)
    pub frame_aware: bool,
}

pub struct CodeBytes {
    pub bytes: Vec<u8>,
    pub entry_off: usize,                // offset del wrapper
    pub raw_off: usize,                  // offset de la fn raw
}

pub struct KindReport {                  // HashMap<block_start, Vec<K>> serializable
    pub blocks: Vec<(usize, Vec<String>)>,
    pub nregs: usize,
}

pub fn inspect(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
) -> ClifInspection;
```

- Reutiliza `try_compile` como fuente única de verdad. Para capturar el IR sin
  duplicar el lowering, se **enhebra un sink de debug opcional** por
  `try_compile`:
  `try_compile(..., debug: Option<&mut ClifDebugSink>)`. El path de producción
  pasa `None` (coste cero, sin drift). Cuando es `Some`, registra el `KindReport`
  (ya computado por `kind_flow` en `lower.rs:436`), `ctx.func.display()` justo
  antes del codegen (`lower.rs:900`), y los bytes del buffer tras la
  concatenación raw+wrapper (`try_compile`, antes de `make_executable`).
  `inspect` corre `try_compile` con el sink activo y capta también el `Result`
  (route/bail).
- Helpers: **reales**, vía `varn-vm::frame::build_jit_helpers()` (nuevo builder
  libre extraído del literal inline de `compile_jit`; `compile_jit` pasa a
  llamarlo — DRY). No requiere `ExecCtx` vivo.
- Linker: el `impl` unitario existente (`lower.rs:93`, `static_target -> None`)
  sirve como linker de inspección; toda cross-call toma el fallback, irrelevante
  en inspección.

### Componente B — `varn-debug/src/clif.rs` (nuevo)

- Entrada: `debug_clif(proto: &FunctionProto, flags: &DebugFlags, helpers: &JitHelpers)`
  (mismo patrón que `debug_bytecode`; el pipeline inyecta los helpers reales).
- Recorre los protos (recursivo por `chunk.constants`).
- Por cada proto: obtiene `isa` de `clif::shared_isa()`, usa el linker unitario,
  llama a `clif::debug::inspect(proto, consts, helpers, isa, &linker)`.
- Renderiza las 4 secciones según los sub-flags activos.
- **Disasm x86-64**: nueva dependencia **`iced-x86`** (Rust puro, sin C, limpia
  en Windows), confinada a `varn-debug` (crate solo-debug) para no engordar el
  crate de JIT de producción. Decodifica `CodeBytes.bytes`, etiquetando las dos
  funciones (`entry`/`raw`) por sus offsets.

### Componente C — `varn-debug/src/flags.rs`

- `+ clif: bool` y sub-flags `clif_route`, `clif_kinds`, `clif_ir`, `clif_asm`.
- `parse`: rama `"clif"` (setea las 4) y prefijo `"clif:"` (idéntico patrón a
  `"lsp:"`, split por `+` o listado por comas, tokens `route|kinds|ir|asm|all`).
- Sumar a `any()`, a `all` y al texto de ayuda de fases.

### Componente D — pipeline

- `varn-pipeline/src/compile.rs`, junto al dispatch de `debug.bytecode`
  (línea 54) y en el bucle de módulos (línea 105): cuando `debug.clif`, construye
  `let helpers = varn_vm::frame::build_jit_helpers();` y llama
  `varn_debug::clif::debug_clif(&proto, debug, &helpers)`. Respeta `no_run`.

## Flujo de datos

```
vn debug -p clif file.vn
  → pipeline (read→lex→parse→check→compile, no_run)
  → proto compilado (top-level + anidados)
  → varn-debug::clif::render(proto, flags)
      para cada proto (recursivo):
        helpers = JitHelpers::dummy()
        insp = varn_jit::clif::debug::inspect(proto, consts, &helpers, isa, &UnitLinker)
        imprime route / kinds / ir / (iced-x86 disasm de insp.code) según flags
  → exit 0
```

## Manejo de errores

- BAIL no es error del comando: se reporta como dato (`route: Err(reason)`) y se
  siguen mostrando las vistas disponibles.
- Fallo de `shared_isa()` (target no soportado): error del comando con mensaje
  claro; imposible en las plataformas objetivo (x86-64).
- Proto sin código (interfaz, decl): se salta con nota.

## Testing / validación

- **Snapshot por vista** sobre funciones canónicas en `-e`:
  - `function f(a:int,b:int):int = a*b + 1` → ROUTE, kinds todos `Int`, IR con
    `imul`/`iadd`, disasm con la aritmética entera.
  - Una función con `DivInt` (o cualquier op no ruteada) → BAIL con la razón
    exacta, más el lattice hasta el punto de bailout.
- **No-regresión**: `vn debug -p clif tests/main.vn` corre sin panic sobre las
  54 módulos; `vn run`/`bench` intactos (path de producción pasa `None` al sink).
- **Governance de tamaño**: `clif.rs` (varn-debug) y `clif/debug.rs` (varn-jit)
  son módulos nuevos y cohesivos; `ast.rs` (921) y `bytecode.rs` (699) no se
  tocan. Métrica autoritativa: `(Get-Content).Count`.

## Limitaciones (v1, documentadas)

- **Disasm sin simbolizar**: helpers dummy → las llamadas muestran direcciones
  centinela, no nombres. Un mapa `dirección→nombre` (v2) simboliza; requiere
  asignar índices de campo distintos en `JitHelpers::dummy()` y un `[&str; N]`.
- **BAIL = route+kinds**: sin IR ni disasm (no compiló).
- **Solo el subset clif**: el codegen del template JIT queda fuera.
- **Sin coste de runtime**: refleja la decisión de compilación, no el
  comportamiento en ejecución (eso sería el tier-differential, otra herramienta).

## Decisiones tomadas (y alternativas descartadas)

1. **Sink de debug enhebrado en `try_compile`** vs lowering duplicado en
   `debug.rs`. Elegido el sink: una sola fuente de verdad, sin riesgo de drift
   entre el path real y el de inspección.
2. **`iced-x86`** vs feature `disas`/capstone de `cranelift-codegen`. Elegido
   `iced-x86`: Rust puro, sin toolchain C, build limpio en Windows; y confinado
   al crate solo-debug.
3. **Helpers reales vía builder extraído** vs `JitHelpers::dummy()` de ~110
   campos. Elegido el builder real: (a) el pipeline ya enlaza `varn-vm`, así que
   no hay dependencia nueva; (b) no requiere `ExecCtx` vivo (los helpers son
   direcciones estáticas + layouts probados); (c) IR/disasm fieles a producción
   (un dummy dejaría el codegen de array/objeto/native-op con offsets a cero);
   (d) DRY — hoy el literal de helpers está inline en `compile_jit`; extraerlo a
   `build_jit_helpers()` lo comparte con la ruta de debug.
4. **Fase de `vn debug`** vs comando nuevo. Elegido fase: reusa toda la
   plomería (`-e`, `no_run`, parsing de fases, recursión por protos).

## Archivos afectados

| archivo | cambio |
|---------|--------|
| `crates/varn-jit/src/clif/debug.rs` | **nuevo** — `inspect`, `ClifInspection`, `KindReport`, `CodeBytes`, `ClifDebugSink` |
| `crates/varn-jit/src/clif/mod.rs` | `pub mod debug;` |
| `crates/varn-jit/src/clif/lower.rs` | `try_compile`/`lower_raw` ganan param `Option<&mut ClifDebugSink>`; captura kinds (l.436) + IR (l.900) + bytes |
| `crates/varn-vm/src/frame.rs` | extraer `pub fn build_jit_helpers() -> JitHelpers` del literal inline de `compile_jit`; `compile_jit` pasa a llamarlo (DRY) |
| `crates/varn-debug/src/clif.rs` | **nuevo** — `debug_clif` renderer + disasm iced-x86 |
| `crates/varn-debug/src/flags.rs` | `clif` + sub-flags, parse, `any`/`all`/ayuda |
| `crates/varn-debug/src/lib.rs` | `pub mod clif;` |
| `crates/varn-debug/Cargo.toml` | dep `iced-x86` |
| `crates/varn-pipeline/src/compile.rs` | wire de `debug.clif` (2 sitios: proto principal l.54, bucle de módulos l.105) |
| `docs/CLI_INSPECT.md` | documentar la fase `clif` y sus sub-fases |
```
