# Typed Isolate Channels Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reemplazar `IsolatePort` (dynamic, bidireccional, unbounded) por channels tipados `Sender<T>`/`Receiver<T>` bounded, transferibles entre isolates, con cierre real (`ChannelClosed extends Error` + `for await`).

**Architecture:** Cola mpmc bounded en una tabla global de canales (`varn-runtime`); endpoints = instancias de clases nativas del contrato `runtime:task` que llevan el id del canal en `_chan`; valores cross-thread siempre heap-independientes (`SendValue`/`ObjData`), con un hook único en el await-resume del VM que materializa envelopes y errores tipados en el heap del consumidor.

**Tech Stack:** Rust (varn-runtime, varn-types, varn-vm, varn-builtins), contratos `varn_contract!`, tests `.vn`.

**Spec:** `docs/superpowers/specs/2026-07-11-isolate-channels-design.md`

## Global Constraints

- Sin ruta legacy: `IsolatePort` se elimina por completo (Task 5). Prohibido dual-path.
- Todo valor que cruza threads debe ser heap-independiente (`SendValue`, `ObjData` via `varn_types::value::new_object`). Nunca `VmValue` de otro heap.
- `capacity >= 1` obligatorio; `channel(0)` → error en runtime con mensaje `channel: capacity must be >= 1`.
- Validación estándar del repo: `./target/release/vn.exe run tests/main.vn` debe terminar `ALL TESTS PASSED`.
- Tras cambiar cualquier contrato `.vn` de builtins: `cargo xtask build-std && cp std.vnb target/release/std.vnb && cp std.vnb target/debug/std.vnb` y `./target/release/vn.exe cache clean` antes de validar.
- Commits: NO commitear cambios que no sean de tu task (hay trabajo de throw/catch sin commitear en el working tree — `git add` selectivo por archivo, nunca `git add -A`).

---

### Task 1: Channel core en varn-runtime

**Files:**
- Create: `crates/varn-lang/crates/varn-runtime/src/channel.rs` → path real: `crates/varn-runtime/src/channel.rs`
- Modify: `crates/varn-runtime/src/lib.rs` (añadir `pub mod channel;` junto a `pub mod isolate;`)
- Test: mismo archivo, `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `varn_types::task::AsyncTask` (`pending()`, `resolve(Value)`), `varn_types::value::{SendValue, ObjData, new_object}`.
- Produces (usado por Task 3/4):
  - `channel::create(capacity: usize) -> u64`
  - `channel::send(id: u64, val: SendValue) -> SendOutcome` con `enum SendOutcome { Sent, Parked(AsyncTask), Closed }` (la task parked resuelve `Value::Bool(true)` al entrar el mensaje, `Value::Bool(false)` si el canal cierra antes)
  - `channel::try_receive(id: u64) -> RecvOutcome` con `enum RecvOutcome { Item(SendValue), Parked(AsyncTask), Closed }` (la task parked resuelve el objeto `{value, done}` heap-independiente)
  - `channel::close(id: u64)`
  - `channel::next_obj(value: varn_types::Value, done: bool) -> varn_types::Value` (objeto `{value, done}`)

- [ ] **Step 1: Escribir tests que fallan (no compilan aún — el módulo no existe)**

```rust
// al final de crates/varn-runtime/src/channel.rs (crear el archivo con los tests
// primero y stubs `todo!()` si prefieres verlos fallar compilando)
#[cfg(test)]
mod tests {
    use super::*;
    use varn_types::Value;

    fn as_done(v: &Value) -> Option<bool> {
        if let Value::Object(o) = v {
            if let Some(Value::Bool(b)) = o.read().inner.get("done").map(|nv| nv_to_value(nv)) {
                return Some(b);
            }
        }
        None
    }

    #[test]
    fn send_then_receive_fifo() {
        let id = create(4);
        assert!(matches!(send(id, SendValue::Int(1)), SendOutcome::Sent));
        assert!(matches!(send(id, SendValue::Int(2)), SendOutcome::Sent));
        let RecvOutcome::Item(SendValue::Int(1)) = try_receive(id) else { panic!("fifo 1") };
        let RecvOutcome::Item(SendValue::Int(2)) = try_receive(id) else { panic!("fifo 2") };
    }

    #[test]
    fn bounded_send_parks_and_wakes_on_receive() {
        let id = create(1);
        assert!(matches!(send(id, SendValue::Int(1)), SendOutcome::Sent));
        let SendOutcome::Parked(task) = send(id, SendValue::Int(2)) else { panic!("must park") };
        assert!(matches!(task.peek_state(), varn_types::task::TaskState::Pending));
        // receive libera el hueco: el parked send entra a la cola y su task resuelve true
        let RecvOutcome::Item(SendValue::Int(1)) = try_receive(id) else { panic!() };
        match task.peek_state() {
            varn_types::task::TaskState::Resolved(Value::Bool(true)) => {}
            s => panic!("parked send should resolve true, got {s:?}"),
        }
        let RecvOutcome::Item(SendValue::Int(2)) = try_receive(id) else { panic!("queued after wake") };
    }

    #[test]
    fn receive_on_empty_parks_and_wakes_on_send() {
        let id = create(1);
        let RecvOutcome::Parked(task) = try_receive(id) else { panic!("must park") };
        assert!(matches!(send(id, SendValue::Int(7)), SendOutcome::Sent));
        match task.peek_state() {
            varn_types::task::TaskState::Resolved(v) => {
                assert_eq!(as_done(&v), Some(false), "value delivered, done=false");
            }
            s => panic!("parked recv should resolve, got {s:?}"),
        }
    }

    #[test]
    fn close_drains_then_reports_closed() {
        let id = create(4);
        send(id, SendValue::Int(1));
        close(id);
        // lo encolado se drena
        let RecvOutcome::Item(SendValue::Int(1)) = try_receive(id) else { panic!("drain") };
        // después: Closed
        assert!(matches!(try_receive(id), RecvOutcome::Closed));
        assert!(matches!(send(id, SendValue::Int(9)), SendOutcome::Closed));
    }

    #[test]
    fn close_wakes_parked_receivers_done_true_and_parked_senders_false() {
        let id = create(1);
        let RecvOutcome::Parked(rtask) = try_receive(id) else { panic!() };
        send(id, SendValue::Int(1)); // resuelve rtask con el valor
        let SendOutcome::Parked(stask) = send(id, SendValue::Int(2)) else { panic!() };
        close(id);
        match stask.peek_state() {
            varn_types::task::TaskState::Resolved(Value::Bool(false)) => {}
            s => panic!("parked send on close must resolve false, got {s:?}"),
        }
        let _ = rtask;
        // nuevo receiver parkeado post-close no existe: try_receive drena {2}? No — el
        // parked send NO entró a la cola (cerró antes): queda solo Closed tras drenar la cola.
        let RecvOutcome::Item(SendValue::Int(_)) = try_receive(id) else {
            // si la cola quedó vacía, directamente Closed también es válido…
            return; // ver Step 3: el parked send rechazado NO encola su valor
        };
    }

