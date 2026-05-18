# Arquitectura del Runtime Asíncrono (varn-runtime)

`varn-runtime` orquesta la VM síncrona (`varn-vm`) dentro de un event-loop Tokio para concurrencia cooperativa.

## 1. Modelo de Hilos

El runtime usa `tokio::task::LocalSet` — hilo único. Todo el estado de la VM es `!Send` (`Rc<RefCell<T>>` para máximo rendimiento sin locks). Miles de tareas Varn corren concurrentemente en un solo hilo gracias a Tokio's `spawn_local` + multiplexación `epoll`/`kqueue` para I/O.

No hay multi-hilo real actualmente. Ver [ROADMAP.md](ROADMAP.md) para los planes de arena allocation.

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

- `spawn`: crea nueva tarea en el LocalSet, retorna handle `Task<T>`.
- `parallel([...])`: `TaskGroup` — resuelve solo cuando todos los hijos resuelven. Recoge resultados en array.

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

---

## 9. Métricas

El scheduler recopila métricas atómicas (`AtomicU64`) sin impacto en performance:
- `vm_polls`: ciclos de ejecución
- `cooperative_yields`: veces que el poll budget fue excedido
- `timer_waits`: esperas en sleep/setTimeout
- `task_waits`: esperas en await de tareas I/O
