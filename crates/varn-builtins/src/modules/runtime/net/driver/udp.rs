use super::*;

impl IoDriver {
    pub fn udp_bind(&self, host: &str, port: i64) -> std::io::Result<i64> {
        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(|e| std::io::Error::new(ErrorKind::InvalidInput, e))?;
        let socket = MioUdpSocket::bind(addr)?;
        let id = next_socket_id();
        {
            let mut reg = self.registry.lock().unwrap_or_else(|e| e.into_inner());
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
        let reg = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        let ustate = reg.udps.get(&id).ok_or_else(|| {
            std::io::Error::new(ErrorKind::NotFound, format!("invalid UDP socket id {id}"))
        })?;
        ustate.socket.send_to(bytes, addr)
    }

    pub fn udp_recv(&self, id: i64, max_len: usize) -> HostPromise {
        let mut reg = self.registry.lock().unwrap_or_else(|e| e.into_inner());
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
        let mut reg = self.registry.lock().unwrap_or_else(|e| e.into_inner());
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
