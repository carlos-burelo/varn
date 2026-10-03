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

        varn_runtime::timer::set_waker(wake_driver);

        Ok(Self {
            registry,
            cmd_tx,
            waker,
        })
    }

    fn run_event_loop(
        mut poll: Poll,
        cmd_rx: Receiver<DriverCommand>,
        registry: Arc<Mutex<IoRegistry>>,
        is_running: Arc<AtomicBool>,
    ) {
        let mut events = Events::with_capacity(1024);

        while is_running.load(Ordering::Relaxed) {
            // Process all pending registration/deregistration commands before polling
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    DriverCommand::RegisterListener(id) => {
                        let mut reg = registry.lock().unwrap();
                        if let Some(lstate) = reg.listeners.get_mut(&id) {
                            let _ = poll.registry().register(
                                &mut lstate.listener,
                                Token(id as usize),
                                Interest::READABLE,
                            );
                        }
                    }
                    DriverCommand::RegisterStream(id) => {
                        let mut reg = registry.lock().unwrap();
                        if let Some(sstate) = reg.streams.get_mut(&id) {
                            let _ = poll.registry().register(
                                &mut sstate.stream,
                                Token(id as usize),
                                Interest::READABLE | Interest::WRITABLE,
                            );
                        }
                    }
                    DriverCommand::RegisterUdp(id) => {
                        let mut reg = registry.lock().unwrap();
                        if let Some(ustate) = reg.udps.get_mut(&id) {
                            let _ = poll.registry().register(
                                &mut ustate.socket,
                                Token(id as usize),
                                Interest::READABLE,
                            );
                        }
                    }
                    DriverCommand::DeregisterListener(mut listener) => {
                        let _ = poll.registry().deregister(&mut listener);
                    }
                    DriverCommand::DeregisterUdp(mut socket) => {
                        let _ = poll.registry().deregister(&mut socket);
                    }
                    DriverCommand::DeregisterStream(mut stream) => {
                        let _ = poll.registry().deregister(&mut stream);
                    }
                    DriverCommand::Wake => {}
                }
            }

            let poll_timeout = match varn_runtime::timer::next_deadline() {
                None => Duration::from_millis(50),
                Some(deadline) => deadline
                    .saturating_duration_since(std::time::Instant::now())
                    .min(Duration::from_millis(50)),
            };
            if let Err(e) = poll.poll(&mut events, Some(poll_timeout)) {
                if e.kind() == ErrorKind::Interrupted {
                    continue;
                }
                break;
            }

            for event in events.iter() {
                let token = event.token();
                if token == WAKER_TOKEN {
                    continue;
                }

                let id = token.0 as i64;
                let mut reg = registry.lock().unwrap();

                // 1. Check Listener event
                if let Some(listener_state) = reg.listeners.get_mut(&id) {
                    if event.is_readable() {
                        let mut resolved_conns: Vec<(HostPromise, i64)> = Vec::new();
                        let mut new_streams: Vec<(i64, StreamState)> = Vec::new();

                        while let Some(task) = listener_state.pending_accepts.pop() {
                            match listener_state.listener.accept() {
                                Ok((mut stream, _)) => {
                                    let conn_id = next_socket_id();
                                    let token = Token(conn_id as usize);
                                    let _ = poll.registry().register(
                                        &mut stream,
                                        token,
                                        Interest::READABLE | Interest::WRITABLE,
                                    );
                                    resolved_conns.push((task, conn_id));
                                    new_streams.push((
                                        conn_id,
                                        StreamState {
                                            stream,
                                            is_connecting: false,
                                            pending_connect: None,
                                            pending_read: None,
                                            pending_write: None,
                                        },
                                    ));
                                }
                                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                                    listener_state.pending_accepts.push(task);
                                    break;
                                }
                                Err(_) => {
                                    task.complete(Ok(SendValue::Int(-1)));
                                }
                            }
                        }

                        for (conn_id, state) in new_streams {
                            reg.streams.insert(conn_id, state);
                        }

                        drop(reg);
                        for (task, conn_id) in resolved_conns {
                            task.complete(Ok(SendValue::Int(conn_id)));
                        }
                        continue;
                    }
                }

                // 2. Check Stream event
                let mut deferred: Vec<(HostPromise, Result<SendValue, SendValue>)> = Vec::new();
                if let Some(stream_state) = reg.streams.get_mut(&id) {
                    // 2a. Pending Connect
                    if stream_state.is_connecting && (event.is_writable() || event.is_readable()) {
                        if let Some(task) = stream_state.pending_connect.take() {
                            stream_state.is_connecting = false;
                            let res = match stream_state.stream.peer_addr() {
                                Ok(_) => Ok(SendValue::Int(id)),
                                Err(_) => match stream_state.stream.take_error() {
                                    Ok(None) => Ok(SendValue::Int(id)),
                                    _ => Ok(SendValue::Int(-1)),
                                },
                            };
                            deferred.push((task, res));
                        }
                    }

                    // 2b. Pending Write
                    if event.is_writable() {
                        if let Some(mut pw) = stream_state.pending_write.take() {
                            match stream_state.stream.write(&pw.data[pw.written..]) {
                                Ok(n) => {
                                    pw.written += n;
                                    if pw.written >= pw.data.len() {
                                        deferred
                                            .push((pw.task, Ok(SendValue::Int(pw.written as i64))));
                                    } else {
                                        stream_state.pending_write = Some(pw);
                                    }
                                }
                                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                                    stream_state.pending_write = Some(pw);
                                }
                                Err(_) => {
                                    deferred.push((pw.task, Ok(SendValue::Int(-1))));
                                }
                            }
                        }
                    }

                    // 2c. Pending Read
                    if event.is_readable() {
                        if let Some(pr) = stream_state.pending_read.take() {
                            let mut buf = vec![0u8; pr.len];
                            match stream_state.stream.read(&mut buf) {
                                Ok(0) => {
                                    deferred.push((pr.task, Ok(SendValue::Null)));
                                }
                                Ok(n) => {
                                    buf.truncate(n);
                                    deferred.push((pr.task, Ok(SendValue::Bytes(buf.to_vec()))));
                                }
                                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                                    stream_state.pending_read = Some(pr);
                                }
                                Err(_) => {
                                    deferred.push((pr.task, Ok(SendValue::Null)));
                                }
                            }
                        }
                    }
                }
                if let Some(udp_state) = reg.udps.get_mut(&id) {
                    if event.is_readable() {
                        while let Some(pr) = udp_state.pending_recvs.pop_front() {
                            let mut buf = vec![0u8; pr.max_len.max(64)];
                            match udp_state.socket.recv_from(&mut buf) {
                                Ok((n, src)) => {
                                    buf.truncate(n);
                                    deferred.push((pr.task, Ok(udp_packet_value(&buf, src))));
                                }
                                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                                    udp_state.pending_recvs.push_front(pr);
                                    break;
                                }
                                Err(_) => {
                                    deferred.push((pr.task, Ok(SendValue::Null)));
                                }
                            }
                        }
                    }
                }
                drop(reg);
                for (task, res) in deferred {
                    task.complete(res);
                }
            }
            for task in varn_runtime::timer::take_due() {
                task.resolve(SendValue::Null);
            }
        }
    }

    pub fn listen(&self, port: i64) -> std::io::Result<i64> {
        let addr: SocketAddr = format!("127.0.0.1:{port}")
            .parse()
            .map_err(|e| std::io::Error::new(ErrorKind::InvalidInput, e))?;
        let listener = TcpListener::bind(addr)?;
        let id = next_socket_id();

        {
            let mut reg = self.registry.lock().unwrap();
            reg.listeners.insert(
                id,
                ListenerState {
                    listener,
                    pending_accepts: Vec::new(),
                },
            );
        }

        let _ = self.cmd_tx.send(DriverCommand::RegisterListener(id));
        let _ = self.waker.wake();
        Ok(id)
    }

    pub fn accept(&self, listener_id: i64) -> HostPromise {
        let mut reg = self.registry.lock().unwrap();
        let listener_state = match reg.listeners.get_mut(&listener_id) {
            Some(l) => l,
            None => {
                let task = HostPromise::pending();
                task.complete(Ok(SendValue::Int(-1)));
                return task;
            }
        };

        // Fast-path: non-blocking accept
        match listener_state.listener.accept() {
            Ok((stream, _)) => {
                let conn_id = next_socket_id();
                reg.streams.insert(
                    conn_id,
                    StreamState {
                        stream,
                        is_connecting: false,
                        pending_connect: None,
                        pending_read: None,
                        pending_write: None,
                    },
                );
                drop(reg);
                let _ = self.cmd_tx.send(DriverCommand::RegisterStream(conn_id));
                let _ = self.waker.wake();
                HostPromise::resolved(SendValue::Int(conn_id))
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                let task = HostPromise::pending();
                listener_state.pending_accepts.push(task.clone());
                drop(reg);
                let _ = self.cmd_tx.send(DriverCommand::Wake);
                let _ = self.waker.wake();
                task
            }
            Err(_) => HostPromise::resolved(SendValue::Int(-1)),
        }
    }

    pub fn connect(&self, host: &str, port: i64) -> HostPromise {
        let addr: SocketAddr = match format!("{host}:{port}").parse() {
            Ok(a) => a,
            Err(_) => {
                if let Ok(mut addrs) = format!("{host}:{port}").to_socket_addrs() {
                    match addrs.next() {
                        Some(a) => a,
                        None => {
                            let t = HostPromise::pending();
                            t.complete(Ok(SendValue::Int(-1)));
                            return t;
                        }
                    }
                } else {
                    let t = HostPromise::pending();
                    t.complete(Ok(SendValue::Int(-1)));
                    return t;
                }
            }
        };

        let stream = match TcpStream::connect(addr) {
            Ok(s) => s,
            Err(_) => {
                let t = HostPromise::pending();
                t.complete(Ok(SendValue::Int(-1)));
                return t;
            }
        };

        let conn_id = next_socket_id();
        let task = HostPromise::pending();

        // Check if already connected (fast path)
        if stream.peer_addr().is_ok() {
            let mut reg = self.registry.lock().unwrap();
            reg.streams.insert(
                conn_id,
                StreamState {
                    stream,
                    is_connecting: false,
                    pending_connect: None,
                    pending_read: None,
                    pending_write: None,
                },
            );
            drop(reg);
            let _ = self.cmd_tx.send(DriverCommand::RegisterStream(conn_id));
            let _ = self.waker.wake();
            task.complete(Ok(SendValue::Int(conn_id)));
            return task;
        }

        let mut reg = self.registry.lock().unwrap();
        reg.streams.insert(
            conn_id,
            StreamState {
                stream,
                is_connecting: true,
                pending_connect: Some(task.clone()),
                pending_read: None,
                pending_write: None,
            },
        );
        drop(reg);
        let _ = self.cmd_tx.send(DriverCommand::RegisterStream(conn_id));
        let _ = self.waker.wake();
        task
    }

    pub fn read(&self, conn_id: i64, len: usize) -> HostPromise {
        let mut reg = self.registry.lock().unwrap();
        let stream_state = match reg.streams.get_mut(&conn_id) {
            Some(s) => s,
            None => {
                let t = HostPromise::pending();
                t.complete(Ok(SendValue::Null));
                return t;
            }
        };

        // Fast path: try immediate non-blocking read
        let mut buf = vec![0u8; len];
        match stream_state.stream.read(&mut buf) {
            Ok(0) => HostPromise::resolved(SendValue::Null),
            Ok(n) => {
                buf.truncate(n);
                HostPromise::resolved(SendValue::Bytes(buf.to_vec()))
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                let task = HostPromise::pending();
                stream_state.pending_read = Some(PendingRead {
                    len,
                    task: task.clone(),
                });
                drop(reg);
                let _ = self.waker.wake();
                task
            }
            Err(_) => HostPromise::resolved(SendValue::Null),
        }
    }

    pub fn write(&self, conn_id: i64, data: Vec<u8>) -> HostPromise {
        let mut reg = self.registry.lock().unwrap();
        let stream_state = match reg.streams.get_mut(&conn_id) {
            Some(s) => s,
            None => {
                let t = HostPromise::pending();
                t.complete(Ok(SendValue::Int(-1)));
                return t;
            }
        };

        // Fast path: try immediate non-blocking write
        match stream_state.stream.write(&data) {
            Ok(n) if n == data.len() => HostPromise::resolved(SendValue::Int(n as i64)),
            Ok(n) => {
                let task = HostPromise::pending();
                stream_state.pending_write = Some(PendingWrite {
                    data,
                    written: n,
                    task: task.clone(),
                });
                drop(reg);
                let _ = self.waker.wake();
                task
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                let task = HostPromise::pending();
                stream_state.pending_write = Some(PendingWrite {
                    data,
                    written: 0,
                    task: task.clone(),
                });
                drop(reg);
                let _ = self.waker.wake();
                task
            }
            Err(_) => HostPromise::resolved(SendValue::Int(-1)),
        }
    }

    pub fn close(&self, conn_id: i64) {
        let mut reg = self.registry.lock().unwrap();
        if let Some(mut stream_state) = reg.streams.remove(&conn_id) {
            let _ = stream_state.stream.shutdown(std::net::Shutdown::Both);
            if let Some(task) = stream_state.pending_connect.take() {
                task.complete(Ok(SendValue::Int(-1)));
            }
            if let Some(pr) = stream_state.pending_read.take() {
                pr.task.complete(Ok(SendValue::Null));
            }
            if let Some(pw) = stream_state.pending_write.take() {
                pw.task.complete(Ok(SendValue::Int(-1)));
            }
            drop(reg);
            let _ = self
                .cmd_tx
                .send(DriverCommand::DeregisterStream(stream_state.stream));
            let _ = self.waker.wake();
        }
    }

    pub fn close_listener(&self, listener_id: i64) {
        let mut reg = self.registry.lock().unwrap();
        if let Some(mut listener_state) = reg.listeners.remove(&listener_id) {
            for task in listener_state.pending_accepts.drain(..) {
                task.complete(Ok(SendValue::Int(-1)));
            }
            drop(reg);
            let _ = self
                .cmd_tx
                .send(DriverCommand::DeregisterListener(listener_state.listener));
            let _ = self.waker.wake();
        }
    }

    pub fn udp_bind(&self, host: &str, port: i64) -> std::io::Result<i64> {
        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(|e| std::io::Error::new(ErrorKind::InvalidInput, e))?;
        let socket = MioUdpSocket::bind(addr)?;
        let id = next_socket_id();
        {
            let mut reg = self.registry.lock().unwrap();
            reg.udps.insert(
                id,
                UdpState {
                    socket,
                    pending_recvs: std::collections::VecDeque::new(),
                },
            );
        }
        let _ = self.cmd_tx.send(DriverCommand::RegisterUdp(id));
        let _ = self.waker.wake();
        Ok(id)
    }

    pub fn udp_send_to(
        &self,
        id: i64,
        bytes: &[u8],
        host: &str,
        port: i64,
    ) -> std::io::Result<usize> {
        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(|e| std::io::Error::new(ErrorKind::InvalidInput, e))?;
        let reg = self.registry.lock().unwrap();
        let ustate = reg.udps.get(&id).ok_or_else(|| {
            std::io::Error::new(ErrorKind::NotFound, format!("invalid UDP socket id {id}"))
        })?;
        ustate.socket.send_to(bytes, addr)
    }

    pub fn udp_recv(&self, id: i64, max_len: usize) -> HostPromise {
        let mut reg = self.registry.lock().unwrap();
        let ustate = match reg.udps.get_mut(&id) {
            Some(u) => u,
            None => {
                let task = HostPromise::pending();
                task.reject_msg(format!("invalid UDP socket id {id}"));
                return task;
            }
        };
        let mut buf = vec![0u8; max_len.max(64)];
        match ustate.socket.recv_from(&mut buf) {
            Ok((n, src)) => {
                buf.truncate(n);
                HostPromise::resolved(udp_packet_value(&buf, src))
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                let task = HostPromise::pending();
                ustate.pending_recvs.push_back(PendingUdpRecv {
                    max_len,
                    task: task.clone(),
                });
                drop(reg);
                let _ = self.waker.wake();
                task
            }
            Err(_) => HostPromise::resolved(SendValue::Null),
        }
    }

    pub fn udp_close(&self, id: i64) {
        let mut reg = self.registry.lock().unwrap();
        if let Some(ustate) = reg.udps.remove(&id) {
            for pr in ustate.pending_recvs {
                pr.task.complete(Ok(SendValue::Null));
            }
            drop(reg);
            let _ = self
                .cmd_tx
                .send(DriverCommand::DeregisterUdp(ustate.socket));
            let _ = self.waker.wake();
        }
    }
}
