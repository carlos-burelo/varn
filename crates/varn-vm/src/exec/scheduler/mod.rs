mod drive;
mod mount;
mod pumps;
mod queue;
mod suspend;

pub(crate) use pumps::{gc_scope, queue_for};
pub(crate) use queue::{ReadyTask, Start, TaskQueue};
pub(crate) use suspend::Frozen;

pub(crate) fn enqueue_detached(
    exec: &crate::exec::ctx::ExecCtx,
    task: std::rc::Rc<varn_types::value::LazyTask>,
    output: varn_types::AsyncTask,
) {
    queue_for(exec).push_ready(ReadyTask {
        start: Start::Fresh(task),
        output,
    });
}
