use std::rc::Rc;

use crate::closure::{VmClosure, VmUpvalue};
use crate::exec::ctx::ExecCtx;
use crate::frame::CallFrame;
use crate::heap::HeapObj;
use crate::task::LazyTask;
use crate::value::VmValue;

const INLINE_ARGS: usize = 4;

fn closure_for(ctx: &mut ExecCtx, task: &LazyTask) -> Rc<VmClosure> {
    let key = Rc::as_ptr(&task.proto) as usize;
    let shareable = task.upvalues.is_empty();
    if shareable {
        let cached = unsafe { &*ctx.static_closures.get() }
            .get(&key)
            .map(|&(_, val)| val);
        if let Some(val) = cached {
            if let Some(HeapObj::VmClosure(closure)) = ctx.heap.get(val.as_heap()) {
                return Rc::clone(closure);
            }
        }
    }
    let constants = ctx.shared_constants(&task.proto);
    let upvalues = task
        .upvalues
        .iter()
        .map(|cell| VmUpvalue::closed(cell.get()))
        .collect();
    let mut closure =
        VmClosure::with_upvalues(Rc::clone(&task.proto), upvalues, constants, ctx.settings);
    closure.module_base = task.module_base;
    let closure = Rc::new(closure);
    if shareable {
        let val = ctx.heap.alloc_vm_closure(Rc::clone(&closure));
        unsafe { &mut *ctx.static_closures.get() }.insert(key, (Rc::clone(&task.proto), val));
    }
    closure
}

pub(super) fn mount(ctx: &mut ExecCtx, task: &LazyTask) {
    let closure = closure_for(ctx, task);
    let args = task.args.cells();
    let alloc = ctx.stack.push_frame(&task.proto);
    let nregs = task.proto.register_count as usize;
    if args.len() <= INLINE_ARGS {
        let mut buf = [VmValue::null(); INLINE_ARGS];
        for (slot, cell) in buf.iter_mut().zip(args) {
            *slot = cell.get();
        }
        ctx.stack.adopt_values(alloc, 0, &buf[..args.len()], nregs);
    } else {
        let values: Vec<VmValue> = args.iter().map(|cell| cell.get()).collect();
        ctx.stack.adopt_values(alloc, 0, &values, nregs);
    }
    let mut frame = CallFrame::new_owned(closure, alloc);
    frame.current_class = task.current_class.clone();
    ctx.frames.push(frame);
}
