# Varn

Lenguaje estáticamente tipado, compilado a bytecode de registros, con VM propia y JIT Cranelift. Estado: pre-1.0 (`0.1.0`), APIs inestables, breaking changes permitidos.

## Pipeline

```
texto → tokens (varn-lexer) → AST (varn-parser) → bind+check (varn-binder, varn-sem, varn-checker, varn-resolver)
→ TIR (varn-emit, varn-tir) → SSA+bytecode (varn-compiler) → exec (varn-vm, varn-jit, varn-jit-abi)
```

Orquesta `varn-pipeline`. CLI `vn`. Sin `varn-runtime`: no existe como crate.

JIT: try/catch puro (sync y async) con `jit_error_flag` único; `await` monta el cuerpo de la tarea aguardada como frame de quien aguarda; un generador es un frame congelado que se reanuda sobre la pila de quien llama. Pendiente: los vivos de un `try` viajan por memoria y fuerzan activación `Framed` (plan §2.2 P2b).

## Crates (28 + xtask)

`varn-binder, varn-builtins, varn-checker, varn-cli, varn-compiler, varn-core, varn-dap, varn-debug, varn-emit, varn-gpu, varn-jit, varn-jit-abi, varn-lexer, varn-lsp, varn-macros, varn-modules, varn-parser, varn-pipeline, varn-pm, varn-resolver, varn-rt, varn-sem, varn-shadow, varn-text, varn-tir, varn-types, varn-ui, varn-vm` + `xtask`.

Fuente: `Cargo.toml` workspace. `default-members` excluye `rt, shadow, xtask`.

## Uso

Toolchain fijada en `rust-toolchain.toml` (Rust 1.97.1).

```bash
cargo build --profile quick --bin vn
./target/quick/vn run hola.vn
./target/quick/vn check hola.vn
./target/quick/vn debug -p hola.vn
```

Comandos `vn`: `run, check, eval, repl, bench, debug, build, test, fmt, pkg, init, doctor, cache, lsp, completions, dap, std-bundle`. Fuente: `crates/varn-cli/src/cli.rs`.

Ejemplo:

```vn
function saludar(nombre: str): str {
    return `hola ${nombre}`
}
print(saludar("mundo"))
```

## Validación

Canónica: `tests/main.vn` (imports 200+ suites). `cargo test` no basta.

```powershell
.\scripts\verify.ps1 -Quick   # iteración: build quick + cuadrante dev-checkout/JIT
.\scripts\verify.ps1 -Fast    # pre-commit: fmt+clippy+audit+release+4 cuadrantes
```

Matriz (`CONTRIBUTING.md`): std `dev-checkout`/`@embedded` × JIT/`VARN_NO_JIT=1`; `verify.ps1` añade un cuadrante 5 con `VARN_JIT_CACHE=verify` (caché JIT recompilada y comparada byte a byte). Caché: `VARN_CACHE_DIR=<temp>` para aislar.

Perfiles: `quick` (iteración, sin LTO), `release` (puerta + benchs), `dist` (binarios publicados). Benchs del README anterior eliminados: números viejos (2026-09-20), re-medir antes de citar.

## Docs

`docs/` parcialmente desactualizada. Código manda (`AGENTS.md` Ley 12). Orden lectura:

- `AGENTS.md` (contrato), `CONTRIBUTING.md` (matriz)
- `docs/ARCHITECTURE.md`, `docs/COMPILER_ARCHITECTURE.md`, `docs/VM_ARCHITECTURE.md`
- `docs/lang/` (lenguaje), `docs/CLI_REFERENCE.md`, `docs/INSTALL.md`
- `docs/plans/2026-10-16-plan-unificado.md` = plan vivo (pendiente por orden)
- `docs/notes/`, `docs/decisions/` = histórico, no spec vigente

GC: young `born: Vec` con umbral `786432` (`crates/varn-vm/src/heap/young.rs`). Sin nursery bump. `JIT_HELPERS_VERSION = 9` (`crates/varn-jit-abi/src/lib.rs`).

## Licencia

Apache-2.0. Ver `LICENSE`.
