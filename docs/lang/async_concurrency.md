# Varn Language — Concurrencia Asíncrona

> Fuentes: `tests/21-async.vn`, `tests/47-isolates-multithread.vn`, `tests/54-channels.vn`, `tests/75-async-shapes.vn`, `tests/77-async-generators.vn`.

---

## 1. `async` / `await`

### Funciones asíncronas

```varn
import { sleep, spawn, parallel } from "std:task"
// tests/21-async.vn

async function asyncAdd(a: int, b: int): int {
    return a + b
}

async function asyncChain(): str {
    const x = await asyncAdd(3, 4)
    const y = await asyncAdd(x, 10)
    return `result=${y}`
}

async function runAsync(): void {
    const r1 = await asyncAdd(5, 6)
    assert("async add", r1 === 11)

    const r2 = await asyncChain()
    assert("async chain", r2 === "result=17")
}

await runAsync()   // top-level await en módulo
```

### Manejo de errores en async

```varn
async function asyncError(): str {
    try {
        throw new Error("async fail")
    } catch (e) {
        return "caught:" + e.message
    }
}
const r3 = await asyncError()
assert("async catch", r3 === "caught:async fail")
```

---

## 2. `spawn` — Tareas concurrentes

```varn
const h1 = spawn(asyncAdd(1, 2))
assert("spawn handle await", await h1 === 3)
```

`spawn` devuelve un handle que puede ser `await`-ed para obtener el resultado.

---

## 3. `parallel` — Ejecución paralela

```varn
const all = await parallel([
    asyncSquare(2),
    asyncSquare(3),
    asyncSquare(4),
])
assert("parallel len",  all.length === 3)
assert("parallel[0]",   all[0] === 4)
assert("parallel[1]",   all[1] === 9)
assert("parallel[2]",   all[2] === 16)
// tests/21-async.vn:47
```

Ejecuta todas las tareas en paralelo y devuelve los resultados en el mismo orden.

---

## 4. `TaskGroup` — Grupos de tareas

```varn
using group = TaskGroup<int>()
const g1 = group.spawn(asyncAdd(7, 8))
const g2 = group.spawn(asyncAdd(9, 10))
const joined = await group.join()
assert("group joined len",  joined.length === 2)
assert("group joined[0]",   joined[0] === 15)
assert("group joined[1]",   joined[1] === 19)
assert("group handle 1",    await g1 === 15)
// tests/21-async.vn:57
```

### Cancelación del grupo

```varn
using cancelGroup = TaskGroup<int>()
const c1 = cancelGroup.spawn(asyncAdd(10, 20))
cancelGroup.cancel()
try {
    await c1
    assert("cancel should reject", false)
} catch (e) {
    assert("group cancel rejects handle", true)
}
```

### Disposición asíncrona (`disposeAsync`)

```varn
using disposeGroup = TaskGroup<int>()
const d1 = disposeGroup.spawn(asyncAdd(30, 40))
await disposeGroup.disposeAsync()
try {
    await d1
} catch (e) {
    assert("disposeAsync rejects handle", true)
}
```

---

## 5. Isolates — Hilos con Heap Propio

Un **isolate** es una unidad de ejecución aislada con su propio heap, GC y espacio de memoria. Se comunica con el hilo principal y otros isolates exclusivamente a través de **canales tipados**.

```varn
import { spawnIsolate, channel, Sender, Receiver, ChannelClosed } from "std:task"
// tests/47-isolates-multithread.vn

// Función que corre en el isolate hijo (debe ser exportada y top-level)
export async function workerMain(rx: Receiver<ValueMsg>, tx: Sender<int>) {
    const data = await rx.receive()
    assert("msg in child", data.value === 10)
    await tx.send(data.value * 2)
}

// Spawn en el isolate padre
const in1 = channel<ValueMsg>(1)
const out1 = channel<int>(1)
const h1 = await spawnIsolate(workerMain, [in1.rx, out1.tx])
await in1.tx.send({ value: 10 })
assert("response", await out1.rx.receive() === 20)
await h1.join()
```

### Restricciones de `spawnIsolate`

