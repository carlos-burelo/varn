# Primeros pasos Varn

Código manda. Fuentes: `crates/varn-cli/src/cli.rs:16-55,58-113`, `Cargo.toml:136-141`, `tests/04-templates.vn:7`, `tests/05-arrays.vn:3`.

## Build

```bash
git clone <repo-url>
cd varn-lang
cargo build --profile quick --bin vn
```

Detalles perfiles en `INSTALL.md`.

## Hola mínimo

Sintaxis verificada contra suite (`function` en `tests/05-arrays.vn:3`, `${}` en `tests/04-templates.vn:7`):

```vn
function saludar(nombre: str): str {
    return `Hola, ${nombre} desde Varn!`
}

print(saludar("Mundo"))
```

```bash
vn run hola.vn
vn check hola.vn
vn debug -p bytecode hola.vn
```

Salida esperada: `Hola, Mundo desde Varn!`

## Comandos núcleo (`cli.rs:16-55`)

| Comando | Args clave (`cli.rs`) | Uso |
|---|---|---|
| `run` | `file`, `--eval`, `--trace`, `--allow-*`, `--sandbox`, `--allow-ffi` (`58-96`) | Ejecutar |
| `check` | `file` (`116-121`) | Solo tipos, sin correr |
| `debug` | `-p/--phase=all`, `--fn NAME`, `--list-phases` (`99-113`) | Inspección fases |
| `build` | `file`, `-o`, `--target=bytecode` (`192-206`) | Artefacto |
| `bench` | `--runs=10`, `--min-clif-coverage` (`138-158`) | Perf |
| `test` / `fmt` / `pkg` / `init` / `doctor` / `cache` | — | Ciclo proyecto |

## Proyecto

```bash
vn init mi-proyecto
cd mi-proyecto
```

Genera `main.vn` + `varn.toml`. Caché global (`~/.varn/cache` o `VARN_CACHE_DIR`); `vn run` no escribe en proyecto.

## Puerta

```bash
vn run tests/main.vn
```

4 cuadrantes antes de commit: ver `INSTALL.md`. Suite canónica, único test válido (Ley 11 AGENTS.md).

## Siguiente

- `docs/CLI_INSPECT.md` — fases debug reales.
- `docs/LBI_ARCHITECTURE.md` + `docs/HOST_BOUNDARY_SPEC.md` — nativos y capabilities.
