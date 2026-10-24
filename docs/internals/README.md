# Internals

Cómo está construido Varn. Para usar el lenguaje, ver [`../lang/`](../lang/README.md).

| Capa | Documento |
|---|---|
| Visión general y mapa de crates | [ARCHITECTURE](ARCHITECTURE.md) |
| Pipeline del compilador y pases | [COMPILER_ARCHITECTURE](COMPILER_ARCHITECTURE.md) |
| Contrato `TIR` / `BackendTy` | [TIR_CONTRATO_TIPADO](TIR_CONTRATO_TIPADO.md) |
| VM y bytecode | [VM_ARCHITECTURE](VM_ARCHITECTURE.md) |
| Runtime, heap y GC | [RUNTIME_ARCHITECTURE](RUNTIME_ARCHITECTURE.md) |
| ABI del JIT | [JIT_ABI_V2_SPEC](JIT_ABI_V2_SPEC.md) |
| Módulos, imagen de std y LBI | [MODULES_SYSTEM](MODULES_SYSTEM.md), [LBI_ARCHITECTURE](LBI_ARCHITECTURE.md), [STDLIB_ARCHITECTURE](STDLIB_ARCHITECTURE.md) |
| Frontera con el host y ABI nativo | [HOST_BOUNDARY_SPEC](HOST_BOUNDARY_SPEC.md), [NATIVE_ABI_SPEC](NATIVE_ABI_SPEC.md) |
| LSP | [LSP_ARCHITECTURE](LSP_ARCHITECTURE.md) |
| UI | [UI-SPEC](UI-SPEC.md) |
