# Hoja de Ruta de Varn

Fecha: 2026-06-05

## Estado actual

El lenguaje está completo y funcional:

- **Lenguaje**: tipos, genéricos, async/await, generators, closures, destructuring, match exhaustivo, `using`, decoradores, named arguments, pipeline `|>`, extensions.
- **VM**: register-based con NaN-boxing, Inline Cache, fast-path calls (~60% de llamadas), upvalues open/closed.
- **Stdlib**: `str`, `int`, `float`, `bool`, `char`, `decimal`, `array`, `range`, `map`, `set`, `math`, `json`, `fs`, `http`, `io`, `net`, `path`, `sys`, `time`, `task`, `collections`, `result`, `option`, `types`, `reflect`, `crypto`.
- **Package manager**: `vn pkg add/remove/install/update` con resolución semver sobre git tags, caché global SHA256-verified.
- **Caché de bytecode**: `.vn/cache/` invalidado por hash de contenido.
- **Compilados portables**: `vn build` → `.vnc`, `vn run program.vnc`.
- **LSP**: servidor en `varn-lsp` con hover, completions, go-to-definition.
- **Suite de tests por defecto**: `tests/main.vn` ejecuta 48 módulos y hoy reporta `PASSED: 686`, `FAILED: 0`.
- **Corpus principal en `tests/`**: `41`, `42` y `47` ya fueron reintegrados a la suite por defecto.

## Inmediato

- [ ] Formato `.vnc` zero-copy (mmap + flat binary) para carga sub-milisegundo.
- [ ] Capability system: enforcement real en runtime (actualmente `has_capability` retorna `true` siempre).
- [ ] Verificación compile-time de divergencia entre IDL `.vn` e implementación Rust.

## Mediano plazo

- [ ] `vn publish` para publicar paquetes al registro.
- [ ] Registro de paquetes opcional (alternativa a git directo).
- [ ] Optimizador: constant folding, dead code elimination sobre el IR.
- [ ] Match guards (`if` dentro de `match`).
- [ ] Definir benchmarks canónicos públicos y separar cifras de integración vs microbenchmarks.
- [x] Reintegrar `41`, `42` y `47` a la suite principal.
- [x] Aislar y corregir el `STATUS_ACCESS_VIOLATION` de `tests/42-stdlib-comprehensive-test.vn`.

## Largo plazo

- [ ] Compilación AOT (Cranelift o LLVM backend).
- [ ] FFI con C/Rust nativo (dynamic loading `.dll`/`.so`).
- [ ] WASM target.
- [ ] Depurador interactivo.
- [ ] Modelo de paralelismo compartido más allá de isolates, sin degradar el hot path actual de la VM.

## Documentación

1. [ARCHITECTURE.md](ARCHITECTURE.md)
2. [WARP-SPEC.md](WARP-SPEC.md)
3. [LBI_ARCHITECTURE.md](LBI_ARCHITECTURE.md)
4. [VM_ARCHITECTURE.md](VM_ARCHITECTURE.md)
5. [COMPILER_ARCHITECTURE.md](COMPILER_ARCHITECTURE.md)
6. [RUNTIME_ARCHITECTURE.md](RUNTIME_ARCHITECTURE.md)
7. [HOST_BOUNDARY_SPEC.md](HOST_BOUNDARY_SPEC.md)
