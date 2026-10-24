# Referencia del CLI `vn`

Fuente: `crates/varn-cli/src/cli.rs` (clap). `vn <comando> --help` es la referencia
viva; esta página la resume.

## Ejecutar y comprobar

| Comando | Uso | Notas |
|---|---|---|
| `run` | `vn run [OPTIONS] [FILE] [-- ARGS...]` | `-e/--eval CODE`, `-v`, `--trace`, permisos (abajo), `--compare-tiers` (corre el programa en intérprete, JIT y JIT tier 1 y compara salidas) |
| `check` | `vn check [-v] FILE` | Compila el programa completo sin ejecutarlo (dependencias y enlace incluidos); sale con error si hay diagnósticos de error. Reutiliza la caché de compilación |
| `eval` | `vn eval [-v] [permisos] CODE` | Evalúa una expresión/programa corto |
| `repl` | `vn repl [--debug-bytecode] [permisos]` | Bucle interactivo; cada línea es un programa aparte |
| `test` | `vn test [PATH] [-f FILTER] [-j JOBS] [--fail-fast] [-v] [permisos]` | Ejecuta cada `.vn` de PATH (por defecto `tests/`, un nivel) |
| `bench` | `vn bench [FILE] [-e CODE] [--runs N=10] [--show-output] [-v] [--all-rows] [--min-clif-coverage PCT] [permisos]` | Tiempos por fase; `--min-clif-coverage` falla si la cobertura JIT queda por debajo |
| `debug` | `vn debug [FILE] [-e CODE] [-p PHASE=all] [--fn NAME] [--list-phases] [--natives] [permisos]` | Inspección por fase; `--natives` imprime la tabla de operaciones nativas; ver [`CLI_INSPECT.md`](CLI_INSPECT.md) |

### Permisos (`run`, `eval`, `repl`, `test`, `debug`, `bench`)

Sin ningún `--allow-*` el programa corre con todos los permisos. Con alguno, solo
lo pedido. `--sandbox` lo niega todo. Detalle en
[`HOST_BOUNDARY_SPEC.md`](internals/HOST_BOUNDARY_SPEC.md).

`-A/--allow-all`, `--allow-read[=PATHS]`, `--allow-write[=PATHS]`,
`--allow-net[=HOSTS]`, `--allow-env[=VARS]`, `--allow-ffi`, `--allow-ui`,
`--sandbox`.

## Construir y formatear

| Comando | Uso | Notas |
|---|---|---|
| `build` | `vn build FILE [-o PATH] [-t bytecode\|exe\|native] [-v]` | `bytecode` escribe un artefacto portable `.vnc` (grafo de módulos compilado; `vn run app.vnc` lo ejecuta). `exe` copia el `vn` en ejecución y le añade la imagen de la std que usa, el grafo y un trailer `VEXE`: el resultado es autocontenido (no lee `.env` ni la std del entorno) y un trailer ilegible es un error. `native` emite un ejecutable vía Cranelift + `lld-link` + `varn-rt` (prototipo AOT, solo x86_64 Windows/MSVC) |
| `fmt` | `vn fmt [PATH] [--check] [-v]` | `--check` no escribe, falla si hay cambios |
| `std-bundle` | `vn std-bundle [--std-dir std] [--out dist/std.vnb]` | Construye la imagen de la std |

## Proyecto y paquetes

| Comando | Uso |
|---|---|
| `init` | `vn init [DIR] [--name NAME]` |
| `pkg` | `vn pkg add ALIAS ORIGIN` · `remove ALIAS` · `install` · `update` · `tree` · `doctor` · `clean` |
| `task` | `vn task [NAME] [-- ARGS...]` — sin nombre lista las tareas de `varn.toml`; con nombre ejecuta sus líneas en orden desde la raíz del proyecto |
| `cache` | `vn cache clean` |
| `doctor` | `vn doctor` — diagnóstico de instalación y de la std resuelta |

## El manifiesto `varn.toml`

