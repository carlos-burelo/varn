# Varn Language - CLI Documentation

Lenguaje compilado con tipado estático y VM register-based optimizada.

## Instalación y Build

```bash
# Build en modo desarrollo
cargo build --bin wr

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

# Opciones:
  -v, --verbose         # Modo verbose
      --debug <PHASES>  # Debug de fases específicas
      --trace           # Tracing de ejecución
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
vn run --debug all program.vn

# Con verbose
vn run -v program.vn
```

### `check` - Verificar tipos

Verifica el programa sin ejecutarlo (type checking).

```bash
vn check <FILE>

# Opciones:
  -v, --verbose         # Modo verbose
      --debug <PHASES>  # Debug de fases específicas
```

**Ejemplos:**
```bash
# Verificar tipos
vn check src/main.vn

# Verbose
vn check -v src/main.vn

# Con debug del checker
vn check --debug check src/main.vn
```

### `eval` - Evaluar código

Evalúa código Varn directamente desde la línea de comandos.

```bash
vn eval <CODE>

# Opciones:
  -v, --verbose         # Modo verbose
      --debug <PHASES>  # Debug de fases específicas
```

**Ejemplos:**
```bash
# Evaluar expresión
vn eval "print(1 + 2)"

# Evaluar código más complejo
vn eval "function double(x: int) = x * 2; print(double(21))"

# Con debug
vn eval --debug all "print('hello')"
```

### `repl` - REPL Interactivo

Inicia un REPL (Read-Eval-Print Loop) interactivo.

```bash
vn repl

# Opciones:
      --debug-bytecode  # Mostrar bytecode generado
```

**Ejemplos:**
```bash
# Iniciar REPL
vn repl

# REPL con debug de bytecode
vn repl --debug-bytecode
```

### `bench` - Benchmark

Ejecuta benchmarks detallados de rendimiento con métricas del VM.

```bash
vn bench <FILE>

# Opciones:
      --runs <N>        # Número de runs (default: 10)
      --no-run          # Solo compilar, no ejecutar
      --with-output     # Mostrar output del programa
      --debug <PHASES>  # Debug de fases específicas
```

**Ejemplos:**
```bash
# Benchmark básico (10 runs)
vn bench tests/main.vn

# Benchmark con 100 runs
vn bench --runs 100 tests/main.vn

# Solo compilación (sin ejecución)
vn bench --no-run tests/main.vn

# Con output visible (normalmente está silenciado)
vn bench --with-output tests/main.vn
```

**Output del benchmark:**
```
Benchmark · tests/main.vn  (10 runs)
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
vn disasm <FILE>

# Opciones:
      --debug <PHASES>  # Debug de fases específicas
```

**Ejemplos:**
```bash
# Ver bytecode
vn disasm tests/main.vn

# Con debug del compilador
vn disasm --debug compile tests/main.vn
```

### `inspect` - Inspeccionar AST/IR

Inspecciona las estructuras internas del compilador (AST, tipos, etc.).

```bash
vn inspect [FILE]

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
vn inspect tests/main.vn

# Solo parsing
vn inspect -p parse tests/main.vn

# Inspeccionar código inline
vn inspect -e "function add(a: int, b: int) = a + b"

# Ver solo el AST checkeado
vn inspect -p check tests/main.vn

# Evaluar y ver todas las fases
vn inspect -e "print(42)"
```

### `info` - Información de archivo

Muestra información sobre un archivo Varn compilado.

```bash
vn info <FILE>

# Opciones:
      --hashes  # Mostrar hashes de módulos
```

**Ejemplos:**
```bash
# Info básica
vn info tests/main.vn

# Con hashes
vn info --hashes tests/main.vn
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

# Shells soportados: bash, zsh, fish, powershell, elvish
```

**Ejemplos:**
```bash
# Bash
vn completions bash > ~/.local/share/bash-completion/completions/wr

# Zsh (agregar a ~/.zshrc: fpath=(~/.zfunc $fpath))
vn completions zsh > ~/.zfunc/_wr

# Fish
vn completions fish > ~/.config/fish/completions/wr.fish

# PowerShell (agregar al $PROFILE)
vn completions powershell > wr.ps1
```

## Flags de Debug

El flag `--debug <PHASES>` acepta fases separadas por comas:

```bash
# Debug de todas las fases
vn run --debug all program.vn

# Debug de fases específicas
vn run --debug parse,check program.vn

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
vn run --debug parse program.vn

# Debug del type checker
vn check --debug check program.vn

# Debug completo
vn run --debug all program.vn
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
   - Usa `vn inspect -p check` para ver el AST tipado

### Debug Avanzado

```bash
# Trace completo de ejecución
vn run --trace --debug all program.vn

# Ver bytecode generado
vn disasm program.vn

# Inspeccionar AST y tipos
vn inspect program.vn

# Ver solo tipos
vn inspect -p check program.vn

# Benchmark con output visible
vn bench --with-output program.vn
```

### Performance Debugging

```bash
# Benchmark detallado
vn bench --runs 100 program.vn

# Ver qué fases son lentas
vn bench --debug all program.vn

# Solo medir compilación (sin ejecución)
vn bench --no-run program.vn
```

## Ejemplos de Workflows

### Desarrollo Normal
```bash
# Verificar tipos
vn check src/main.vn

# Ejecutar
vn src/main.vn

# Con debug si hay problemas
vn run --debug all src/main.vn
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
vn inspect -p parse src/main.vn

# Ver tipos inferidos
vn inspect -p check src/main.vn

# Ver bytecode
vn disasm src/main.vn

# Trace de ejecución
vn run --trace src/main.vn
```

### Integración con Editor
```bash
# Iniciar LSP para tu editor
vn lsp

# Generar completions para tu shell
vn completions zsh > ~/.zfunc/_wr
```

## Contribuir

Ver README.md para guías de contribución y arquitectura del proyecto.

## Licencia

MIT License - Ver LICENSE file