    #[test]
    fn close_is_idempotent_and_unknown_id_is_closed() {
        let id = create(1);
        close(id);
        close(id);
        assert!(matches!(try_receive(9999999), RecvOutcome::Closed));
        assert!(matches!(send(9999999, SendValue::Null), SendOutcome::Closed));
    }
}
```

- [ ] **Step 2: Correr y verificar que falla**

Run: `cargo test -p varn-runtime channel 2>&1 | tail -5`
Expected: error de compilación (`create` / `SendOutcome` no existen).

- [ ] **Step 3: Implementación**

```rust
//! Bounded mpmc channels compartidos process-wide entre isolates.
//!
//! Los endpoints Varn (`Sender<T>`/`Receiver<T>`, contrato runtime:task) guardan
//! solo el `u64` de esta tabla. Todo valor que se entrega desde otro thread es
//! heap-independiente (`SendValue` / `ObjData`): la materialización en el heap
//! del consumidor ocurre en el await-resume del VM.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use varn_types::task::AsyncTask;
use varn_types::value::{new_object, ObjData, SendValue};
use varn_types::Value;

pub enum SendOutcome {
    Sent,
    Parked(AsyncTask),
    Closed,
}

pub enum RecvOutcome {
    Item(SendValue),
    Parked(AsyncTask),
    Closed,
}

#[derive(Default)]
struct ChannelState {
    queue: VecDeque<SendValue>,
    closed: bool,
    recv_waiters: VecDeque<AsyncTask>,
    send_waiters: VecDeque<(SendValue, AsyncTask)>,
}

struct ChannelCore {
    capacity: usize,
    state: Mutex<ChannelState>,
}

// AsyncTask viaja entre threads igual que en IsolatePort (ver isolate.rs):
// solo se toca bajo el Mutex de state y resolve() es el mismo mecanismo que
// ya usa el waker del puerto.
struct Table(Mutex<HashMap<u64, std::sync::Arc<ChannelCore>>>);
unsafe impl Send for Table {}
unsafe impl Sync for Table {}

static REGISTRY: OnceLock<Table> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn registry() -> &'static Table {
    REGISTRY.get_or_init(|| Table(Mutex::new(HashMap::new())))
}

fn core_of(id: u64) -> Option<std::sync::Arc<ChannelCore>> {
    registry().0.lock().unwrap().get(&id).cloned()
}

/// Objeto `{value, done}` heap-independiente (mismo patrón que el error de
/// spawnIsolate en task.rs).
pub fn next_obj(value: Value, done: bool) -> Value {
    let mut obj = ObjData::new();
    obj.set_field(Rc::from("value"), value);
    obj.set_field(Rc::from("done"), Value::Bool(done));
    new_object(obj)
}

pub fn create(capacity: usize) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    registry().0.lock().unwrap().insert(
        id,
        std::sync::Arc::new(ChannelCore {
            capacity: capacity.max(1),
            state: Mutex::new(ChannelState::default()),
        }),
    );
    id
}

pub fn send(id: u64, val: SendValue) -> SendOutcome {
    let Some(core) = core_of(id) else { return SendOutcome::Closed };
    let mut st = core.state.lock().unwrap();
    if st.closed {
        return SendOutcome::Closed;
    }
    // Entrega directa a un receiver parkeado (la cola está vacía si hay waiters).
    if let Some(w) = st.recv_waiters.pop_front() {
        drop(st);
        w.resolve(next_obj(val.to_value(), false));
        return SendOutcome::Sent;
    }
    if st.queue.len() < core.capacity {
        st.queue.push_back(val);
        return SendOutcome::Sent;
    }
    let task = AsyncTask::pending();
    st.send_waiters.push_back((val, task.clone()));
    SendOutcome::Parked(task)
}

pub fn try_receive(id: u64) -> RecvOutcome {
    let Some(core) = core_of(id) else { return RecvOutcome::Closed };
    let mut st = core.state.lock().unwrap();
    if let Some(v) = st.queue.pop_front() {
        // liberó hueco: promover un send parkeado
        if let Some((pv, ptask)) = st.send_waiters.pop_front() {
            st.queue.push_back(pv);
            drop(st);
            ptask.resolve(Value::Bool(true));
        }
        return RecvOutcome::Item(v);
    }
    if st.closed {
        return RecvOutcome::Closed;
    }
    let task = AsyncTask::pending();
    st.recv_waiters.push_back(task.clone());
    RecvOutcome::Parked(task)
}

pub fn close(id: u64) {
    let Some(core) = core_of(id) else { return };
    let mut st = core.state.lock().unwrap();
    if st.closed {
        return;
    }
    st.closed = true;
    let recvs: Vec<AsyncTask> = st.recv_waiters.drain(..).collect();
    let sends: Vec<(SendValue, AsyncTask)> = st.send_waiters.drain(..).collect();
    drop(st);
    for w in recvs {
        w.resolve(next_obj(Value::Null, true));
    }
    for (_, w) in sends {
        // el valor parkeado NO entra a la cola: send(...) tras close = false
        w.resolve(Value::Bool(false));
    }
}
```

Nota para el test 5 (`close_wakes_parked...`): el valor del send parkeado rechazado no se encola — tras drenar la cola solo queda `Closed`. Ajustar la aserción final del test a `assert!(matches!(try_receive(id), RecvOutcome::Item(SendValue::Int(1)) | RecvOutcome::Closed))` según quede el orden de drenaje (el send de `Int(1)` resolvió al receiver parkeado, así que la cola queda vacía → `Closed`). Si `nv_to_value` no es visible desde el test, usa el helper público que exista en `varn_types::value` para leer campos de `ObjData` o simplifica `as_done` a un match sobre `Value::Object` con `.read().inner.get("done")`.

En `crates/varn-runtime/src/lib.rs` añadir `pub mod channel;`.

- [ ] **Step 4: Correr tests**

Run: `cargo test -p varn-runtime channel 2>&1 | tail -5`
Expected: `test result: ok. 6 passed`

- [ ] **Step 5: Commit**

```bash
git add crates/varn-runtime/src/channel.rs crates/varn-runtime/src/lib.rs
git commit -m ":sparkles: runtime: bounded mpmc channel table for isolates"
```

---

### Task 2: Endpoints transferibles en SendValue

**Files:**
- Modify: `crates/varn-types/src/value/sendable.rs`
- Test: `#[cfg(test)]` en el mismo archivo

