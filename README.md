# Varn

**Sintaxis de alto nivel, rendimiento de bajo nivel.** Varn es un lenguaje
estáticamente tipado con sintaxis familiar (al estilo TypeScript), compilado a
bytecode de registros que ejecuta una VM propia con JIT Cranelift.

```vn
import { describe, it, expect } from "std:test"

enum Forma { Circulo(float), Rect(float, float) }

function area(f: Forma): float {
    return match (f) {
        Circulo(r) => 3.14159 * r * r,
        Rect(w, h) => w * h,
    }
}

async function total(formas: Forma[]): float {
    return formas.map(area).reduce((acc, a) => acc + a, 0.0)
}

await describe("formas", async () => {
    await it("suma áreas", async () => {
        expect(await total([Forma.Rect(2.0, 3.0), Forma.Rect(1.0, 1.0)])).toBe(7.0)
    })
})
```

> Estado: pre-1.0 (`0.1.0`). APIs inestables; los cambios incompatibles se
> aceptan cuando traen una ganancia medible ([`AGENTS.md`](AGENTS.md), Ley 10).

## Qué ofrece

- **Tipos estáticos de punta a punta**: el tipo que conoce el checker llega al TIR, al bytecode y al JIT. `dynamic` existe, pero es explícito.
- **Nulos explícitos**: `T?`, `?.`, `??` y estrechamiento por flujo de control.
- **Tipos algebraicos**: enums con payload, uniones, tuplas `#[…]` y records `#{…}` inmutables con igualdad estructural, `match` exhaustivo con guardas.
- **Clases e interfaces**: herencia, abstractas, constructores primarios, getters/setters, extensiones, decoradores.
- **Async real**: `async`/`await`, generadores y generadores async, tareas, `TaskGroup`, canales, isolates en hilos del SO.
- **Errores**: `try`/`catch`/`finally`, multi-catch tipado y operador `try` sobre `Result`/`Option`/nulables.
- **Permisos de host**: `--sandbox` y `--allow-read/write/net/env/ffi/ui`, al estilo Deno.
- **Std amplia**: `fs`, `http`, `ws`, `net`, `sqlite`, `crypto`, `encoding` (JSON, CSV, TOML, msgpack…), `time`, `regex`, `task`, `test`, `gpu`, `ui`, `text`…
- **Tooling**: LSP, DAP, formateador, gestor de paquetes y `vn debug` para ver cada fase del compilador.

## Empezar

Requiere Rust (la toolchain está fijada en `rust-toolchain.toml`; rustup la instala sola).

```bash
git clone https://github.com/carlos-burelo/varn
cd varn
cargo build --profile quick --bin vn

./target/quick/vn run hola.vn
./target/quick/vn check hola.vn
./target/quick/vn debug -p bytecode hola.vn
```

Guía paso a paso: [`docs/GETTING_STARTED.md`](docs/GETTING_STARTED.md).
Instalación y perfiles: [`docs/INSTALL.md`](docs/INSTALL.md).

## Comandos

`run`, `check`, `eval`, `repl`, `test`, `bench`, `debug`, `build`, `fmt`, `pkg`,
`init`, `doctor`, `cache`, `lsp`, `dap`, `completions`, `std-bundle`.
Referencia completa y variables de entorno: [`docs/CLI_REFERENCE.md`](docs/CLI_REFERENCE.md).

## Cómo está hecho

```
texto → tokens → AST → check → TIR tipado → SSA → pases → bytecode → VM ⇄ JIT (Cranelift)
```

28 crates en un workspace (`varn-lexer`, `varn-parser`, `varn-checker`, `varn-emit`,
`varn-compiler`, `varn-vm`, `varn-jit`, `varn-builtins`, `varn-lsp`…). Cada fase
produce un artefacto inmutable que consume la siguiente, y las dependencias van en
una sola dirección. Mapa completo: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Documentación

| Para | Leer |
|---|---|
| Aprender el lenguaje | [`docs/lang/`](docs/lang/README.md) — cada rasgo citado con su test |
| Usar el CLI | [`docs/CLI_REFERENCE.md`](docs/CLI_REFERENCE.md), [`docs/CLI_INSPECT.md`](docs/CLI_INSPECT.md) |
| La biblioteca estándar | [`docs/STDLIB_ARCHITECTURE.md`](docs/STDLIB_ARCHITECTURE.md), [`docs/lang/standard_library.md`](docs/lang/standard_library.md) |
| Entender el compilador | [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md), [`docs/COMPILER_ARCHITECTURE.md`](docs/COMPILER_ARCHITECTURE.md), [`docs/TIR_CONTRATO_TIPADO.md`](docs/TIR_CONTRATO_TIPADO.md) |
| Entender el runtime | [`docs/VM_ARCHITECTURE.md`](docs/VM_ARCHITECTURE.md), [`docs/RUNTIME_ARCHITECTURE.md`](docs/RUNTIME_ARCHITECTURE.md), [`docs/JIT_ABI_V2_SPEC.md`](docs/JIT_ABI_V2_SPEC.md) |
| Módulos, nativos y permisos | [`docs/MODULES_SYSTEM.md`](docs/MODULES_SYSTEM.md), [`docs/LBI_ARCHITECTURE.md`](docs/LBI_ARCHITECTURE.md), [`docs/HOST_BOUNDARY_SPEC.md`](docs/HOST_BOUNDARY_SPEC.md) |
| Qué viene | [`docs/plans/2026-10-16-plan-unificado.md`](docs/plans/2026-10-16-plan-unificado.md) (plan vivo), [`docs/ROADMAP.md`](docs/ROADMAP.md) |

`docs/decisions/` (ADRs), `docs/notes/` y los `docs/AUDIT_*` son registro
histórico, no especificación. Si un documento contradice al código, manda el código.

## Contribuir

Lee [`AGENTS.md`](AGENTS.md) (contrato de arquitectura) y
[`CONTRIBUTING.md`](CONTRIBUTING.md). La prueba canónica es la suite
`tests/main.vn` (más de 3500 casos), no `cargo test`:

```powershell
.\scripts\verify.ps1 -Quick   # iteración: build quick + un cuadrante
.\scripts\verify.ps1 -Fast    # antes de commitear: fmt + clippy + release + 5 cuadrantes
```

Los cuadrantes cruzan la procedencia de la std (`dev-checkout` / `@embedded`) con
JIT / intérprete (`VARN_NO_JIT=1`), más la verificación de la caché JIT.

## Licencia

Apache-2.0. Ver [`LICENSE`](LICENSE).
