use crate::value::SendValue;
use crate::wake::WakeToken;
use std::sync::atomic::{fence, AtomicU32, Ordering};
use std::sync::Mutex;

pub type Completion = Result<SendValue, SendValue>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostOpen {
    Plain,
    ReceiveNext,
    Receive,
    SendAck,
}

struct Slot {
    done: Option<Completion>,
    first: Option<WakeToken>,
    more: Vec<WakeToken>,
}

impl Slot {
    fn push(&mut self, token: WakeToken) {
        if self.first.is_none() {
            self.first = Some(token);
            return;
        }
        self.more.reserve_exact(1);
        self.more.push(token);
    }
}

struct Inner {
    slot: Mutex<Slot>,
    ref_count: AtomicU32,
}

pub struct HostPromise(*mut Inner);

unsafe impl Send for HostPromise {}
unsafe impl Sync for HostPromise {}

impl Clone for HostPromise {
    fn clone(&self) -> Self {
        unsafe {
            (*self.0).ref_count.fetch_add(1, Ordering::Relaxed);
        }
        HostPromise(self.0)
    }
}

impl Drop for HostPromise {
    fn drop(&mut self) {
        unsafe {
            if (*self.0).ref_count.fetch_sub(1, Ordering::Release) == 1 {
                fence(Ordering::Acquire);
                drop(Box::from_raw(self.0));
            }
        }
    }
}

impl PartialEq for HostPromise {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl Eq for HostPromise {}

impl std::fmt::Debug for HostPromise {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.peek() {
            None => write!(f, "HostPromise(<pending>)"),
            Some(Ok(v)) => write!(f, "HostPromise({v:?})"),
            Some(Err(v)) => write!(f, "HostPromise(<rejected:{v:?}>)"),
        }
    }
}

impl HostPromise {
    fn alloc(done: Option<Completion>) -> Self {
        let inner = Box::new(Inner {
            slot: Mutex::new(Slot {
                done,
                first: None,
                more: Vec::new(),
            }),
            ref_count: AtomicU32::new(1),
        });
        HostPromise(Box::into_raw(inner))
    }

    #[inline(always)]
    fn slot(&self) -> std::sync::MutexGuard<'_, Slot> {
        unsafe { (*self.0).slot.lock().unwrap_or_else(|e| e.into_inner()) }
    }

    #[inline(always)]
    pub fn identity(&self) -> usize {
        self.0 as usize
    }

    pub fn pending() -> Self {
        Self::alloc(None)
    }

    pub fn resolved(v: SendValue) -> Self {
        Self::alloc(Some(Ok(v)))
    }

    pub fn rejected(v: SendValue) -> Self {
        Self::alloc(Some(Err(v)))
    }

    pub fn rejected_msg(msg: impl Into<String>) -> Self {
        Self::rejected(SendValue::Str(msg.into()))
    }

    #[inline]
    pub fn is_pending(&self) -> bool {
        self.slot().done.is_none()
    }

    pub fn peek(&self) -> Option<Completion> {
        self.slot().done.clone()
    }

    pub fn complete(&self, result: Completion) {
        let (first, more) = {
            let mut slot = self.slot();
            if slot.done.is_some() {
                return;
            }
            slot.done = Some(result);
            (slot.first.take(), std::mem::take(&mut slot.more))
        };
        for token in first.into_iter().chain(more) {
            token.fire();
        }
    }

    #[inline]
    pub fn resolve(&self, v: SendValue) {
        self.complete(Ok(v));
    }

    #[inline]
    pub fn reject(&self, v: SendValue) {
        self.complete(Err(v));
    }

    #[inline]
    pub fn reject_msg(&self, msg: impl Into<String>) {
        self.reject(SendValue::Str(msg.into()));
    }

    pub fn watch(&self, token: WakeToken) {
        {
            let mut slot = self.slot();
            if slot.done.is_none() {
                slot.push(token);
                return;
            }
        }
        token.fire();
    }
}
