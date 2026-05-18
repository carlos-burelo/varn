use std::sync::atomic::{AtomicUsize, Ordering};

pub type TaskId = usize;

static NEXT_TASK_ID: AtomicUsize = AtomicUsize::new(1);

pub fn alloc_task_id() -> TaskId {
    NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed)
}
