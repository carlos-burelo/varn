# Async P2b-2: Partición del CFG en funciones `async` lineales — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transformar funciones `async` con puntos de suspensión lineales (`await` sin bucles ni `try/catch` que los crucen) en máquinas de estados en la representación SSA (`varn-compiler`), partiendo el CFG en cada suspensión y generando el layout de estado y el bloque despachador de entrada, sin violar invariantes de dominancia SSA ni alterar el comportamiento en `tests/main.vn`.

**Architecture:** Se organiza en tres submódulos dentro de `crates/varn-compiler/src/passes/state_machine/`:
1. `layout.rs`: Mapea cada punto de suspensión a su discriminante y slots de variables vivas (`live_after`), calculando `state_size`.
2. `transform.rs`: Realiza la partición de bloques en cada `InstKind::Await`, construye los bloques de continuación con parámetros de bloque SSA, e inyecta el prólogo despachador en la entrada de la función.
3. `mod.rs`: Punto de entrada del pase que filtra funciones elegibles y delega en el layout y la transformación.

**Tech Stack:** Rust (workspace de 19 crates), `varn-compiler` (SSA, pases, verify), `varn-types` (`FunctionProto`). Verificación con el binario `vn`, nunca con `cargo test`.

**Spec:** `docs/superpowers/specs/2026-08-16-modelo-asincrono-design.md` — §3.1 el pase, §3.8 mecánica, §3.9 verificación de bytecode.

---

## Global Constraints

- **PROHIBIDO comandos de Git y PROHIBIDO `cargo test`.** La validación real es `tests/main.vn` y el verificador SSA `verify::verify`.
- **Matriz de 4 obligatoria** al cerrar cada tarea: `run` × `VARN_NO_JIT` × std de árbol/`@embedded`.
- `cargo build --release` con **cero warnings nuevos** (`unused_crate_dependencies = "warn"`).
- **Dominancia SSA**: Todo bloque de continuación generado debe recibir sus valores vivos mediante parámetros de bloque o instrucciones de carga válidas, pasando `verify::verify(&ssa)`.

---

## File Structure

| Fichero | Responsabilidad tras el cambio |
|---|---|
| `crates/varn-compiler/src/ssa/ir.rs` | Métodos `alloc_value`, `alloc_block`, `block_mut` en `SsaFunc` |
| `crates/varn-compiler/src/passes/state_machine/layout.rs` (**nuevo**) | Mapeo de slots de estado, discriminantes y cálculo de `state_size` |
| `crates/varn-compiler/src/passes/state_machine/transform.rs` (**nuevo**) | Partición de CFG, creación de continuaciones y prólogo despachador |
| `crates/varn-compiler/src/passes/state_machine/mod.rs` | Coordinador del pase, orquestación y filtros de elegibilidad |

---

## Task 1: Helpers de construcción en `SsaFunc`

Permitir que los pases puedan asignar nuevos valores y bloques SSA sin violar encapsulación.

**Files:**
- Modify: `crates/varn-compiler/src/ssa/ir.rs`

- [ ] **Step 1: Añadir helpers a `SsaFunc`**

```rust
impl SsaFunc {
    #[inline]
    pub fn alloc_value(&mut self, ty: HirType) -> Value {
        let v = Value(self.values.len() as u32);
        self.values.push(ValueDef { ty });
        v
    }

    #[inline]
    pub fn alloc_block(&mut self) -> BlockId {
        let id = BlockId(self.blocks.len() as u32);
        self.blocks.push(Block {
            params: Vec::new(),
            insts: Vec::new(),
            term: Terminator::Unreachable,
            preds: Vec::new(),
        });
        id
    }

    #[inline]
    pub fn block_mut(&mut self, id: BlockId) -> &mut Block {
        &mut self.blocks[id.0 as usize]
    }
}
```

- [ ] **Step 2: Compilar y verificar**

Verificar compilación limpia y matriz de 4 sin cambios.

---

## Task 2: Cálculo de Layout de Estado (`layout.rs`)

Mapear cada punto de suspensión a su discriminante y slots de variables vivas.

**Files:**
- Create: `crates/varn-compiler/src/passes/state_machine/layout.rs`
- Modify: `crates/varn-compiler/src/passes/state_machine/mod.rs`

- [ ] **Step 1: Crear `layout.rs`**

Definir `StateLayout` que calcule el slot para cada valor vivo en `point.live` y determine el `state_size` total de la función.

---

## Task 3: Partición de CFG y Prólogo Despachador (`transform.rs`)

Transformar funciones `async` lineales.

**Files:**
- Create: `crates/varn-compiler/src/passes/state_machine/transform.rs`
- Modify: `crates/varn-compiler/src/passes/state_machine/mod.rs`

- [ ] **Step 1: Implementar partición de bloques en `InstKind::Await`**
- [ ] **Step 2: Construir el bloque de prólogo despachador sobre `state[0]`**
- [ ] **Step 3: Conectar el pase en `mod.rs` para funciones `async` con puntos lineales**

---

## Task 4: Verificación Integral

- [ ] **Step 1: Verificar paso de `verify::verify(&ssa)` en todo el corpus**
- [ ] **Step 2: Comprobar con `vn debug -p ssa` y `vn debug -p bytecode`**
- [ ] **Step 3: Matriz de 4 verde (1094/0) y benchmark de estabilidad**
