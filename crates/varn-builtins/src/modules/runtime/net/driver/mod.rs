use mio::net::{TcpListener, TcpStream, UdpSocket as MioUdpSocket};
use mio::{Events, Interest, Poll, Token, Waker};
use rustc_hash::FxHashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tls::Tls;
use varn_types::{value::SendValue, HostPromise};

const WAKER_TOKEN: Token = Token(usize::MAX);
static NEXT_SOCKET_ID: AtomicI64 = AtomicI64::new(1);

pub(super) fn failure(what: &str, cause: impl std::fmt::Display) -> SendValue {
    crate::runtime::blocking::failure(format!("{what}: {cause}"))
}

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

type Settled = (HostPromise, Result<SendValue, SendValue>);

struct StreamState {
    stream: TcpStream,
    tls: Option<Tls>,
    is_connecting: bool,
    pending_connect: Option<HostPromise>,
    pending_read: Option<PendingRead>,
    pending_write: Option<PendingWrite>,
}

impl StreamState {
    fn plain(stream: TcpStream) -> Self {
        Self {
            stream,
            tls: None,
            is_connecting: false,
            pending_connect: None,
            pending_read: None,
            pending_write: None,
        }
    }

    fn accepted(
        stream: TcpStream,
        tls: &Option<Arc<rustls::ServerConfig>>,
    ) -> Result<Self, String> {
        let mut state = Self::plain(stream);
        if let Some(config) = tls {
            state.tls = Some(Tls::server(config)?);
        }
        Ok(state)
    }

    fn read_plain(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.tls.as_mut() {
            Some(tls) => tls.read(&mut self.stream, buf),
            None => self.stream.read(buf),
        }
    }

    fn write_plain(&mut self, data: &[u8]) -> std::io::Result<usize> {
        match self.tls.as_mut() {
            Some(tls) => tls.write(&mut self.stream, data),
            None => self.stream.write(data),
        }
    }

    fn pump(&mut self) -> std::io::Result<()> {
        match self.tls.as_mut() {
            Some(tls) => tls.pump(&mut self.stream),
            None => Ok(()),
        }
    }

    fn end_session(&mut self) {
        if let Some(tls) = self.tls.as_mut() {
            tls.close(&mut self.stream);
        }
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    fn fail_pending(&mut self, cause: &SendValue, out: &mut Vec<Settled>) {
        if let Some(task) = self.pending_connect.take() {
            out.push((task, Err(cause.clone())));
        }
        if let Some(read) = self.pending_read.take() {
            out.push((read.task, Err(cause.clone())));
        }
        if let Some(write) = self.pending_write.take() {
            out.push((write.task, Err(cause.clone())));
        }
    }
}

struct ListenerState {
    listener: TcpListener,
    tls: Option<Arc<rustls::ServerConfig>>,
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
mod tls;
mod tcp;
mod udp;
