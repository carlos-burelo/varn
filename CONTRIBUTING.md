# Contribuir a Varn

## Configuración

Requisitos: Rust stable, Cargo.

```bash
git clone https://github.com/tu-usuario/Varn
cd Varn
cargo build --bin wr
```

Verificar que todo funciona:

```bash
cargo run --bin vn -- tests/main.vn
# PASSED: 534 / FAILED: 0
```

## Antes de un PR

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
cargo test --workspace
cargo run --bin vn -- tests/main.vn   # suite completa
```

## Estructura del workspace

```
crates/
├── varn-core        # AST, OpCode, ModuleId — sin dependencias internas
├── varn-lexer       # Tokenizer
├── varn-parser      # Parser → AST
├── varn-checker     # Type checker + resolución de módulos
├── varn-compiler    # Codegen → FunctionProto / bytecode
├── varn-vm          # VM register-based
├── varn-types       # Tipos compartidos: VmValue, Chunk, FunctionProto, Value
├── varn-builtins    # Stdlib nativa en Rust (str, array, math, fs, http…)
├── varn-modules     # Resolución de paquetes y manifests
├── varn-pm          # Package manager (add/install/update/remove)
├── varn-op-macros   # Proc macros para bindings nativos
├── varn-cli         # Binario `wr`, pipeline completo
├── varn-debug       # Inspección, profiling, disassembly
├── varn-diagnostics # Reporte de errores
├── varn-runtime     # Async runtime
└── varn-base        # Utilidades base
```

La jerarquía de dependencias es estricta: `varn-core` no depende de ningún crate interno. Los crates de más alto nivel (`varn-cli`, `varn-vm`) dependen de los de más bajo nivel, nunca al revés.

## Convenciones

**Código Rust:**
- Sin `unwrap()` en paths que reciben input externo — usa `?` o manejo explícito.
- Errores como `String` en la interfaz pública de crates de bajo nivel (para evitar dependencias de tipos de error).
- Tipos `Rc<T>` en el compilador/VM (single-threaded). `Arc<T>` solo donde hay concurrencia real.

**Stdlib nativa (`varn-builtins`):**
- Cada módulo stdlib tiene un archivo `.vn` (interfaz) y una implementación Rust.
- Registrar funciones con `#[varn_fn]`, clases con `#[varn_class]`.
- `NativeFnResult` = `Result<VmValue, String>` — tipo canónico para funciones nativas.

**Tests:**
- Los tests de integración viven en `tests/` como archivos `.vn`.
- `tests/main.vn` ejecuta la suite completa. Debe pasar al 100% en todo PR.
- Tests unitarios Rust en `#[cfg(test)]` dentro del crate correspondiente.

**Formato `.vnc` y cache:**
- `CACHE_FORMAT_VERSION` en `varn-cli/src/pipeline/compile.rs` — incrementar si cambias `FunctionProto`, `Chunk`, o `PoolEntry`.
- El formato `.vnc` usa el mismo versioning. Cambiar la versión invalida compilados previos (comportamiento correcto).

## Política de PRs

1. Un PR = un foco. No mezcles refactors con features.
2. Si cambias `FunctionProto` o `Chunk`: incrementa `CACHE_FORMAT_VERSION`.
3. Si añades un módulo stdlib: implementa el `.vn` de interfaz + la implementación Rust + tests.
4. Si cambias el resolver de paquetes o el formato de `varn.lock`: documenta la migración.
5. Sin código muerto, sin rutas alternativas heredadas, sin `TODO` sin issue asociado.

## Reportar bugs

Abre un issue con:
- Versión de Rust (`rustc --version`)
- Código `.vn` mínimo que reproduce el problema
- Output esperado vs. obtenido
- Si es un crash: `RUST_BACKTRACE=full vn run programa.vn`
