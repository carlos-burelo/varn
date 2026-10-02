# ADR 0019: Tareas como objetos de estado compactos (diseño objetivo)

## Estado
Implementado el núcleo (2026-10-02), matriz 4/4 verde. Medido:
solape `8×sleep(100)→111ms`, `parallel` 500 tareas en 5ms con tipos
preservados (`isError=true`), tablas `suspend_live` emitidas
(`worker [async, state_size=3, suspend_live=1]`), soak 3000 tareas
(72k parks, 100% released, heap live 2.6%), 100k tareas en 12.5s con pico
540MB frente a Go (12.0s, 855MB). Liberación de frames implementada con
fallback seguro (ver Consecuencias).

## Revisión 2026-10-02: la tarea aparcada no retiene `ExecCtx`
La primera implementación aparcaba el `ExecCtx` entero con los frames
liberados al stash (1312 B del contexto + 384 B del `FrameStack` + el mapa de
linker, la cola y el layout por fork). Medido con dhat a 100k tareas, el
contexto era ~3 KB de los ~4.7 KB por tarea.

Decisión: un único mecanismo. `scheduler/` se divide por dominio
(`suspend`, `queue`, `mount`, `drive`, `pumps`):
- `Frozen` es el estado congelado de una tarea (frames vivos, handlers,
  upvalues abiertos). La cola lista guarda semillas (`Start::Fresh(LazyTask)`)
  o reanudaciones (`Start::Resume(Frozen, Delivery)`); ninguna lleva contexto.
- Un `ExecCtx` se toma de un pool acotado por cola, se monta/descongela, se
  ejecuta y se recicla. Si la tarea no puede congelarse se rechaza con error
  interno: se borró el camino de "retención completa" (ley 8).
- `Linker` pasa a tabla compartida (como `modules`), las capacidades a `Rc`,
  la `TaskQueue` de un contexto es perezosa, el `FrameLayout` vive en el proto y
  el pool de constantes del fork sale de `proto_constants` (antes se
  re-internaba el pool completo por tarea).
- La parte local de la cola ya no está bajo `Mutex`: solo `fired`/`dead` y el
  condvar cruzan hilos. La generación del condvar se captura antes de
  `wake_settled` para no perder un wake que llegue entre drenar y esperar.

Defecto encontrado y corregido: un GC disparado desde dentro de una tarea solo
enraizaba a esa tarea y a su cola propia (vacía); las pilas de las hermanas
aparcadas y la del root que bombea quedaban sin raíz y el siguiente `await`
reanudaba con valores nulos (`cannot store 'null' in an int register`). El
alcance del GC ahora es: contexto en ejecución, todo contexto que bombee y el
`Frozen` de toda cola (`tests/154-task-gc-roots.vn`).

Resultado (release, mismo método de pico para todos): 100k × 12s pasa de
540MB a 184MB; 1M × 8s de 21.3s y 5.0GB a 10.04s y 1.5GB (Go: 11.69s y 8.5GB;
Node: 8.32s y 538MB; Bun: 8.40s y 769MB).

## Revisión 2 (2026-10-02): núcleo de tareas sin maquinaria por tarea
Perfil del ciclo congelar/reanudar a 1M tareas: la maquinaria de colas, mapas y
cierres costaba más que el modelo (`wake_settled` 514 ms, `watch+park` 308 ms,
montaje 279 ms, `freeze` 203 ms, frente a ~1.1 s de bytecode real).

Decisión (un único mecanismo por pieza, se borró lo anterior):
- `AsyncTask`: un `Mutex<Slot>`; sin pool global; `is_pending` sin clonar; el
  observador de despertar es un `WakeToken` (`WakeQueue` + token) sin `Box`.
- `TaskQueue`: slab de tareas aparcadas con generación, token = índice +
  generación. Se borraron `parked`/`waiters` (`HashMap`), `watch` con cierre y
  la notificación de "muerta" por observador. Purga amortizada del slab para
  tareas cuyo output ya liquidó. El orden entre esperas del mismo handle es
  FIFO (antes LIFO por `Vec::pop`).
- `LazyTask` = `proto` + upvalues + `module_base` + `TaskArgs` inline; sin
  `Closure` portable. El montaje reutiliza el closure estático si no hay
  upvalues. `Closure` portable conserva solo lo que usa el arranque del VM.
- `Frozen`: primer frame inline y solo slots vivos (`Keep`: `suspend_live` ∪
  upvalues abiertos), mismo código para completos y compactos.
- `timer::sleep_task` despierta al driver solo si el timer es el más cercano.

Resultado (release, mismo método de pico): 100k × 12s: 137MB; 1M × 8s: 9.10s y
1.0GB; ceder+reanudar a 1M: 3.1s → 1.5s. Node 8.32s/538MB, Bun 8.40s/769MB,
Go 11.69s/8.5GB. Lo que queda es el modelo de heap (3 objetos por tarea con su
entrada de identidad), fuera del alcance de esta decisión.

## Contexto
El pase `state_machine` (`varn-compiler`) ya particiona el CFG en cada
`Await`/`Yield`, ordena en RPO y calcula `state_size = 1 + max_live`
(`layout.rs`). El runtime no lo consume: `drive_fork` (`scheduler.rs`)
aparca el `ExecCtx` entero y el GC reúne raíces de todos los forks
en cada minor (`ctx.rs::run_minor_gc`). El coste por tarea parada es un
contexto completo y el GC menor es O(parqueadas).

Pasos previos ya tomados hacia este diseño (no revertir):
- Tablas compartidas (`globals`, `modules`, `resources`, `proto_constants`,
  `static_closures`, `metadata`, `hashable_keys`): una sola tabla por
  proceso bajo el contrato single-thread del heap. Los forks solo poseen
  estado de ejecución (stack, frames, handlers, staging).
- GC sin duplicados: las tablas compartidas se reúnen una vez (dueño 0) en
  el menor y una vez en `major_roots` (`major_roots_local` por fork).
- Wake O(1) por handle (`waiters`/`fired`/`dead`) en vez de escaneo lineal.
- Un solo hilo I/O (`varn-io-driver` fusiona wheel de timers + mio).

## Decisión
La tarea parada retiene solo slots vivos: el compilador emite
`FunctionProto.suspend_live` (live sets `live_after` + `live_in` de todos
los `Try` handlers + reg 0 implícito, ordenado por resume ip) y el
scheduler poda en park (`prune_parked_top`): nula Dyn/Ref muertos del
frame superior salvo slots con upvalues abiertos; sin entrada exacta o
ante cualquier inconsistencia, retención completa (fallback seguro).

## Consecuencias
- Retención por tarea ∝ slots vivos; verificado `worker [async,
  state_size=3, suspend_live=1]` y matriz 4/4 verde.
- Al aparcar se liberan los frames al stash (`SuspendedState`) y se
  reconstruyen idénticos al reanudar; cualquier duda usa el fork completo.
  El GC traza el stash con forwarding en menor y mayor.
- Sin número (bench 100k sleeps, GC vs parqueadas) la ganancia cuantitativa
  sigue pendiente de medición.
