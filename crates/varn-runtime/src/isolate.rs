use std::sync::Arc;
use std::sync::mpsc::{channel, Sender, Receiver};
use varn_types::value::SendValue;
use varn_types::value::VmValuePayload;
use varn_types::task::AsyncTask;

#[derive(Clone, Debug)]
pub struct IsolatePort {
    pub tx: Sender<SendValue>,
    pub rx: Arc<std::sync::Mutex<Receiver<SendValue>>>,
    pub waker: Arc<std::sync::Mutex<Option<AsyncTask>>>,
    pub remote_waker: Arc<std::sync::Mutex<Option<AsyncTask>>>,
}

// SAFETY: All fields are thread‑safe.
unsafe impl Send for IsolatePort {}
unsafe impl Sync for IsolatePort {}

impl IsolatePort {
    pub fn new() -> (Self, Self) {
        let (tx1, rx1) = channel();
        let (tx2, rx2) = channel();
        
        let waker1 = Arc::new(std::sync::Mutex::new(None));
        let waker2 = Arc::new(std::sync::Mutex::new(None));
        
        let port1 = IsolatePort {
            tx: tx1,
            rx: Arc::new(std::sync::Mutex::new(rx2)),
            waker: waker1.clone(),
            remote_waker: waker2.clone(),
        };
        let port2 = IsolatePort {
            tx: tx2,
            rx: Arc::new(std::sync::Mutex::new(rx1)),
            waker: waker2,
            remote_waker: waker1,
        };
        (port1, port2)
    }

    pub fn send(&self, val: SendValue) -> Result<(), String> {
        if let Some(waker) = self.remote_waker.lock().unwrap().take() {
            waker.resolve(val.to_value());
            Ok(())
        } else {
            self.tx.send(val).map_err(|_| "Receiver dropped".to_string())
        }
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