**Interfaces:**
- Consumes: nada de Task 1 (solo tipos).
- Produces (usado por Task 3/4):
  - `SendValue::ChannelSender(u64)` y `SendValue::ChannelReceiver(u64)` — variantes nuevas.
  - `Value::to_sendable()` detecta instancias de clase `Sender`/`Receiver` (objeto con clase de ese nombre y campo `_chan: Int`) y emite la variante.
  - `SendValue::to_value()` para las variantes nuevas produce un objeto marcador heap-independiente `{ "__chanEndpoint": "tx"|"rx", "__chanId": int }` — el hook del VM (Task 3) lo convierte en instancia real al materializar en el heap del consumidor. `to_value_ctx` con ctx disponible la mintea directo (Task 3 implementa el helper de minteo; aquí solo el marcador).

- [ ] **Step 1: Test que falla**

```rust
#[cfg(test)]
mod channel_endpoint_tests {
    use super::*;

    #[test]
    fn endpoint_variants_roundtrip_marker() {
        let tx = SendValue::ChannelSender(42);
        let v = tx.to_value();
        let Value::Object(o) = &v else { panic!("marker must be object") };
        let guard = o.read();
        assert!(guard.inner.contains_key("__chanEndpoint"));
        assert!(guard.inner.contains_key("__chanId"));
    }
}
```

Run: `cargo test -p varn-types channel_endpoint 2>&1 | tail -3`
Expected: FAIL compilación (`ChannelSender` no existe).

- [ ] **Step 2: Implementación**

En el enum (línea ~7 de sendable.rs):

```rust
    Set(Vec<SendValue>),
    /// Endpoint de canal (id en varn_runtime::channel). Se transfiere por
    /// referencia: ambos lados comparten el mismo canal.
    ChannelSender(u64),
    ChannelReceiver(u64),
```

En `Value::to_sendable()`, en el brazo `Value::Object(obj)`, ANTES del walk genérico de campos:

```rust
            Value::Object(obj) => {
                {
                    let guard = obj.read();
                    if let Some(cls) = guard.class() {
                        let cname = cls.name();
                        if cname == "Sender" || cname == "Receiver" {
                            if let Some(Value::Int(id)) =
                                guard.inner.get("_chan").map(nv_to_value)
                            {
                                return Ok(if cname == "Sender" {
                                    SendValue::ChannelSender(id as u64)
                                } else {
                                    SendValue::ChannelReceiver(id as u64)
                                });
                            }
                            return Err(format!("{cname}: endpoint sin _chan"));
                        }
                    }
                }
                // …walk genérico existente…
```

(Verificar el accessor real del nombre de clase: `ClassObj` — grep `pub fn name` en `crates/varn-vm/src/exec/class.rs` o donde viva `ClassObj`; si es campo público usar `cls.name.as_ref()`. Si `class()` no existe en ese guard, mirar cómo lo lee `crates/varn-vm/src/exec/props.rs:255`.)

En `SendValue::to_value()` (mismo archivo, el inverso):

```rust
            SendValue::ChannelSender(id) => endpoint_marker("tx", *id),
            SendValue::ChannelReceiver(id) => endpoint_marker("rx", *id),
```

con:

```rust
fn endpoint_marker(dir: &str, id: u64) -> Value {
    let mut obj = ObjData::new();
    obj.set_field(std::rc::Rc::from("__chanEndpoint"), Value::Str(std::rc::Rc::from(dir)));
    obj.set_field(std::rc::Rc::from("__chanId"), Value::Int(id as i64));
    new_object(obj)
}
```

Si `to_value_ctx` tiene un match propio por variante, darle el mismo brazo marcador (el minteo real lo añade Task 3 en el VM).

- [ ] **Step 3: Correr tests**

Run: `cargo test -p varn-types channel_endpoint 2>&1 | tail -3` → PASS.
Run: `cargo check -p varn-vm 2>&1 | grep -c "^error"` → `0` (si el match de `to_value_ctx`/consumidores es exhaustivo y falla, añadir los brazos marcador donde el compilador lo pida).

- [ ] **Step 4: Commit**

```bash
git add crates/varn-types/src/value/sendable.rs
git commit -m ":sparkles: types: transferable channel endpoints in SendValue"
```

---

### Task 3: Contrato nuevo + impls + hook de materialización en await

La task más grande: canal end-to-end DENTRO de un isolate.

**Files:**
- Modify: `crates/varn-builtins/src/modules/std/task/runtime/task_runtime.vn` (contrato)
- Modify: `crates/varn-builtins/src/modules/std/task/task.rs` (impls nativas)
- Modify: `crates/varn-builtins/src/modules/std/task/task.vn` (facade std:task re-exporta)
- Modify: `crates/varn-vm/src/exec/ctx_tasks.rs` (hook await-resume)
- Create: `crates/varn-vm/src/exec/host_values.rs` (materialización de marcadores)
- Test: `tests/54-channels.vn` (nuevo; se registra en main.vn en Task 5)

**Interfaces:**
- Consumes: Task 1 (`varn_runtime::channel::{create, send, try_receive, close, next_obj, SendOutcome, RecvOutcome}`), Task 2 (variantes marcador).
- Produces (superficie Varn):
  - `runtime:task` exporta `channel<T>(capacity: int): Channel<T>`, `declare class Channel<T> { tx: Sender<T>; rx: Receiver<T>; }`, `Sender<T>.send/close/dispose`, `Receiver<T>.receive/next/dispose`, `ChannelClosed extends Error`.
  - `std:task` re-exporta todo lo anterior.
  - VM: cualquier task rechazada con objeto `{__hostErrorClass: str, message: str}` se lanza como instancia real de esa clase intrínseca; cualquier valor resuelto con marcador `__chanEndpoint` se mintea como instancia `Sender`/`Receiver`.

- [ ] **Step 1: Test .vn que falla**

Crear `tests/54-channels.vn`:

