use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token, Waker};
use rustc_hash::FxHashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use varn_types::{AsyncTask, Value};

const WAKER_TOKEN: Token = Token(usize::MAX);
static NEXT_SOCKET_ID: AtomicI64 = AtomicI64::new(1);

pub fn next_socket_id() -> i64 {
    NEXT_SOCKET_ID.fetch_add(1, Ordering::SeqCst)
}

struct PendingRead {
    len: usize,
    task: AsyncTask,
}

struct PendingWrite {
    data: Vec<u8>,
    written: usize,
    task: AsyncTask,
}

struct StreamState {
    stream: TcpStream,
    is_connecting: bool,
    pending_connect: Option<AsyncTask>,
    pending_read: Option<PendingRead>,
    pending_write: Option<PendingWrite>,
}

struct ListenerState {
    listener: TcpListener,
    pending_accepts: Vec<AsyncTask>,
}

enum DriverCommand {
    RegisterListener(i64),
    RegisterStream(i64),
    DeregisterListener(TcpListener),
    DeregisterStream(TcpStream),
    Wake,
}

struct IoRegistry {
    listeners: FxHashMap<i64, ListenerState>,
    streams: FxHashMap<i64, StreamState>,
}

pub struct IoDriver {
    registry: Arc<Mutex<IoRegistry>>,
    cmd_tx: Sender<DriverCommand>,
    waker: Arc<Waker>,
    #[allow(dead_code)]
    is_running: Arc<AtomicBool>,
}

static DRIVER: OnceLock<IoDriver> = OnceLock::new();

pub fn driver() -> &'static IoDriver {
    DRIVER.get_or_init(|| IoDriver::new().expect("Failed to initialize mio IoDriver"))
}

