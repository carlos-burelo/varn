# Varn Language - CLI Documentation

Lenguaje compilado con tipado estático y VM register-based optimizada.

## Instalación y Build

```bash
# Build en modo desarrollo
cargo build --bin wr

# Build en modo release (optimizado)
cargo build --bin wr --release

# Ejecutar directamente
cargo run --bin wr -- <command> [args]
```

## Comandos Principales

### `run` - Ejecutar programa

Ejecuta un archivo Varn. También es el comando por defecto si no se especifica subcomando.

```bash
wr run <FILE> [-- <ARGS>...]

# Opciones:
  -v, --verbose         # Modo verbose
      --debug <PHASES>  # Debug de fases específicas
      --trace           # Tracing de ejecución
```

**Ejemplos:**
```bash
# Ejecutar un programa (run es implícito)
wr tests/main.wr

# Explícito
wr run tests/main.wr

# Con argumentos para el script
wr run script.wr -- arg1 arg2

# Con tracing
wr run --trace tests/main.wr

# Con debug de todas las fases
wr run --debug all program.wr

# Con verbose
wr run -v program.wr
```

### `check` - Verificar tipos

Verifica el programa sin ejecutarlo (type checking).

```bash
wr check <FILE>

# Opciones:
  -v, --verbose         # Modo verbose
      --debug <PHASES>  # Debug de fases específicas
```

**Ejemplos:**
```bash
# Verificar tipos
wr check src/main.wr

# Verbose
wr check -v src/main.wr

# Con debug del checker
wr check --debug check src/main.wr
```

### `eval` - Evaluar código

Evalúa código Varn directamente desde la línea de comandos.

```bash
wr eval <CODE>

# Opciones:
  -v, --verbose         # Modo verbose
      --debug <PHASES>  # Debug de fases específicas
```

**Ejemplos:**
```bash
# Evaluar expresión
wr eval "print(1 + 2)"

# Evaluar código más complejo
wr eval "function double(x: int) = x * 2; print(double(21))"

# Con debug
wr eval --debug all "print('hello')"
```

### `repl` - REPL Interactivo

Inicia un REPL (Read-Eval-Print Loop) interactivo.

```bash
wr repl

# Opciones:
      --debug-bytecode  # Mostrar bytecode generado
```

**Ejemplos:**
```bash
# Iniciar REPL
wr repl

# REPL con debug de bytecode
wr repl --debug-bytecode
```

### `bench` - Benchmark

Ejecuta benchmarks detallados de rendimiento con métricas del VM.

```bash
wr bench <FILE>

# Opciones:
      --runs <N>        # Número de runs (default: 10)
      --no-run          # Solo compilar, no ejecutar
      --with-output     # Mostrar output del programa
      --debug <PHASES>  # Debug de fases específicas
```

**Ejemplos:**
```bash
# Benchmark básico (10 runs)
wr bench tests/main.wr

# Benchmark con 100 runs
wr bench --runs 100 tests/main.wr

# Solo compilación (sin ejecución)
wr bench --no-run tests/main.wr

# Con output visible (normalmente está silenciado)
wr bench --with-output tests/main.wr
```

**Output del benchmark:**
```
Benchmark · tests/main.wr  (10 runs)
Source  43 lines  1.1 KB  88 tokens

Phase           min      p50     mean      max       σ      total
────────── ───────── ──────── ──────── ──────── ──────── ─────────
read         26.8 µs  43.4 µs  43.1 µs  87.6 µs  18 µs    431 µs    ░░ 2%
lex          15.3 µs  16.4 µs  16.9 µs  21.2 µs  1.67 µs  169 µs    ░░ 1%
parse        16.5 µs  23.4 µs  26.8 µs  59.9 µs  12.4 µs  268 µs    ░░ 1%
check        74 µs    78.2 µs  81.9 µs  113 µs   10.9 µs  819 µs    ░░ 4%
compile      6.6 µs   7 µs     7.38 µs  9.5 µs   821 ns   73.8 µs   ░░ 0%
execute      1.723 ms 1.909 ms 1.929 ms 2.101 ms 127 µs   19.29 ms  ██ 92%
────────── ───────── ──────── ──────── ──────── ──────── ─────────
total        1.862 ms 2.077 ms 2.105 ms 2.393 ms          21.05 ms

Throughput: 475.1 runs/s  (mean end-to-end: 2.105 ms)

Parser Breakdown
  program_loop      18.4 µs    64%
  stmt_or_decl      10.5 µs    36%
  ...

Checker Breakdown
  bind              21.9 µs    57%
  check_stmts       16 µs      42%
  ...

VM Profile
  IC hits                         0  (0.0% hit rate)
  IC misses                       0
  calls vm-fast                 687  (60.3%)
  calls slow/prepare             23  (2.0%)
  calls native                  429  (37.7%)
  heap allocs                 1 359

Register VM Stats
  reg loads                       0
  reg stores                      0
  frame pushes                  133
  frame pops                    192
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
wr disasm <FILE>

# Opciones:
      --debug <PHASES>  # Debug de fases específicas
```

**Ejemplos:**
```bash
# Ver bytecode
wr disasm tests/main.wr

# Con debug del compilador
wr disasm --debug compile tests/main.wr
```

### `inspect` - Inspeccionar AST/IR

Inspecciona las estructuras internas del compilador (AST, tipos, etc.).

