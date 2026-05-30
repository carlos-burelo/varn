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
  Source  46 lines  1.4 KB  94 tokens

  Phase             min        p50       mean        max         σ      total       %
  ──────────  ─────────  ─────────  ─────────  ─────────  ────────  ─────────  ──────
  read          18.2 µs    21.8 µs    21.4 µs      25 µs   2.06 µs     214 µs    3.5%
  lex           12.2 µs    12.5 µs    12.9 µs    14.9 µs    769 ns     129 µs    2.0%
  parse          7.9 µs     9.3 µs    12.9 µs    28.7 µs   7.14 µs     129 µs    1.5%
  check         56.4 µs    63.3 µs    64.2 µs    77.8 µs   6.65 µs     642 µs   10.3%
  compile       31.3 µs    37.5 µs    37.6 µs    42.2 µs    2.8 µs     376 µs    6.1%
  optimize       4.2 µs     4.4 µs    5.79 µs    18.6 µs   4.07 µs    63.7 µs    0.7%
  execute        442 µs     466 µs     477 µs     531 µs   27.1 µs   4.774 ms   75.8%
  ──────────  ─────────  ─────────  ─────────  ─────────  ────────  ─────────  ──────
  total          572 µs     614 µs     633 µs     738 µs             6.328 ms    100%

  Throughput: 1627.6 runs/s  (p50 end-to-end: 614 µs)
  Total pipeline time: 6.328 ms
  Module precompilation (cold startup): 43.5 ms
  Cold-start throughput: 22.7 runs/s  (precompile + p50: 44.12 ms)
  Execution measured with stdout muted (--show-output to disable)

  Parser Breakdown
  program_loop       6.5 µs    59%
  stmt_or_decl       4.5 µs    41%
  block                0 ns     0%
  recover              0 ns     0%
  total               11 µs


  Checker Breakdown
  load_globals         0 ns     0%
  bind              14.6 µs    44%
  merge_core           0 ns     0%
  enrich_calls         0 ns     0%
  check_stmts       18.1 µs    55%
  annotations        300 ns     1%
  finalize           100 ns     0%
  total             33.1 µs


  VM Opcode Hotspots
  LoadGlobalIdx               309    16%
  LoadConst                   205    10%
  Call                        193    10%
  LoadNull                    185     9%
  LoadInt                     109     6%
  DefineGlobalIdx             106     5%
  GetProperty                  98     5%
  Move                         76     4%
  MakeClosure                  70     4%
  Eq                           66     3%
  JumpIfTrue                   36     2%
  CallMethod                   35     2%
  total                     1 959


  VM Profile
  IC hits                       185  (93.0% hit rate)
  IC misses                      14
    GetProp IC hits              85  (96.6% hit rate)
    CallMethod IC hits          100  (90.1% hit rate)
  calls vm-fast                 109  (19.2%)
  calls slow/prepare             24  (4.2%)
  calls native                  436  (76.6%)
  heap allocs                   648

  GC Stats
  nursery allocs                357
  minor gc runs                   1
  minor gc promoted               0
  gc collections                  2
  gc freed                      575
  heap live (post-gc)            73
  heap total slots              639

  Register VM Stats
  Move opcodes                   76
  frame pushes                  140
  frame pops                    199


  JIT Compiler & Execution Stats
  freshly compiled                    0  (success: 0, failed: 0)
  using cached JIT                   99
  total compile time               0 ns
  total machine code                0 B
  JIT runs                          183  (86.7%)
  interpreted runs                   28  (13.3%)


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

# Shells soportados: bash, zsh, fish, powershell, elvish
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
vn completions powershell > vn.ps1
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

En benchmark de la suite completa (534 tests):
- **Throughput**: ~475 runs/s
- **Fast-path calls**: 60.3%
- **Slow-path calls**: 2.0%
- **Native calls**: 37.7%
- **Frame operations**: ~133 pushes, ~192 pops
- **Heap allocations**: ~1,359 per run

## Testing

```bash
# Ejecutar suite de tests completa
vn run tests/main.vn

# O explícitamente
vn tests/main.vn

# Debería mostrar:
# ════════════════════════════════════════
# PASSED: 534
# FAILED: 0
# ALL TESTS PASSED
```

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
