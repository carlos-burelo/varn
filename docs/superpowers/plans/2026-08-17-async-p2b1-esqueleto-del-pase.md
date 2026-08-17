# Async P2b-1: esqueleto del pase de máquinas de estados — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Poner en pie el pase de máquinas de estados con su convención de retorno y su fontanería, y hacerlo transformar el caso trivial —una `async` sin ningún `await`— con comportamiento idéntico. Sin partir todavía ningún CFG.

**Architecture:** Tres tareas, cada una verificable por separado. Se fija la convención `Poll` (que no puede ser un valor de retorno, porque una función del VM devuelve un solo `VmValue`); se publica `state_size` en `FunctionProto`; se registra el pase en el pipeline como identidad, verificado con oráculo byte a byte; y sólo entonces transforma la única forma que no requiere partir nada: una función `async` cuyo cuerpo no suspende.

**Tech Stack:** Rust, `varn-compiler` (`ssa/`, `passes/`), `varn-types` (`FunctionProto`). Verificación con el binario `vn`, nunca con `cargo test`.

**Spec:** `docs/superpowers/specs/2026-08-16-modelo-asincrono-design.md` — §3.1 el pase, §3.8 mecánica (las cinco decisiones cerradas)

## Global Constraints

- **PROHIBIDO `cargo test`.** Irrelevante como señal en este repo.
- **Matriz de 4** al cerrar cada tarea: `run` × `VARN_NO_JIT` × std de árbol/`@embedded`. Script `verify.sh`. Debe salir **1094/0** en las cuatro celdas.
- **Oráculo byte a byte** (`exact.sh`) sobre `tests/main.vn` y el corpus. Las tareas 1-3 **no pueden moverlo**. La tarea 4 sí lo mueve, y ahí se re-captura sólo tras revisar.
- `cargo build --release` con **cero warnings nuevos**. **No añadir dependencias.**
- Localizar por símbolo, no por número de línea: los números describen el árbol en `3edb889`.
- Directorio temporal (el shell del harness no conserva exports entre llamadas; exportar en cada llamada):
  ```bash
  export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
  ```
- Base: `3edb889`.

---

## Decisión de plan: la convención `Poll`

El spec (§3.8) dice que `poll` es una función de bytecode normal. Una función del VM devuelve **un solo `VmValue`**, así que `Poll<T>` no puede ser el valor de retorno sin alocar un `EnumVariant` por cada poll — justo el coste que el proyecto viene a eliminar.

**`Poll` es por tanto una convención sobre el discriminante que el estado ya lleva**, no un tipo de retorno:

| Slot | Contenido |
|---|---|
| `state[0]` | discriminante: punto de reanudación, o un valor reservado |
| retorno de `poll` | el valor asociado (`Ready` → el resultado; `Yielded` → lo emitido; `Pending` → sin usar) |

Discriminantes reservados:

```
STATE_DONE      = 0     ->  Ready(retorno)
STATE_YIELDED   = 1     ->  Yielded(retorno)
2..             ->  Pending, y el número es el punto de reanudación
```

El llamante lee `state[0]` tras la llamada. **No es coste extra**: ya tiene el estado en la mano y ya lo va a tocar para reanudar.

> Esta convención la deriva el plan, no la fijaba el spec. Si al implementarla aparece una razón para cambiarla, es una decisión de diseño y hay que subirla, no resolverla dentro de una tarea.

---

## File Structure

| Fichero | Responsabilidad |
|---|---|
| `crates/varn-types/src/chunk/proto.rs` | `FunctionProto` publica `state_size` |
| `crates/varn-compiler/src/passes/state_machine/mod.rs` (**nuevo**) | El pase. Consume `ssa::suspend::analyze` y `ssa::liveness` |
| `crates/varn-compiler/src/passes/mod.rs` | Registra el pase |
| `crates/varn-compiler/src/ssa/emit/mod.rs` | Propaga `state_size` al proto |