```bash
wr inspect [FILE]

# Opciones:
  -e, --eval <CODE>      # Evaluar código directamente
  -p, --phases <PHASES>  # Fases a mostrar (default: all)
```

**Fases disponibles:**
- `parse` - Abstract Syntax Tree
- `check` - Type-checked AST
- `compile` - Bytecode/IR
- `all` - Todas las fases (default)

**Ejemplos:**
```bash
# Inspeccionar todas las fases
wr inspect tests/main.wr

# Solo parsing
wr inspect -p parse tests/main.wr

# Inspeccionar código inline
wr inspect -e "function add(a: int, b: int) = a + b"

# Ver solo el AST checkeado
wr inspect -p check tests/main.wr

# Evaluar y ver todas las fases
wr inspect -e "print(42)"
```

### `info` - Información de archivo

Muestra información sobre un archivo Varn compilado.

```bash
wr info <FILE>

# Opciones:
      --hashes  # Mostrar hashes de módulos
```

**Ejemplos:**
```bash
# Info básica
wr info tests/main.wr

# Con hashes
wr info --hashes tests/main.wr
```

### `doctor` - Diagnóstico

Ejecuta diagnósticos del sistema y configuración.

```bash
wr doctor
```

**Ejemplo:**
```bash
wr doctor
```

### `lsp` - Language Server Protocol

Inicia el servidor LSP para integración con IDEs.

```bash
wr lsp
```

**Uso con VSCode/editores:**
```bash
# El LSP se comunica por stdio
wr lsp
```

### `init` - Inicializar proyecto

Inicializa un nuevo proyecto Varn con estructura básica.

```bash
wr init [DIR]

# Opciones:
      --name <NAME>  # Nombre del proyecto
```

**Ejemplos:**
```bash
# Crear proyecto en directorio actual
wr init

# Crear proyecto en directorio específico
wr init my-project

# Con nombre personalizado
wr init my-project --name "Mi Proyecto"
```

### `completions` - Autocompletado shell

Genera scripts de autocompletado para tu shell.

```bash
wr completions <SHELL>

# Shells soportados: bash, zsh, fish, powershell, elvish
```

**Ejemplos:**
```bash
# Bash
wr completions bash > ~/.local/share/bash-completion/completions/wr

# Zsh (agregar a ~/.zshrc: fpath=(~/.zfunc $fpath))
wr completions zsh > ~/.zfunc/_wr

# Fish
wr completions fish > ~/.config/fish/completions/wr.fish

# PowerShell (agregar al $PROFILE)
wr completions powershell > wr.ps1
```

## Flags de Debug

El flag `--debug <PHASES>` acepta fases separadas por comas:

```bash
# Debug de todas las fases
wr run --debug all program.wr

# Debug de fases específicas
wr run --debug parse,check program.wr

# Fases disponibles:
# - lex: Lexer/Tokenizer
# - parse: Parser
# - check: Type checker
# - compile: Compiler/Codegen
# - vm: Virtual Machine
# - all: Todas las fases
```

**Ejemplos:**
```bash
# Ver output del parser
wr run --debug parse program.wr

# Debug del type checker
wr check --debug check program.wr

# Debug completo
wr run --debug all program.wr
```

## Comando Implícito `run`

Si el primer argumento no es un subcomando conocido, se asume `run`:

```bash
# Estos son equivalentes:
wr tests/main.wr
wr run tests/main.wr

# Con argumentos
wr script.wr -- arg1 arg2
wr run script.wr -- arg1 arg2
```

## Variables de Entorno

```bash
# Nivel de log (trace, debug, info, warn, error)
RUST_LOG=debug wr run program.wr

# Backtrace completo en errores
RUST_BACKTRACE=1 wr run program.wr
RUST_BACKTRACE=full wr run program.wr
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
wr run tests/main.wr

# O explícitamente
wr tests/main.wr

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
   - Usa `wr check` para ver detalles del type checker
   - Usa `wr inspect -p check` para ver el AST tipado

### Debug Avanzado

```bash
# Trace completo de ejecución
wr run --trace --debug all program.wr

# Ver bytecode generado
wr disasm program.wr

# Inspeccionar AST y tipos
wr inspect program.wr

# Ver solo tipos
wr inspect -p check program.wr

# Benchmark con output visible
wr bench --with-output program.wr
```

### Performance Debugging

```bash
# Benchmark detallado
wr bench --runs 100 program.wr

# Ver qué fases son lentas
wr bench --debug all program.wr

# Solo medir compilación (sin ejecución)
wr bench --no-run program.wr
```

## Ejemplos de Workflows

### Desarrollo Normal
```bash
# Verificar tipos
wr check src/main.wr

# Ejecutar
wr src/main.wr

# Con debug si hay problemas
wr run --debug all src/main.wr
```

### Testing
```bash
# Ejecutar tests
wr tests/main.wr

# Benchmark de tests
wr bench tests/main.wr
```

### Inspección y Debug
```bash
# Ver AST
wr inspect -p parse src/main.wr

# Ver tipos inferidos
wr inspect -p check src/main.wr

# Ver bytecode
wr disasm src/main.wr

# Trace de ejecución
wr run --trace src/main.wr
```

### Integración con Editor
```bash
# Iniciar LSP para tu editor
wr lsp

# Generar completions para tu shell
wr completions zsh > ~/.zfunc/_wr
```

## Contribuir

Ver README.md para guías de contribución y arquitectura del proyecto.

## Licencia

MIT License - Ver LICENSE file
