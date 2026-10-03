use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use varn_types::value::SendValue;
use varn_types::HostPromise;

pub enum SendOutcome {
    Sent,
    Parked(HostPromise),
    Closed,
}

pub enum RecvOutcome {
    Item(SendValue),
    Parked(HostPromise),
    Closed,
}

#[derive(Default)]
struct ChannelState {
    queue: VecDeque<SendValue>,
    closed: bool,
    recv_waiters: VecDeque<HostPromise>,
    send_waiters: VecDeque<(SendValue, HostPromise)>,
}

struct ChannelCore {
    capacity: usize,
    state: Mutex<ChannelState>,
}

struct Table(Mutex<HashMap<u64, std::sync::Arc<ChannelCore>>>);

static REGISTRY: OnceLock<Table> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn registry() -> &'static Table {
    REGISTRY.get_or_init(|| Table(Mutex::new(HashMap::new())))
}

fn core_of(id: u64) -> Option<std::sync::Arc<ChannelCore>> {
    registry().0.lock().unwrap().get(&id).cloned()
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
        w.resolve(val);
        return SendOutcome::Sent;
    }
    if st.queue.len() < core.capacity {
        st.queue.push_back(val);
        return SendOutcome::Sent;
    }
    let task = HostPromise::pending();
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
            ptask.resolve(SendValue::Bool(true));
        }
        return RecvOutcome::Item(v);
    }
    if st.closed {
        return RecvOutcome::Closed;
    }
    let task = HostPromise::pending();
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
    let recvs: Vec<HostPromise> = st.recv_waiters.drain(..).collect();
    let sends: Vec<(SendValue, HostPromise)> = st.send_waiters.drain(..).collect();
    drop(st);
    for w in recvs {
        w.reject(SendValue::Null);
    }
    for (_, w) in sends {
        // el valor parkeado NO entra a la cola: send(...) tras close = false
        w.resolve(SendValue::Bool(false));
    }
}
