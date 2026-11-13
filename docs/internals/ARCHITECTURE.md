# Arquitectura de Varn

Mapa del compilador y del runtime. Cada cita es un archivo o símbolo del repo; si
contradice al código, manda el código. Contrato de diseño: [`AGENTS.md`](../../AGENTS.md).

## Pipeline

```
texto ─lex─▶ tokens ─parse─▶ AST ─check─▶ CheckResult ─emit─▶ TIR ─link─▶ TIR ─compile─▶ SSA ─▶ bytecode ─▶ VM ⇄ JIT
```

Orquesta `vn-pipeline` (`Session::compile` → `Compilation::advance`, `compilation.rs`):
lee la fuente, compila (con caché de grafo de módulos si no hay flags de depuración) y
ejecuta.

| Fase | Dónde | Entrada → salida |
|---|---|---|
| lex | `vn_lexer::scan` (`vn-pipeline/src/lex.rs`) | texto → `(tokens, lexeme_buf, errores)` |
| parse | `vn_parser::parse` (`parse.rs`) | tokens → `Program` + `AstArena` + `AtomInterner` |
| check | `vn_checker::Checker` (`check.rs`) | AST + resolvedor de imports → `vn_sem::output::CheckResult` |
| emit | `vn_emit::emit_module` (`lower.rs::emit_tir`) | AST + `CheckResult` → `vn_tir::TirModule` |
| link | `vn_tir::link::link` (`lower.rs::lower_module`) | TIR del importador + paquetes inlinables de sus dependencias → TIR con llamadas directas |
| compile | `vn_compiler::from_tir::compile_module` (`lower.rs::compile_tir`) | TIR → `FunctionProto` (SSA optimizado → bytecode de registros) |
| execute | `execute.rs`: `boot_machine` → `enter_main` | `FunctionProto` → ejecución en `vn_vm::Vm` |

Orden de compilación (`compile.rs::build_executable`): se descubre el grafo de imports
(`module_precompile::discover`; un ciclo es error), se bajan las dependencias antes que
el módulo de entrada (`lower_dependencies`, orden topológico inverso) y cada módulo bajado
publica sus plantillas en una `TemplateLibrary` que consume el paso `link` de sus
importadores. Ver [`COMPILER_ARCHITECTURE.md`](COMPILER_ARCHITECTURE.md) y
[`ADR-0028`](../decisions/ADR-0028-inlining-entre-modulos.md).

No hay fase JIT en el pipeline: el tiering vive dentro de la VM
(`vn-vm/src/jit/tiering.rs`) y llama a `vn-jit` cuando una función se calienta.

El checker **no** produce un AST tipado: produce `CheckResult` (bind, diagnósticos,
tabla de tipos por expresión, mapeos de llamadas, desazucarados, objetivos de test).
El TIR lo emite después `vn-emit` a partir de esos artefactos.

## Crates

31 miembros del workspace (`Cargo.toml`): 30 crates + `xtask`. Grafo de dependencias
real en [`CRATES_STATE.md`](../dev/CRATES_STATE.md).

| Capa | Crates |
|---|---|
| Hoja | `vn-core` (AST, opcodes, diagnósticos, ids), `vn-types` (valores runtime, chunks, SSA portable), `vn-tir`, `vn-jit-abi` |
| Frontend | `vn-lexer`, `vn-parser`, `vn-sem` (tipos semánticos), `vn-binder`, `vn-resolver`, `vn-checker`, `vn-modules` |
| Backend | `vn-emit` (AST → TIR), `vn-compiler` (TIR → SSA → bytecode, regalloc interno) |
| Runtime | `vn-vm`, `vn-jit` (Cranelift), `vn-builtins` (nativos + registro de módulos), `vn-macros`, `vn-gpu`, `vn-ui`, `vn-text`, `vn-rt` (AOT mínimo) |
| Herramientas | `vn-pipeline`, `vn-cli` (`vn`), `vn-lsp`, `vn-dap`, `vn-debug`, `vn-pm`, `vn-shadow` |

No existen `varn-runtime` ni `varn-regalloc`: el regalloc es
`vn-compiler/src/regalloc/` (`run_post_passes` al final de `compile_module`).

## Bytecode

`OpCode` (`vn-core/src/opcode.rs`, `repr(u8)`): 152 variantes. Las palabras de
código son `u16` (`OpCode::from_u16`). Excepciones: `Try`, `PopTry`, `Throw`; la VM
las resuelve con `ExceptionRange` del chunk.

## Runtime en una frase por pieza

- **Valor**: `VmValue` de 16 bytes (`tag` + `payload`); kinds NULL/BOOL/INT/FLOAT/HEAP/SSO/SYMBOL; strings ≤ 8 bytes viven inline (SSO).
- **GC**: no copiante; generación joven `born` (umbral `YOUNG_THRESHOLD`) + old-gen con marcas pegajosas (`vn-vm/src/heap/`).
- **Async**: `await` monta la tarea aguardada como frame de quien aguarda; solo una celda pendiente suspende.
- **Generadores**: frame congelado que se reanuda sobre la pila de quien llama.
- **JIT**: Cranelift, activaciones `Native`/`Framed`, try/catch puro sin `longjmp` en `vn-jit`. Ver [`JIT_ABI_V2_SPEC.md`](JIT_ABI_V2_SPEC.md).
- **Std**: siempre desde una imagen `std.vnb` (embebida o construida en caché), con interfaz, bytecode y plantillas de inlining por módulo. Ver [`MODULES_SYSTEM.md`](MODULES_SYSTEM.md).

## Más detalle

- [`COMPILER_ARCHITECTURE.md`](COMPILER_ARCHITECTURE.md) — TIR → SSA → pases → bytecode.
- [`TIR_CONTRATO_TIPADO.md`](TIR_CONTRATO_TIPADO.md) — contrato `TirModule`/`BackendTy`.
- [`VM_ARCHITECTURE.md`](VM_ARCHITECTURE.md), [`RUNTIME_ARCHITECTURE.md`](RUNTIME_ARCHITECTURE.md) — VM, scheduler, isolates.
- [`LBI_ARCHITECTURE.md`](LBI_ARCHITECTURE.md), [`NATIVE_ABI_SPEC.md`](NATIVE_ABI_SPEC.md), [`HOST_BOUNDARY_SPEC.md`](HOST_BOUNDARY_SPEC.md) — nativos, ABI, permisos.
