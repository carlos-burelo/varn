# Arquitectura de Varn

Mapa del compilador y del runtime. Cada cita es un archivo o símbolo del repo; si
contradice al código, manda el código. Contrato de diseño: [`AGENTS.md`](../../AGENTS.md).

## Pipeline

```
texto ─lex─▶ tokens ─parse─▶ AST ─check─▶ CheckResult ─emit─▶ TIR ─compile─▶ SSA ─▶ bytecode ─▶ VM ⇄ JIT
```

Orquesta `varn-pipeline` (`run` en `src/lib.rs`): lee la fuente, compila (con caché
de grafo de módulos si no hay flags de depuración) y ejecuta.

| Fase | Dónde | Entrada → salida |
|---|---|---|
| lex | `varn_lexer::scan` (`varn-pipeline/src/lex.rs`) | texto → `(tokens, lexeme_buf, errores)` |
| parse | `varn_parser::parse` (`parse.rs`) | tokens → `Program` + `AstArena` + `AtomInterner` |
| check | `varn_checker::Checker` (`check.rs`) | AST + resolvedor de imports → `varn_sem::output::CheckResult` |
| emit | `varn_emit::emit_module` (`compile.rs::emit_tir`) | AST + `CheckResult` → `varn_tir::TirModule` |
| compile | `varn_compiler::from_tir::compile_module` (`compile.rs::compile_tir`) | TIR → `FunctionProto` (SSA optimizado → bytecode de registros) |
| execute | `execute.rs`: `boot_machine` → `enter_main` | `FunctionProto` → ejecución en `varn_vm::Vm` |

No hay fase JIT en el pipeline: el tiering vive dentro de la VM
(`varn-vm/src/jit/tiering.rs`) y llama a `varn-jit` cuando una función se calienta.

El checker **no** produce un AST tipado: produce `CheckResult` (bind, diagnósticos,
tabla de tipos por expresión, mapeos de llamadas, desazucarados, objetivos de test).
El TIR lo emite después `varn-emit` a partir de esos artefactos.

## Crates

29 miembros del workspace (`Cargo.toml`): 28 crates + `xtask`. Grafo de dependencias
real en [`CRATES_STATE.md`](../dev/CRATES_STATE.md).

| Capa | Crates |
|---|---|
| Hoja | `varn-core` (AST, opcodes, diagnósticos, ids), `varn-types` (valores runtime, chunks, SSA portable), `varn-tir`, `varn-jit-abi` |
| Frontend | `varn-lexer`, `varn-parser`, `varn-sem` (tipos semánticos), `varn-binder`, `varn-resolver`, `varn-checker`, `varn-modules` |
| Backend | `varn-emit` (AST → TIR), `varn-compiler` (TIR → SSA → bytecode, regalloc interno) |
| Runtime | `varn-vm`, `varn-jit` (Cranelift), `varn-builtins` (nativos + registro de módulos), `varn-macros`, `varn-gpu`, `varn-ui`, `varn-text`, `varn-rt` (AOT mínimo) |
| Herramientas | `varn-pipeline`, `varn-cli` (`vn`), `varn-lsp`, `varn-dap`, `varn-debug`, `varn-pm`, `varn-shadow` |

No existen `varn-runtime` ni `varn-regalloc`: el regalloc es
`varn-compiler/src/regalloc/` (`run_post_passes` al final de `compile_module`).

## Bytecode

`OpCode` (`varn-core/src/opcode.rs`, `repr(u8)`): 148 variantes. Las palabras de
código son `u16` (`OpCode::from_u16`). Excepciones: `Try`, `PopTry`, `Throw`; la VM
las resuelve con `ExceptionRange` del chunk.

## Runtime en una frase por pieza

- **Valor**: `VmValue` de 16 bytes (`tag` + `payload`); kinds NULL/BOOL/INT/FLOAT/HEAP/SSO/SYMBOL; strings ≤ 8 bytes viven inline (SSO).
- **GC**: no copiante; generación joven `born` (umbral `YOUNG_THRESHOLD`) + old-gen con marcas pegajosas (`varn-vm/src/heap/`).
- **Async**: `await` monta la tarea aguardada como frame de quien aguarda; solo una celda pendiente suspende.
- **Generadores**: frame congelado que se reanuda sobre la pila de quien llama.
- **JIT**: Cranelift, activaciones `Native`/`Framed`, try/catch puro sin `longjmp` en `varn-jit`. Ver [`JIT_ABI_V2_SPEC.md`](JIT_ABI_V2_SPEC.md).
- **Std**: siempre desde una imagen `std.vnb` (embebida o construida en caché). Ver [`MODULES_SYSTEM.md`](MODULES_SYSTEM.md).

## Más detalle

- [`COMPILER_ARCHITECTURE.md`](COMPILER_ARCHITECTURE.md) — TIR → SSA → pases → bytecode.
- [`TIR_CONTRATO_TIPADO.md`](TIR_CONTRATO_TIPADO.md) — contrato `TirModule`/`BackendTy`.
- [`VM_ARCHITECTURE.md`](VM_ARCHITECTURE.md), [`RUNTIME_ARCHITECTURE.md`](RUNTIME_ARCHITECTURE.md) — VM, scheduler, isolates.
- [`LBI_ARCHITECTURE.md`](LBI_ARCHITECTURE.md), [`NATIVE_ABI_SPEC.md`](NATIVE_ABI_SPEC.md), [`HOST_BOUNDARY_SPEC.md`](HOST_BOUNDARY_SPEC.md) — nativos, ABI, permisos.
