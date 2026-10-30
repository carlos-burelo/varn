use super::*;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

fn bind_reuse_listener(addr: SocketAddr) -> std::io::Result<TcpListener> {
    let domain = if addr.is_ipv4() {
        socket2::Domain::IPV4
    } else {
        socket2::Domain::IPV6
    };
    let sock = socket2::Socket::new(domain, socket2::Type::STREAM, None)?;
    #[cfg(not(windows))]
    sock.set_reuse_address(true)?;
    sock.set_nonblocking(true)?;
    sock.bind(&addr.into())?;
    sock.listen(128)?;
    Ok(TcpListener::from_std(sock.into()))
}

impl IoDriver {
    pub fn listen(&self, port: i64, host: &str) -> std::io::Result<i64> {
        self.listen_with(port, host, None)
    }

    pub fn listen_tls(
        &self,
        port: i64,
        host: &str,
        cert_pem: &str,
        key_pem: &str,
    ) -> Result<i64, String> {
        let config = tls::server_config(cert_pem, key_pem)?;
        self.listen_with(port, host, Some(config))
            .map_err(|e| format!("cannot listen on {host}:{port}: {e}"))
    }

    fn listen_with(
        &self,
        port: i64,
        host: &str,
        tls: Option<Arc<rustls::ServerConfig>>,
    ) -> std::io::Result<i64> {
        let invalid = |message: String| std::io::Error::new(ErrorKind::InvalidInput, message);
        let ip: std::net::IpAddr = host
            .parse()
            .map_err(|_| invalid(format!("'{host}' is not an IP address")))?;
        let port = u16::try_from(port).map_err(|_| invalid(format!("port {port} is out of range")))?;
        let addr = SocketAddr::new(ip, port);
        let listener = bind_reuse_listener(addr)?;
        let id = next_socket_id();

        {
            let mut reg = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            reg.listeners.insert(
                id,
                ListenerState {
                    listener,
                    tls,
                    pending_accepts: Vec::new(),
                },
            );
        }

        let _ = self.cmd_tx.send(DriverCommand::RegisterListener(id));
        let _ = self.waker.wake();
        Ok(id)
    }

