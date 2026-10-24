# Runtime: tareas, scheduler, isolates

Complementa a [`VM_ARCHITECTURE.md`](VM_ARCHITECTURE.md) (intérprete, frames,
excepciones, generadores). No existe un crate `varn-runtime`: el runtime vive en
`varn-vm` (ejecución, scheduler), `varn-builtins` (nativos de host) y la std en Varn.

## Tareas y `await`

- Llamar a una función `async` crea una **tarea perezosa** (`LazyTask`, `varn-vm/src/task.rs`): no corre hasta que alguien la aguarda o la lanza.
- `await` (`ExecCtx::begin_await`, `exec/ctx_tasks.rs`) monta el cuerpo de una tarea perezosa como frame de quien aguarda (`scheduler::mount`): sin hilo ni cola nueva. Si el valor ya está resuelto o rechazado continúa o lanza en el acto; solo una celda pendiente (`TaskHandle`) suspende.
- Suspender congela los frames de la tarea (`scheduler/suspend.rs`: `freeze`/`thaw`, por frame y en términos relativos); los drivers la reanudan con `resolve_awaiter`/`reject_awaiter` y la reentrada usa OSR en modo `Resume` si hay código JIT.
- `spawn(task)` la convierte en tarea desacoplada (`scheduler::enqueue_detached`) y devuelve un `TaskHandle`.

## Scheduler (`varn-vm/src/exec/scheduler/`)

Cooperativo y de un solo hilo por isolate: `drive.rs` (bucle principal),
`mount.rs` (montar tareas), `queue.rs` (cola de listas), `pumps.rs` (bombea timers
y promesas de host hasta que una celda se liquida), `suspend.rs` (congelar/descongelar).

Temporizadores: `varn-builtins/src/runtime/timer.rs` (`sleep_task`,
`next_deadline`, `take_due`).

## Isolates

`spawnIsolate(fn, args)` (`varn-vm/src/exec/host/isolates.rs`): un hilo del SO por
isolate (`std::thread::Builder` con pila `VM_STACK_BYTES`), `Vm` y heap propios. El
`fn` debe ser una función exportada de nivel superior: el hijo la vuelve a cargar
por nombre. Devuelve un `IsolateHandle` con `join()`.

Canales entre isolates: `varn-builtins/src/runtime/channel.rs` — registro global
por id con `Mutex`, `create/send/try_receive/close`, resultados
`SendOutcome`/`RecvOutcome` (`Sent|Parked|Closed`, `Item|Parked|Closed`).
Superficie Varn (`runtime:task` → `std:task/channel`): `channel<T>(capacity)` →
`{ tx: Sender<T>, rx: Receiver<T> }`; `Sender.send/close/dispose`,
`Receiver.receive/next/dispose`, `ChannelClosed extends Error`.

## En la std (`std/task/`)

Escrito en Varn sobre `runtime:task`: `spawn`, `sleep`, `parallel`, `cancel`,
`delay`, `setTimeout`/`clearTimeout` y el namespace `Task` (`combinators.vn`);
`TaskGroup<T>` con `spawn/join/cancel` (`group.vn`); `Mutex`, `Semaphore`,
`WaitGroup`, `RateLimiter` (`sync.vn`); `Cron` (`cron.vn`).

## AOT (`varn-rt`)

Runtime mínimo para `vn build --native`: `main` → `_varn_main`, `varn_rt_print`,
`varn_rt_str_concat`, `varn_rt_panic`. Sin scheduler, canales ni GC.
