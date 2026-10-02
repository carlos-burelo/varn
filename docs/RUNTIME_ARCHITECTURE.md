# Arquitectura de Concurrencia: Tareas, Máquinas de Estados e Isolates

Este documento especifica cómo Varn ejecuta `async`/`await`, generadores e Isolates, así como el análisis comparativo de rendimiento frente a otros runtimes de la industria.

---

## Tabla de Contenidos

- [1. Modelo de Ejecución Asíncrono](#1-modelo-de-ejecución-asíncrono)
- [2. Máquinas de Estados SSA y Layout de Estado](#2-máquinas-de-estados-ssa-y-layout-de-estado)
- [3. Mecánica de Suspensión y Reanudación (`async`/`await`)](#3-mecánica-de-suspensión-y-reanudación-asyncawait)
- [4. Generadores y Rendimiento (`yield`)](#4-generadores-y-rendimiento-yield)
- [5. Primitivas de Concurrencia](#5-primitivas-de-concurrencia)
  - [`spawn`, `sleep` y `parallel`](#spawn-sleep-y-parallel)
  - [`TaskGroup` y Concurrencia Estructurada (`using`)](#taskgroup-y-concurrencia-estructurada-using)
- [6. Concurrencia Multinúcleo mediante Isolates](#6-concurrencia-multinúcleo-mediante-isolates)
  - [Aislamiento de Heap](#aislamiento-de-heap)
  - [Canales Tipados (`SendValue` & `SendEnvelope`)](#canales-tipados-sendvalue--sendenvelope)
- [7. Análisis Comparativo de Concurrencia (Varn vs Node.js vs Bun)](#7-análisis-comparativo-de-concurrencia-varn-vs-nodejs-vs-bun)
  - [7.1 Tabla Comparativa de Concurrencia](#71-tabla-comparativa-de-concurrencia)
  - [7.2 Huella de Memoria por Tarea](#72-huella-de-memoria-por-tarea)
  - [7.3 Escalado y Límites Empíricos](#73-escalado-y-límites-empíricos)

---

## 1. Modelo de Ejecución Asíncrono

Varn utiliza un modelo asíncrono basado en **máquinas de estados de coste cero** compiladas directamente en SSA sobre objetos continuos `ObjData`, complementado con primitivas de I/O y temporizadores no bloqueantes y paralelismo multinúcleo mediante Isolates.

```mermaid
flowchart TD
    subgraph Isolate Execution ["Hilo de Ejecución del Isolate"]
        A["Función async / Generador"] --> B["Transformación a Máquina de Estados (SSA)"]
        B --> C["Partición CFG + Layout Determinista (state_size)"]
        C --> D["Bytecode con Continuaciones RPO"]
        D --> E["Ejecución en VM / JIT Cranelift"]
        E -- "Await / Yield" --> F["Suspensión reactiva sobre AsyncTask"]
        F -- "on_settle / event loop" --> G["Reanudación inmediata en dest_reg"]
    end

    subgraph Multi Thread Isolates ["Paralelismo Multinúcleo real"]
        H["Isolate Principal (Hilo 1)"] <-->|"varn_runtime::channel (Sender/Receiver)"| I["Isolate Worker (Hilo 2)"]
        I <-->|"SendEnvelope serializado"| J["Isolate Worker (Hilo N)"]
    end
```

---

## 2. Máquinas de Estados SSA y Layout de Estado

El compilador (`crates/varn-compiler/src/passes/state_machine`) transforma toda función `async` y generador (`function*`, `async function*`) en una máquina de estados:

1. **Análisis Formal de Liveness**: `Liveness::analyze` computa con precisión el conjunto de variables vivas que cruzan cada punto de suspensión (`live_after`).
2. **Layout de Slots Determinista**: `StateLayout::compute` asigna slots continuos en un `ObjData` para almacenar el discriminante (`state[0]`) y las variables vivas, reutilizando slots entre suspensiones disjuntas.
3. **Partición Universal del CFG**:
   - Divide los bloques en cada `InstKind::Await` y `InstKind::Yield`.
   - Soporta control de flujo lineal, bucles (`while`, `for`) y bloques protegidos (`try / catch / finally`).
   - `reorder_blocks_rpo` y `compute_preds` mantienen los bloques en orden topológico/RPO estricto para que la asignación lineal de registros preserve intactos los rangos de vida sin colisiones de registros físicos.

---

## 3. Mecánica de Suspensión y Reanudación (`async`/`await`)

### Tipado en el Checker
Declarar una función `async` transforma automáticamente su tipo de retorno en `Task<T>` en `varn-checker`.

### Suspensión y Liquidación
- Al alcanzar un `OpCode::Await`:
  - Si el `TaskHandle` ya está resuelto (`TaskState::Resolved`), escribe el valor inmediatamente en `dest_reg` (**camino rápido de coste cero**).
  - Si el `TaskHandle` está `Pending`, la tarea suspende guardando sus registros en el `state_obj` y registrando la continuación reactiva en `AsyncTask::on_settle`.
- Al resolver la tarea esperada, el callback reactivo despierta la continuación y reanuda la ejecución en el bloque correspondiente.

---

## 4. Generadores y Rendimiento (`yield`)

- Las funciones generadoras (`function*`) y generadores asíncronos (`async function*`) se compilan bajo el mismo pipeline de máquinas de estados.
- Al emitir `yield`, la función suspende dejando `state[0] = STATE_YIELDED` y retorna el valor producido.
- La siguiente llamada a `.next()` reanuda directamente en la continuación sin necesidad de clonar contextos de ejecución pesados.

---

## 5. Primitivas de Concurrencia

### `spawn`, `sleep` y `parallel`
- `sleep(ms)`: Registra un temporizador no bloqueante en el runtime y retorna un `TaskHandle<void>` pendiente que se resuelve asíncronamente al expirar el tiempo.
- `parallel([t1, t2, t3])`: Agrega múltiples tareas asíncronas y las ejecuta en paralelo. Por ejemplo, `parallel([sleep(100), sleep(100), sleep(100)])` se completa en **~100 ms** (aceleración concurrente 3x).
- `spawn(fn, ...args)`: Lanza una tarea en el runtime retornando inmediatamente su `TaskHandle`.

### `TaskGroup` y Concurrencia Estructurada (`using`)
`TaskGroup` implementa el protocolo de concurrencia estructurada:
- Toda tarea lanzada con `group.spawn(...)` queda acotada al ciclo de vida del grupo.
- **Cancelación**: `cancel()` marca el grupo y cancela los joins en vuelo; las
  tareas parqueadas cuyo output ya resolvió se descartan sin drivear. Las
  hijas ya en ejecución terminan (su resultado se descarta), no se preemptan.
- **Timeouts**: `join(ms)` rechaza con `Error("TaskGroup.join timed out")` si expira.
- **Gestión Determinista de Ámbito (`using`)**: Al salir del bloque `using`, el compilador garantiza que se invoca `group.dispose()`, cancelando cualquier tarea huérfana.

### Cesión cooperativa (`Task.yield`)
El scheduler es cooperativo sin preempción: una tarea CPU-bound no cede
sola. `await Task.yield()` aparca la tarea al final de la cola lista y
sigue con las demás (intercalado determinista `a1,b1,a2,b2,a3` medido).
Para paralelismo real de CPU, usar Isolates.

### Garantías de orden (qué es determinista y qué no)
- Determinista: cola lista FIFO, timers por `(deadline, seq)`, canales
  `VecDeque` FIFO, wake O(1) por handle.
- No determinista: carreras timer-vs-I/O (wall-clock entre hilos) y
  consolidación de `parallel` con rechazos simultáneos (gana el primero
  que liquida).

```typescript
import { sleep, TaskGroup } from "std:task"

async function procesarParalelo(): Task<int> {
    using group = new TaskGroup<int>();
    
    group.spawn(async () => {
        await sleep(50);
        return 10;
    });
    group.spawn(async () => {
        await sleep(50);
        return 20;
    });
    
    let resultados = await group.join();
    return resultados[0] + resultados[1];
}
```

---

## 6. Concurrencia Multinúcleo mediante Isolates

Para paralelismo real en múltiples núcleos de CPU sin contención de memoria compartida, Varn implementa **Isolates**:

### Aislamiento de Heap
Cada Isolate se ejecuta en su propio hilo del sistema operativo, con su propia instancia de la máquina virtual (`varn-vm`) y su propio Heap independiente con GC generacional dedicado.

### Canales Tipados (`SendValue` & `SendEnvelope`)
La comunicación inter-Isolate se realiza mediante paso de mensajes tipados a través de canales no bloqueantes:

1. **Inferencia Estructural**: Los valores enviados se validan estructuralmente.
2. **Serialización Ligera**: Se convierten a `SendValue` y se encapsulan en `SendEnvelope`.
3. **Transferencia Segura**: Se transmiten por canales lock-free (`Sender<T>` / `Receiver<T>`) y se reconstruyen localmente en el heap del Isolate receptor.

---

## 7. Análisis Comparativo de Concurrencia (Varn vs Node.js vs Bun)

### 7.1 Tabla Comparativa de Concurrencia

| Característica | Node.js (V8 + libuv) | Bun (JSC + Zig) | **Varn (Register VM + Rust/ASM)** |
| :--- | :--- | :--- | :--- |
| **Huella de memoria por tarea / promesa** | ~2.5 KB – 4.0 KB por `Promise` + Closure *(reportado)* | ~1.0 KB – 1.5 KB *(reportado)* | **~1 KB por `Task` aparcada *(medido 100k–1M en esta máquina)*** |
| **Modelo de Concurrencia** | Single-thread (Event Loop) | Single-thread (Event Loop nativo en Zig) | **Híbrido: Corrutinas cooperativas + Isolates Multihilo Reales** |
| **Aprovechamiento de Núcleos CPU** | Requiere `cluster` (procesos separados) | Requiere `worker_threads` | **Nativo: `spawnIsolate` en hilos del SO con canales tipados** |
| **Contención de Garbage Collector (GC)** | Pausas globales de GC aumentan con el heap | GC optimizado en JSC | **GC por Isolate**: el GC de un hilo no congela a los demás |
| **Medido aquí (100k sleeps 12s)** | 12.04s · 118MB pico | 12.08s · 196MB pico | **12.10s · 137MB pico; spawn 19ms** (Go: 12.28s · 854MB) |
| **Medido aquí (1M sleeps 8s)** | 8.32s · 538MB pico | 8.40s · 769MB pico | **9.10s · 1.0GB pico; spawn 251ms** (Go: 11.69s · 8.5GB) |

### 7.2 Huella de Memoria por Tarea

Medido en esta máquina (32 GB RAM, Windows x86-64, build release; pico =
`PeakWorkingSet64` del proceso, mismo método para los cuatro runtimes).
Una tarea aparcada retiene solo su estado congelado (`Frozen`: frames con
los slots vivos por `suspend_live`, handlers y upvalues abiertos); no retiene
un `ExecCtx`. Los contextos de ejecución salen de un pool acotado por cola y
se reciclan al aparcar o terminar, de modo que hay uno por tarea *en
ejecución* y no uno por tarea viva.

| Escala | Original | Tarea congelada | Núcleo rediseñado (ADR-0019 rev. 2) |
| :--- | :--- | :--- | :--- |
| 100k × sleep 12s | ~12.5s · 540MB | 12.18s · 184MB | 12.10s · 137MB |
| 1M × sleep 8s | 21.3s · 5.0GB | 10.04s · 1.5GB | 9.10s · 1.0GB |
| 1M × `await Task.yield()` (ciclo ceder+reanudar) | — | 3.1s | 1.5s |

Origen de la mejora, medido con dhat (100k tareas, pico global): 471MB →
274MB al compartir `Linker` y capacidades entre forks, crear la `TaskQueue` de
forma perezosa, cachear el `FrameLayout` en el proto (no por `FrameStore`) y
tomar las constantes del pool compartido `proto_constants` en vez de
re-internarlas por tarea; 274MB → 162MB al dejar de retener el `ExecCtx`.
Rediseño del núcleo (segunda ronda), medido con el perfil del ciclo
congelar/reanudar (1M tareas, ms acumulados: `wake_settled` 514, `watch+park`
308, montaje 279, `freeze` 203): el coste era maquinaria, no el modelo.
- `AsyncTask`: un solo `Mutex`, sin pool global con mutex, lectura de estado sin
  clonar (`is_pending`) y observadores tipados. Un observador de despertar es
  un `WakeToken` (cola + token, sin `Box` ni cierre); el primero vive inline.
- La cola de tareas: las aparcadas viven en un slab indexado por token con
  generación; no hay `HashMap` de aparcadas ni de esperas, y el despertar es
  un índice directo. El orden entre tareas que esperan un mismo handle pasa de
  LIFO a FIFO (el orden de registro). Las tareas abandonadas (output ya
  liquidado) se purgan de forma amortizada al duplicarse el slab.
- Creación: `LazyTask` ya no envuelve un `Closure` portable ni copia el pool de
  constantes; guarda `proto`, upvalues y argumentos inline. Una función async
  sin upvalues reutiliza su closure estático al montarse.
- `Frozen` en una sola asignación para el caso de un frame y con solo los slots
  vivos (`suspend_live` ∪ slots con upvalues abiertos), no el frame completo.
- Timers: el driver solo se despierta (llamada de sistema) cuando el timer
  nuevo pasa a ser el más cercano, no en cada `sleep`.

Piso restante por tarea (~0.9 KB): tres objetos de heap por tarea (la tarea
perezosa y los dos handles) con su entrada en el índice de identidad, dos
`AsyncTask` (~120 B) y el `Frozen` (~180 B); más ~25 MB fijos de nursery.

Node y Bun siguen por delante a 1M (538MB / 769MB; 0.3s de sobrecarga frente
a ~1.1s de Varn). Lo que queda es el modelo de objetos de heap: cada valor
visible para la VM paga un slot más una entrada de hash de identidad, y una
tarea crea tres.

### 7.3 Escalado y Límites Empíricos

1. **Multinúcleo por Isolates**: cada Isolate corre en su propio hilo con heap y GC dedicados; la comunicación es por canales tipados. Sin medición de escalado lineal publicada: pendiente.
2. **Capacidad de Nursery**: `NURSERY_CAPACITY = 65 536` objetos jóvenes con umbral de colección en `3/4` (49 152). Un GC lo dispara el contexto que esté ejecutando (el root o una tarea), y reúne raíces del contexto que lo dispara, de todo contexto que esté bombeando la cola (`pump_until`) y del estado congelado de toda tarea lista o aparcada. `tests/154-task-gc-roots.vn` fija ese contrato.
3. **Evolución del I/O Host**: el backend de red de Varn corre en un bucle reactivo basado en `mio` (IOCP / epoll) fusionado con la wheel de timers: un solo hilo `varn-io-driver` liquida timers vencidos y eventos TCP/UDP. UDP también va por el driver con fast-path no bloqueante (sin hilo por `recv`). El wake del scheduler es O(1) por handle vía mapa `waiters`, no escaneo lineal. Medido en esta máquina: solape `8×sleep(100)` Varn 111ms / Go 100 / Node 103 / Bun 122; `parallel` 500 tareas Varn 5ms / Go 0 / Node 0 / Bun 8; 100k tareas dormidas Varn 12.10s y 137MB pico frente a Go 12.28s y 854MB (~8.4KB/tarea); a 1M, Varn 9.10s y 1.0GB frente a Go 11.69s y 8.5GB.
