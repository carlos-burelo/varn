# Mapa Rápido del Workspace

Este repo es un monorepo Rust. Para moverse rápido, usa estas reglas antes de leer notas antiguas más abajo.

## Dónde mirar primero

- CLI y orquestación: [crates/varn-cli/src/cli.rs](crates/varn-cli/src/cli.rs), [crates/varn-cli/src/commands/](crates/varn-cli/src/commands/), [crates/varn-cli/src/pipeline/](crates/varn-cli/src/pipeline/)
- Pipeline compartido: [crates/varn-pipeline/src/](crates/varn-pipeline/src/)
- Lexer y parser: [crates/varn-lexer/src/](crates/varn-lexer/src/), [crates/varn-parser/src/](crates/varn-parser/src/)
- Type checker: [crates/varn-checker/src/](crates/varn-checker/src/)
- Codegen: [crates/varn-compiler/src/](crates/varn-compiler/src/)
- VM y runtime: [crates/varn-vm/src/](crates/varn-vm/src/), [crates/varn-runtime/src/](crates/varn-runtime/src/)
- Builtins, módulos y paquetes: [crates/varn-builtins/](crates/varn-builtins/), [crates/varn-modules/](crates/varn-modules/), [crates/varn-pm/](crates/varn-pm/)
- LSP y debug: [crates/varn-lsp/](crates/varn-lsp/), [crates/varn-debug/](crates/varn-debug/)
- Tipos compartidos: [crates/varn-types/](crates/varn-types/), [crates/varn-core/](crates/varn-core/), [crates/varn-base/](crates/varn-base/)
- Tests de integración: [tests/main.vn](tests/main.vn) y [tests/](tests/)
- Docs de referencia: [docs/CLI_REFERENCE.md](docs/CLI_REFERENCE.md), [docs/GETTING_STARTED.md](docs/GETTING_STARTED.md), [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [docs/CRATES_STATE.md](docs/CRATES_STATE.md), [docs/COMPILER_ARCHITECTURE.md](docs/COMPILER_ARCHITECTURE.md), [docs/VM_ARCHITECTURE.md](docs/VM_ARCHITECTURE.md), [docs/RUNTIME_ARCHITECTURE.md](docs/RUNTIME_ARCHITECTURE.md)

## Cómo moverse más rápido

- Empieza por el crate que decide el comportamiento, no por los wrappers.
- Si cambias la CLI, toca juntos `crates/varn-cli/src/cli.rs`, el comando en `crates/varn-cli/src/commands/`, `docs/CLI_REFERENCE.md`, `docs/CLI_INSPECT.md`, `docs/GETTING_STARTED.md` y `README.md`.
- Si cambias sintaxis o análisis, revisa `varn-parser`, `varn-checker` y el test `.vn` más cercano antes de tocar la VM.
- Si cambias ejecución o bytecode, valida `varn-compiler`, `varn-vm`, `varn-runtime` y luego el flujo `vn run tests/main.vn`.
- Si cambias paquetes o módulos, revisa `varn-pm`, `varn-modules` y la documentación de arquitectura asociada.

## Validación rápida

- CLI: `cargo check -p varn-cli` y `target/release/vn.exe --help`
- Cambios de parser/checker/compiler/vm: validar ejecutando la suite de integración `target/release/vn.exe tests/main.vn`
- Integración del lenguaje: `cargo run --bin vn -- tests/main.vn`

## Skills disponibles

- [varn-repo-map](.claude/skills/varn-repo-map/SKILL.md)
- [varn-cli-sync](.claude/skills/varn-cli-sync/SKILL.md)
- [varn-test-triage](.claude/skills/varn-test-triage/SKILL.md)
- [varn-implementation-governor](.claude/skills/varn-implementation-governor/SKILL.md)

## Nota

El contenido legado más abajo sigue siendo útil como referencia histórica, pero para trabajar rápido usa primero este mapa y la [CLI_REFERENCE](docs/CLI_REFERENCE.md).

# Varn Language - CLI Documentation

Lenguaje compilado con tipado estático y VM register-based optimizada.

## Instalación y Build

```bash
# Build en modo desarrollo
cargo build --bin vn

# Build en modo release (optimizado)
cargo build --bin vn --release

# Ejecutar directamente
cargo run --bin vn -- <command> [args]
```

## Comandos Principales

### `run` - Ejecutar programa

Ejecuta un archivo Varn. También es el comando por defecto si no se especifica subcomando.

```bash
vn run <FILE> [-- <ARGS>...]

Ejecutar un archivo Varn

Usage: vn.exe run [OPTIONS] <FILE> [-- <ARGS>...]

Arguments:
  <FILE>     Archivo Varn a ejecutar
  [ARGS]...  Argumentos para el script

Options:
  -v, --verbose  Modo verbose
      --trace    Tracing de ejecución
      --strict   Advertir sobre tipos Dynamic implícitos
  -h, --help     Print help
PS C:\Users\x\dev\varn>


```

**Ejemplos:**
```bash
# Ejecutar un programa (run es implícito)
vn tests/main.vn

# Explícito
vn run tests/main.vn

# Con argumentos para el script
vn run script.vn -- arg1 arg2

# Con tracing
vn run --trace tests/main.vn

# Con debug de todas las fases
vn debug program.vn

# Con verbose
vn run -v program.vn
```

### `check` - Verificar tipos

Verifica el programa sin ejecutarlo (type checking).

```bash
vn check <FILE>

Verificar tipos sin ejecutar

Usage: vn.exe check [OPTIONS] <FILE>

Arguments:
  <FILE>  Archivo Varn a verificar

Options:
  -v, --verbose  Modo verbose
      --strict   Advertir sobre tipos Dynamic implícitos
  -h, --help     Print help
PS C:\Users\x\dev\varn>
```

**Ejemplos:**
```bash
# Verificar tipos
vn check src/main.vn

# Verbose
vn check -v src/main.vn

```

### `eval` - Evaluar código

Evalúa código Varn directamente desde la línea de comandos.

```bash
vn eval <CODE>

Evaluar código directamente desde la línea de comandos

Usage: vn.exe eval [OPTIONS] <CODE>

Arguments:
  <CODE>  Código Varn a evaluar

Options:
  -v, --verbose  Modo verbose
  -h, --help     Print help
```

**Ejemplos:**
```bash
# Evaluar expresión
vn eval "print(1 + 2)"

# Evaluar código más complejo
vn eval "function double(x: int) = x * 2; print(double(21))"

```

### `repl` - REPL Interactivo

Inicia un REPL (Read-Eval-Print Loop) interactivo.

```bash
vn repl

Iniciar REPL interactivo

Usage: vn.exe repl [OPTIONS]

Options:
      --debug-bytecode  Mostrar bytecode generado en cada evaluación
  -h, --help            Print help
```

**Ejemplos:**
```bash
# Iniciar REPL
vn repl

```

### `bench` - Benchmark

Ejecuta benchmarks detallados de rendimiento con métricas del VM.

```bash
vn bench <FILE>

Ejecutar benchmarks de rendimiento

Usage: vn.exe bench [OPTIONS] <FILE>

Arguments:
  <FILE>  Archivo Varn a medir

Options:
      --runs <N>     Número de runs (default: 10) [default: 10]
      --show-output  Mostrar output del programa (normalmente silenciado)
  -h, --help         Print help
```

**Ejemplos:**
```bash
# Benchmark básico (10 runs)
vn bench tests/main.vn

# Benchmark con 100 runs
vn bench --runs 100 tests/main.vn

# Con output visible (normalmente está silenciado)
vn bench --show-output tests/main.vn
```

**Output del benchmark:**
```
Benchmark · \\?\C:\Users\x\dev\varn\tests\main.vn  (10 runs)
  Source  50 lines  1.5 KB  102 tokens

  Phase             min        p50       mean        max         σ      total       %
  ──────────  ─────────  ─────────  ─────────  ─────────  ────────  ─────────  ──────
  read    64.2 µs    71.8 µs    76.5 µs     102 µs   11.9 µs     765 µs    0.1%
  lex    63.7 µs    67.2 µs    69.7 µs    90.1 µs   7.47 µs     697 µs    0.1%
  parse    61.5 µs    82.5 µs    85.4 µs     121 µs   18.6 µs     854 µs    0.2%
  check    96.6 µs     107 µs     112 µs     141 µs   13.6 µs   1.122 ms    0.2%
  compile    64.4 µs    74.1 µs    73.8 µs    82.3 µs   4.21 µs     738 µs    0.1%
  optimize     8.6 µs     9.1 µs    11.3 µs    32.5 µs   6.71 µs     125 µs    0.0%
  execute   47.73 ms   54.36 ms   54.08 ms   69.65 ms   6.01 ms   540.8 ms   99.2%
  ──────────  ─────────  ─────────  ─────────  ─────────  ────────  ─────────  ──────
  total        48.08 ms   54.77 ms   54.51 ms   70.22 ms             545.1 ms    100%

  Throughput: 18.3 runs/s  (p50 end-to-end: 54.77 ms)
  Total pipeline time: 545.1 ms
  Module precompilation (cold startup): 90.91 ms
  Cold-start throughput: 6.9 runs/s  (precompile + p50: 145.7 ms)
  Execution measured with stdout muted (--show-output to disable)

Parser Breakdown
  program_loop      52.7 µs    62%
  stmt_or_decl      31.9 µs    38%
  block                0 ns     0%
  recover              0 ns     0%
  total             84.6 µs


Checker Breakdown
  load_globals       100 ns     0%
  bind              30.5 µs    50%
  merge_core         100 ns     0%
  enrich_calls       100 ns     0%
  check_stmts       30.2 µs    49%
  annotations        500 ns     1%
  finalize           100 ns     0%
  total             61.6 µs


VM Opcode Hotspots
  LoadGlobalIdx             2 160    20%
  LoadConst                 1 247    11%
  LoadNull                    981     9%
  Call                        907     8%
  DefineGlobalIdx             774     7%
  LoadInt                     563     5%
  MakeClosure                 399     4%
  GetProperty                 392     4%
  Move                        382     4%
  Eq                          358     3%
  CallMethod                  297     3%
  JumpIfFalse                 248     2%
  total                    10 875


VM Profile
  IC hits                       471  (90.8% hit rate)
  IC misses                      48
    GetProp IC hits             242  (98.4% hit rate)
    CallMethod IC hits          229  (83.9% hit rate)
  calls vm-fast                 734  (39.6%)
  calls slow/prepare             83  (4.5%)
  calls native                1 038  (56.0%)
  heap allocs                 2 112

GC Stats
  nursery allocs              1 443
  minor gc runs                   1
  minor gc promoted               0
  gc collections                  2
  gc freed                    1 826
  heap live (post-gc)           286
  heap total slots            2 103

Register VM Stats
  Move opcodes                  382
  frame pushes                  489
  frame pops                    550


JIT Compiler & Execution Stats
  freshly compiled                  108  (success: 108, failed: 0)
  using cached JIT                  492
  total compile time           1.786 ms
  total machine code           432.0 KB
  JIT runs                          711  (90.1%)
  interpreted runs                   78  (9.9%)

```

**Métricas explicadas:**
- **IC hits/misses**: Inline Cache hit rate (property access optimization)
- **calls vm-fast**: Llamadas optimizadas por fast-path (60%+)
- **calls slow/prepare**: Llamadas que requieren preparación completa (2%)
- **calls native**: Llamadas a funciones nativas (37%)
- **heap allocs**: Número de allocations en el heap
- **frame pushes/pops**: Creación/destrucción de frames de ejecución

### `disasm` - Disassembly

Muestra el bytecode desarmado (disassembly).

```bash
vn debug -p bytecode <FILE>

# Opciones:
      --debug <PHASES>  # Debug de fases específicas
```

**Ejemplos:**
```bash
# Ver bytecode
vn debug -p bytecode tests/main.vn

```

### `debug` - Inspeccionar AST/IR

Inspecciona las estructuras internas del compilador (AST, tipos, etc.).

```bash
vn debug [FILE]

Inspeccionar AST, tipos, bytecode y otras estructuras internas

Usage: vn.exe debug [OPTIONS] [FILE]

Arguments:
  [FILE]  Archivo Varn a inspeccionar

Options:
  -e, --eval <CODE>    Evaluar código directamente en lugar de archivo
  -p, --phase <PHASE>  Fase a mostrar: tokens, ast, check, bytecode, symbols, binds, types[:N], expr, modules, graph, caps, scope, errors, trace, info, lsp[:sub], all [default: all]
  -h, --help           Print help
PS C:\Users\x\dev\varn>
```

**Ejemplos:**
```bash
# Inspeccionar todas las fases
vn debug tests/main.vn

# Solo parsing
vn debug -p parse tests/main.vn

# Inspeccionar código inline
vn debug -e "function add(a: int, b: int) = a + b"

# Ver solo el AST checkeado
vn debug -p check tests/main.vn

# Evaluar y ver todas las fases
vn debug -e "print(42)"
```

### `doctor` - Diagnóstico

Ejecuta diagnósticos del sistema y configuración.

```bash
vn doctor
```

**Ejemplo:**
```bash
vn doctor
```

### `lsp` - Language Server Protocol

Inicia el servidor LSP para integración con IDEs.

```bash
vn lsp
```

**Uso con VSCode/editores:**
```bash
# El LSP se comunica por stdio
vn lsp
```

### `init` - Inicializar proyecto

Inicializa un nuevo proyecto Varn con estructura básica.

```bash
vn init [DIR]

# Opciones:
      --name <NAME>  # Nombre del proyecto
```

**Ejemplos:**
```bash
# Crear proyecto en directorio actual
vn init

# Crear proyecto en directorio específico
vn init my-project

# Con nombre personalizado
vn init my-project --name "Mi Proyecto"
```

### `completions` - Autocompletado shell

Genera scripts de autocompletado para tu shell.

```bash
vn completions <SHELL>

Generar scripts de autocompletado para el shell

Usage: vn.exe completions <SHELL>

Arguments:
  <SHELL>  Shell para el que generar completions [possible values: bash, zsh, fish, power-shell, elvish]

Options:
  -h, --help  Print help

# Shells soportados: bash, zsh, fish, power-shell, elvish
```

**Ejemplos:**
```bash
# Bash
vn completions bash > ~/.local/share/bash-completion/completions/vn

# Zsh (agregar a ~/.zshrc: fpath=(~/.zfunc $fpath))
vn completions zsh > ~/.zfunc/_vn

# Fish
vn completions fish > ~/.config/fish/completions/vn.fish

# PowerShell (agregar al $PROFILE)
vn completions power-shell > vn.ps1
```

## Flags de Debug

El flag `--debug <PHASES>` acepta fases separadas por comas:

```bash
# Debug de todas las fases
vn debug program.vn


# Fases disponibles:
# - tokens: Lexer/Tokenizer
# - ast: Parser
# - check: Type checker
# - bytecode: Compiler/Codegen
# - symbols: Symbol table
# - binds: Binding graph
# - types: Typed AST
# - expr: Expression tree
# - modules: Module graph
# - graph: Import graph
# - caps: Capability graph
# - scope: Scope tree
# - errors: Diagnostics
# - trace: Execution trace
# - info: Runtime info
# - lsp: LSP submodes
# - all: Todas las fases
```

**Ejemplos:**
```bash
# Ver output del parser
vn debug -p ast program.vn

# Debug del type checker
vn debug -p check program.vn

# Debug completo
vn debug program.vn
```

## Comando Implícito `run`

Si el primer argumento no es un subcomando conocido, se asume `run`:

```bash
# Estos son equivalentes:
vn tests/main.vn
vn run tests/main.vn

# Con argumentos
vn script.vn -- arg1 arg2
vn run script.vn -- arg1 arg2
```

## Variables de Entorno

```bash
# Nivel de log (trace, debug, info, warn, error)
RUST_LOG=debug vn run program.vn

# Backtrace completo en errores
RUST_BACKTRACE=1 vn run program.vn
RUST_BACKTRACE=full vn run program.vn
```

## Arquitectura del VM

### Register-Based VM

Varn usa una VM basada en registros (no stack-based) con las siguientes características:

- **NaN-boxing**: Valores representados en 64 bits usando tagged NaNs
- **Inline Caching (IC)**: Cache de property access para optimización
- **Fast-path calls**: Optimización de llamadas comunes (60%+ de llamadas)
- **Frame-based execution**: Call frames con registros locales
- **Upvalues**: Closures con captura de variables (open/closed upvalues)

### Optimizaciones Implementadas

1. **Fast-path para llamadas (60.3% de llamadas)**:
   - VM closures simples (sin generator, async, rest params complejos)
   - Native functions directas
   - Bound methods (VM y nativos)
   - Reducción de 95% en overhead (39% → 2% slow path)

2. **Inline Cache**:
   - GetProp/SetProp optimizados con cache por clase y slot
   - Tracking separado por tipo de operación
   - Cache de métodos y getters en vtables

3. **Profiling integrado**:
   - IC hit/miss rates por operación
   - Call path distribution
   - Frame push/pop tracking
   - Heap allocation monitoring

### Métricas de Performance

Las métricas de performance dependen mucho del workload. Como referencia actual del repo (`--runs 10`, build `dev`, 2026-06-05):
- **`tests/45-simple-file-test.vn`**: p50 `912 µs`
- **`tests/21-async.vn`**: p50 `1.901 ms`
- **`tests/47-isolates-multithread.vn`**: p50 `33.16 ms`
- **Fast-path / IC / JIT**: documentar siempre junto al archivo benchmarkeado; ya no se asume una cifra global única.

## Testing y Política de Integridad

El repo no utiliza `cargo test` para tests unitarios del código Rust (se implementarán posteriormente). Toda validación de corrección y rendimiento se realiza mediante la suite de integración en Varn y benchmarks comparativos.

### Ejecución de la Suite de Tests

```bash
# Ejecutar suite de integración completa
vn run tests/main.vn

# O de manera implícita
vn tests/main.vn
```

Debería mostrar al final:
```text
════════════════════════════════════════
Modules executed in suite: 48
PASSED: 686
FAILED: 0
ALL TESTS PASSED
```

Nota de realidad del corpus:
- `tests/main.vn` es la suite por defecto y hoy integra `48` módulos.
- `tests/41-advanced-enums.vn`, `tests/42-stdlib-comprehensive-test.vn` y `tests/47-isolates-multithread.vn` ya están reintegrados.

### Política de Benchmarks y Rendimiento JIT

Para medir la velocidad de ejecución y compararnos contra V8 (Node.js) y JavaScriptCore (Bun), se deben seguir las siguientes directrices de integridad:

1. **Uso de la herramienta `bench`**:
   * Las mediciones de tiempo de ejecución puro deben realizarse usando el comando `vn bench <FILE>` (por ejemplo, `vn bench tests/main.vn`).
   * El comando `vn bench` utiliza snapshots del heap de la VM para precargar los módulos de la librería estándar, aislando el tiempo de ejecución puro del script de usuario del tiempo de arranque y precompilación.

2. **Diferencia entre `vn run` y `vn bench` (El flag de Optimización)**:
   * Por defecto, la optimización del asignador de registros (`varn_compiler::codegen::regalloc_post::OPTIMIZE_ENABLED`) está deshabilitada (`false`) en ejecuciones normales (`vn run`) para favorecer la velocidad de compilación inicial de desarrollo.
   * La optimización del asignador de registros se activa automáticamente como `true` **únicamente** durante `vn bench`.
   * **Rendimiento Esperado**: La optimización del asignador de registros proporciona un aumento de rendimiento de **~5.5x** (reduciendo el tiempo de ejecución de `fib(35)` de 1891 ms a 321 ms). Por lo tanto, para comparar la velocidad punta de ejecución de Varn contra Node.js o Bun, siempre se debe medir usando el comando `bench` o forzando la optimización.

3. **Métricas de Referencia contra JITs Comerciales (`fib(35)`)**:
   * **Bun (JSC)**: ~73.5 ms (Línea base / 1.0x)
   * **Node.js (V8)**: ~78.3 ms (~1.06x)
   * **Varn (JIT Optimizado con `CallSelf`)**: ~268.6 ms - 279.6 ms (~3.6x - 3.8x respecto a Bun)
   * **Varn (JIT Unoptimizado)**: ~1891.0 ms (~25.7x respecto a Bun)

*Nota: La diferencia actual de ~3.6x de Varn JIT respecto a Bun se redujo desde la original de 4.3x gracias a la optimización de llamadas de auto-recursividad estática (`OpCode::CallSelf`), que bypassa los helpers de Rust y el lookup de closures en el JIT.*

## Troubleshooting

### Errores Comunes

1. **"module not found: ..."**
   - Verifica que los imports usen rutas relativas correctas
   - Ejemplo: `import "./module"` no `import "module"`

2. **"value is not callable: ..."**
   - El valor no es una función válida
   - Verifica que el tipo sea correcto

3. **"stack underflow"**
   - Error interno del VM, reportar como bug

4. **Errores de tipos**
   - Usa `vn check` para ver detalles del type checker
   - Usa `vn debug -p check` para ver el AST tipado

### Debug Avanzado

```bash
# Trace completo de ejecución
vn run --trace --debug all program.vn

# Ver bytecode generado
vn debug -p bytecode program.vn

# Inspeccionar AST y tipos
vn debug program.vn

# Ver solo tipos
vn debug -p check program.vn

# Benchmark con output visible
vn bench --show-output program.vn
```

### Performance Debugging

```bash
# Benchmark detallado
vn bench --runs 100 program.vn

# Ver qué fases son lentas
vn debug program.vn
```

## Ejemplos de Workflows

### Desarrollo Normal
```bash
# Verificar tipos
vn check src/main.vn

# Ejecutar
vn src/main.vn

# Con debug si hay problemas
vn debug src/main.vn
```

### Testing
```bash
# Ejecutar tests
vn tests/main.vn

# Benchmark de tests
vn bench tests/main.vn
```

### Inspección y Debug
```bash
# Ver AST
vn debug -p parse src/main.vn

# Ver tipos inferidos
vn debug -p check src/main.vn

# Ver bytecode
vn debug -p bytecode src/main.vn

# Trace de ejecución
vn run --trace src/main.vn
```

### Integración con Editor
```bash
# Iniciar LSP para tu editor
vn lsp

# Generar completions para tu shell
vn completions zsh > ~/.zfunc/_vn
```

## Contribuir

Ver README.md para guías de contribución y arquitectura del proyecto.

## Licencia

MIT License - Ver LICENSE file
