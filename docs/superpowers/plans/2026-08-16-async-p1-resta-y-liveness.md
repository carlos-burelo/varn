# Async P1: resta y liveness compartida — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Borrar los cinco mecanismos de suspensión sin productor y extraer la liveness SSA a un análisis compartido, dejando el terreno listo para el pase de máquinas de estados.

**Architecture:** Dos mitades independientes. La primera es supresión pura: cinco mecanismos que ningún código produce (`VmSuspend::Task`, `GenChannel`, `AsyncQueue`, `deferred_tasks`, `set_timer`/`clear_timer`), cuyo borrado no puede cambiar el comportamiento por construcción. La segunda extrae el dataflow de liveness que hoy vive dentro de `assign_registers` a un módulo propio con dos consumidores, sin alterar una sola instrucción de bytecode.

**Tech Stack:** Rust (workspace de 19 crates), `varn-compiler` (HIR→SSA→pases), `varn-types`, `varn-vm`. Verificación con el binario `vn`, nunca con `cargo test`.

**Spec:** `docs/superpowers/specs/2026-08-16-modelo-asincrono-design.md` (§1.2 inventario de mecanismos muertos, §3.1 fuente de la liveness, §7 pasos 0 y 1)

## Global Constraints

- **Prohibido `cargo test`.** Es irrelevante como señal de correctitud en este repo. La validación es `tests/main.vn`.
- **Matriz de 4 obligatoria** al cerrar cada tarea: `run` × `VARN_NO_JIT` × procedencia de la std (árbol / `@embedded`). Purgar con `vn cache clean` al cambiar de procedencia.
- **Criterio de fallo del paso 0:** cualquier cambio de comportamiento significa que algo no estaba muerto. Parar e investigar, no adaptar.
- **Criterio de fallo del paso 1:** cualquier diff en el corpus de asignación de registros (`bytecode_identity.sh`). **No** es comparación byte a byte de `tests/main.vn`: la asignación de registros ya es no determinista en `ed0af33` — ver Task 0, paso 3.
- **Compilación:** `cargo build --release` debe terminar con **cero warnings nuevos**. El workspace tiene `unused_crate_dependencies = "warn"`.
- **Baseline:** `ed0af33`. El spec está en `9493cdd`.
- **Los números de línea de este plan describen el árbol en `ed0af33`.** En cuanto una tarea edita un fichero, los números de las tareas posteriores sobre ESE fichero quedan desplazados. Afecta a `crates/varn-types/src/generator.rs` (tareas 2 y 3) y a `crates/varn-vm/src/exec/ctx.rs` (tareas 2 y 4). **Localizar siempre por símbolo; usar el número sólo como pista.**
- Directorio de trabajo temporal, usado por todas las tareas:
  `C:\Users\x\AppData\Local\Temp\claude\c--Users-x-dev-varn\bda2e401-8480-4efd-a75b-a3cad75ca949\scratchpad`
  En los comandos aparece como `$SCRATCH`. **El shell del harness no conserva estado entre llamadas**, así que hay que exportarlo en cada llamada que lo use, y los scripts generados definen su propia ruta internamente:
  ```bash
  export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
  mkdir -p "$SCRATCH/baseline"
  ```

---

## File Structure

**Paso 0 — supresión.** Ningún fichero nuevo. Se tocan:

| Fichero | Responsabilidad tras el cambio |
|---|---|
| `crates/varn-vm/src/exec/mod.rs` | `VmSuspend` con dos variantes (`Yield`, `Await`) en vez de tres |
| `crates/varn-cli/src/bench/harness.rs` | Driver de bench sin el brazo muerto |
| `crates/varn-pipeline/src/execute.rs` | Driver de ejecución sin el brazo muerto |
| `crates/varn-vm/src/generator.rs` | Driver de generador sin el brazo muerto |
| `crates/varn-types/src/generator.rs` | Sólo `GeneratorDriver` y `GeneratorObj`; sin `GenChannel`, `AsyncQueue`, `AsyncQueueInner`, `make_iter_result` |
| `crates/varn-types/src/lib.rs` | Exports sin `GenChannel` ni `AsyncQueue` |
| `crates/varn-types/src/value/mod.rs` | `Value` sin la variante `AsyncQueue` |
| `crates/varn-types/src/value/traits.rs` | Cuatro impls sin el brazo `AsyncQueue` |
| `crates/varn-core/src/type_tag.rs` | `TypeTag` sin `AsyncQueue` |
| `crates/varn-vm/src/heap/obj.rs`, `structs.rs`, `intern.rs`, `access.rs`, `gc.rs` | Heap sin el objeto `AsyncQueue` |
| `crates/varn-vm/src/exec/ctx.rs` | `ExecCtx` sin `gen_channel` ni `deferred_tasks` |
| `crates/varn-types/src/native.rs`, `native_ctx.rs` | Trait del host sin `set_timer`/`clear_timer` |
| `crates/varn-vm/src/heap/native.rs`, `crates/varn-vm/src/exec/host/mod.rs`, `crates/varn-builtins/src/dispatch.rs` | Implementaciones correspondientes borradas |

**Paso 1 — liveness.**

