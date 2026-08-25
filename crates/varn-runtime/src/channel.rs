//! Bounded mpmc channels compartidos process-wide entre isolates.
//!
//! Los endpoints Varn (`Sender<T>`/`Receiver<T>`, contrato runtime:task) guardan
//! solo el `u64` de esta tabla. Todo valor que se entrega desde otro thread es
//! heap-independiente (`SendValue` / `ObjData`). Valores compuestos (Array/Object/Map/Set)
//! entregados por direct handoff al receiver se convierten en el thread del sender;
//! la materialización en el heap del consumidor vive en el hook de await-resume
//! del VM (`host_values::open_resolved`).

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use varn_types::task::AsyncTask;
use varn_types::value::{new_object, value_to_nv, ObjRef, SendEnvelope, SendValue};
use varn_types::{Value, VmValue};

/// Direct/parked receiver handoff value. Scalar payloads ride inside a plain
/// `{value, done:false}` object (heap-independent, embeddable by
/// `ObjData::set_field`); non-scalars are wrapped in a [`SendEnvelope`] so the
/// producing thread never allocates on the consumer's GC heap — the consumer's
/// await-resume hook materializes them. See `SendValue::is_direct_scalar`.
fn deliver_direct(val: SendValue) -> Value {
    if val.is_direct_scalar() {
        next_obj(val.to_value(), false)
    } else {
        SendEnvelope::deliver(val)
    }
}

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

// AsyncTask viaja entre threads: solo se toca bajo el Mutex de state y
// resolve() despierta al consumidor parked en el otro thread.
struct Table(Mutex<HashMap<u64, std::sync::Arc<ChannelCore>>>);

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
    new_object(ObjRef::from_pairs([
        (Rc::from("value"), value_to_nv(&value)),
        (Rc::from("done"), VmValue::from_bool(done)),
    ]))
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
    let Some(core) = core_of(id) else {
        return SendOutcome::Closed;
    };
    let mut st = core.state.lock().unwrap();
    if st.closed {
        return SendOutcome::Closed;
    }
    // Entrega directa a un receiver parkeado (la cola está vacía si hay waiters).
    if let Some(w) = st.recv_waiters.pop_front() {
        drop(st);
        w.resolve(deliver_direct(val));
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
    let Some(core) = core_of(id) else {
        return RecvOutcome::Closed;
    };
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