    pub fn accept(&self, listener_id: i64) -> HostPromise {
        let mut reg = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        let listener_state = match reg.listeners.get_mut(&listener_id) {
            Some(l) => l,
            None => {
                return HostPromise::rejected(failure(
                    "accept",
                    format!("listener {listener_id} is closed"),
                ));
            }
        };

        let tls = listener_state.tls.clone();
        match listener_state.listener.accept() {
            Ok((stream, _)) => {
                let state = match StreamState::accepted(stream, &tls) {
                    Ok(state) => state,
                    Err(e) => return HostPromise::rejected(failure("accept", e)),
                };
                let conn_id = next_socket_id();
                reg.streams.insert(conn_id, state);
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
            Err(e) => HostPromise::rejected(failure("accept", e)),
        }
    }

    pub fn connect(&self, host: &str, port: i64) -> HostPromise {
        self.open(host, port, None)
    }

    pub fn connect_tls(&self, host: &str, port: i64, extra_roots: Option<&str>) -> HostPromise {
        match Tls::client(host, extra_roots) {
            Ok(tls) => self.open(host, port, Some(tls)),
            Err(e) => HostPromise::rejected(failure("connect", e)),
        }
    }

    fn open(&self, host: &str, port: i64, tls: Option<Tls>) -> HostPromise {
        let task = HostPromise::pending();
        let target = format!("{host}:{port}");
        match target.parse::<SocketAddr>() {
            Ok(addr) => self.connect_addr(addr, task.clone(), tls),
            Err(_) => {
                let pending = task.clone();
                crate::runtime::blocking::detach(move || {
                    let addrs: Vec<SocketAddr> = target
                        .to_socket_addrs()
                        .map(Iterator::collect)
                        .unwrap_or_default();
                    let mut last = format!("cannot resolve {target}");
                    for addr in addrs {
                        match std::net::TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
                            .and_then(|stream| stream.set_nonblocking(true).map(|()| stream))
                        {
                            Ok(stream) => {
                                driver().adopt(TcpStream::from_std(stream), pending, tls);
                                return;
                            }
                            Err(e) => last = format!("{addr}: {e}"),
                        }
                    }
                    pending.reject(failure("connect", last));
                });
            }
        }
        task
    }

    fn adopt(&self, stream: TcpStream, task: HostPromise, tls: Option<Tls>) {
        let conn_id = next_socket_id();
        let secure = tls.is_some();
        let mut reg = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        reg.streams.insert(
            conn_id,
            StreamState {
                tls,
                pending_connect: secure.then(|| task.clone()),
                ..StreamState::plain(stream)
            },
        );
        drop(reg);
        let _ = self.cmd_tx.send(DriverCommand::RegisterStream(conn_id));
        let _ = self.waker.wake();
        if !secure {
            task.resolve(SendValue::Int(conn_id));
        }
    }

    fn connect_addr(&self, addr: SocketAddr, task: HostPromise, tls: Option<Tls>) {
        let stream = match TcpStream::connect(addr) {
            Ok(stream) => stream,
            Err(e) => {
                task.reject(failure("connect", format!("{addr}: {e}")));
                return;
            }
        };
        let conn_id = next_socket_id();
        let mut reg = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        reg.streams.insert(
            conn_id,
            StreamState {
                tls,
                is_connecting: true,
                pending_connect: Some(task),
                ..StreamState::plain(stream)
            },
        );
        drop(reg);
        let _ = self.cmd_tx.send(DriverCommand::RegisterStream(conn_id));
        let _ = self.waker.wake();
    }

    pub fn read(&self, conn_id: i64, len: usize) -> HostPromise {
        let mut reg = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        let stream_state = match reg.streams.get_mut(&conn_id) {
            Some(s) => s,
            None => {
                let t = HostPromise::pending();
                t.complete(Ok(SendValue::Null));
                return t;
            }
        };

        let mut buf = vec![0u8; len];
        match stream_state.read_plain(&mut buf) {
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
            Err(e) => HostPromise::rejected(failure("read", e)),
        }
    }

    pub fn write(&self, conn_id: i64, data: Vec<u8>) -> HostPromise {
        let mut reg = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        let stream_state = match reg.streams.get_mut(&conn_id) {
            Some(s) => s,
            None => {
                return HostPromise::rejected(failure(
                    "write",
                    format!("connection {conn_id} is closed"),
                ));
            }
        };

        match stream_state.write_plain(&data) {
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
            Err(e) => HostPromise::rejected(failure("write", e)),
        }
    }

    pub fn close(&self, conn_id: i64) {
        let mut reg = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut stream_state) = reg.streams.remove(&conn_id) {
            stream_state.end_session();
            if let Some(task) = stream_state.pending_connect.take() {
                task.reject(failure("connect", "connection closed"));
            }
            if let Some(pr) = stream_state.pending_read.take() {
                pr.task.complete(Ok(SendValue::Null));
            }
            if let Some(pw) = stream_state.pending_write.take() {
                pw.task.reject(failure("write", "connection closed"));
            }
            drop(reg);
            let _ = self
                .cmd_tx
                .send(DriverCommand::DeregisterStream(stream_state.stream));
            let _ = self.waker.wake();
        }
    }

    pub fn close_listener(&self, listener_id: i64) {
        let mut reg = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut listener_state) = reg.listeners.remove(&listener_id) {
            for task in listener_state.pending_accepts.drain(..) {
                task.reject(failure("accept", "listener closed"));
            }
            drop(reg);
            let _ = self
                .cmd_tx
                .send(DriverCommand::DeregisterListener(listener_state.listener));
            let _ = self.waker.wake();
        }
    }
}