```vn
import { channel, ChannelClosed } from "std:task";

async function testBasics(): void {
    const ch = channel<int>(2)
    await ch.tx.send(1)
    await ch.tx.send(2)
    assert("ch recv 1", await ch.rx.receive() === 1)
    assert("ch recv 2", await ch.rx.receive() === 2)
}

async function testTypedClose(): void {
    const ch = channel<str>(1)
    ch.tx.close()
    let typed = false
    let sendClosed = false
    try {
        await ch.rx.receive()
    } catch (e) {
        typed = e instanceof ChannelClosed
    }
    try {
        await ch.tx.send("nope")
    } catch (e) {
        sendClosed = e instanceof ChannelClosed
    }
    assert("recv closed typed", typed)
    assert("send closed typed", sendClosed)
}

async function testForAwait(): void {
    const ch = channel<int>(4)
    await ch.tx.send(10)
    await ch.tx.send(20)
    ch.tx.close()
    let sum = 0
    for await (const v of ch.rx) {
        sum = sum + v
    }
    assert("for-await drains then ends", sum === 30)
}

async function testUsingDispose(): void {
    const ch = channel<int>(1)
    {
        using tx = ch.tx
        await tx.send(5)
    }
    // dispose cerró el canal: drena y termina
    assert("drained after dispose", await ch.rx.receive() === 5)
    let closed = false
    try { await ch.rx.receive() } catch (e) { closed = e instanceof ChannelClosed }
    assert("closed after dispose", closed)
}

async function main(): void {
    await testBasics()
    await testTypedClose()
    await testForAwait()
    await testUsingDispose()
    print("[PASSED] 54. Typed channels")
}

main()
```

Run: `./target/release/vn.exe run tests/54-channels.vn`
Expected: FAIL — `channel` no existe en std:task.

- [ ] **Step 2: Contrato `task_runtime.vn`**

Reemplazar el bloque `IsolatePort` (dejarlo intacto hasta Task 5; AÑADIR debajo):

```vn
export declare class ChannelClosed extends Error {
    constructor();
}

export declare class Sender<T = dynamic> {
    send(msg: T): Task<void>;
    close(): void;
    dispose(): void;
}

export declare class Receiver<T = dynamic> {
    receive(): Task<T>;
    next(): Task<{ value: T?, done: bool }>;
    dispose(): void;
}

export declare class Channel<T = dynamic> {
    tx: Sender<T>;
    rx: Receiver<T>;
}

export declare function channel<T = dynamic>(capacity: int): Channel<T>;
```

**Probe inmediato** (antes de implementar nada): `cargo check -p varn-builtins --features runtime` (regla del repo) y luego un `.vn` de dos líneas por `vn check` que haga `const ch = channel<int>(2)` y verifique con `vn debug -p lsp:hovers` que `ch.tx` tipa `Sender<int>`. Si el checker no instancia `T` a través de la clase `Channel<T>`, fallback documentado: `channel` devuelve `{ tx: Sender<T>, rx: Receiver<T> }` como objeto anónimo en la firma. Uno de los dos DEBE tipar; si ninguno, parar y reportar.

- [ ] **Step 3: Impls nativas en `task.rs`**

Dentro del `varn_contract!` de `TaskRuntime` añadir:

```rust
        fn channel(ctx: &mut dyn NativeCtx, capacity: i64) -> Result<VmValue, String> {
            if capacity < 1 {
                return Err("channel: capacity must be >= 1".to_string());
            }
            let id = varn_runtime::channel::create(capacity as usize);
            let ch_nv = ctx
                .alloc_instance("Channel")
                .ok_or("channel: Channel class not registered")?;
            let tx_nv = crate::modules::std::task::alloc_endpoint(ctx, "Sender", id)?;
            let rx_nv = crate::modules::std::task::alloc_endpoint(ctx, "Receiver", id)?;
            ctx.set_field(ch_nv, "tx", tx_nv);
            ctx.set_field(ch_nv, "rx", rx_nv);
            Ok(ch_nv)
        }
```

Helper compartido (en `task.rs`, fuera de los macros, `pub(crate)`):

```rust
pub(crate) fn alloc_endpoint(
    ctx: &mut dyn NativeCtx,
    class_name: &str,
    id: u64,
) -> Result<VmValue, String> {
    let nv = ctx
        .alloc_instance(class_name)
        .ok_or_else(|| format!("channel: {class_name} class not registered"))?;
    ctx.set_field(nv, "_chan", VmValue::from_int(id as i64));
    if class_name == "Receiver" {
        // for-await: Symbol.asyncIterator devuelve el propio receiver (self-iterator)
        let self_val = ctx.extract(nv);
        let iter_nv = ctx.intern(varn_types::Value::native_bound(
            self_val,
            receiver_self_iterator,
            "[Symbol.asyncIterator]",
        ));
        ctx.set_field(nv, "Symbol.asyncIterator", iter_nv);
    }
    Ok(nv)
}

fn receiver_self_iterator(
    _ctx: &mut dyn NativeCtx,
    args: &[VmValue],
) -> Result<VmValue, String> {
    args.first().copied().ok_or("receiver iterator: missing self".into())
}
```

(Firma exacta de `Value::native_bound`: copiar de `crates/varn-vm/src/exec/advanced.rs:190` — `Value::native_bound(extracted_value, fn, name)`. Si `native_bound` exige que la fn viva en varn-vm y no es constructible desde builtins, alternativa: registrar `next` como único protocolo y setear `"Symbol.asyncIterator"` con una función nativa del contrato `receiverIter(this)` declarada en el contrato como método — resolver en el probe del Step 2.)

Nuevo bloque contract-class para `Sender`/`Receiver`:

```rust
pub struct SenderImpl;
pub struct ReceiverImpl;
pub struct ChannelClosedImpl;

fn chan_id(ctx: &mut dyn NativeCtx, this: VmValue) -> Option<u64> {
    let nv = ctx.get_field(this, "_chan")?;
    match ctx.extract(nv) {
        Value::Int(i) => Some(i as u64),
        _ => None,
    }
}

fn closed_error_obj(msg: &str) -> Value {
    // Marcador heap-independiente; el hook del VM lo mintea como instancia
    // ChannelClosed en el heap del consumidor (funciona igual same-thread y
    // cross-thread).
    let mut obj = varn_types::value::ObjData::new();
    obj.set_field(std::rc::Rc::from("__hostErrorClass"), Value::Str(std::rc::Rc::from("ChannelClosed")));
    obj.set_field(std::rc::Rc::from("message"), Value::Str(std::rc::Rc::from(msg)));
    varn_types::value::new_object(obj)
}

varn_contract! {
    module: "runtime:task",
    class: "Sender",
    contract: "src/modules/std/task/runtime/task_runtime.vn",
    impl SenderImpl {
        fn send(ctx: &mut dyn NativeCtx, this: VmValue, msg: VmValue) -> VmValue {
            let out = varn_types::AsyncTask::pending();
            let Some(id) = chan_id(ctx, this) else {
                out.reject(closed_error_obj("channel closed"));
                return ctx.intern(Value::TaskHandle(out));
            };
            let send_val = match ctx.to_sendable(msg) {
                Ok(v) => v,
                Err(e) => {
                    out.reject_msg(format!("send: {e}"));
                    return ctx.intern(Value::TaskHandle(out));
                }
            };
            match varn_runtime::channel::send(id, send_val) {
                varn_runtime::channel::SendOutcome::Sent => out.resolve(Value::Null),
                varn_runtime::channel::SendOutcome::Closed => {
                    out.reject(closed_error_obj("channel closed"));
                }
                varn_runtime::channel::SendOutcome::Parked(task) => {
                    let out2 = out.clone();
                    task.on_settle(move |res| match res {
                        Ok(Value::Bool(true)) => out2.resolve(Value::Null),
                        _ => out2.reject(closed_error_obj("channel closed")),
                    });
                }
            }
            ctx.intern(Value::TaskHandle(out))
        }

        fn close(ctx: &mut dyn NativeCtx, this: VmValue) {
            if let Some(id) = chan_id(ctx, this) {
                varn_runtime::channel::close(id);
            }
        }

        fn dispose(ctx: &mut dyn NativeCtx, this: VmValue) {
            if let Some(id) = chan_id(ctx, this) {
                varn_runtime::channel::close(id);
            }
        }
    }
}

varn_contract! {
    module: "runtime:task",
    class: "Receiver",
    contract: "src/modules/std/task/runtime/task_runtime.vn",
    impl ReceiverImpl {
        fn next(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            let out = varn_types::AsyncTask::pending();
            let Some(id) = chan_id(ctx, this) else {
                out.resolve(varn_runtime::channel::next_obj(Value::Null, true));
                return ctx.intern(Value::TaskHandle(out));
            };
            match varn_runtime::channel::try_receive(id) {
                varn_runtime::channel::RecvOutcome::Item(v) => {
                    let val_nv = v.to_value_ctx(ctx);
                    let val = ctx.extract(val_nv);
                    out.resolve(varn_runtime::channel::next_obj(val, false));
                }
                varn_runtime::channel::RecvOutcome::Closed => {
                    out.resolve(varn_runtime::channel::next_obj(Value::Null, true));
                }
                varn_runtime::channel::RecvOutcome::Parked(task) => {
                    let out2 = out.clone();
                    task.on_settle(move |res| {
                        if let Ok(v) = res {
                            out2.resolve(v); // ya es {value, done} heap-independiente
                        }
                    });
                }
            }
            ctx.intern(Value::TaskHandle(out))
        }

        fn receive(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            let out = varn_types::AsyncTask::pending();
            let Some(id) = chan_id(ctx, this) else {
                out.reject(closed_error_obj("channel closed"));
                return ctx.intern(Value::TaskHandle(out));
            };
            match varn_runtime::channel::try_receive(id) {
                varn_runtime::channel::RecvOutcome::Item(v) => {
                    let val_nv = v.to_value_ctx(ctx);
                    let val = ctx.extract(val_nv);
                    out.resolve(val);
                }
                varn_runtime::channel::RecvOutcome::Closed => {
                    out.reject(closed_error_obj("channel closed"));
                }
                varn_runtime::channel::RecvOutcome::Parked(task) => {
                    let out2 = out.clone();
                    task.on_settle(move |res| match res {
                        Ok(v) => {
                            // v = {value, done}: done=true → cierre tipado
                            let done = matches!(
                                &v,
                                Value::Object(o) if matches!(
                                    o.read().inner.get("done").map(varn_types::value::nv_to_value),
                                    Some(Value::Bool(true))
                                )
                            );
                            if done {
                                out2.reject(closed_error_obj("channel closed"));
                            } else if let Value::Object(o) = &v {
                                let inner_val = o
                                    .read()
                                    .inner
                                    .get("value")
                                    .map(varn_types::value::nv_to_value)
                                    .unwrap_or(Value::Null);
                                out2.resolve(inner_val);
                            }
                        }
                        Err(e) => out2.reject(e),
                    });
                }
            }
            ctx.intern(Value::TaskHandle(out))
        }

        fn dispose(ctx: &mut dyn NativeCtx, this: VmValue) {
            if let Some(id) = chan_id(ctx, this) {
                varn_runtime::channel::close(id);
            }
        }
    }
}
```

(`nv_to_value` ya se usa en sendable.rs; verificar su path público en varn-types y ajustar el import. `ChannelClosed` no necesita impl de métodos: clase declare vacía con constructor default — si `varn_contract!` exige bloque impl para toda clase del contrato, darle `impl ChannelClosedImpl {}` vacío o registrarla sin macro según haga el contrato con clases sin métodos nativos.)

- [ ] **Step 4: Hook de materialización en el VM**

Crear `crates/varn-vm/src/exec/host_values.rs`:

```rust
//! Materializa en el heap del consumidor los valores-marcador que los
//! builtins producen cross-thread (donde no hay ctx del heap destino):
//! - `{__hostErrorClass, message}` → instancia real de esa clase intrínseca
//!   (instanceof funciona en el catch).
//! - `{__chanEndpoint: "tx"|"rx", __chanId}` → instancia Sender/Receiver.
//! Punto único: await-resume (ctx_tasks).

use varn_types::Value;

use super::ctx::ExecCtx;
use crate::value::VmValue;

fn marker_str(val: &Value, key: &str) -> Option<String> {
    if let Value::Object(o) = val {
        if let Some(Value::Str(s)) = o.read().inner.get(key).map(varn_types::value::nv_to_value) {
            return Some(s.to_string());
        }
    }
    None
}

fn marker_int(val: &Value, key: &str) -> Option<i64> {
    if let Value::Object(o) = val {
        if let Some(Value::Int(i)) = o.read().inner.get(key).map(varn_types::value::nv_to_value) {
            return Some(i);
        }
    }
    None
}

/// Valores resueltos: convertir endpoints-marcador en instancias.
pub fn open_resolved(ctx: &mut ExecCtx, val: Value) -> Value {
    if let Some(dir) = marker_str(&val, "__chanEndpoint") {
        if let Some(id) = marker_int(&val, "__chanId") {
            let class_name = if dir == "tx" { "Sender" } else { "Receiver" };
            if let Some(nv) = mint_endpoint(ctx, class_name, id as u64) {
                return ctx.heap.extract(nv);
            }
        }
    }
    val
}

/// Valores rechazados: convertir errores-marcador en instancias tipadas.
pub fn open_rejected(ctx: &mut ExecCtx, val: Value) -> Value {
    if let Some(class_name) = marker_str(&val, "__hostErrorClass") {
        if let Some(cls) = ctx.get_class(&class_name) {
            let inst_nv = ctx.heap.alloc_object();
            if let Some(crate::heap::HeapObj::Object(o)) = ctx.heap.get_mut(inst_nv.as_heap_idx()) {
                o.borrow_mut().set_class(cls);
            }
            let msg = marker_str(&val, "message").unwrap_or_default();
            let msg_nv = ctx.heap.intern(Value::Str(std::rc::Rc::from(msg.as_str())));
            if let Some(crate::heap::HeapObj::Object(o)) = ctx.heap.get_mut(inst_nv.as_heap_idx()) {
                o.borrow_mut().set_field_nv(std::rc::Rc::from("message"), msg_nv);
                let name_nv = ctx.heap.intern(Value::Str(std::rc::Rc::from(class_name.as_str())));
                o.borrow_mut().set_field_nv(std::rc::Rc::from("name"), name_nv);
            }
            return ctx.heap.extract(inst_nv);
        }
    }
    val
}

fn mint_endpoint(ctx: &mut ExecCtx, class_name: &str, id: u64) -> Option<VmValue> {
    // mismo trabajo que alloc_endpoint de builtins, pero con ExecCtx
    use varn_types::NativeCtx;
    let nv = ctx.alloc_instance(class_name)?;
    ctx.set_field(nv, "_chan", VmValue::from_int(id as i64));
    // Symbol.asyncIterator igual que en builtins (self-iterator); ver Task 3 Step 3
    Some(nv)
}
```

