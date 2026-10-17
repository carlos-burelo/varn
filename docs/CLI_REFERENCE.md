# Referencia del CLI `vn`

Fuente: `crates/varn-cli/src/cli.rs` (clap). `vn <comando> --help` es la referencia
viva; esta página la resume.

## Ejecutar y comprobar

| Comando | Uso | Notas |
|---|---|---|
| `run` | `vn run [OPTIONS] [FILE] [-- ARGS...]` | `-e/--eval CODE`, `-v`, `--trace`, permisos (abajo), `--compare-tiers` (corre el programa en intérprete, JIT y JIT tier 1 y compara salidas) |
| `check` | `vn check [-v] FILE` | Solo lex + parse + check; sale con error si hay diagnósticos de error |
| `eval` | `vn eval [-v] CODE` | Evalúa una expresión/programa corto |
| `repl` | `vn repl [--debug-bytecode]` | Bucle interactivo |
| `test` | `vn test [PATH] [-f FILTER] [-j JOBS] [--fail-fast] [-v]` | Ejecuta cada `.vn` de PATH (por defecto `tests/`) |
| `bench` | `vn bench [FILE] [-e CODE] [--runs N=10] [--show-output] [-v] [--all-rows] [--min-clif-coverage PCT]` | Tiempos por fase; `--min-clif-coverage` falla si la cobertura JIT queda por debajo |
| `debug` | `vn debug [FILE] [-e CODE] [-p PHASE=all] [--fn NAME] [--list-phases]` | Inspección por fase; ver [`CLI_INSPECT.md`](CLI_INSPECT.md) |

### Permisos (`run`)

Sin ningún `--allow-*` el programa corre con todos los permisos. Con alguno, solo
lo pedido. `--sandbox` lo niega todo. Detalle en
[`HOST_BOUNDARY_SPEC.md`](HOST_BOUNDARY_SPEC.md).

`-A/--allow-all`, `--allow-read[=PATHS]`, `--allow-write[=PATHS]`,
`--allow-net[=HOSTS]`, `--allow-env[=VARS]`, `--allow-ffi`, `--allow-ui`,
`--sandbox`.

## Construir y formatear

| Comando | Uso | Notas |
|---|---|---|
| `build` | `vn build FILE [-o PATH] [-t TARGET=bytecode] [--native] [-v]` | `bytecode` escribe un artefacto portable `.vnc` (grafo de módulos compilado; `vn run app.vnc` lo ejecuta). `native`/`--native` emite un ejecutable vía Cranelift + `lld-link` + `varn-rt` (prototipo AOT) |
| `fmt` | `vn fmt [PATH] [--check] [-v]` | `--check` no escribe, falla si hay cambios |
| `std-bundle` | `vn std-bundle [--std-dir std] [--out dist/std.vnb]` | Construye la imagen de la std |

## Proyecto y paquetes

| Comando | Uso |
|---|---|
| `init` | `vn init [DIR] [--name NAME]` |
| `pkg` | `vn pkg add ALIAS ORIGIN` · `remove ALIAS` · `install` · `update` · `tree` · `doctor` · `clean` |
| `cache` | `vn cache clean` |
| `doctor` | `vn doctor` — diagnóstico de instalación y de la std resuelta |

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