El pase va en `passes/state_machine/` (directorio, no fichero suelto) porque va a crecer: los cortes de CFG, los bucles y el `try` llegan en planes posteriores y cada uno merece su módulo. Empezar en directorio evita el refactor de mudanza a mitad del proyecto.

**Orden en el pipeline.** El pase corre **después** de `passes::optimize_with` y **antes** de `assign_registers`, no dentro del bucle a punto fijo de `optimize_with`. Motivo: transforma una función en otra con forma distinta, y volver a pasarle `licm`/`cse`/`cfg` por encima sería reoptimizar una máquina de estados como si fuera código normal. `try_compile_function` (`ssa/mod.rs`) es el sitio.

---

## Task 1: `state_size` en `FunctionProto`

**Files:**
- Modify: `crates/varn-types/src/chunk/proto.rs`
- Modify: `crates/varn-compiler/src/ssa/emit/mod.rs`

**Interfaces:**
- Produces: `FunctionProto.state_size: u16`, `0` para toda función no transformada. La Task 3 lo rellena para el caso trivial; el camino de coste cero de planes posteriores lo lee en runtime.

- [ ] **Step 1: Confirmar el punto de partida**

```bash
cd /c/Users/x/dev/varn/varn-lang
git log --oneline -1     # esperado: 3edb889
git status --porcelain   # esperado: vacío
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
"$SCRATCH/exact.sh"      # esperado: OK en los dos
```

- [ ] **Step 2: Añadir el campo**

En `crates/varn-types/src/chunk/proto.rs`, dentro de `pub struct FunctionProto`, junto a los demás campos con `#[serde(default)]`:

```rust
    /// Palabras que ocupa el objeto de estado de esta función si es una
    /// máquina de estados; `0` si no lo es.
    ///
    /// Lo publica el proto —igual que `register_count`— para que el sitio de
    /// llamada pueda reservar el estado **sin conocer el callee en
    /// compilación**: lo lee en runtime. Eso es lo que mete al despacho
    /// dinámico (métodos de interfaz, callbacks, `dynamic`) en el camino de
    /// coste cero. Ver spec §3.8.
    #[serde(default)]
    pub state_size: u16,
```

`#[serde(default)]` mantiene compatible la deserialización de artefactos, y de todas formas `CACHE_FORMAT_VERSION = BUILD_FINGERPRINT` invalida las cachés al cambiar el crate.

- [ ] **Step 3: Rellenarlo donde se construye el proto**

Son **tres sitios**, y dos están fuera de `varn-compiler` — el compilador los señalará todos, pero conviene saberlo antes para no sorprenderse:

- `crates/varn-compiler/src/ssa/emit/mod.rs:111` — el real.
- `crates/varn-debug/src/lib.rs:38` — `resolved_copy`, en otro crate.
- `crates/varn-vm/src/jit/tiering.rs:352` — `bare_proto`, helper de pruebas.

Poner `state_size: 0` en los tres: todavía no hay ninguna máquina de estados.

`FunctionProto` implementa `PartialEq` a mano (`proto.rs:308`) y `Hash` también, ambos enumerando campos uno a uno. **`state_size` debe entrar en los dos**: dos protos con distinto tamaño de estado no son intercambiables, y omitirlo haría que una caché los confundiera. Anotarlo en el informe.

- [ ] **Step 4: Verificar**

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cd /c/Users/x/dev/varn/varn-lang
cargo build --release 2>&1 | grep -E "^(error|warning)" | head; echo "(vacio = limpio)"
"$SCRATCH/exact.sh"
"$SCRATCH/verify.sh"
```

Esperado: build limpio, byte a byte `OK`, matriz de 4 con **1094/0**. Un campo que siempre vale `0` no puede mover el bytecode.

- [ ] **Step 5: Commit**

```bash
cd /c/Users/x/dev/varn/varn-lang
git add crates/varn-types/src/chunk/proto.rs crates/varn-compiler/src/ssa/emit/mod.rs
git commit -m "feat(types): FunctionProto publica state_size

