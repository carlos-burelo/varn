# Channels tipados para isolates

**Fecha:** 2026-07-11
**Estado:** Aprobado (diseño). Pendiente: plan de implementación.

## Problema

La mensajería entre isolates es hoy un único `IsolatePort` bidireccional sin tipos:

```vn
export declare class IsolatePort {
    send(msg: dynamic): void;
    receive(): Task<dynamic>;
}
export declare function spawnIsolate(fn: dynamic, args: Array<dynamic>): Task<IsolatePort>;
```

Consecuencias medidas (scan `scripts/find-dynamic-inference.ps1`, 2026-07-11):
`47-isolates-multithread.vn` concentra 12 de los 16 `dynamic` inferidos que quedan en
la suite — payloads (`data.value`, `msg.val`, `reply.reply`) y los propios métodos del
puerto. El checker no puede verificar ningún protocolo de mensajes; todo error de
shape se descubre en runtime. Además el puerto no tiene semántica de cierre (los
loops de diálogo terminan por mensaje-centinela `"exit"`), no hay backpressure
(cola unbounded), y un solo canal bidireccional impide topologías con más de un
flujo tipado por worker.

## Decisiones (con alternativas descartadas)

| # | Decisión | Descartado |
|---|----------|------------|
| 1 | Channels estilo Rust/Go: `Sender<T>` / `Receiver<T>` como clases-recurso separadas | `IsolatePort<In, Out>` genérico mínimo (no resuelve cierre, backpressure ni multiplicidad); protocolo-enum sobre el puerto actual |
| 2 | Endpoints 100 % sendables desde v1: se crean con `channel<T>()` en cualquier isolate y viajan como args de spawn o como mensajes | Par bootstrap creado por `spawnIsolate` (menos runtime, pero limita topologías y obliga a una segunda fase igual) |
| 3 | Cierre: `for await` sobre `Receiver` termina limpio; `receive()` directo lanza `ChannelClosed extends Error` | `RecvResult<T> { Msg(T), Closed }` (match obligatorio por mensaje, verboso); `Task<T?>` (null-checks, T no puede ser nullable) |
| 4 | Bounded desde v1: `channel<T>(capacity)` obligatorio, `send(): Task<void>` awaitea con buffer lleno | Unbounded como hoy (sin backpressure; capacity retrofit rompería la firma de `send`) |
| 5 | `IsolatePort` se **elimina** (replacement over extension) | Ruta dual port+channels (prohibido por `<evolution_strategy>`) |
| 6 | Sendabilidad de `T` no se verifica estáticamente en v1; `to_sendable` sigue siendo el guard en runtime | Constraint `Sendable` estructural en el checker (análisis profundo de tipos, obra aparte) |
| 7 | Cierre explícito (`close()` / `using`); sin refcount cross-isolate | Auto-close al morir todos los senders (GC distribuido, hoyo de complejidad) |

## 1. API — contrato `runtime:task` (`task_runtime.vn`)

```vn
export declare class ChannelClosed extends Error {}

export declare class Sender<T = dynamic> {
    send(msg: T): Task<void>;   // awaitea con buffer lleno; ChannelClosed si el canal cerró
    close(): void;              // idempotente
    dispose(): void;            // alias de close() → Disposable, usable con `using`
}

export declare class Receiver<T = dynamic> {
    receive(): Task<T>;         // drena buffer; ChannelClosed cuando cerrado y vacío
    dispose(): void;            // cierra el canal entero: sends posteriores lanzan
    // implementa el protocolo async-iteration → `for await (const m of rx)`
}

export declare function channel<T = dynamic>(capacity: int): { tx: Sender<T>, rx: Receiver<T> };

export declare class IsolateHandle {
    join(): Task<void>;         // re-lanza en el parent si el worker lanzó
}

export declare function spawnIsolate(fn: dynamic, args: Array<dynamic>): Task<IsolateHandle>;
```

`std:task` (facade) re-exporta `channel`, `Sender`, `Receiver`, `ChannelClosed`,
`IsolateHandle` y envuelve `spawnIsolate`, igual que hace hoy con `spawn`/`sleep`.

## 2. Semántica de canal

- **mpmc estilo Go.** Un canal es una cola bounded compartida process-wide; los
  endpoints son referencias (handles con id de canal) a una tabla global de canales
  (`Arc`). Varios senders/receivers sobre el mismo canal compiten por la cola.
- **Transferencia.** `to_sendable` special-casea `Sender`/`Receiver`: transfiere el
  handle (id), no copia el canal. Un endpoint puede viajar en los args de
  `spawnIsolate` o dentro de un mensaje por otro canal.
