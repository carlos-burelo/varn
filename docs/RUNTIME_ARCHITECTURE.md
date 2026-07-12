# Arquitectura del Runtime Asíncrono (varn-runtime)

`varn-runtime` orquesta la VM síncrona (`varn-vm`) sobre Tokio para concurrencia cooperativa y paralelismo explícito con isolates.

## 1. Modelo de Hilos

El scheduler crea un runtime Tokio multi-thread compartido (`Builder::new_multi_thread()`), pero ejecuta cada raíz Varn en un `tokio::task::LocalSet` porque la VM sigue siendo `!Send` en su hot path.

Eso da un modelo híbrido:

- **Concurrencia local**: `spawn`, `parallel`, timers y generators viven dentro del `LocalSet` del isolate/raíz actual.
- **Paralelismo real**: `spawnIsolate(...)` levanta otro worker con `std::thread::spawn` y su propia VM; la mensajería es por channels tipados `Sender<T>`/`Receiver<T>` transferidos como argumentos.
- **Boundary de memoria**: no hay heap mutable compartido entre isolates; solo cruzan valores sendables.

---

## 2. Scheduler Cooperativo

El scheduler interactúa con la VM solo a través del trait `TaskRunner` — ignora `CallFrame` y NaN-boxing.

### Poll Budget
Para evitar que un loop infinito ahogue el event-loop:
- Budget: 256 ciclos de polling por tarea.
- Si se supera: `tokio::task::yield_now().await` — cede el hilo, permite que otras tareas progresen, luego retoma.

---

## 3. Tipos de Suspend

Cuando la VM no puede continuar sincrónicamente, emite un `VmSuspend`:

| Variante | Causa | Acción del runtime |
|----------|-------|--------------------|
| `Suspend::Task(AsyncTask)` | `await expr` | `tokio::select!` sobre la tarea y canal de cancelación. Reanuda con `push_resume_value` al resolver. |
| `Suspend::Timer(Duration)` | `sleep(ms)` / `setTimeout` | `tokio::time::sleep(dur)`. |
| `Suspend::Yield(VmValue)` | `yield val` en generator | Envía valor al `GenChannel`. Suspende hasta `next()`. |

El frame queda "congelado" en el heap hasta la reanudación.

---

## 4. Async/Await

```Varn
async function fetchData(): str {
    const resp = await http.get("https://api.example.com")
    return resp.body
}
```

1. El compilador emite `OpAwait`.
2. La VM evalúa la expresión → `AsyncTask`.
3. Emite `Suspend::Task(task)`.
4. El scheduler ejecuta la tarea Tokio.
5. Al resolver: `push_resume_value(result)`, la VM continúa desde el mismo frame.

---

## 5. spawn y parallel

```Varn
const task = spawn(fetchData())
const [a, b] = await parallel([fetchA(), fetchB()])
```

- `spawn`: crea nueva tarea en el `LocalSet`, retorna handle `Task<T>`.
- `parallel([...])`: dispara varios children sobre el scheduler actual y resuelve un array cuando terminan.
- `spawnIsolate(fn, args)`: inicia otro hilo, carga el módulo del `fn` exportado y retorna un `IsolateHandle` (`join()` espera al worker; rechaza con `Error` tipado si el worker lanzó).

### Channels tipados

Mensajería entre isolates (y dentro de uno): `channel<T>(capacity)` retorna
`{ tx: Sender<T>, rx: Receiver<T> }`.

- **Tabla global de canales** (`varn-runtime::channel`): cada canal es una cola
  mpmc bounded (`capacity >= 1`) identificada por `u64`; los endpoints Varn solo
  llevan ese id (`_chan`), por eso transferirlos a otro isolate es copiar un
  entero — ambos lados comparten el mismo canal.
- **Backpressure**: `send` sobre cola llena parkea al productor (resuelve `true`
  al entrar el mensaje, `false` si el canal cierra antes); `receive` sobre cola
  vacía parkea al consumidor.
- **Valores cross-thread**: siempre heap-independientes (`SendValue`, incluidos
  enums vía `SendEnumVariant` y endpoints anidados). La materialización en el
  heap del consumidor ocurre en un único hook del await-resume del VM
  (`host_values::open_resolved` / `open_rejected`), que también mintea errores
  tipados (`ChannelClosed extends Error`).
- **Cierre real**: `close()` despierta a todos los waiters; lo encolado se
  drena, después `receive()` rechaza con `ChannelClosed` y
  `for await (const v of rx)` termina el loop.

---

## 6. Generators

```Varn
function* range(n: int) {
    let i = 0
    while (i < n) {
        yield i
        i = i + 1
    }
}

for (const item of range(5)) {
    print(item)
}
```

El runtime tiene un planificador dedicado para generators y `async function*`. Loop de polling hasta `Suspend::Yield`. El scheduler empaqueta `{ value: V, done: bool }` y lo envía al `GenChannel` del consumidor.

---

## 7. using y AsyncDisposable

```Varn
async function main(): void {
    using file = await fs.open("data.txt")
    const content = await file.readAll()
}  // file.asyncDispose() called automatically
```

`using` garantiza `dispose()` (o `asyncDispose()`) al salir del scope — incluso con `return`/`throw` intermedios. El compilador inlinea la llamada. El runtime no necesita lógica especial.

---

## 8. Gestión de Memoria

Sin GC mark-and-sweep. El heap usa `Rc<RefCell<T>>`:
- Cuando la última referencia desaparece del scope Rust, la memoria se libera inmediatamente (RAII).
- Ciclos de referencias son teóricamente posibles pero infrecuentes en código Varn típico.

Cada isolate tiene su propio heap/VM. El boundary cross-thread ocurre a través de serialización a `SendValue`.

---

## 9. Métricas

El scheduler recopila métricas atómicas (`AtomicU64`) sin tocar el heap de la VM:
- `root_tasks`: raíces ejecutadas
- `spawned_tasks`: children creados por `spawn`
- `spawned_async_gens`: generadores async lanzados
- `vm_polls`: ciclos de ejecución
- `cooperative_yields`: veces que el poll budget fue excedido
- `timer_waits`: esperas en sleep/setTimeout
- `task_waits`: esperas en `await`
