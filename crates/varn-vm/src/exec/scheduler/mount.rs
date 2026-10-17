use std::rc::Rc;

use crate::closure::{VmClosure, VmUpvalue};
use crate::exec::ctx::ExecCtx;
use crate::frame::CallFrame;
use crate::heap::HeapObj;
use crate::task::LazyTask;
use crate::value::VmValue;

const INLINE_ARGS: usize = 4;

fn closure_for(
    ctx: &mut ExecCtx,
    proto: &Rc<varn_types::FunctionProto>,
    upvalues: &[VmValue],
    module_base: u32,
) -> Rc<VmClosure> {
    let key = Rc::as_ptr(proto) as usize;
    let shareable = upvalues.is_empty();
    if shareable {
        let cached = unsafe { &*ctx.static_closures.get() }
            .get(&key)
            .map(|&(_, val)| val);
        if let Some(val) = cached
            && let Some(HeapObj::VmClosure(closure)) = ctx.heap.get(val.as_heap())
        {
            return Rc::clone(closure);
        }
    }
    let constants = ctx.shared_constants(proto);
    let upvalues = upvalues.iter().map(|&v| VmUpvalue::closed(v)).collect();
    let mut closure = VmClosure::with_upvalues(Rc::clone(proto), upvalues, constants, ctx.settings);
    closure.module_base = module_base;
    let closure = Rc::new(closure);
    if shareable {
        let val = ctx.heap.alloc_vm_closure(Rc::clone(&closure));
        unsafe { &mut *ctx.static_closures.get() }.insert(key, (Rc::clone(proto), val));
    }
    closure
}

pub(crate) fn mount(ctx: &mut ExecCtx, task: &LazyTask, return_reg: u16) {
    let upvalues: Vec<VmValue> = task.upvalues.iter().map(|cell| cell.get()).collect();
    let mut buf = [VmValue::null(); INLINE_ARGS];
    let cells = task.args.cells();
    let heap_args: Vec<VmValue>;
    let args: &[VmValue] = if cells.len() <= INLINE_ARGS {
        for (slot, cell) in buf.iter_mut().zip(cells) {
            *slot = cell.get();
        }
        &buf[..cells.len()]
    } else {
        heap_args = cells.iter().map(|cell| cell.get()).collect();
        &heap_args
    };
    mount_parts(
        ctx,
        &task.proto,
        &upvalues,
        task.module_base,
        args,
        task.current_class.clone(),
        return_reg,
    );
}

pub(crate) fn mount_parts(
    ctx: &mut ExecCtx,
    proto: &Rc<varn_types::FunctionProto>,
    upvalues: &[VmValue],
    module_base: u32,
    args: &[VmValue],
    current_class: Option<Rc<varn_types::ClassObj>>,
    return_reg: u16,
) {
    let closure = closure_for(ctx, proto, upvalues, module_base);
    let alloc = ctx.stack.push_frame(proto);
    let nregs = proto.register_count as usize;
    ctx.stack.adopt_values(alloc, 0, args, nregs);
    let mut frame = CallFrame::new_owned(closure, alloc);
    frame.current_class = current_class;
    frame.return_reg = return_reg;
    frame.awaited = return_reg != CallFrame::NO_RETURN_REG;
    ctx.frames.push(frame);
}
