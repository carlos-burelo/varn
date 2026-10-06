use super::*;

impl IoDriver {
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
}