Un solo archivo, `varn.toml`, con campos planos: `name`, `version`, `main`, `std`,
`exports`, `dependencies`, `dev_dependencies`, `peer_dependencies`, `workspaces` y
`tasks`, `fmt` y `test`. Una clave desconocida es un error que nombra la clave y el archivo. Las
secciones `[package]` y `[project]` y el nombre `vn.toml` ya no existen.

```toml
name = "app"
version = "0.1.0"
main = "main.vn"

[dependencies]

[tasks]
dev = ["check main.vn", "run main.vn"]
```

`[fmt]` y `[test]` aceptan `include` y `exclude`: patrones relativos a la raíz del
proyecto con `*`, `?` y `**`, que limitan los archivos que `vn fmt` y `vn test` recorren
(un archivo dado explícitamente no se filtra).

Cada línea de una tarea es una línea de comando de `vn` (sin el `vn`); la primera que
falla corta la secuencia, `task otra` dentro de una tarea la encadena y un ciclo es un
error. Los argumentos tras `--` se añaden a la última línea. La raíz del proyecto es el
directorio con `varn.toml` más cercano al archivo (comandos con archivo) o al cwd
(comandos de proyecto). Al arrancar, antes de interpretar el comando, se leen en este orden de prioridad `.env.local`,
`.env` y `.vn/.env` del cwd y de la raíz del proyecto que contiene el cwd; lo mismo vale
para el `std` de `varn.toml`. `vn task` entra en la raíz del proyecto después de eso.

## Herramientas de editor

| Comando | Uso |
|---|---|
| `lsp` | `vn lsp [-p PORT] [--tcp HOST:PORT] [--stdio]` (stdio por defecto) |
| `dap` | `vn dap` (adaptador de depuración por stdio) |
| `completions` | `vn completions bash\|zsh\|fish\|power-shell\|elvish` |

## Variables de entorno

| Variable | Efecto |
|---|---|
| `VARN_STD` | Raíz de la std (directorio fuente) o `@embedded` |
| `VARN_STD_BUNDLE` | Imagen `.vnb` a usar cuando la std es `@embedded` y el binario no la lleva dentro |
| `VARN_CACHE_DIR` | Directorio de caché (aislar corridas: un directorio temporal) |
| `VARN_HOME` | Directorio base de Varn |
| `VARN_NO_JIT=1` | Solo intérprete |
| `VARN_NO_CLIF` | Desactiva el backend Cranelift |
| `VARN_JIT_TIER` / `VARN_JIT_TIER_STRAIGHT` / `VARN_JIT_OSR` | Umbrales de tiering (con bucle / sin bucle / back-edges para OSR) |
| `VARN_JIT_CACHE` | `off`, `verify` (recompila y compara con la caché) o activo por defecto |
| `VARN_CLIF_OPT` | `opt_level` de Cranelift (por defecto `speed`) |
| `VARN_CLIF_VERIFY` | Activa el verificador de Cranelift |
| `VARN_CLIF_TRACE`, `VARN_CLIF_PRINT_IR` | Traza de lowering / IR de Cranelift |
| `VARN_GC_TRACE=1` | Una línea por colección menor |
| `VARN_ALLOC_PROFILE`, `VARN_ALLOC_SLOW` | Perfil de asignaciones / fuerza el camino lento de asignación en JIT |
| `VARN_UI_REAL=1` | Backend de ventanas real para `std:ui` (por defecto headless) |
| `VARN_LSP_DEBUG` | Registro de depuración del LSP |

## Verificación del repo

`scripts/verify.ps1`:

- `-Quick`: perfil `quick`, sin lint ni bench, solo cuadrante 1 (dev-checkout + JIT).
- `-Fast`: release, sin benchmark; cuadrantes 1–4 + cuadrante 5 (`VARN_JIT_CACHE=verify`).
- `-SkipLint`: omite `cargo fmt --check` y `clippy`.
- `-CleanCache`: `vn cache clean` antes de cada cuadrante.

`scripts/verify.sh`: `--fast|-f`, `--skip-lint`, `--clean-cache`; sin `-Quick` ni cuadrante 5.
