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
    let Some(core) = core_of(id) else { return SendOutcome::Closed };
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

#[cfg(test)]
mod tests {
    use super::*;
    use varn_types::value::nv_to_value;
    use varn_types::Value;

    fn as_done(v: &Value) -> Option<bool> {
        if let Value::Object(o) = v {
            if let Some(Value::Bool(b)) = o.read().get("done").map(nv_to_value) {
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

    // NOTA (deviation): el test original del brief parkeaba un receiver y luego
    // intentaba parkear un sender en el MISMO canal. Es estructuralmente
    // imposible: `send` con `recv_waiters` no vacío siempre hace hand-off directo
    // (nunca toca `queue`/capacity), así que tras resolver el receiver la cola
    // queda vacía y el siguiente `send` en un canal de capacidad 1 vuelve a caber
    // (`Sent`, no `Parked`) — confirmado corriendo el test tal cual contra la
    // implementación de referencia del brief (falla con "must panic" en la
    // desestructuración `SendOutcome::Parked`). El invariante de la
    // implementación garantiza que `recv_waiters` y `send_waiters` nunca están
    // simultáneamente no vacíos para el mismo canal, así que se separan las dos
    // propiedades (receiver parkeado -> done:true, sender parkeado -> false) en
    // dos canales distintos, preservando exactamente lo que el nombre del test
    // afirma.
    #[test]
    fn close_wakes_parked_receivers_done_true_and_parked_senders_false() {
        // Receiver parkeado: close() debe resolverlo a {value: null, done: true}.
        let rid = create(1);
        let RecvOutcome::Parked(rtask) = try_receive(rid) else { panic!() };
        close(rid);
        match rtask.peek_state() {
            varn_types::task::TaskState::Resolved(v) => {
                assert_eq!(as_done(&v), Some(true), "parked recv on close must resolve done=true");
            }
            s => panic!("parked recv on close must resolve, got {s:?}"),
        }

        // Sender parkeado (canal lleno): close() debe resolverlo a false y NO
        // encolar su valor.
        let sid = create(1);
        assert!(matches!(send(sid, SendValue::Int(1)), SendOutcome::Sent));
        let SendOutcome::Parked(stask) = send(sid, SendValue::Int(2)) else { panic!("must park") };
        close(sid);
        match stask.peek_state() {
            varn_types::task::TaskState::Resolved(Value::Bool(false)) => {}
            s => panic!("parked send on close must resolve false, got {s:?}"),
        }
        // el valor parkeado (2) NO se encoló: tras drenar el único Int(1) que
        // había en cola, solo queda Closed.
        let RecvOutcome::Item(SendValue::Int(1)) = try_receive(sid) else { panic!("drain") };
        assert!(matches!(try_receive(sid), RecvOutcome::Closed));
    }

    #[test]
    fn parked_receiver_gets_scalar_as_value_done_object() {
        // Scalar payloads keep the plain `{value, done:false}` handoff.
        let id = create(1);
        let RecvOutcome::Parked(task) = try_receive(id) else { panic!("must park") };
        assert!(matches!(send(id, SendValue::Int(42)), SendOutcome::Sent));
        match task.peek_state() {
            varn_types::task::TaskState::Resolved(v) => {
                assert_eq!(as_done(&v), Some(false));
                if let Value::Object(o) = &v {
                    assert!(matches!(
                        o.read().get("value").map(nv_to_value),
                        Some(Value::Int(42))
                    ));
                } else {
                    panic!("scalar handoff must be a {{value,done}} object");
                }
            }
            s => panic!("parked recv should resolve, got {s:?}"),
        }
    }

    #[test]
    fn parked_receiver_gets_composite_as_send_envelope() {
        // Non-scalar payloads are carried heap-independently for consumer-side
        // materialization — never allocated on the producer's GC heap.
        use varn_types::value::SendEnvelope;
        let id = create(1);
        let RecvOutcome::Parked(task) = try_receive(id) else { panic!("must park") };
        let payload = SendValue::Array(vec![SendValue::Int(1), SendValue::Int(2)]);
        assert!(matches!(send(id, payload), SendOutcome::Sent));
        match task.peek_state() {
            varn_types::task::TaskState::Resolved(v) => {
                let env = SendEnvelope::from_value(&v).expect("composite must be a SendEnvelope");
                assert!(!env.wrap, "channel delivers bare (wrap=false)");
                assert!(matches!(&env.sv, SendValue::Array(items) if items.len() == 2));
            }
            s => panic!("parked recv should resolve, got {s:?}"),
        }
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
