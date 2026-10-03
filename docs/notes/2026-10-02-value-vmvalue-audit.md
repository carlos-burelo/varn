# Auditoría de `Value` y `VmValue`

Fecha: 2026-10-02. Auditoría original; resuelta por ADR-0021 (`Value` eliminado, los tres fallos de corrupción corregidos y cubiertos por `tests/157-task-value-handles.vn`).

## Resumen
`Value` se presenta como el valor portable, pero no lo es: una parte de sus
variantes guarda handles del heap de la VM. No existe una regla de
propiedad ni de enraizado para esa parte, y el GC la compensa con casos
especiales por contenedor. El resultado medido son **dos fallos de
corrupción silenciosa** (abajo) y una capa de conversión que cuesta en cada
tarea.

## Las representaciones que conviven
| Representación | Tamaño | Dónde vive |
| :--- | :--- | :--- |
| `VmValue` | 16 B, `Copy` (tag + payload), handle al heap | registros, pilas, arrays, campos |
| `Value` | 24 B, 25 variantes, `Clone` con `Rc`/`Arc`/`Box` | nativos, tareas, closures portables, estado de `AsyncTask` |
| `HeapObj` | slot de 48 B, 26 variantes | tabla del heap |
| `SendValue` | copia profunda | frontera entre isolates |

Cada tipo de valor existe en tres o cuatro formas y se traduce con
`intern`/`extract` (`heap/intern.rs`, `heap/access.rs`, `object.rs`,
`sendable.rs`, `marshal.rs`, `host_values.rs`: ~1700 líneas entre los seis,
incluido `ObjData`).
Puntos de llamada: 79 `extract` en `varn-vm` y 16 en `varn-builtins`; 77 y 37
`intern`; 331, 229 y 110 referencias a `Value::` en `varn-vm`, `varn-types` y
`varn-builtins`.

## Por qué `Value` no es portable
Tres caminos meten handles del heap dentro de un `Value`:
1. `VmValueRef(VmValue)`: `extract` de una instancia devuelve
   `Value::VmValue(Box::new(VmValueRef(nv)))`, es decir el índice crudo del
   heap en una caja (`heap/intern.rs:188`, `object.rs:518`).
2. `ObjRef`, `MapRef`, `SetRef` guardan `Cell<VmValue>` / `MapKey(VmValue)`
   por dentro: un `Value::Object` solo tiene sentido en el heap que lo creó.
3. `Rc<ClassObj>` y los closures por `VmClosurePayload`.

Un `Value` que contiene uno de esos caminos y sobrevive a un GC apunta a un
slot que el GC movió o reutilizó.

## Fallos confirmados
El GC menor tiene un arreglo explícito por contenedor para reescribir esos
índices (`ChildSlot`: `BoundMethodReceiver`, `ClassVtableItem`, `ClassStatic`,
`EnumVariantPayload`, `Upvalue`, `Spread`, `ModuleExport`), pero ninguno para
`LazyTask` ni para el estado resuelto de un `AsyncTask`.

**F1. Instancias como argumentos de una tarea diferida.** Con un GC entre la
llamada async y su arranque, la tarea lee otro objeto.

**F2. Resultado de tarea resuelto antes de un GC y consumido después.** El
marcado mantiene vivo el objeto pero no actualiza el índice guardado.

Ambos dan `784431000` en lugar de `1999000` / `5997000`, sin error, en JIT y en
intérprete. No fallan: `parallel` con instancias como resultado, y una
instancia local a una tarea a través de un `await` (viven en la pila congelada,
que sí es raíz).

Repro F1 y F2 (clase con un `int`, un `churn` que asigna 400 000 objetos que
escapan a un global para forzar GC menores):

```vn
import { parallel, sleep, spawn, Task } from "std:task";
class SB { v: int; constructor(v: int) { this.v = v; } }
let sink: SB = new SB(0);
function churn(n: int): int { for (let i = 0; i < n; i = i + 1) { sink = new SB(i); } return sink.v; }

async function useArg(b: SB): Task<int> { await sleep(5); return b.v; }
let batchA: Array<Task<int>> = [];
for (let i = 0; i < 2000; i = i + 1) { batchA.push(useArg(new SB(i))); }
churn(400000);
let ra = await parallel(batchA);          // F1: suma esperada 1999000

async function makeRes(i: int): Task<SB> { return new SB(i * 3); }
let hs = [];
for (let i = 0; i < 2000; i = i + 1) { hs.push(spawn(makeRes(i))); }
await sleep(20);
churn(400000);
let s = 0;
for (let i = 0; i < hs.length; i = i + 1) { const r = await hs[i]; s = s + r.v; }
                                           // F2: suma esperada 5997000
```

## Coste
Un argumento instancia de una tarea paga `extract` (una `Box<dyn>` por
argumento), un `Rc<LazyTask>` y un `intern` al montar. `LazyTask` guardaba
además `resolved_constants` y un `Closure` portable, ya eliminados. Cada
nativo que devuelve `Vec<Value>` reconstruye arrays por la frontera. En las
llamadas ordinarias a nativos con arrays no se copia (medido: 0–1 ms por 2000
llamadas sobre 1M elementos), así que el coste caliente está en tareas e
isolates, no en el camino general.

## Qué obliga a mantener el diseño actual
- `AsyncTask` cruza hilos (el driver de I/O y los isolates liquidan tareas),
  y por eso su estado es un `Value` y no un `VmValue`.
- Los nativos del `NativeCtx` ya reciben `VmValue` en buena parte
  (`VnArray(VmValue)`); migrar el resto es incremental.

## Opciones
- **A. Parches por contenedor.** Reescribir `LazyTask.args` y el estado
  resuelto en el GC. Corrige F1 y F2 pero repite el patrón: el próximo
  contenedor con un `Value` y un handle vuelve a fallar.
- **B. Tabla de raíces externas.** `VmValueRef` deja de ser un índice y pasa a
  ser una referencia a una tabla gestionada por el heap, que el GC actualiza
  (como los handles persistentes de V8/JNI). Cualquier contenedor portable
  puede guardar una instancia sin que el GC lo conozca, y se pueden borrar
  los arreglos `ChildSlot` por contenedor. Coste: una entrada de tabla por
  `extract` de instancia, y una liberación en `Drop`.
- **C. Sacar `Value` del camino de la VM.** Tareas, closures y constantes en
  `VmValue`; `Value` queda para el borde con nativos externos y `SendValue`
  para isolates. Es la corrección de fondo, y la más grande: toca los 521
  usos de `HeapObj::` y los nativos.

Recomendación: B para cerrar F1/F2 y la clase entera de fallos con un cambio
acotado, y C después, por piezas (primero `LazyTask` y el estado de tareas,
que además eliminan conversiones).