- **Cierre.** `close()`/`dispose()` en cualquier endpoint cierra el canal completo:
  `send` posterior lanza `ChannelClosed`; `receive` drena lo pendiente y después
  lanza; `for await` consume lo pendiente y termina el loop sin excepción.
- **Backpressure.** `send` encola si hay espacio y resuelve; con buffer lleno queda
  pending (integración `AsyncTask::pending` + waker del scheduler de Varn, despertado
  por `receive`). `capacity >= 1` requerido; `capacity <= 0` → `RangeError`.
- **spawn.** `spawnIsolate` conserva la resolución actual de workers (función
  exportada por módulo; anónimas/locales rechazadas) y devuelve `IsolateHandle`.
  `join()` espera la terminación del worker y re-lanza su error en el parent.

## 3. Tipado

- **El tipo viaja en el canal, no en spawn.** Los args de `spawnIsolate` siguen
  siendo `Array<dynamic>`; la anotación del worker (`rx: Receiver<Msg>`) es la
  frontera de confianza, como cualquier boundary FFI.
- **Protocolo heterogéneo = enum con payload** + match exhaustivo al recibir.
  `send` con tipo incorrecto → error de compilación normal (WR3001).
- Con esto los 12 dynamics de isolates del scan quedan tipados; los restantes del
  archivo (`catch (e)`) ya se resolvieron con `catch: Error` (2026-07-11).

## 4. Uso de referencia (migración de `tests/47`)

```vn
enum Msg { Val(int), Exit }
enum Reply { Result(int), Bye }

export async function workerDialog(rx: Receiver<Msg>, tx: Sender<Reply>) {
    for await (const msg of rx) {
        match msg {
            Val(n) => await tx.send(Reply.Result(n + 1)),
            Exit   => { await tx.send(Reply.Bye); break },
        }
    }
}

const { tx, rx } = channel<Msg>(8)
const { tx: rtx, rx: rrx } = channel<Reply>(8)
const handle = await spawnIsolate(workerDialog, [rx, rtx])
await tx.send(Msg.Val(1))
match await rrx.receive() { Result(n) => assert("dialog", n === 2), Bye => {} }
await tx.send(Msg.Exit)
await handle.join()
```

## 5. Riesgos técnicos (a resolver en el plan)

1. **`for await` sobre clase nativa.** El VM debe despachar el protocolo
   async-iterator en una instancia builtin. Relacionado: `Symbol.asyncIterator` en
   object literals infiere `dynamic` (hoyo conocido del checker, `50-opt:20`). Puede
   requerir arreglar ese camino primero; si el costo explota, fallback v1:
   `while (true) { try { await rx.receive() } catch ... }` documentado y `for await`
   como fase 2 — decisión en el plan con evidencia.
2. **`send` bounded async.** Integración waker con el scheduler propio; cuidado con
   deadlock parent-child (ambos bloqueados en send con buffers llenos) — documentar,
   no detectar, en v1.
3. **Transferencia de endpoints.** Tabla global de canales + handle transferido;
   definir si el handle de origen queda invalidado (move) o compartido (alias). v1:
   alias (mpmc lo permite); sin invalidación.
4. **Cache de bytecode.** Cambio de contrato `runtime:task` invalida `.vnc`/bundle —
   requiere `cargo xtask build-std` + copiar `std.vnb` junto al exe.

## 6. Migración

- `tests/47-isolates-multithread.vn` reescrito con channels (casos 1-6 equivalentes:
  ping-pong, args múltiples, diálogo con cierre real en vez de centinela `"exit"`,
  paralelismo, errores de spawn) + casos nuevos: backpressure con `capacity 1`,
  `using tx`, `ChannelClosed` en send-after-close, endpoint transferido por mensaje.
- `varn_contract!` impl de `IsolatePort` en `task.rs` se elimina; `isolate.rs` del
  runtime se reescribe sobre la tabla de canales.
- `docs/WARP-SPEC.md` y `docs/RUNTIME_ARCHITECTURE.md`: sección de isolates.
- Bump de `HOST_API_VERSION` (breaking en `runtime:task`).

## 7. Testing

- **Rust unit** (`varn-runtime`): bounded block/wake, close-drain, mpmc concurrente,
  transfer cross-isolate, send-after-close, capacity 1.
- **Integración `.vn`**: `tests/47` reescrito; `tests/main.vn` re-habilita el import
  (hoy comentado).
- **`tests/errors/`**: send con tipo incorrecto (WR3001); `channel(0)` → RangeError.

## Fuera de alcance (v1)

- Constraint estático `Sendable`.
- Auto-close por refcount cross-isolate.
- `select`/multiplexado de canales.
- `trySend`/`tryReceive` no bloqueantes.
