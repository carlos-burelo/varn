# Estado del Workspace de Crates

Cada fase del pipeline vive en su propio crate. La jerarquía de dependencias es estricta — `varn-core` no depende de ningún crate interno.

## Diagrama del Pipeline

```mermaid
graph TD
    SourceText["Código Fuente (.wr)"] --> Parser

    subgraph Frontend
        Lexer["varn-lexer (Tokenizer)"]
        Parser["varn-parser (AST)"]
        Core["varn-core (AST nodes, OpCode)"]
        Lexer --> Parser
        Parser -.usa.-> Core
    end

    subgraph MiddleEnd["Type Checking"]
        Checker["varn-checker (tipos, CFA, SemanticDB)"]
        Checker -.usa.-> Core
    end

    Parser -- AST --> Checker

    subgraph Backend["Compilación"]
        Compiler["varn-compiler (bytecode, FunctionProto)"]
        Types["varn-types (VmValue, Chunk, ClassObj)"]
        Compiler -.usa.-> Types
    end

    Checker -- TypedAST --> Compiler

    subgraph Execution["Ejecución"]
        VM["varn-vm (register-based, NaN-boxing, IC)"]
        Runtime["varn-runtime (Tokio LocalSet, async, I/O)"]
        Builtins["varn-builtins (stdlib nativa Rust, LBI)"]
        Runtime --> VM
        VM -.usa.-> Types
        VM -.usa.-> Builtins
    end

    Compiler -- FunctionProto --> Runtime

    subgraph Tools["Herramientas"]
        CLI["varn-cli (binario wr)"]
        LSP["varn-lsp (IDE integration)"]
        PM["varn-pm (package manager)"]
        Debug["varn-debug (profiling, disasm)"]
    end

    CLI --> Runtime
    LSP --> Checker
```

## Responsabilidades

### `varn-core`
AST nodes, OpCode, ModuleId, Span. Sin lógica de ejecución. Compartido por todos los crates.

### `varn-lexer`
Tokenizer. UTF-8, escape sequences, ASI. Solo gramática.

### `varn-parser`
Recursive-Descent + Pratt parsing. Produce AST de `varn-core`. No sabe de tipos ni módulos.

### `varn-checker`
Type checker multi-fase: hoisting, inferencia, CFA, narrowing. Produce TypedAST + SemanticDB. No emite bytecode.

### `varn-types`
Tipos compartidos por VM y builtins: `VmValue`, `Chunk`, `FunctionProto`, `ClassObj`, `Closure`, `ResourceStore`, `NativeCtx`.

### `varn-compiler`
Lowering de AST a bytecode. Slots estáticos, upvalues, constant pool, back-patching, peephole optimizations.

### `varn-vm`
VM register-based con NaN-boxing. Inline Cache (GetProp/SetProp por clase+slot), fast-path calls (~60%), upvalues open/closed.

### `varn-runtime`
Scheduler Tokio LocalSet. Async/await, spawn, generators, timers. Single-threaded (`!Send` por `Rc`).

### `varn-builtins`
Implementaciones nativas de stdlib. LBI: `#[varn_module]` + `#[varn_fn]`/`#[varn_class]` inyectan `NativeOpEntry` en secciones del linker. `build_module()` ensambla el objeto Varn en startup.

### `varn-op-macros`
Proc macros: `#[varn_module]`, `#[varn_fn]`, `#[varn_class]`, `#[varn_constructor]`, `#[varn_method]`, `#[varn_getter]`, `#[varn_static]`, `#[varn_extends]`, `#[varn_namespace]`.

### `varn-modules`
Registro canónico de módulos (`MODULE_REGISTRY`). Resolución topológica. Especificadores `std:*`, `builtin:*`.

### `varn-cli`
Binario `wr`. Pipeline completo: `run`, `check`, `build`, `bench`, `disasm`, `inspect`, `repl`, `eval`, `info`, `init`, `lsp`, `doctor`, `add`, `remove`, `install`, `update`.

### `varn-lsp`
LSP con tower-lsp + tokio. Consulta SemanticDB. Hover, completions, go-to-definition, semantic tokens.

### `varn-pm`
Package manager. Resolución semver sobre GitHub/Gitea tags API. Caché global `~/.wr/cache/`. SHA256 integrity. Lockfile `varn.lock`.

### `varn-debug`
Profiling, disassembly, inspección de estructuras internas.

### `varn-diagnostics`
Reporte de errores con spans y subrayados. Formato CLI y LSP.

### `varn-base`
Utilidades comunes compartidas.
