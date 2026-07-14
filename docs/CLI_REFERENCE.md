# CLI de Varn

Referencia completa del binario `vn`.

## Reglas generales

- Si no indicas un subcomando y el primer argumento no empieza por `-`, `vn` asume `run`.
- `-h` / `--help` muestran la ayuda del comando actual.
- `-V` / `--version` muestran la versión.
- `run` y `check` aceptan `--strict`; `run` además acepta `--trace` y `-v`.

## Comandos de primer nivel

### `run`

Ejecuta un archivo Varn.

```bash
vn run archivo.vn
vn archivo.vn
vn run archivo.vn -- arg1 arg2
vn run -v --trace archivo.vn
```

Flags:

- `-v, --verbose`: salida más detallada.
- `--trace`: trazado de ejecución.
- `--strict`: advierte sobre tipos `Dynamic` implícitos.

### `check`

Verifica tipos sin ejecutar el programa.

```bash
vn check archivo.vn
vn check -v --strict archivo.vn
```

Flags:

- `-v, --verbose`: salida más detallada.
- `--strict`: advierte sobre tipos `Dynamic` implícitos.

### `eval`

Evalúa código inline desde la línea de comandos.

```bash
vn eval "print(1 + 2)"
vn eval -v "function double(x: int) = x * 2; print(double(21))"
```

Flags:

- `-v, --verbose`: salida más detallada.

### `repl`

Abre el REPL interactivo.

```bash
vn repl
vn repl --debug-bytecode
```

Flags:

- `--debug-bytecode`: imprime el bytecode generado para cada evaluación.

Comandos dentro del REPL:

- `.help`: muestra la ayuda interna.
- `.clear`: limpia el buffer actual.
- `.exit` / `.quit`: sale del REPL.
- `Ctrl+D`: EOF para salir.

### `bench`

Mide tiempos del pipeline y de la ejecución.

```bash
vn bench archivo.vn
vn bench --runs 100 archivo.vn
vn bench --show-output archivo.vn
vn bench archivo.vnc
```

Flags:

- `--runs <N>`: número de ejecuciones, por defecto `10`.
- `--show-output`: muestra la salida del programa durante el benchmark.

Notas:

- Para fuentes `.vn`, el benchmark actual mide `read`, `lex`, `parse`, `check`, `compile`, `optimize` y `execute`.
- Para compilados `.vnc`, mide `load` y `execute`.
- `tests/main.vn` es una suite de integración; para comparar performance usa archivos focalizados y documenta build y runs.

### `debug`

Inspecciona el pipeline sin ejecutar la VM.

```bash
vn debug archivo.vn
vn debug -p ast archivo.vn
vn debug -p check archivo.vn
vn debug -p hir archivo.vn
vn debug -p ssa archivo.vn
vn debug -p bytecode archivo.vn
vn debug -e "function add(a: int, b: int) = a + b"
```

Flags:

- `-e, --eval <CODE>`: evalúa código inline en vez de archivo.
- `-p, --phase <PHASE>`: fase a mostrar. Valores: `tokens`, `ast`, `check`, `bytecode`, `hir`, `ssa`, `symbols`, `binds`, `types[:N]`, `expr`, `modules`, `graph`, `caps`, `scope`, `errors`, `trace`, `info`, `lsp[:sub]`, `all`.

Fases del optimizer:

- `hir`: dump del HIR (High-level IR) con colores ANSI. Muestra la estructura de cada función tras el lowering del AST: bindings resueltos, azúcar expandido, tipos anotados por el checker.
- `ssa`: dump del SSA CFG (Braun). Muestra basic blocks, valores SSA, parámetros de bloque y terminators. Funciones con construcciones fuera de la cobertura actual aparecen con un aviso detallado.

### `build`

Compila un archivo Varn a `.vnc`.

```bash
vn build archivo.vn
vn build archivo.vn -o out.vnc
vn build -v archivo.vn
```

Flags:

- `-o, --output <PATH>`: ruta de salida del binario compilado.
- `-v, --verbose`: salida más detallada.

### `pkg`

Gestión de paquetes.

```bash
vn pkg add mathlib github.com/user/mathlib@^1.2.3
vn pkg remove mathlib
vn pkg install
vn pkg update
```

Subcomandos:

- `add <alias> <origin>`: agrega una dependencia al proyecto.
- `remove <alias>`: elimina una dependencia.
- `install`: instala dependencias del proyecto.
- `update`: actualiza dependencias.

### `init`

Inicializa un nuevo proyecto Varn.

```bash
vn init
vn init mi-proyecto
vn init mi-proyecto --name "Mi Proyecto"
```

Flags:

- `--name <NAME>`: nombre del proyecto.

`varn.json` acepta la clave opcional `"std"` para fijar la std del proyecto
(anula `VARN_STD` y el default del toolchain):

```json
{ "name": "mi-proyecto", "std": "../otra-std/std.vnb" }
```

La ruta apunta a un `.vnb` o a un árbol con `std.json`; relativa al directorio
de `varn.json`. Ver [STDLIB_ARCHITECTURE.md](STDLIB_ARCHITECTURE.md).

### `doctor`

Ejecuta diagnósticos del sistema y configuración.

```bash
vn doctor
```

Entre otras cosas, reporta la std activa y su procedencia (resolución:
`varn.json` `"std"` → env `VARN_STD` → `<exe_dir>/std.vnb`):

```
std: bundle C:\...\std.vnb v0.1.0 (via toolchain)
std: source tree C:\...\std v0.1.0 (via VARN_STD)
std: embedded registry only (no std tree/bundle found)
```

Ver [STDLIB_ARCHITECTURE.md](STDLIB_ARCHITECTURE.md) para el detalle de
formato `.vnb`, resolución y dev workflow.

### `cache`

Gestión del caché local del proyecto.

```bash
vn cache clean
```

Subcomandos:

- `clean`: elimina los archivos de caché del proyecto actual.

### `lsp`

Inicia el servidor LSP por stdio.

```bash
vn lsp
```

### `completions`

Genera scripts de autocompletado para shell.

```bash
vn completions bash
vn completions zsh
vn completions fish
vn completions power-shell
vn completions elvish
```

Shells soportados: `bash`, `zsh`, `fish`, `power-shell`, `elvish`.

## Comandos útiles relacionados

- `vn help <subcomando>`: ayuda específica del subcomando.
- `vn --help`: vista general del CLI.

---

## Variables de entorno

| Variable | Efecto |
|----------|--------|
| `VARN_NO_JIT=1` | Apaga el JIT **por completo**: no compila (0 B de código máquina) y no entra a código compilado. Se propaga por construcción a isolates, generadores y a las VMs del bench. Es la herramienta para partir un fallo en "¿representación o codegen?" — corre la suite en los dos modos. |
| `VN_OPT_TRACE=1` | Traza del compilador (`varn-opt`): módulos y funciones compiladas. |
| `VARN_STD` | Ruta a la stdlib activa (`std.vnb` o árbol `std/`). Orden de resolución: `varn.json` → `VARN_STD` → `<exe>/std.vnb`. Ver [STDLIB_ARCHITECTURE.md](STDLIB_ARCHITECTURE.md). |
| `VARN_HOME` | Directorio raíz de la toolchain (caché de paquetes, stdlib instalada). |
| `VARN_LOCK_UPDATE=1` | Permite reescribir `varn.lock` durante la resolución de dependencias. |
| `VARN_DEBUG_OPS=1` | Vuelca el registro de ops nativas (LBI) al arrancar. |

Nota operativa: tras recompilar `vn`, hay que regenerar el bundle de la stdlib
(`cargo xtask build-std` y copiar `target/std.vnb` junto al ejecutable) o `vn` aborta
con *"std bundle was built by a different compiler build"*.
