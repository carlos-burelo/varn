use mio::net::{TcpListener, TcpStream, UdpSocket as MioUdpSocket};
use mio::{Events, Interest, Poll, Token, Waker};
use rustc_hash::FxHashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use varn_types::{value::SendValue, HostPromise};

const WAKER_TOKEN: Token = Token(usize::MAX);
static NEXT_SOCKET_ID: AtomicI64 = AtomicI64::new(1);

pub fn next_socket_id() -> i64 {
    NEXT_SOCKET_ID.fetch_add(1, Ordering::SeqCst)
}

fn udp_packet_value(buf: &[u8], src: SocketAddr) -> SendValue {
    SendValue::Array(vec![
        SendValue::Bytes(buf.to_vec()),
        SendValue::Str(src.ip().to_string()),
        SendValue::Int(src.port() as i64),
    ])
}

struct PendingRead {
    len: usize,
    task: HostPromise,
}

struct PendingWrite {
    data: Vec<u8>,
    written: usize,
    task: HostPromise,
}

struct StreamState {
    stream: TcpStream,
    is_connecting: bool,
    pending_connect: Option<HostPromise>,
    pending_read: Option<PendingRead>,
    pending_write: Option<PendingWrite>,
}

struct ListenerState {
    listener: TcpListener,
    pending_accepts: Vec<HostPromise>,
}

struct PendingUdpRecv {
    max_len: usize,
    task: HostPromise,
}

struct UdpState {
    socket: MioUdpSocket,
    pending_recvs: std::collections::VecDeque<PendingUdpRecv>,
}

enum DriverCommand {
    RegisterListener(i64),
    RegisterStream(i64),
    RegisterUdp(i64),
    DeregisterListener(TcpListener),
    DeregisterStream(TcpStream),
    DeregisterUdp(MioUdpSocket),
    Wake,
}

struct IoRegistry {
    listeners: FxHashMap<i64, ListenerState>,
    streams: FxHashMap<i64, StreamState>,
    udps: FxHashMap<i64, UdpState>,
}

pub struct IoDriver {
    registry: Arc<Mutex<IoRegistry>>,
    cmd_tx: Sender<DriverCommand>,
    waker: Arc<Waker>,
}

static DRIVER: OnceLock<IoDriver> = OnceLock::new();

pub fn driver() -> &'static IoDriver {
    DRIVER.get_or_init(|| IoDriver::new().expect("Failed to initialize mio IoDriver"))
}

fn wake_driver() {
    let d = driver();
    let _ = d.cmd_tx.send(DriverCommand::Wake);
    let _ = d.waker.wake();
}

impl IoDriver {
    fn new() -> std::io::Result<Self> {
        let poll = Poll::new()?;
        let waker = Arc::new(Waker::new(poll.registry(), WAKER_TOKEN)?);
        let registry = Arc::new(Mutex::new(IoRegistry {
            listeners: FxHashMap::default(),
            streams: FxHashMap::default(),
            udps: FxHashMap::default(),
        }));
        let is_running = Arc::new(AtomicBool::new(true));
        let (cmd_tx, cmd_rx) = channel::<DriverCommand>();

        let reg_clone = Arc::clone(&registry);
        let run_clone = Arc::clone(&is_running);

        std::thread::Builder::new()
            .name("varn-io-driver".into())
            .spawn(move || {
                Self::run_event_loop(poll, cmd_rx, reg_clone, run_clone);
            })?;

        crate::runtime::timer::set_waker(wake_driver);

        Ok(Self {
            registry,
            cmd_tx,
            waker,
        })
    }
}

mod event_loop;
mod tcp;
mod udp;
