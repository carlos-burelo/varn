use std::rc::Rc;

use varn_types::value::SendValue;
use varn_types::{Completion, HostOpen};

use super::ctx::ExecCtx;
use crate::task::{settle, Outcome, TaskCell};
use crate::value::VmValue;

pub(crate) fn complete_host(ctx: &mut ExecCtx, cell: &Rc<TaskCell>) {
    let Some((promise, open)) = cell.take_host() else {
        return;
    };
    let Some(done) = promise.peek() else {
        return;
    };
    let outcome = materialize(ctx, done, open);
    settle(&mut ctx.heap, cell, outcome);
}

fn open_value(ctx: &mut ExecCtx, sv: &SendValue) -> VmValue {
    sv.to_value_ctx(ctx)
}

fn closed_error(ctx: &mut ExecCtx) -> VmValue {
    SendValue::Error {
        class: varn_types::value::CHANNEL_CLOSED_CLASS.to_string(),
        message: "channel closed".to_string(),
    }
    .to_value_ctx(ctx)
}

fn iter_result(ctx: &mut ExecCtx, value: VmValue, done: bool) -> VmValue {
    ctx.heap.alloc_object_with_shape_slice(
        &varn_types::iter_result_shape(),
        &[value, VmValue::from_bool(done)],
    )
}

fn materialize(ctx: &mut ExecCtx, done: Completion, open: HostOpen) -> Outcome {
    match (open, done) {
        (HostOpen::Plain, Ok(sv)) => Ok(open_value(ctx, &sv)),
        (HostOpen::Plain, Err(sv)) => Err(sv.to_value_ctx(ctx)),
        (HostOpen::ReceiveNext, Ok(sv)) => {
            let value = open_value(ctx, &sv);
            Ok(iter_result(ctx, value, false))
        }
        (HostOpen::ReceiveNext, Err(_)) => {
            let null = VmValue::null();
            Ok(iter_result(ctx, null, true))
        }
        (HostOpen::Receive, Ok(sv)) => Ok(open_value(ctx, &sv)),
        (HostOpen::Receive, Err(_)) => Err(closed_error(ctx)),
        (HostOpen::SendAck, Ok(SendValue::Bool(true))) => Ok(VmValue::null()),
        (HostOpen::SendAck, _) => Err(closed_error(ctx)),
    }
}
