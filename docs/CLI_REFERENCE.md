# Manual de Referencia de la CLI (`vn`)

Este documento constituye la guía oficial de referencia para el ejecutable CLI unificado **`vn`** del lenguaje Varn.

---

## Tabla de Contenidos

- [1. Visión General](#1-visión-general)
- [2. Lista Global de Subcomandos](#2-lista-global-de-subcomandos)
- [3. Detalle de Comandos](#3-detalle-de-comandos)
  - [`vn run`](#vn-run)
  - [`vn check`](#vn-check)
  - [`vn build`](#vn-build)
  - [`vn bench`](#vn-bench)
  - [`vn debug`](#vn-debug)
  - [`vn eval`](#vn-eval)
  - [`vn repl`](#vn-repl)
  - [`vn pkg`](#vn-pkg)
  - [`vn init`](#vn-init)
  - [`vn doctor`](#vn-doctor)
  - [`vn cache`](#vn-cache)
  - [`vn lsp`](#vn-lsp)
  - [`vn completions`](#vn-completions)
- [4. Variables de Entorno](#4-variables-de-entorno)

---

## 1. Visión General

El ejecutable `vn` es la herramienta unificada que gestiona la compilación, comprobación de tipos, ejecución, pruebas de rendimiento y servidor de lenguaje LSP para proyectos escritos en Varn.

Sintaxis general:
```bash
vn [SUBCOMANDO] [OPCIONES] [ARCHIVOS...]
```

---

## 2. Lista Global de Subcomandos

| Subcomando | Descripción |
|---|---|
| `run` | Compila (si es necesario) y ejecuta un archivo `.vn` o paquete `.vnc`. |
| `check` | Ejecuta el comprobador semántico y de tipos sin generar bytecode ni ejecutar. |
| `build` | Compila un script `.vn` a un artefacto binario de bytecode portable `.vnc`. |
| `bench` | Ejecuta un programa reportando métricas de compilación, parsing, GC y ejecución VM. |
| `debug` | Inspecciona las representaciones intermedias (`ast`, `check`, `hir`, `ssa`, `bytecode`). |
| `eval` | Evalúa una cadena de código Varn directamente desde la línea de comandos. |
| `repl` | Inicia una sesión de lectura, evaluación e impresión interactiva (REPL). |
| `pkg` | Gestiona las dependencias del proyecto (`add`, `remove`, `install`, `update`). |
| `init` | Inicializa un nuevo proyecto estructurado con `varn.json`. |
| `doctor` | Realiza un diagnóstico del entorno, binarios y procedencia de la stdlib. |
| `cache` | Gestiona y limpia la caché local de bytecode en `.vn/cache/`. |
| `lsp` | Inicia el servidor de lenguaje Language Server Protocol (stdio). |
| `completions` | Genera scripts de autocompletado para Bash, Zsh, PowerShell y Fish. |

---

## 3. Detalle de Comandos

### `vn run`
Sintaxis: `vn run <archivo.vn | archivo.vnc> [-- arg1 arg2...]`
- Ejecuta el programa. Si se le pasa un `.vn`, ejecuta todo el pipeline. Si se le pasa un `.vnc`, omitirá parsing y type-checking.

### `vn check`
Sintaxis: `vn check <archivo.vn> [-v]`
- Valida la corrección de tipos y llena la SemanticDB. Flag `-v` muestra advertencias detalladas.

### `vn build`
Sintaxis: `vn build <archivo.vn> [-o salida.vnc]`
- Empaqueta el programa en un artefacto compilado `.vnc`.

### `vn bench`
Sintaxis: `vn bench <archivo.vn> [--runs N] [-v]`
- Perfila la ejecución reportando desglose de tiempos de parser, checker, VM, GC y JIT.

### `vn debug`
Sintaxis: `vn debug -p <fase> <archivo.vn>`
- Permite volcar las fases internas. Ver [CLI_INSPECT.md](CLI_INSPECT.md).

### `vn pkg`
- `vn pkg add <alias> <origen>`: Añade una dependencia.
- `vn pkg install`: Instala las dependencias del `varn.json`.

---

## 4. Variables de Entorno

| Variable | Descripción |
|---|---|
| `VARN_STD` | Controla la procedencia de la stdlib (`dev-checkout` o `@embedded`). |
| `VARN_NO_JIT` | Fuerza la ejecución exclusiva en modo intérprete ignorando Cranelift JIT. |