impl IoDriver {
    fn new() -> std::io::Result<Self> {
        let poll = Poll::new()?;
        let waker = Arc::new(Waker::new(poll.registry(), WAKER_TOKEN)?);
        let registry = Arc::new(Mutex::new(IoRegistry {
            listeners: FxHashMap::default(),
            streams: FxHashMap::default(),
        }));
        let is_running = Arc::new(AtomicBool::new(true));
        let (cmd_tx, cmd_rx) = channel::<DriverCommand>();

        let reg_clone = Arc::clone(&registry);
        let run_clone = Arc::clone(&is_running);

        std::thread::Builder::new()
            .name("varn-mio-driver".into())
            .spawn(move || {
                Self::run_event_loop(poll, cmd_rx, reg_clone, run_clone);
            })?;

        Ok(Self {
            registry,
            cmd_tx,
            waker,
            is_running,
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
                    DriverCommand::DeregisterListener(mut listener) => {
                        let _ = poll.registry().deregister(&mut listener);
                    }
                    DriverCommand::DeregisterStream(mut stream) => {
                        let _ = poll.registry().deregister(&mut stream);
                    }
                    DriverCommand::Wake => {}
                }
            }

            if let Err(e) = poll.poll(&mut events, Some(Duration::from_millis(50))) {
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
                        let mut resolved_conns: Vec<(AsyncTask, i64)> = Vec::new();
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
                                    task.settle(Ok(Value::Int(-1)));
                                }
                            }
                        }

                        for (conn_id, state) in new_streams {
                            reg.streams.insert(conn_id, state);
                        }

                        drop(reg);
                        for (task, conn_id) in resolved_conns {
                            task.settle(Ok(Value::Int(conn_id)));
                        }
                        continue;
                    }
                }

                // 2. Check Stream event
                if let Some(stream_state) = reg.streams.get_mut(&id) {
                    // 2a. Pending Connect
                    if stream_state.is_connecting && (event.is_writable() || event.is_readable()) {
                        if let Some(task) = stream_state.pending_connect.take() {
                            stream_state.is_connecting = false;
                            match stream_state.stream.peer_addr() {
                                Ok(_) => task.settle(Ok(Value::Int(id))),
                                Err(_) => match stream_state.stream.take_error() {
                                    Ok(None) => task.settle(Ok(Value::Int(id))),
                                    _ => task.settle(Ok(Value::Int(-1))),
                                },
                            }
                        }
                    }

                    // 2b. Pending Write
                    if event.is_writable() {
                        if let Some(mut pw) = stream_state.pending_write.take() {
                            match stream_state.stream.write(&pw.data[pw.written..]) {
                                Ok(n) => {
                                    pw.written += n;
                                    if pw.written >= pw.data.len() {
                                        pw.task.settle(Ok(Value::Int(pw.written as i64)));
                                    } else {
                                        stream_state.pending_write = Some(pw);
                                    }
                                }
                                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                                    stream_state.pending_write = Some(pw);
                                }
                                Err(_) => {
                                    pw.task.settle(Ok(Value::Int(-1)));
                                }
                            }
                        }
                    }

                    // 2c. Pending Read
                    if event.is_readable() {
                        if let Some(pr) = stream_state.pending_read.take() {
                            let mut buf = vec![0u8; pr.len];
                            match stream_state.stream.read(&mut buf) {
                                Ok(n) => {
                                    buf.truncate(n);
                                    let s = String::from_utf8_lossy(&buf).into_owned();
                                    pr.task.settle(Ok(Value::Str(std::rc::Rc::from(s.as_str()))));
                                }
                                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                                    stream_state.pending_read = Some(pr);
                                }
                                Err(_) => {
                                    pr.task.settle(Ok(Value::Str(std::rc::Rc::from(""))));
                                }
                            }
                        }
                    }
                }
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

    pub fn accept(&self, listener_id: i64) -> AsyncTask {
        let mut reg = self.registry.lock().unwrap();
        let listener_state = match reg.listeners.get_mut(&listener_id) {
            Some(l) => l,
            None => {
                let task = AsyncTask::pending();
                task.settle(Ok(Value::Int(-1)));
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
                AsyncTask::resolved(Value::Int(conn_id))
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                let task = AsyncTask::pending();
                listener_state.pending_accepts.push(task.clone());
                drop(reg);
                let _ = self.cmd_tx.send(DriverCommand::Wake);
                let _ = self.waker.wake();
                task
            }
            Err(_) => AsyncTask::resolved(Value::Int(-1)),
        }
    }

    pub fn connect(&self, host: &str, port: i64) -> AsyncTask {
        let addr: SocketAddr = match format!("{host}:{port}").parse() {
            Ok(a) => a,
            Err(_) => {
                if let Ok(mut addrs) = format!("{host}:{port}").to_socket_addrs() {
                    match addrs.next() {
                        Some(a) => a,
                        None => {
                            let t = AsyncTask::pending();
                            t.settle(Ok(Value::Int(-1)));
                            return t;
                        }
                    }
                } else {
                    let t = AsyncTask::pending();
                    t.settle(Ok(Value::Int(-1)));
                    return t;
                }
            }
        };

        let stream = match TcpStream::connect(addr) {
            Ok(s) => s,
            Err(_) => {
                let t = AsyncTask::pending();
                t.settle(Ok(Value::Int(-1)));
                return t;
            }
        };

        let conn_id = next_socket_id();
        let task = AsyncTask::pending();

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
            task.settle(Ok(Value::Int(conn_id)));
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

    pub fn read(&self, conn_id: i64, len: usize) -> AsyncTask {
        let mut reg = self.registry.lock().unwrap();
        let stream_state = match reg.streams.get_mut(&conn_id) {
            Some(s) => s,
            None => {
                let t = AsyncTask::pending();
                t.settle(Ok(Value::Str(std::rc::Rc::from(""))));
                return t;
            }
        };

        // Fast path: try immediate non-blocking read
        let mut buf = vec![0u8; len];
        match stream_state.stream.read(&mut buf) {
            Ok(0) => AsyncTask::resolved(Value::Str(std::rc::Rc::from(""))),
            Ok(n) => {
                buf.truncate(n);
                let s = String::from_utf8_lossy(&buf).into_owned();
                AsyncTask::resolved(Value::Str(std::rc::Rc::from(s.as_str())))
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                let task = AsyncTask::pending();
                stream_state.pending_read = Some(PendingRead { len, task: task.clone() });
                drop(reg);
                let _ = self.waker.wake();
                task
            }
            Err(_) => AsyncTask::resolved(Value::Str(std::rc::Rc::from(""))),
        }
    }

    pub fn write(&self, conn_id: i64, data: Vec<u8>) -> AsyncTask {
        let mut reg = self.registry.lock().unwrap();
        let stream_state = match reg.streams.get_mut(&conn_id) {
            Some(s) => s,
            None => {
                let t = AsyncTask::pending();
                t.settle(Ok(Value::Int(-1)));
                return t;
            }
        };

        // Fast path: try immediate non-blocking write
        match stream_state.stream.write(&data) {
            Ok(n) if n == data.len() => AsyncTask::resolved(Value::Int(n as i64)),
            Ok(n) => {
                let task = AsyncTask::pending();
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
                let task = AsyncTask::pending();
                stream_state.pending_write = Some(PendingWrite {
                    data,
                    written: 0,
                    task: task.clone(),
                });
                drop(reg);
                let _ = self.waker.wake();
                task
            }
            Err(_) => AsyncTask::resolved(Value::Int(-1)),
        }
    }

    pub fn close(&self, conn_id: i64) {
        let mut reg = self.registry.lock().unwrap();
        if let Some(mut stream_state) = reg.streams.remove(&conn_id) {
            let _ = stream_state.stream.shutdown(std::net::Shutdown::Both);
            if let Some(task) = stream_state.pending_connect.take() {
                task.settle(Ok(Value::Int(-1)));
            }
            if let Some(pr) = stream_state.pending_read.take() {
                pr.task.settle(Ok(Value::Str(std::rc::Rc::from(""))));
            }
            if let Some(pw) = stream_state.pending_write.take() {
                pw.task.settle(Ok(Value::Int(-1)));
            }
            drop(reg);
            let _ = self.cmd_tx.send(DriverCommand::DeregisterStream(stream_state.stream));
            let _ = self.waker.wake();
        }
    }

    pub fn close_listener(&self, listener_id: i64) {
        let mut reg = self.registry.lock().unwrap();
        if let Some(mut listener_state) = reg.listeners.remove(&listener_id) {
            for task in listener_state.pending_accepts.drain(..) {
                task.settle(Ok(Value::Int(-1)));
            }
            drop(reg);
            let _ = self.cmd_tx.send(DriverCommand::DeregisterListener(listener_state.listener));
            let _ = self.waker.wake();
        }
    }
}