Registrar `pub mod host_values;` en `crates/varn-vm/src/exec/mod.rs` (junto a `advanced`). En `ctx_tasks.rs`, dentro del match del await (líneas ~128 en adelante), envolver:

```rust
                        match res_val {
                            Ok(resolved) => {
                                let resolved = crate::exec::host_values::open_resolved(&mut fork, resolved);
                                let resolved_nv = fork.heap.intern(resolved);
                                // …resto igual…
                            }
                            Err(v) => {
                                let v = crate::exec::host_values::open_rejected(&mut fork, v);
                                // …resto del camino de throw igual…
                            }
                        }
```

Buscar el/los otros puntos de await-resume (hay camino JIT/scheduler): `grep -rn "VmSuspend::Await" crates/varn-vm/src --include="*.rs"` y aplicar los dos `open_*` en cada sitio donde el valor de un task se entrega al código Varn. El `Symbol.asyncIterator` en `mint_endpoint`: replicar exactamente lo que quedó en `alloc_endpoint` (si el probe del Step 3 eligió otra vía, copiarla aquí — misma lógica en ambos sitios, extraer helper si es posible sin ciclos de dependencia varn-vm↔varn-builtins; si builtins no puede compartirlo, duplicar con comentario cruzado).

- [ ] **Step 5: Facade `task.vn`**

```vn
import { taskSpawn, taskSleep, taskParallel, channel as nativeChannel,
         Sender, Receiver, Channel, ChannelClosed,
         IsolatePort, spawnIsolate as nativeSpawnIsolate } from "runtime:task";

export function channel<T = dynamic>(capacity: int): Channel<T> {
    return nativeChannel<T>(capacity);
}

export { Sender, Receiver, Channel, ChannelClosed };
```

(Sintaxis exacta de re-export: copiar el patrón que ya use la stdlib — grep `export {` en `std/*.vn`; si no existe re-export directo, wrappear como hace `spawnIsolate` hoy.)

- [ ] **Step 6: Build + validar test**

```bash
cargo build --release -p varn-cli &&
cargo xtask build-std &&
cp std.vnb target/release/std.vnb && cp std.vnb target/debug/std.vnb &&
./target/release/vn.exe cache clean &&
./target/release/vn.exe run tests/54-channels.vn
```

Expected: `[PASSED] 54. Typed channels`

También: `./target/release/vn.exe check tests/54-channels.vn` → exit 0, y hover probe:
`./target/release/vn.exe debug -p lsp:hovers tests/54-channels.vn 2>&1 | grep "ch "` debe mostrar `Channel<int>`, no `dynamic`.

- [ ] **Step 7: Commit**

```bash
git add crates/varn-builtins/src/modules/std/task/ crates/varn-vm/src/exec/host_values.rs \
        crates/varn-vm/src/exec/mod.rs crates/varn-vm/src/exec/ctx_tasks.rs tests/54-channels.vn
git commit -m ":sparkles: channels: typed Sender/Receiver + ChannelClosed + for-await (same-isolate)"
```

---

### Task 4: spawnIsolate nuevo + transferencia cross-isolate

**Files:**
- Modify: `crates/varn-builtins/src/modules/std/task/runtime/task_runtime.vn` (spawnIsolate → IsolateHandle)
- Modify: `crates/varn-builtins/src/modules/std/task/task.rs` (spawnIsolate impl)
- Modify: `crates/varn-vm/src/exec/frame_ctrl.rs:583-692` (`spawn_isolate`: sin puerto inyectado, con resultado joineable)
- Modify: `crates/varn-types/src/native_ctx.rs:145` (firma del trait `spawn_isolate`)
- Test: extender `tests/54-channels.vn`

**Interfaces:**
- Consumes: Tasks 1-3 completas.
- Produces:
  - contrato: `declare class IsolateHandle { join(): Task<void>; }`, `spawnIsolate(fn: dynamic, args: Array<dynamic>): Task<IsolateHandle>`
  - trait: `fn spawn_isolate(&mut self, module_path: &str, export_name: &str, args: Vec<SendValue>) -> Result<varn_types::AsyncTask, String>` — la AsyncTask resuelve `Null` al terminar el worker o rechaza `{__hostErrorClass:"Error", message}` si el worker lanzó.

- [ ] **Step 1: Test que falla (extender tests/54-channels.vn)**

```vn
export async function echoWorker(rx: Receiver<int>, tx: Sender<int>) {
    for await (const v of rx) {
        await tx.send(v * 2)
    }
    tx.close()
}

async function testCrossIsolate(): void {
    const a = channel<int>(4)
    const b = channel<int>(4)
    const handle = await spawnIsolate(echoWorker, [a.rx, b.tx])
    await a.tx.send(21)
    assert("cross-isolate echo", await b.rx.receive() === 42)
    a.tx.close()
    await handle.join()
    let done = false
    try { await b.rx.receive() } catch (e) { done = e instanceof ChannelClosed }
    assert("worker closed reply channel", done)
}
```

Añadir import de `spawnIsolate, Sender, Receiver` y `await testCrossIsolate()` en `main()`. Run → FAIL (spawnIsolate devuelve IsolatePort viejo / firma del worker no calza).

- [ ] **Step 2: Contrato**

En `task_runtime.vn` (reemplaza la línea vieja de spawnIsolate):

```vn
export declare class IsolateHandle {
    join(): Task<void>;
}

export declare function spawnIsolate(fn: dynamic, args: Array<dynamic>): Task<IsolateHandle>;
```

Facade `task.vn`: cambiar el tipo de retorno del wrapper a `Task<IsolateHandle>` y re-exportar `IsolateHandle`.

- [ ] **Step 3: Trait + VM**