El sitio de llamada necesita el tamano del objeto de estado para
reservarlo. Publicarlo en el proto —como register_count— permite leerlo
en RUNTIME, asi que el camino de coste cero vale tambien con callee
dinamico: metodos de interfaz, callbacks, dynamic. Ver spec 3.8.

Cero para toda funcion no transformada, que hoy son todas."
```

---

## Task 2: El pase, como identidad

Registrar el pase en el pipeline sin que transforme nada. El objetivo es aislar el riesgo de fontanería del riesgo de transformación: si el bytecode se mueve aquí, el problema es dónde se enchufó, no qué hace.

**Files:**
- Create: `crates/varn-compiler/src/passes/state_machine/mod.rs`
- Modify: `crates/varn-compiler/src/passes/mod.rs`
- Modify: `crates/varn-compiler/src/ssa/mod.rs`

**Interfaces:**
- Consumes: `SsaFunc`, `ssa::suspend::analyze`, `SsaFunc.is_async`/`is_generator`.
- Produces: `pub fn run(func: &mut SsaFunc) -> u16` — devuelve el `state_size` resultante (`0` si no transformó).

- [ ] **Step 1: Crear el pase**

`crates/varn-compiler/src/passes/state_machine/mod.rs`:

```rust
//! Transformación de funciones suspendibles en máquinas de estados.
//!
//! Corre FUERA del bucle a punto fijo de `optimize_with`, después de él y
//! antes de la asignación de registros: transforma una función en otra de
//! forma distinta, y volver a pasarle `licm`/`cse`/`cfg` por encima sería
//! reoptimizar una máquina de estados como si fuera código normal.
//!
//! ## La convención `Poll`
//!
//! Una función del VM devuelve un solo `VmValue`, así que `Poll<T>` no puede
//! ser el valor de retorno sin alocar por cada poll. Es una convención sobre
//! el discriminante que el estado ya lleva:
//!
//! | `state[0]`      | significado                        |
//! |-----------------|------------------------------------|
//! | `STATE_DONE`    | `Ready`; el retorno es el resultado |
//! | `STATE_YIELDED` | `Yielded`; el retorno es lo emitido |
//! | `>= FIRST_RESUME` | `Pending`; el número es el punto de reanudación |
//!
//! El llamante lee `state[0]` tras la llamada — algo que ya iba a hacer.

use crate::ssa::ir::SsaFunc;
use crate::ssa::suspend;

/// `state[0]` cuando la máquina terminó.
pub const STATE_DONE: u32 = 0;
/// `state[0]` cuando la máquina emitió un valor y sigue viva.
pub const STATE_YIELDED: u32 = 1;
/// Primer discriminante que denota un punto de reanudación.
pub const FIRST_RESUME: u32 = 2;

/// Transforma `func` si es suspendible. Devuelve el tamaño del objeto de
/// estado en palabras, o `0` si no la transformó.
pub fn run(func: &mut SsaFunc) -> u16 {
    if !func.is_async && !func.is_generator {
        return 0;
    }
    let points = suspend::analyze(func);
    if !points.is_empty() {
        // Los cortes de CFG llegan en el plan siguiente. Hasta entonces, una
        // función que sí suspende se deja intacta y sigue por el camino
        // actual (`run_lazy_task_sync`), que aún está vivo.
        return 0;
    }
    // Caso trivial: la Task 4 lo trata.
    0
}
```

- [ ] **Step 2: Declarar el módulo y enchufarlo**

En `crates/varn-compiler/src/passes/mod.rs`, declarar `pub mod state_machine;` junto a los demás. **No lo metas dentro del bucle de `optimize_with`.**

En `crates/varn-compiler/src/ssa/mod.rs`, dentro de `try_compile_function`, entre `optimize_with` y `verify`:

```rust
    crate::passes::optimize_with(&mut ssa, &crate::hir::ctor_summary::current());
    let state_size = crate::passes::state_machine::run(&mut ssa);
    if let Err(why) = verify::verify(&ssa) {
        panic!("ssa: verify failed for {}: {}", f.name, why);
    }
    let mut proto = emit::emit_function(ssa, f, source_file)?;
    proto.state_size = state_size;
    Ok(proto)