| Fichero | Responsabilidad |
|---|---|
| `crates/varn-compiler/src/ssa/liveness.rs` (**nuevo**) | Único dueño del dataflow de liveness sobre valores SSA. Expone `analyze(&SsaFunc) -> Liveness` con `def`, `end`, `live_in`, `live_out`, `term_idx` y la consulta `live_across(idx)`. |
| `crates/varn-compiler/src/ssa/mod.rs` | Declara el módulo |
| `crates/varn-compiler/src/ssa/emit/regs.rs` | Pasa a **consumir** el análisis en vez de calcularlo. Conserva sólo la asignación de registros (linear scan con pools segregados por tipo). |

Este reparto es el que fija la decomposición: `regs.rs` hoy mezcla dos responsabilidades (analizar y asignar) en una función de ~200 líneas. Separarlas es correcto con independencia de este proyecto, y es lo que permite que el pase del plan siguiente consuma el mismo análisis en vez de duplicarlo.

---

## Task 0: Captura de referencia

Sin esta captura no hay forma de demostrar que las supresiones no cambiaron nada. **Ejecutar antes de tocar una sola línea.**

**Files:**
- Create: `$SCRATCH/baseline/*.txt` (fuera del repo, no se commitea)

**Interfaces:**
- Produces: cuatro ficheros de salida de referencia y un volcado de bytecode, consumidos como oráculo por todas las tareas siguientes.

- [ ] **Step 1: Confirmar el baseline**

```bash
cd /c/Users/x/dev/varn/varn-lang
git log --oneline -1        # esperado: 9493cdd docs(spec): ...
git status --porcelain      # esperado: vacío
cargo build --release 2>&1 | tail -5
```

- [ ] **Step 2: Capturar la matriz de 4**

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
mkdir -p "$SCRATCH/baseline"
cd /c/Users/x/dev/varn/varn-lang

./target/release/vn.exe cache clean
./target/release/vn.exe run ./tests/main.vn                > "$SCRATCH/baseline/run-tree.txt" 2>&1
VARN_NO_JIT=1 ./target/release/vn.exe run ./tests/main.vn  > "$SCRATCH/baseline/run-tree-nojit.txt" 2>&1

./target/release/vn.exe cache clean
VARN_STD=@embedded ./target/release/vn.exe run ./tests/main.vn               > "$SCRATCH/baseline/run-emb.txt" 2>&1
VARN_NO_JIT=1 VARN_STD=@embedded ./target/release/vn.exe run ./tests/main.vn > "$SCRATCH/baseline/run-emb-nojit.txt" 2>&1

wc -l "$SCRATCH"/baseline/*.txt
```

Esperado: los cuatro ficheros con contenido y sin fallos. Si alguno viene rojo **en el baseline**, parar: el árbol no está sano y ninguna comparación posterior significaría nada.

- [ ] **Step 3: Capturar el corpus de asignación de registros**

Oráculo del paso 1.

> **No se compara `tests/main.vn` byte a byte.** En `ed0af33` la asignación de registros **ya es no determinista** entre corridas: 5/5 corridas distintas sobre un fichero de 2 funciones, con `VARN_NO_JIT=1`, independiente del grafo de módulos. El diff es una permutación de registros físicos (`r6`↔`r7`) con los mismos opcodes. El orden de volcado de los bloques `MODULE BYTECODE` también varía. Es un defecto pre-existente, no algo que introduzca este plan.
>
> El oráculo es un corpus fijo de ficheros sueltos con los números de registro canonicalizados, verificado estable 5/5. El gate real de correctitud sigue siendo la matriz de 4, que sí es determinista.

Crear en `$SCRATCH/corpus/` seis ficheros que ejerciten los caminos de la liveness: `01-loops.vn` (bucles anidados, back-edges), `02-try.vn` (try/catch/finally, arista `InstKind::Try`), `03-branches.vn` (if/else con args de bloque), `04-floats.vn` (mezcla int/float, pools segregados), `05-presion.vn` (10 temporales vivos a la vez), `06-closures.vn` (`LoadCaptured`/`StoreCaptured`).

```bash
cd /c/Users/x/dev/varn/varn-lang
norm() { sed -e 's/\x1b\[[0-9;]*m//g' -e 's/\br[0-9]\+/rN/g' -e 's/\[[0-9]\+\]/[N]/g'; }
: > "$SCRATCH/baseline/corpus.txt"
for f in "$SCRATCH"/corpus/*.vn; do
  echo "### $(basename "$f")" >> "$SCRATCH/baseline/corpus.txt"
  ./target/release/vn.exe debug -p bytecode "$f" 2>&1 | norm >> "$SCRATCH/baseline/corpus.txt"
done
wc -l "$SCRATCH/baseline/corpus.txt"
```

Antes de aceptarlo como oráculo, **confirmar que es estable**: repetir la captura cinco veces y comprobar que las cinco salidas son idénticas. Si no lo son, el corpus toca algo no determinista adicional y hay que reducirlo hasta que lo sea.

- [ ] **Step 4: Guardar el recuento de líneas de partida**

El spec exige saldo de líneas fuertemente negativo (§6).

```bash
cd /c/Users/x/dev/varn/varn-lang
git ls-files 'crates/**/*.rs' | xargs wc -l | tail -1 > "$SCRATCH/baseline/loc.txt"
cat "$SCRATCH/baseline/loc.txt"
```

- [ ] **Step 5: Definir el script de verificación reutilizable**

Todas las tareas siguientes lo invocan. Crearlo una vez.

```bash
cat > "$SCRATCH/verify.sh" <<'EOF'
#!/usr/bin/env bash
# Matriz de 4 contra la referencia. Sale != 0 si algo difiere.
# Define su propia ruta: el shell del harness no conserva exports entre llamadas.
set -u
SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
cd /c/Users/x/dev/varn/varn-lang || exit 1
B="$SCRATCH/baseline"
O="$SCRATCH/current"
mkdir -p "$O"
fail=0