`native_ctx.rs`: firma nueva (quitar el parámetro `port`):

```rust
    fn spawn_isolate(
        &mut self,
        _module_path: &str,
        _export_name: &str,
        _args: Vec<SendValue>,
    ) -> Result<varn_types::AsyncTask, String> {
        Err("spawn_isolate: unsupported in this context".into())
    }
```

`frame_ctrl.rs` `spawn_isolate`: quitar toda la construcción del IsolatePort del hilo hijo (las ~25 líneas de `get_class("IsolatePort")` hasta `set_field_nv("_port", …)`), los args ya no llevan puerto:

```rust
            let mut vm_args = Vec::new();
            for arg in args {
                let v_nv = arg.to_value_ctx(&mut machine.ctx);
                vm_args.push(v_nv);
            }
```

y al final del closure del thread, en lugar de solo loguear, resolver/rechazar la task de join (declarada antes del `thread::spawn`):

```rust
        let done = varn_types::AsyncTask::pending();
        let done_t = done.clone();
        std::thread::spawn(move || {
            // …setup igual…
            match machine.ctx.call_vm(func_nv, &vm_args) {
                Ok(res) => {
                    let val = machine.ctx.heap.extract(res);
                    if let varn_types::Value::Task(lazy) = val {
                        let handle = machine.ctx.run_lazy_task_sync(lazy.as_ref());
                        if let varn_types::task::TaskState::Rejected(e) = handle.peek_state() {
                            done_t.reject(worker_error(&format!("{e}")));
                            return;
                        }
                    }
                    done_t.resolve(varn_types::Value::Null);
                }
                Err(e) => done_t.reject(worker_error(&format!("{e}"))),
            }
        });
        Ok(done)
```

con helper local:

```rust
fn worker_error(msg: &str) -> varn_types::Value {
    let mut obj = varn_types::value::ObjData::new();
    obj.set_field(std::rc::Rc::from("__hostErrorClass"), varn_types::Value::Str(std::rc::Rc::from("Error")));
    obj.set_field(std::rc::Rc::from("message"), varn_types::Value::Str(std::rc::Rc::from(msg)));
    varn_types::value::new_object(obj)
}
```

Los paths de error tempranos del thread (module load fail, export not found) también `done_t.reject(worker_error(...))` en vez de solo loguear.

**Nota `to_value_ctx` de endpoints en el hijo:** los args llegan como `SendValue::ChannelSender/Receiver`; `to_value_ctx` debe mintear la instancia real (el hijo tiene ctx). Si Task 2 dejó solo el marcador en `to_value_ctx`, moverlo aquí: en `to_value_ctx` (donde tenga acceso a `dyn NativeCtx`) mintear via `alloc_instance` + `_chan` + `Symbol.asyncIterator` — misma lógica que `alloc_endpoint`; el worker carga `std:task` antes (ya lo hace `spawn_isolate`), así que las clases existen.

- [ ] **Step 4: Impl builtins `spawnIsolate`**

En `task.rs`, el cuerpo actual de `spawnIsolate` cambia el final (desde `let (port_parent, port_child) = …` hasta el `Ok(instance_nv)`):

```rust
            let done = ctx.spawn_isolate(&resolved_path, &export_name, worker_args)?;
            let handle_nv = ctx
                .alloc_instance("IsolateHandle")
                .ok_or("spawnIsolate: IsolateHandle class not registered")?;
            let done_nv = ctx.intern(Value::TaskHandle(done));
            ctx.set_field(handle_nv, "_done", done_nv);
            Ok(handle_nv)
```

y bloque contract-class:

```rust
pub struct IsolateHandleImpl;

varn_contract! {
    module: "runtime:task",
    class: "IsolateHandle",
    contract: "src/modules/std/task/runtime/task_runtime.vn",
    impl IsolateHandleImpl {
        fn join(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            ctx.get_field(this, "_done").unwrap_or(VmValue::null())
        }
    }
}
```

Los rejects existentes de spawnIsolate (módulo no cargable, closure anónima) se dejan como están en esta task (objetos `{message}`) — el test 47 ya matchea por `message`.

- [ ] **Step 5: Build + validar**

Mismo pipeline de build/bundle/cache que Task 3 Step 6, luego:
`./target/release/vn.exe run tests/54-channels.vn` → `[PASSED] 54. Typed channels`.

- [ ] **Step 6: Commit**

```bash
git add crates/varn-builtins/src/modules/std/task/ crates/varn-vm/src/exec/frame_ctrl.rs \
        crates/varn-types/src/native_ctx.rs crates/varn-types/src/value/sendable.rs tests/54-channels.vn
git commit -m ":sparkles: channels: cross-isolate endpoint transfer + IsolateHandle.join"
```

---

### Task 5: Migración, purga de IsolatePort, docs, validación total

**Files:**
- Rewrite: `tests/47-isolates-multithread.vn`
- Modify: `tests/main.vn` (re-habilitar 47, añadir 54)
- Modify: `crates/varn-builtins/src/modules/std/task/runtime/task_runtime.vn` (borrar `IsolatePort`)
- Modify: `crates/varn-builtins/src/modules/std/task/task.rs` (borrar `IsolatePortImpl` y su bloque)
- Delete-content: `crates/varn-runtime/src/isolate.rs` (borrar `IsolatePort`; si el archivo queda vacío, eliminarlo y quitar `pub mod isolate;` del lib.rs)
- Modify: `crates/varn-builtins/src/modules/std/task/task.vn` (quitar import/re-export de IsolatePort)
- Create: `tests/errors/invalid-channel-send-type.vn`
- Modify: `docs/WARP-SPEC.md` (sección isolates/channels), `docs/RUNTIME_ARCHITECTURE.md` (sección isolates)

**Interfaces:**
- Consumes: todo lo anterior.
- Produces: árbol sin ninguna referencia a `IsolatePort` (`grep -rn "IsolatePort" crates tests std docs --include="*.rs" --include="*.vn"` → solo specs/plans históricos).

- [ ] **Step 1: Reescribir `tests/47-isolates-multithread.vn`**

Cobertura equivalente + nueva. Esqueleto completo:

```vn
import { spawnIsolate, sleep, channel, Sender, Receiver, ChannelClosed } from "std:task";

enum Msg { Val(int), Exit }

// 1. ping-pong básico
export async function workerMain(rx: Receiver<{ value: int }>, tx: Sender<int>) {
    const data = await rx.receive();
    assert("msg in child", data.value === 10);
    await tx.send(data.value * 2);
}

// 2. args múltiples de inicio (sin canal de entrada)
export async function workerArgs(tx: Sender<str>, num: int, text: str, arr: Array<int>, obj: { x: str }) {
    assert("num arg", num === 42);
    assert("text arg", text === "hello");
    assert("arr arg length", arr.length === 3);
    assert("arr arg item", arr[1] === 2);
    assert("obj arg item", obj.x === "nested");
    await tx.send("args_ok");
}

// 3. diálogo continuo con protocolo enum y cierre real (sin centinela string)
export async function workerDialog(rx: Receiver<Msg>, tx: Sender<int>) {
    for await (const msg of rx) {
        match msg {
            Val(n) => await tx.send(n + 1),
            Exit   => break,
        }
    }
    tx.close()
}

// 4. paralelo
export async function workerParallel(tx: Sender<int>, id: int, delayMs: int) {
    await sleep(delayMs);
    await tx.send(id * 10);
}

function localUnexported(tx: Sender<str>) {}

async function testIsolates() {
    // ---- 1 ----
    const in1 = channel<{ value: int }>(1)
    const out1 = channel<int>(1)
    const h1 = await spawnIsolate(workerMain, [in1.rx, out1.tx])
    await in1.tx.send({ value: 10 })
    assert("response from basic worker", await out1.rx.receive() === 20)
    await h1.join()

    // ---- 2 ----
    const out2 = channel<str>(1)
    await spawnIsolate(workerArgs, [out2.tx, 42, "hello", [1, 2, 3], { x: "nested" }])
    assert("response from args worker", await out2.rx.receive() === "args_ok")

    // ---- 3 ----
    const in3 = channel<Msg>(2)
    const out3 = channel<int>(2)
    const h3 = await spawnIsolate(workerDialog, [in3.rx, out3.tx])
    await in3.tx.send(Msg.Val(1))
    assert("dialog 1", await out3.rx.receive() === 2)
    await in3.tx.send(Msg.Val(100))
    assert("dialog 2", await out3.rx.receive() === 101)
    await in3.tx.send(Msg.Exit)
    await h3.join()
    let dialogClosed = false
    try { await out3.rx.receive() } catch (e) { dialogClosed = e instanceof ChannelClosed }
    assert("dialog closed", dialogClosed)

    // ---- 4 ----
    const c1 = channel<int>(1)
    const c2 = channel<int>(1)
    const c3 = channel<int>(1)
    await spawnIsolate(workerParallel, [c1.tx, 1, 30])
    await spawnIsolate(workerParallel, [c2.tx, 2, 0])
    await spawnIsolate(workerParallel, [c3.tx, 3, 10])
    assert("w2 result", await c2.rx.receive() === 20)
    assert("w3 result", await c3.rx.receive() === 30)
    assert("w1 result", await c1.rx.receive() === 10)

    // ---- 5/6: errores de spawn (igual que antes) ----
    let threwAnon = false
    try {
        await spawnIsolate((tx: Sender<str>) => {}, [])
    } catch (e) {
        threwAnon = true
        assert("error message for anon function", e.message.indexOf("must be a function reference") >= 0)
    }
    assert("should throw for anon function", threwAnon)

    let threwUnexported = false
    try {
        await spawnIsolate(localUnexported, [])
    } catch (e) {
        threwUnexported = true
    }
    assert("should throw for unexported function", threwUnexported)

    print("[PASSED] 47. Isolates + typed channels")
}

testIsolates()
```

(Detalles de espera: si el runner necesita mantener vivo el main hasta terminar tasks, conservar el patrón que tenga hoy el final del 47 actual — mirar las últimas líneas antes de reescribir. Los casos 5/6 rechazan con `{message}` plano: bajo `catch (e): Error` el checker acepta `e.message`.)

- [ ] **Step 2: `tests/main.vn`**

Descomentar `import "./47-isolates-multithread.vn"` y añadir `import "./54-channels.vn"` tras el 53. Actualizar el print final de conteo (`54 side-effect modules` → el número real).

- [ ] **Step 3: Fixture de error de tipos**

`tests/errors/invalid-channel-send-type.vn`:

```vn
// expect: error[Varn3001]
import { channel } from "std:task";

async function main(): void {
    const ch = channel<int>(1)
    await ch.tx.send("not an int")
}
main()
```

Run: `./target/release/vn.exe check tests/errors/invalid-channel-send-type.vn` → exit != 0 con `error[WR3001]`.

- [ ] **Step 4: Purga IsolatePort**

- `task_runtime.vn`: borrar `export declare class IsolatePort {...}`.
- `task.rs`: borrar `IsolatePortImpl`, su `varn_contract!` y el struct.
- `isolate.rs`: borrar `IsolatePort` completo; si queda vacío, `git rm` + quitar `pub mod isolate;`.
- `task.vn`: quitar `IsolatePort` del import.
- Verificar: `grep -rn "IsolatePort" crates tests std --include="*.rs" --include="*.vn"` → 0 resultados.
- **Bump `HOST_API_VERSION`** (breaking en `runtime:task`): `grep -rn "HOST_API_VERSION" crates/varn-core/src` → incrementar la constante en 1 y actualizar `"hostApi"` en `std/std.json` al mismo valor (el bundle rechaza mismatch — spec stdlib §3).

- [ ] **Step 5: Docs**

- `docs/WARP-SPEC.md`: añadir subsección de channels tras try/catch o donde viva concurrencia (ejemplo corto: channel + spawnIsolate + for await + ChannelClosed).
- `docs/RUNTIME_ARCHITECTURE.md`: reemplazar la descripción de IsolatePort por la tabla de canales (id → cola bounded, waiters, materialización en await-resume).

- [ ] **Step 6: Validación total**

```bash
cargo build --release -p varn-cli -p varn-lsp &&
cargo build -p varn-cli -p varn-lsp &&
cargo xtask build-std &&
cp std.vnb target/release/std.vnb && cp std.vnb target/debug/std.vnb &&
./target/release/vn.exe cache clean &&
cargo test -p varn-runtime -p varn-types -p varn-lsp &&
./target/release/vn.exe run tests/main.vn
```

Expected: suite completa `ALL TESTS PASSED` (base previa: 674 + los asserts nuevos de 47/54).

Extra: `pwsh -NoProfile -File scripts/find-dynamic-inference.ps1 -InferredOnly -Path tests/47-isolates-multithread.vn` → 0 inferred (los 12 dynamics de isolates muertos).

- [ ] **Step 7: Commit**

```bash
git add tests/47-isolates-multithread.vn tests/54-channels.vn tests/main.vn tests/errors/invalid-channel-send-type.vn \
        crates/varn-builtins/src/modules/std/task/ crates/varn-runtime/src/ docs/WARP-SPEC.md docs/RUNTIME_ARCHITECTURE.md
git commit -m ":recycle: isolates: remove IsolatePort, typed channels are the only messaging API"
```