```

Ajustar a la forma real de la función (hoy hace `emit::emit_function(...)` como expresión final). `lower_module` tiene la misma estructura y también debe llamarlo — el top-level puede llevar `await`.

- [ ] **Step 3: Verificar que es identidad**

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cd /c/Users/x/dev/varn/varn-lang
cargo build --release 2>&1 | grep -E "^(error|warning)" | head; echo "(vacio = limpio)"
"$SCRATCH/exact.sh"
"$SCRATCH/verify.sh"
```

Esperado: **byte a byte `OK`**. El pase devuelve `0` en todos los caminos y no muta `func`. Un `DIFF` aquí significa que se enchufó donde no tocaba — para e investiga antes de seguir; es exactamente el riesgo que esta tarea aísla.

- [ ] **Step 4: Commit**

```bash
cd /c/Users/x/dev/varn/varn-lang
git add crates/varn-compiler/src/passes/state_machine/mod.rs crates/varn-compiler/src/passes/mod.rs crates/varn-compiler/src/ssa/mod.rs
git commit -m "feat(compiler): esqueleto del pase de maquinas de estados

Registrado en el pipeline entre optimize_with y assign_registers, fuera
del bucle a punto fijo: transforma una funcion en otra de forma distinta,
y reoptimizarla con licm/cse/cfg despues seria tratar una maquina de
estados como codigo normal.

No transforma nada todavia. Aisla el riesgo de fontaneria del riesgo de
transformacion: bytecode identico byte a byte."
```

---

## Task 3: Reconocer el caso trivial

Una función `async` cuyo cuerpo **no suspende** sigue siendo una máquina de estados: de un solo estado. Es la única forma que no requiere partir ningún CFG, así que ejercita la fontanería entera sin tocar la parte arriesgada.

**Files:**
- Modify: `crates/varn-compiler/src/passes/state_machine/mod.rs`

**Interfaces:**
- Produces: `state_size = 1` para `async` sin puntos de suspensión (sólo el discriminante). El pase sigue sin mutar el CFG.

- [ ] **Step 1: Escribir la prueba antes que el código**

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cat > "$SCRATCH/trivial.vn" <<'EOF'
async function sinAwait(x: int): int { return x + 1; }
async function conAwait(x: int): int { return await sinAwait(x); }
function normal(x: int): int { return x + 1; }
function* gen(): int { yield 1; }
function* genSinYield(): void { return; }
async function* agenSinYield(): void { return; }
print(await conAwait(1));
EOF
cd /c/Users/x/dev/varn/varn-lang
./target/release/vn.exe debug -p suspend "$SCRATCH/trivial.vn" 2>&1 | sed 's/\x1b\[[0-9;]*m//g'
```

Anotar del volcado cuáles tienen puntos de suspensión. Expectativa a verificar en el paso 3:

| Función | `is_async` | puntos | `state_size` esperado |
|---|---|---|---|
| `sinAwait` | sí | 0 | **1** |
| `conAwait` | sí | 1 | 0 (aún no se transforma) |
| `normal` | no | 0 | 0 |
| `gen` | generador | 1 | 0 (aún no) |
| `genSinYield` | generador | 0 | 0 (generador, no async — gate lo excluye aunque no tenga puntos) |
| `agenSinYield` | async + generador | 0 | 0 (`is_generator` excluye aunque `is_async` sea true y no suspenda) |

- [ ] **Step 2: Implementar**

En `run`, sustituir el `// Caso trivial` por:

```rust
    // Una `async` que no suspende sigue siendo una máquina de estados: de un
    // solo estado. No hay CFG que partir ni valores que guardar, así que el
    // objeto de estado es sólo el discriminante.
    //
    // El caso importa por sí mismo: es donde el camino de coste cero se ve
    // más claro (el estado nunca sale de los registros del marco), y es la
    // forma que ejercita toda la fontanería sin tocar la parte arriesgada.
    1
```