./target/release/vn.exe cache clean >/dev/null 2>&1
./target/release/vn.exe run ./tests/main.vn                > "$O/run-tree.txt" 2>&1
VARN_NO_JIT=1 ./target/release/vn.exe run ./tests/main.vn  > "$O/run-tree-nojit.txt" 2>&1
./target/release/vn.exe cache clean >/dev/null 2>&1
VARN_STD=@embedded ./target/release/vn.exe run ./tests/main.vn               > "$O/run-emb.txt" 2>&1
VARN_NO_JIT=1 VARN_STD=@embedded ./target/release/vn.exe run ./tests/main.vn > "$O/run-emb-nojit.txt" 2>&1

for f in run-tree run-tree-nojit run-emb run-emb-nojit; do
  if diff -q "$B/$f.txt" "$O/$f.txt" >/dev/null 2>&1; then
    echo "OK   $f"
  else
    echo "DIFF $f"
    diff "$B/$f.txt" "$O/$f.txt" | head -20
    fail=1
  fi
done
exit $fail
EOF
chmod +x "$SCRATCH/verify.sh"
"$SCRATCH/verify.sh"
```

Esperado: cuatro líneas `OK` y código de salida 0. Si el script no da verde contra el árbol sin modificar, el script está mal — arreglarlo antes de seguir.

- [ ] **Step 6: Commit del punto de partida**

No hay cambios en el repo que commitear; la captura vive fuera. Confirmar sólo que el árbol sigue limpio:

```bash
cd /c/Users/x/dev/varn/varn-lang && git status --porcelain
```

Esperado: salida vacía.

---

## Task 1: Borrar `VmSuspend::Task`

Variante con **cero productores** y tres consumidores que la ignoran con `=> {}`.

**Files:**
- Modify: `crates/varn-vm/src/exec/mod.rs:31`
- Modify: `crates/varn-cli/src/bench/harness.rs:142`
- Modify: `crates/varn-pipeline/src/execute.rs:152`
- Modify: `crates/varn-vm/src/generator.rs:120`

**Interfaces:**
- Consumes: la referencia de la Task 0.
- Produces: `enum VmSuspend { Yield { value: VmValue, dest_reg: u8 }, Await { value: varn_types::Value, dest_reg: u16 } }`. Las tareas siguientes asumen esas dos variantes.

- [ ] **Step 1: Demostrar que no hay productor**

```bash
cd /c/Users/x/dev/varn/varn-lang
grep -rn "VmSuspend::Task" --include=*.rs crates/
```

Esperado: exactamente 4 líneas — la definición en `exec/mod.rs` y los 3 consumidores. **Si aparece cualquier línea que asigne `VmSuspend::Task(...)` a algo, parar**: la premisa del borrado es falsa.

- [ ] **Step 2: Borrar la variante**

En `crates/varn-vm/src/exec/mod.rs`, borrar la línea 31:

```rust
    Task(varn_types::AsyncTask),
```

El enum queda:

```rust
pub enum VmSuspend {
    Yield {
        value: VmValue,
        dest_reg: u8,
    },
    Await {
        value: varn_types::Value,
        dest_reg: u16,
    },
}
```

- [ ] **Step 3: Ajustar los tres consumidores**

En `crates/varn-cli/src/bench/harness.rs:142`, borrar la línea entera:

```rust
                Some(varn_vm::exec::VmSuspend::Task(_task)) => {}
```

En `crates/varn-pipeline/src/execute.rs:152`, borrar la línea entera:

```rust
                Some(varn_vm::exec::VmSuspend::Task(_task)) => {}
