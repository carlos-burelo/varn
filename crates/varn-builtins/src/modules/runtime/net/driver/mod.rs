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
use varn_core::{ErrorField, StdErrorClass};
use varn_types::{value::SendValue, HostPromise, NativeError};

const WAKER_TOKEN: Token = Token(usize::MAX);
static NEXT_SOCKET_ID: AtomicI64 = AtomicI64::new(1);

fn raised(error: NativeError) -> SendValue {
    SendValue::from_native_error(&error)
}

pub(super) fn closed(what: &str, detail: impl std::fmt::Display) -> SendValue {
    raised(NativeError::std(StdErrorClass::Closed, format!("{what}: {detail}")))
}

pub(super) fn io_failure(what: &str, e: &std::io::Error) -> SendValue {
    let kind = e.kind();
    let class = if kind == ErrorKind::ConnectionReset
        || kind == ErrorKind::ConnectionAborted
        || kind == ErrorKind::BrokenPipe
        || kind == ErrorKind::NotConnected
        || kind == ErrorKind::UnexpectedEof
    {
        StdErrorClass::Closed
    } else if kind == ErrorKind::TimedOut {
        StdErrorClass::Timeout
    } else {
        StdErrorClass::Net
    };
    raised(NativeError::std(class, format!("{what}: {e}")))
}

pub(super) fn tls_failure(e: &std::io::Error) -> SendValue {
    if e.kind() == ErrorKind::InvalidData {
        raised(NativeError::std(StdErrorClass::Tls, e.to_string()))
    } else {
        io_failure("tls", e)
    }
}

pub(super) fn tls_setup_failure(detail: impl std::fmt::Display) -> SendValue {
    raised(NativeError::std(StdErrorClass::Tls, detail.to_string()))
}

fn at_endpoint(error: NativeError, host: &str, port: u16) -> SendValue {
    raised(
        error
            .with_str(ErrorField::Host, host)
            .with_int(ErrorField::Port, i64::from(port)),
    )
}

pub(super) fn connect_failure(host: &str, port: u16, detail: impl std::fmt::Display) -> SendValue {
    at_endpoint(
        NativeError::std(StdErrorClass::Connect, format!("{host}:{port}: {detail}")),
        host,
        port,
    )
}

pub(super) fn resolve_failure(host: &str, port: u16) -> SendValue {
    at_endpoint(
        NativeError::std(StdErrorClass::Resolve, format!("cannot resolve {host}:{port}")),
        host,
        port,
    )
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
    target: Option<(String, u16)>,
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
            target: None,
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