- [ ] **Step 3: Verificar contra la tabla del paso 1**

Hace falta ver el `state_size` resultante. `crates/varn-debug/src/bytecode.rs:114-118` ya imprime una cabecera con la forma `(arity: N, regs: N, upvalues: N)` seguida de un `flags_str`. Añadir ahí `state_size` — en la tupla si es siempre relevante, o dentro de `flags_str` si prefieres que sólo aparezca cuando es distinto de cero. Es información permanente y útil, no instrumentación temporal.

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cd /c/Users/x/dev/varn/varn-lang
cargo build --release 2>&1 | grep -E "^(error|warning)" | head
./target/release/vn.exe debug -p bytecode "$SCRATCH/trivial.vn" 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -E "^fn |state_size"
```

Esperado: `sinAwait` con `state_size=1`; las otras tres con `0`. **Las cuatro filas deben salir como dice la tabla.**

- [ ] **Step 4: Verificar comportamiento**

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
"$SCRATCH/verify.sh"
"$SCRATCH/exact.sh"
```

`verify.sh`: **1094/0**. `exact.sh`: dará `DIFF` **sólo** por las líneas de `state_size` que el volcado ahora imprime, no por instrucciones. Comprobar leyendo el diff que **ninguna línea de instrucción cambió**, y reportarlo; no re-capturar la referencia — eso lo hace el controlador tras revisar.

- [ ] **Step 5: Commit**

```bash
cd /c/Users/x/dev/varn/varn-lang
git add crates/varn-compiler/src/passes/state_machine/mod.rs crates/varn-debug/src/bytecode.rs
git commit -m "feat(compiler): el pase reconoce la async que no suspende

Una async sin puntos de suspension es una maquina de estados de un solo
estado: sin CFG que partir y sin valores que guardar, el objeto de estado
es solo el discriminante. state_size = 1.

Es la forma que ejercita toda la fontaneria sin tocar la parte
arriesgada, y donde el camino de coste cero se ve mas claro: el estado
nunca sale de los registros del marco.

El volcado -p bytecode muestra state_size."
```

---

## Cierre del plan

- [ ] **Matriz de 4 completa, incluido `bench`**

```bash
cd /c/Users/x/dev/varn/varn-lang
./target/release/vn.exe cache clean
./target/release/vn.exe bench ./tests/main.vn -v 2>&1 | grep -E "p50 e2e|cobertura clif"
VARN_NO_JIT=1 ./target/release/vn.exe bench ./tests/main.vn -v 2>&1 | tail -3
```

- [ ] **Contar cuántas funciones del corpus caen en el caso trivial**

Insumo del plan siguiente: dice cuánto cubre ya el camino sin partir CFG.

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cd /c/Users/x/dev/varn/varn-lang
n=0; t=0
for f in tests/*.vn std/*.vn; do
  n=$((n + $(./target/release/vn.exe debug -p bytecode "$f" 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | grep -c "state_size=1")))
  t=$((t + $(./target/release/vn.exe debug -p ssa "$f" 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | grep -c "\[async\]")))
done
echo "async sin suspension: $n de $t funciones async"
```

---

## Fuera de alcance de este plan

- **Partir el CFG.** Es el plan siguiente y el corazón del proyecto. Necesita los cortes, la selección de campos por `live_after`, y la emisión del prólogo de `poll`.
- Bucles con `await` (7 de 127 puntos) y `try` cruzando suspensión (13 de 127): planes posteriores, y el spec §9 los marca como los que rompen las implementaciones ingenuas.
- `function*` y `async function*`: el mismo pase, con las variantes `Yielded`.
- El camino de coste cero en el sitio de llamada: necesita el pase funcionando primero.
- **Nada se borra todavía.** `fork_for_task`, `run_lazy_task_sync`, `NanGenDriver`, `VmSuspend`, `jit_await`, `jit_suspend_buf`, el `longjmp`, `LazyTask` y el `unsafe impl Send` de `AsyncTask` siguen intactos y siguen siendo el camino real de ejecución.