```

En `crates/varn-vm/src/generator.rs:120`, el brazo cubre dos patrones. Dejar sólo el que queda:

```rust
                Some(VmSuspend::Await { .. }) => {
```

- [ ] **Step 4: Compilar**

```bash
cd /c/Users/x/dev/varn/varn-lang && cargo build --release 2>&1 | grep -E "^(error|warning)" | head -20
```

Esperado: sin salida. Si el compilador exige un brazo `_ =>` en algún `match`, **no añadirlo**: significa que el `match` dejó de ser exhaustivo por otra razón y hay que mirarla.

- [ ] **Step 5: Verificar comportamiento idéntico**

```bash
"$SCRATCH/verify.sh"
```

Esperado: cuatro `OK`, salida 0.

- [ ] **Step 6: Commit**

```bash
cd /c/Users/x/dev/varn/varn-lang
git add crates/varn-vm/src/exec/mod.rs crates/varn-cli/src/bench/harness.rs crates/varn-pipeline/src/execute.rs crates/varn-vm/src/generator.rs
git commit -m "refactor(vm): borrar VmSuspend::Task, variante sin productores"
```

---

## Task 2: Borrar `GenChannel` y `ExecCtx.gen_channel`

`GenChannel::new` tiene **cero llamadas**. `gen_channel` sólo se asigna `None`, en dos sitios.

**Files:**
- Modify: `crates/varn-types/src/generator.rs:20-45` (borrar `GenChannel` y su `impl`)
- Modify: `crates/varn-types/src/lib.rs:16` (export)
- Modify: `crates/varn-vm/src/exec/ctx.rs:19,43,136,304`

**Interfaces:**
- Produces: `ExecCtx` sin el campo `gen_channel`. La Task 4 toca el mismo struct.

- [ ] **Step 1: Demostrar que no hay constructor**

```bash
cd /c/Users/x/dev/varn/varn-lang
grep -rn "GenChannel::new" --include=*.rs crates/ ; echo "---llamadas arriba (esperado: ninguna)---"
grep -rn "gen_channel" --include=*.rs crates/
```

Esperado: primera búsqueda vacía. Segunda con 4 líneas, todas `None` o la declaración. **Si alguna asigna `Some(...)`, parar.**

- [ ] **Step 2: Borrar el struct**

En `crates/varn-types/src/generator.rs`, borrar el bloque completo de las líneas 20-45: el `#[derive(Debug)] pub struct GenChannel { ... }` y su `impl GenChannel { ... }`.

Comprobar tras borrar que `use std::sync::atomic::{AtomicBool, Ordering};` (línea 5) y `use crate::task::AsyncTask;` (línea 1) siguen usados por lo que queda en el fichero. Si no, borrarlos también — el workspace trata los `unused` como warning y el criterio es cero warnings nuevos.

- [ ] **Step 3: Quitar el export**

En `crates/varn-types/src/lib.rs:16`:

```rust
pub use generator::{AsyncQueue, GeneratorDriver, GeneratorObj};
```

(`AsyncQueue` sale en la Task 3; aquí sólo desaparece `GenChannel`.)

- [ ] **Step 4: Quitar el campo de `ExecCtx`**

En `crates/varn-vm/src/exec/ctx.rs`, borrar:
- línea 19: `use varn_types::generator::GenChannel;`
- línea 43: `pub gen_channel: Option<Rc<GenChannel>>,`
- línea 136: `gen_channel: None,` (constructor `new`)
- línea 304: `gen_channel: None,` (constructor `fork_for_task`)

- [ ] **Step 5: Compilar y verificar**

```bash
cd /c/Users/x/dev/varn/varn-lang && cargo build --release 2>&1 | grep -E "^(error|warning)" | head -20
"$SCRATCH/verify.sh"
```

Esperado: build sin salida, cuatro `OK`.

- [ ] **Step 6: Commit**

```bash
cd /c/Users/x/dev/varn/varn-lang
git add crates/varn-types/src/generator.rs crates/varn-types/src/lib.rs crates/varn-vm/src/exec/ctx.rs
git commit -m "refactor(types): borrar GenChannel, protocolo nunca instanciado"
```

---

## Task 3: Borrar `AsyncQueue`

La supresión más ancha: una variante de `Value` que **nunca se construye**, pero cableada en 11 sitios incluyendo un brazo de trazado del GC. Su `make_iter_result` usa `value_to_nv`, el bug ya documentado y corregido en `varn-vm/src/generator.rs:11-19` — nació roto.

**Files:**
- Modify: `crates/varn-types/src/generator.rs:64-117` (`AsyncQueueInner`, `AsyncQueue`, `make_iter_result`)
- Modify: `crates/varn-types/src/lib.rs:16`
- Modify: `crates/varn-types/src/value/mod.rs:13,133`
- Modify: `crates/varn-types/src/value/traits.rs:82,141,176,280`
- Modify: `crates/varn-core/src/type_tag.rs:34,66`
- Modify: `crates/varn-vm/src/heap/obj.rs:8,46,80`
- Modify: `crates/varn-vm/src/heap/structs.rs:95`
- Modify: `crates/varn-vm/src/heap/intern.rs:115,199`
- Modify: `crates/varn-vm/src/heap/access.rs:95`
- Modify: `crates/varn-vm/src/gc.rs:277`

**Interfaces:**
- Produces: `Value` y `HeapObj` sin la variante `AsyncQueue`; `TypeTag` sin `AsyncQueue`.

- [ ] **Step 1: Demostrar que no hay constructor**

```bash
cd /c/Users/x/dev/varn/varn-lang
grep -rn "AsyncQueue::new\|AsyncQueue(Rc::new\|\.push(\|\.close(" --include=*.rs crates/ | grep -i asyncqueue
echo "--- constructores arriba (esperado: ninguno fuera de generator.rs) ---"
grep -rn "AsyncQueue" --include=*.rs crates/ | wc -l
```

Esperado: ningún sitio construye un `AsyncQueue` fuera de su propia definición. Si alguno lo hace, **parar**.

- [ ] **Step 2: Borrar la definición**

En `crates/varn-types/src/generator.rs`, borrar por **símbolo, no por número de línea** — la Task 2 ya quitó `GenChannel` de este mismo fichero y desplazó todo lo que venía después. Borrar estos cuatro bloques completos:

- `pub struct AsyncQueueInner { ... }` (con su `#[derive(Debug)]`)
- `pub struct AsyncQueue(pub Rc<RefCell<AsyncQueueInner>>);` (con su `#[derive(Clone, Debug)]`)
- `impl Default for AsyncQueue { ... }` e `impl AsyncQueue { ... }`
- la función libre `fn make_iter_result(value: Value, done: bool) -> Value { ... }` al final del fichero — sólo la usaba `AsyncQueue`

Referencia: en `ed0af33` ocupaban las líneas 64-117.

El fichero debe quedar con `GeneratorDriver` y `GeneratorObj` y nada más. Revisar los `use` de cabecera: `AsyncTask`, `RefCell`, `AtomicBool`, `Ordering` probablemente dejan de usarse. Quitar los que sobren.

- [ ] **Step 3: Quitar de `Value`**

En `crates/varn-types/src/value/mod.rs`:
- línea 13: dejar `use crate::generator::GeneratorObj;`
- línea 133: borrar `AsyncQueue(AsyncQueue),`

En `crates/varn-types/src/value/traits.rs`, borrar los cuatro brazos:
- línea 82: `Value::AsyncQueue(_) => TypeTag::AsyncQueue,`
- línea 141: `Value::AsyncQueue(q) => Rc::as_ptr(&q.0).hash(state),`
- línea 176: `(Value::AsyncQueue(a), Value::AsyncQueue(b)) => Rc::ptr_eq(&a.0, &b.0),`
- línea 280: `Value::AsyncQueue(_) => write!(f, "[AsyncQueue]"),`

- [ ] **Step 4: Quitar de `TypeTag`**

En `crates/varn-core/src/type_tag.rs`, borrar la línea 34 (`AsyncQueue,`) y la 66 (`Self::AsyncQueue => "AsyncQueue",`).

- [ ] **Step 5: Quitar del heap y del GC**

- `crates/varn-vm/src/heap/obj.rs`: línea 8 (quitar `AsyncQueue` del `use`), línea 46 (`AsyncQueue(AsyncQueue),`), línea 80 (`HeapObj::AsyncQueue(_) => TypeTag::AsyncQueue,`)
- `crates/varn-vm/src/heap/structs.rs:95`: `HeapObj::AsyncQueue(q) => Some(Rc::as_ptr(&q.0) as usize),`
- `crates/varn-vm/src/heap/intern.rs`: líneas 115 y 199
- `crates/varn-vm/src/heap/access.rs:95`
- `crates/varn-vm/src/gc.rs:277`: el brazo `HeapObj::AsyncQueue(q) => { ... }` completo

- [ ] **Step 6: Comprobar que no queda rastro**

```bash
cd /c/Users/x/dev/varn/varn-lang
grep -rn "AsyncQueue" --include=*.rs crates/
```

Esperado: **cero líneas**. Éste es el criterio de la tarea.

- [ ] **Step 7: Compilar y verificar**

```bash
cd /c/Users/x/dev/varn/varn-lang && cargo build --release 2>&1 | grep -E "^(error|warning)" | head -20
"$SCRATCH/verify.sh"
```

Esperado: build sin salida, cuatro `OK`. Un cambio de comportamiento aquí sería sorprendente y debe investigarse, no absorberse.

- [ ] **Step 8: Commit**

```bash
cd /c/Users/x/dev/varn/varn-lang
git add -A crates/
git commit -m "refactor(types,vm): borrar AsyncQueue, variante de Value sin constructor

Estaba cableada en Value, TypeTag, HeapObj, Hash, PartialEq, Display,
gc.rs, intern.rs, access.rs y structs.rs sin que ningún camino la
construyera. Su make_iter_result usaba value_to_nv, el bug documentado
en varn-vm/src/generator.rs:11-19: nacio rota."
```

---

## Task 4: Borrar `deferred_tasks`

Campo de `ExecCtx` que nunca se lee ni se escribe: sólo se inicializa a `FxHashMap::default()` en los dos constructores.

**Files:**
- Modify: `crates/varn-vm/src/exec/ctx.rs:44,137,305`

**Interfaces:**
- Consumes: `ExecCtx` tal como lo dejó la Task 2.
- Produces: `ExecCtx` sin `deferred_tasks`.

- [ ] **Step 1: Demostrar que está muerto**

```bash
cd /c/Users/x/dev/varn/varn-lang
grep -rn "deferred_tasks" --include=*.rs crates/
```

Esperado: exactamente 3 líneas (declaración + dos inicializaciones). **Si alguna lo lee o inserta, parar.**

- [ ] **Step 2: Borrar el campo**

En `crates/varn-vm/src/exec/ctx.rs`, borrar por **símbolo, no por número de línea** — la Task 2 ya quitó `gen_channel` de este mismo fichero y desplazó lo que venía después:

- la declaración del campo: `pub deferred_tasks: FxHashMap<usize, Rc<LazyTask>>,`
- sus **dos** inicializaciones `deferred_tasks: FxHashMap::default(),` — una en el constructor `new`, otra en `fork_for_task`

Referencia: en `ed0af33` eran las líneas 44, 137 y 305. Confirmar con `grep -n deferred_tasks crates/varn-vm/src/exec/ctx.rs` que salen exactamente tres.

Comprobar si `LazyTask` sigue usado en el fichero tras el borrado; si no, quitar su `use`.

- [ ] **Step 3: Compilar y verificar**

```bash
cd /c/Users/x/dev/varn/varn-lang && cargo build --release 2>&1 | grep -E "^(error|warning)" | head -20
"$SCRATCH/verify.sh"
```

- [ ] **Step 4: Commit**

```bash
cd /c/Users/x/dev/varn/varn-lang
git add crates/varn-vm/src/exec/ctx.rs
git commit -m "refactor(vm): borrar ExecCtx.deferred_tasks, nunca leido ni escrito"
```

---

## Task 5: Borrar `set_timer` y `clear_timer`

Siete implementaciones del trait del host, **cero llamadas** desde el lenguaje. `setTimeout` no existe en la stdlib. Vuelven en el paso 6 del spec con implementación real sobre la cola de timers del scheduler.

**Files:**
- Modify: `crates/varn-types/src/native_ctx.rs:130` (declaración en el trait)
- Modify: `crates/varn-types/src/native.rs:214,304`
- Modify: `crates/varn-vm/src/heap/native.rs:191`
- Modify: `crates/varn-vm/src/exec/host/mod.rs:336,346`
- Modify: `crates/varn-builtins/src/dispatch.rs:412`

**Interfaces:**
- Produces: el trait `NativeCtx` sin `set_timer` ni `clear_timer`. `suspend_timer` **se queda**: sí tiene llamante (`runtime:task::sleep`) y lo sustituye el paso 3 del spec, no éste.

- [ ] **Step 1: Demostrar que no hay llamante**

```bash
cd /c/Users/x/dev/varn/varn-lang
grep -rn "set_timer\|clear_timer" --include=*.rs crates/
grep -rn "setTimeout\|setInterval\|clearTimeout" --include=*.vn std/ crates/
```

Esperado: la primera lista sólo declaraciones e implementaciones, ninguna invocación. La segunda, **vacía**. Si `std/` expone algo que llegue a `set_timer`, parar.

- [ ] **Step 2: Borrar del trait y de las implementaciones**

Borrar la declaración en `crates/varn-types/src/native_ctx.rs:130` y las implementaciones en los cinco ficheros listados arriba. Cada una es un método completo, incluyendo el cuerpo `Err("...".into())` o `Ok(())`.

**No tocar `suspend_timer`** en `crates/varn-vm/src/exec/host/mod.rs:355`. Es otro método, tiene llamante y muere en otro paso.

- [ ] **Step 3: Compilar y verificar**

```bash
cd /c/Users/x/dev/varn/varn-lang && cargo build --release 2>&1 | grep -E "^(error|warning)" | head -20
"$SCRATCH/verify.sh"
```

- [ ] **Step 4: Medir el saldo de líneas del paso 0**

El spec (§6) exige saldo fuertemente negativo.

```bash
cd /c/Users/x/dev/varn/varn-lang
git ls-files 'crates/**/*.rs' | xargs wc -l | tail -1
echo "--- partida ---"; cat "$SCRATCH/baseline/loc.txt"
git diff --shortstat ed0af33..HEAD -- crates/
```

Esperado: inserciones muy por debajo de las supresiones. Si el saldo es positivo, algo se implementó donde tocaba borrar.

- [ ] **Step 5: Commit**

```bash
cd /c/Users/x/dev/varn/varn-lang
git add -A crates/
git commit -m "refactor(host): borrar set_timer/clear_timer, 7 impls sin llamantes

Vuelven en el paso 6 del spec sobre la cola de timers del scheduler.
suspend_timer se mantiene: tiene llamante (runtime:task::sleep) y lo
sustituye el paso 3."
```

---

## Task 6: Extraer la liveness SSA a módulo propio

Fin del paso 0, comienzo del paso 1. Refactor puro con el criterio más duro del plan: **la asignación de registros del corpus no puede cambiar**.

**Files:**
- Create: `crates/varn-compiler/src/ssa/liveness.rs`
- Modify: `crates/varn-compiler/src/ssa/mod.rs` (declarar el módulo)
- Modify: `crates/varn-compiler/src/ssa/emit/regs.rs:29-134` (consumir en vez de calcular)

**Interfaces:**
- Consumes: `SsaFunc`, `InstKind`, `Terminator`, `Value` de `crate::ssa::ir`; `crate::ssa::verify::inst_uses`.
- Produces:
  ```rust
  pub struct Liveness {
      pub def: Vec<u32>,                      // valor -> punto de definición (u32::MAX si nunca)
      pub end: Vec<u32>,                      // valor -> último punto donde sigue vivo
      pub live_in: Vec<FxHashSet<u32>>,       // por bloque
      pub live_out: Vec<FxHashSet<u32>>,      // por bloque
      pub term_idx: Vec<u32>,                 // bloque -> punto de su terminador
  }
  impl Liveness {
      pub fn analyze(ssa: &SsaFunc) -> Liveness;
      pub fn live_across(&self, idx: u32) -> Vec<u32>;
  }
  ```
  El plan siguiente (pase de máquinas de estados) consume `live_across` para elegir los campos del objeto de estado.

- [ ] **Step 1: Comprobar que el oráculo sigue verde antes de tocar el compilador**

El script `bytecode_identity.sh` y su corpus los creó la Task 0. Aquí sólo se ejecuta.

```bash
export SCRATCH="/c/Users/x/AppData/Local/Temp/claude/c--Users-x-dev-varn/bda2e401-8480-4efd-a75b-a3cad75ca949/scratchpad"
"$SCRATCH/bytecode_identity.sh"
```

Esperado: `OK corpus de registros identico`, exit 0. Aún no se ha tocado el compilador. Si da `DIFF`, alguna de las tareas 1-5 cambió la asignación de registros y hay que investigarlo **antes** de empezar la extracción — de lo contrario el diff de esta tarea mezclaría dos causas.

- [ ] **Step 2: Crear el módulo con el análisis extraído**

Crear `crates/varn-compiler/src/ssa/liveness.rs`. El cuerpo de `analyze` es exactamente el código que hoy ocupa `regs.rs:29-134`, movido sin cambios semánticos:

```rust
//! Liveness sobre valores SSA: único dueño del dataflow.
//!
//! Dos consumidores: la asignación de registros (`emit::regs`) y el pase de
//! máquinas de estados. Tenerlo dos veces es como se separan.
//!
//! `end` es un intervalo LINEAL, no liveness exacta por punto: para un valor
//! definido dentro de un bucle y usado en la cabecera, el intervalo abarca
//! todo el bucle. Es una sobre-aproximación conservadora — nunca declara
//! muerto algo vivo — así que es correcta para asignar registros y correcta
//! para elegir campos de estado, a costa de guardar de más en bucles.

use super::ir::{InstKind, SsaFunc, Terminator, Value};
use rustc_hash::FxHashSet;

pub struct Liveness {
    pub def: Vec<u32>,
    pub end: Vec<u32>,
    pub live_in: Vec<FxHashSet<u32>>,
    pub live_out: Vec<FxHashSet<u32>>,
    pub term_idx: Vec<u32>,
}

impl Liveness {
    pub fn analyze(ssa: &SsaFunc) -> Liveness {
        let nvals = ssa.values.len();
        let nblocks = ssa.blocks.len();
        let mut def = vec![u32::MAX; nvals];
        let mut last = vec![0u32; nvals];
        let mut term_idx = vec![0u32; nblocks];
        let mut defs: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut uses: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut succ: Vec<Vec<usize>> = vec![Vec::new(); nblocks];
        let mut idx = 0u32;
        for (b, block) in ssa.blocks.iter().enumerate() {
            let mut local_defined: FxHashSet<u32> = FxHashSet::default();
            for p in &block.params {
                if def[p.0 as usize] == u32::MAX {
                    def[p.0 as usize] = idx;
                }
                defs[b].insert(p.0);
                local_defined.insert(p.0);
            }
            idx += 1;
            for inst in &block.insts {
                for u in crate::ssa::verify::inst_uses(&inst.kind) {
                    if last[u.0 as usize] < idx {
                        last[u.0 as usize] = idx;
                    }
                    if !local_defined.contains(&u.0) {
                        uses[b].insert(u.0);
                    }
                }
                if let Some(d) = inst.dest {
                    if def[d.0 as usize] == u32::MAX {
                        def[d.0 as usize] = idx;
                    }
                    defs[b].insert(d.0);
                    local_defined.insert(d.0);
                }
                if let InstKind::Try { handler } = &inst.kind {
                    succ[b].push(handler.0 as usize);
                }
                idx += 1;
            }
            let mut touch = |v: Value, uses: &mut FxHashSet<u32>| {
                if last[v.0 as usize] < idx {
                    last[v.0 as usize] = idx;
                }
                if !local_defined.contains(&v.0) {
                    uses.insert(v.0);
                }
            };
            match &block.term {
                Terminator::Return(Some(v)) | Terminator::Throw(v) => touch(*v, &mut uses[b]),
                Terminator::Branch {
                    cond,
                    then_blk,
                    then_args,
                    else_blk,
                    else_args,
                } => {
                    touch(*cond, &mut uses[b]);
                    then_args
                        .iter()
                        .chain(else_args)
                        .for_each(|a| touch(*a, &mut uses[b]));
                    succ[b].push(then_blk.0 as usize);
                    succ[b].push(else_blk.0 as usize);
                }
                Terminator::Jump { target, args } => {
                    args.iter().for_each(|a| touch(*a, &mut uses[b]));
                    succ[b].push(target.0 as usize);
                }
                Terminator::Return(None) | Terminator::Unreachable => {}
            }
            term_idx[b] = idx;
            idx += 1;
        }

        let mut live_in: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut live_out: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); nblocks];
        let mut changed = true;
        while changed {
            changed = false;
            for b in (0..nblocks).rev() {
                let mut out = FxHashSet::default();
                for &s in &succ[b] {
                    out.extend(live_in[s].iter().copied());
                }
                let mut nin = uses[b].clone();
                nin.extend(out.iter().copied().filter(|v| !defs[b].contains(v)));
                if out != live_out[b] || nin != live_in[b] {
                    live_out[b] = out;
                    live_in[b] = nin;
                    changed = true;
                }
            }
        }

        let mut end = last;
        for b in 0..nblocks {
            for &v in &live_out[b] {
                if end[v as usize] < term_idx[b] {
                    end[v as usize] = term_idx[b];
                }
            }
        }
        for v in 0..nvals {
            if def[v] != u32::MAX && end[v] < def[v] {
                end[v] = def[v];
            }
        }

        Liveness {
            def,
            end,
            live_in,
            live_out,
            term_idx,
        }
    }

    /// Valores vivos cruzando el punto `idx`: definidos antes y todavía vivos
    /// después. Es la consulta que necesita el pase de máquinas de estados
    /// para decidir qué guarda el objeto de estado.
    pub fn live_across(&self, idx: u32) -> Vec<u32> {
        (0..self.def.len() as u32)
            .filter(|&v| {
                let d = self.def[v as usize];
                d != u32::MAX && d < idx && self.end[v as usize] > idx
            })
            .collect()
    }
}
```

- [ ] **Step 3: Declarar el módulo**

En `crates/varn-compiler/src/ssa/mod.rs`, añadir junto a las demás declaraciones de módulo:

```rust
pub mod liveness;
```

- [ ] **Step 4: Hacer que `regs.rs` consuma el análisis**

En `crates/varn-compiler/src/ssa/emit/regs.rs`, los límites exactos importan:

- **Línea 27, `let nvals = ssa.values.len();` — SE QUEDA.** La línea 145 (`let mut order: Vec<usize> = (0..nvals)`) sigue usándola. Borrarla rompe la compilación.
- **Líneas 28-134 — se sustituyen.** `nblocks` (línea 28) sólo lo usaba el análisis y se va con él.

Sustituir las líneas 28-134 por:

```rust
    let lv = crate::ssa::liveness::Liveness::analyze(ssa);
    let def = lv.def;
    let end = lv.end;
```

Sin `mut`: verificado que el código posterior sólo lee `def[v]` (líneas 146, 148, 168) y `end[v]` (línea 203), nunca los muta.

Todo lo que sigue (desde `let mut base = 1 + nparams as u32;`, línea 136) queda **sin tocar**: sigue leyendo `def`, `end` y `nvals` con los mismos nombres.

Ajustar los `use` de cabecera: `Terminator` y `Value` probablemente dejan de usarse en `regs.rs`, y `FxHashSet` puede seguir haciendo falta más abajo. Quitar sólo los que el compilador marque.

- [ ] **Step 5: Compilar**

```bash
cd /c/Users/x/dev/varn/varn-lang && cargo build --release 2>&1 | grep -E "^(error|warning)" | head -20
```

Esperado: sin salida.

- [ ] **Step 6: Verificar el corpus de registros — el criterio de esta tarea**

```bash
"$SCRATCH/bytecode_identity.sh"
```

Esperado: `OK corpus de registros identico`, exit 0. **Cualquier diff tumba la tarea**: significa que la extracción cambió el análisis. Si aparece, sospechar de un cambio accidental en la recolección de `succ`, en el orden de inserción de `defs`/`uses`, o en la propagación de `end` desde `live_out`.

El corpus está canonicalizado por registro, así que **no** salta ante la permutación `r6`↔`r7` que el árbol ya produce de por sí. Lo que sí detecta: un cambio en la secuencia de opcodes, un `Move` de más o de menos, o un cambio en el número de registros que necesita una función — que es exactamente lo que rompería una liveness mal extraída.

- [ ] **Step 7: Verificar comportamiento**

```bash
"$SCRATCH/verify.sh"
```

Esperado: cuatro `OK`.

- [ ] **Step 8: Comprobar `live_across` sobre un caso conocido**

Prueba de humo del método nuevo, que aún no tiene consumidor. Confirma que el análisis se puede consultar por punto.

```bash
cd /c/Users/x/dev/varn/varn-lang
cat > "$SCRATCH/live.vn" <<'EOF'
async function f(x: int): int {
    const antes = x * 10;
    const r = await g(x);
    return r + antes;
}
async function g(x: int): int { return x * 2; }
print(await f(3));
EOF
./target/release/vn.exe debug -p ssa "$SCRATCH/live.vn" 2>&1 | sed -n '/fn f:/,/^fn /p'
```

Esperado: el volcado muestra `antes` definido antes del `await` y usado después. Ése es exactamente el valor que `live_across` debe devolver en el punto del `await`, y el que el objeto de estado tendrá que guardar en el plan siguiente. Anotar el número de valor SSA que sale: es el caso de prueba de la primera tarea del plan 2.

- [ ] **Step 9: Commit**

```bash
cd /c/Users/x/dev/varn/varn-lang
git add crates/varn-compiler/src/ssa/liveness.rs crates/varn-compiler/src/ssa/mod.rs crates/varn-compiler/src/ssa/emit/regs.rs
git commit -m "refactor(compiler): extraer liveness SSA a modulo propio

assign_registers mezclaba analizar y asignar en una funcion de ~200
lineas. El dataflow pasa a ssa/liveness.rs con dos consumidores: la
asignacion de registros y el pase de maquinas de estados del plan
siguiente. Asignacion de registros sin cambios en el corpus.

end es un intervalo lineal, no liveness exacta por punto: sobre-
aproximacion conservadora, correcta para ambos consumidores, con coste
de guardar de mas en bucles."
```

---

## Cierre del plan

- [ ] **Verificación final de la matriz de 4 con `bench`**

`verify.sh` cubre `run`. El spec exige también `bench`.

```bash
cd /c/Users/x/dev/varn/varn-lang
./target/release/vn.exe cache clean
./target/release/vn.exe bench ./tests/main.vn -v 2>&1 | tail -30
VARN_NO_JIT=1 ./target/release/vn.exe bench ./tests/main.vn -v 2>&1 | tail -10
```

Esperado: verde en ambos, sin regresión frente a los números previos.

- [ ] **Saldo de líneas del plan completo**

```bash
cd /c/Users/x/dev/varn/varn-lang
git diff --shortstat 9493cdd..HEAD -- crates/
```

Esperado: supresiones muy por encima de inserciones. Es el criterio de §6 del spec.

---

## Fuera de alcance de este plan

- El pase de máquinas de estados (paso 2 del spec). Es el plan siguiente y **se escribirá después de que este plan aterrice**, cuando la API real de `Liveness` esté en el árbol y no sea una previsión.
- El scheduler y el I/O por readiness (paso 3).
- La concurrencia estructurada (paso 4).
- `Sendable(T)` y el reparto (paso 5).
- `fork_for_task`, `run_lazy_task_sync`, `NanGenDriver`, `wait_task_handle_value`, el `unsafe impl Send` y los `thread::spawn` de `net.rs` siguen **intactos** al terminar este plan. Los borran los pasos 2 y 3.
