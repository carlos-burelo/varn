use super::*;

impl IoDriver {
    pub(super) fn run_event_loop(
        mut poll: Poll,
        cmd_rx: Receiver<DriverCommand>,
        registry: Arc<Mutex<IoRegistry>>,
        is_running: Arc<AtomicBool>,
    ) {
        let mut events = Events::with_capacity(1024);

        while is_running.load(Ordering::Relaxed) {
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    DriverCommand::RegisterListener(id) => {
                        let mut reg = registry.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(lstate) = reg.listeners.get_mut(&id) {
                            let _ = poll.registry().register(
                                &mut lstate.listener,
                                Token(id as usize),
                                Interest::READABLE,
                            );
                        }
                    }
                    DriverCommand::RegisterStream(id) => {
                        let mut reg = registry.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(sstate) = reg.streams.get_mut(&id) {
                            let _ = poll.registry().register(
                                &mut sstate.stream,
                                Token(id as usize),
                                Interest::READABLE | Interest::WRITABLE,
                            );
                        }
                    }
                    DriverCommand::RegisterUdp(id) => {
                        let mut reg = registry.lock().unwrap_or_else(|e| e.into_inner());
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
                let mut reg = registry.lock().unwrap_or_else(|e| e.into_inner());

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

                let mut deferred: Vec<(HostPromise, Result<SendValue, SendValue>)> = Vec::new();
                if let Some(stream_state) = reg.streams.get_mut(&id) {
                    if stream_state.is_connecting && (event.is_writable() || event.is_readable()) {
                        if let Some(task) = stream_state.pending_connect.take() {
                            stream_state.is_connecting = false;
                            let res = match stream_state.stream.peer_addr() {
                                Ok(_) => Ok(SendValue::Int(id)),
                                Err(_) => match stream_state.stream.take_error() {
                                    Ok(None) => Ok(SendValue::Int(id)),
                                    Ok(Some(_)) | Err(_) => Ok(SendValue::Int(-1)),
                                },
                            };
                            deferred.push((task, res));
                        }
                    }

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
}
