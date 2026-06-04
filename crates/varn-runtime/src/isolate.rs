use std::sync::Arc;
use std::sync::mpsc::{channel, Sender, Receiver};
use varn_types::value::SendValue;
use varn_types::value::VmValuePayload;

#[derive(Clone, Debug)]
pub struct IsolatePort {
    pub tx: Sender<SendValue>,
    pub rx: Arc<std::sync::Mutex<Receiver<SendValue>>>,
}

// SAFETY: All fields are thread‑safe (Arc, Sender, and a Mutex guarding the Receiver).
unsafe impl Send for IsolatePort {}
unsafe impl Sync for IsolatePort {}

impl IsolatePort {
    pub fn new() -> (Self, Self) {
        let (tx1, rx1) = channel();
        let (tx2, rx2) = channel();
        
        let port1 = IsolatePort {
            tx: tx1,
            rx: Arc::new(std::sync::Mutex::new(rx2)),
        };
        let port2 = IsolatePort {
            tx: tx2,
            rx: Arc::new(std::sync::Mutex::new(rx1)),
        };
        (port1, port2)
    }

    pub fn send(&self, val: SendValue) -> Result<(), String> {
        self.tx.send(val).map_err(|_| "Receiver dropped".to_string())
    }

    // Blocking receive used by spawned thread
    pub fn receive_blocking(&self) -> Option<SendValue> {
        let rx = self.rx.lock().unwrap();
        rx.recv().ok()
    }
}

impl VmValuePayload for IsolatePort {
    fn clone_payload(&self) -> Box<dyn VmValuePayload> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