```varn
// Solo funciones top-level exportadas son válidas
let threwAnon = false
try {
    await spawnIsolate((tx: Sender<str>) => {}, [])   // lambda: error
} catch (e) {
    assert("error for anon", e.message.indexOf("must be a function reference") >= 0)
}

// Funciones no exportadas también dan error
let threwUnexported = false
try {
    await spawnIsolate(localUnexported, [])
} catch (e) {
    assert("error for unexported", e.message.indexOf("is not a top-level exported function") >= 0)
}
```

### Worker con múltiples argumentos

```varn
export async function workerArgs(tx: Sender<str>, num: int, text: str, arr: Array<int>, obj: { x: str }) {
    assert("num arg",    num === 42)
    assert("text arg",   text === "hello")
    assert("arr length", arr.length === 3)
    assert("obj arg",    obj.x === "nested")
    await tx.send("args_ok")
}

await spawnIsolate(workerArgs, [out2.tx, 42, "hello", [1, 2, 3], { x: "nested" }])
```

### Guardián `isIsolate`

```varn
// Evita re-ejecutar la suite en el módulo hijo
if (!isIsolate) {
    await testIsolates()
}
```

---

## 6. Canales Tipados (`channel`)

```varn
import { channel, ChannelClosed, Sender, Receiver } from "std:task"
// tests/54-channels.vn

const ch = channel<int>(2)     // canal con capacidad 2
await ch.tx.send(1)
await ch.tx.send(2)
assert("ch recv 1", await ch.rx.receive() === 1)
assert("ch recv 2", await ch.rx.receive() === 2)
```

### Close y `ChannelClosed`

```varn
const ch = channel<str>(1)
ch.tx.close()
let typed = false
try {
    await ch.rx.receive()
} catch (e) {
    typed = e instanceof ChannelClosed
}
assert("recv closed typed", typed)

let sendClosed = false
try {
    await ch.tx.send("nope")
} catch (e) {
    sendClosed = e instanceof ChannelClosed
}
assert("send closed typed", sendClosed)
```

### `for await` sobre canal

```varn
const ch = channel<int>(4)
await ch.tx.send(10)
await ch.tx.send(20)
ch.tx.close()
let sum = 0
for await (const v of ch.rx) {
    sum = sum + v
}
assert("for-await drains", sum === 30)
```

### `using` para auto-close del Sender

```varn
const ch = channel<int>(1)
{
    using tx = ch.tx      // tx.dispose() cierra el canal al salir del bloque
    await tx.send(5)
}
assert("drained after dispose", await ch.rx.receive() === 5)
let closed = false
try { await ch.rx.receive() } catch (e) { closed = e instanceof ChannelClosed }
assert("closed after dispose", closed)
```

### Canal cross-isolate

```varn
export async function echoWorker(rx: Receiver<int>, tx: Sender<int>) {
    for await (const v of rx) {
        await tx.send(v * 2)
    }
    tx.close()
}

const a = channel<int>(4)
const b = channel<int>(4)
const handle = await spawnIsolate(echoWorker, [a.rx, b.tx])
await a.tx.send(21)
assert("cross-isolate echo", await b.rx.receive() === 42)
a.tx.close()
await handle.join()
```

---

## 7. Protocolo Enum en Canales

Para comunicaciones con ciclo de vida bien definido:

```varn
enum Msg { Val(int), Exit }

export async function workerDialog(rx: Receiver<Msg>, tx: Sender<int>) {
    let open = true
    for await (const msg of rx) {
        match (msg) {
            Val(n)    => await tx.send(n + 1),
            Msg.Exit  => open = false,
        }
        if (!open) break
    }
    tx.close()
}

// En el isolate padre:
await in3.tx.send(Msg.Val(1))
assert("dialog 1", await out3.rx.receive() === 2)
await in3.tx.send(Msg.Exit)
await h3.join()
```

---

## 8. `sleep`

```varn
await sleep(1)    // pausa de ~1ms (disponible en pruebas como tests/21-async.vn)
```

---

## 9. Top-Level `await`

Varn soporta `await` a nivel de módulo:

```varn
await runAsync()          // tests/21-async.vn:98
await runAsyncGenerators()   // tests/77-async-generators.vn:105
if (!isIsolate) {
    await testIsolates()
}
```
