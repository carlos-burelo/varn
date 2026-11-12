mod drive;
mod mount;
mod pumps;
mod queue;
mod suspend;

pub(crate) use drive::request_osr;
pub(crate) use mount::{mount, mount_parts};
pub(crate) use pumps::{adopt_host, gc_scope, queue_for};
pub(crate) use queue::{ReadyTask, Start, TaskQueue};
pub(crate) use suspend::{freeze_frame, thaw_frame, FrameBuffers, Frozen, FrozenFrame};

pub(crate) fn enqueue_detached(
    exec: &crate::exec::ctx::ExecCtx,
    task: std::rc::Rc<crate::task::LazyTask>,
    output: std::rc::Rc<crate::task::TaskCell>,
) {
    queue_for(exec).push_ready(ReadyTask {
        start: Start::Fresh(task),
        output,
        step: None,
    });
}
