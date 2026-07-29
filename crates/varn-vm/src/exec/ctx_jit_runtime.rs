use crate::value::VmValue;

use super::ctx::ExecCtx;
use super::ctx_jit_values::jit_propagate_error;

/// Loop back-edge safepoint. Mirrors the interpreter's `OpCode::Loop`
/// handler: collect the nursery (and pace the major GC) so long call-free
/// allocating loops don't overflow the nursery into the old generation.
/// Takes no `VmValue` arguments by design — the caller has flushed every VM
/// register to the stack, so all roots are visible and get reloaded after.
pub extern "C" fn jit_gc_safepoint(ctx: *mut ExecCtx) {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.gc_backedge_safepoint();
    }
}

pub extern "C" fn jit_negate(ctx: *mut ExecCtx, v: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        crate::exec::arith::negate(v, &mut ctx_ref.heap)
    }
}

pub extern "C" fn jit_logical_not(_ctx: *mut ExecCtx, v: VmValue) -> VmValue {
    crate::exec::compare::logical_not(v)
}

pub extern "C" fn jit_div(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match crate::exec::arith::div(a, b, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_modulo(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match crate::exec::arith::modulo(a, b, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_pow(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match crate::exec::arith::pow(a, b, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_get_index(
    ctx: *mut ExecCtx,
    args: *const varn_jit::JitGetIndexArgs,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = &*args;
        match crate::exec::collections::get_index(args.obj, args.key, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_set_index(ctx: *mut ExecCtx, args: *const varn_jit::JitSetIndexArgs) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = &*args;
        match crate::exec::collections::set_index(args.obj, args.key, args.val, &mut ctx_ref.heap) {
            Ok(()) => {}
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub unsafe extern "C" fn jit_array_get_fast(
    ctx: *mut ExecCtx,
    obj: VmValue,
    key: VmValue,
) -> VmValue {
    // Fast path: heap array with integer key — no string allocation, no dispatch table
    if obj.is_heap() {
        let heap_idx = obj.as_heap_idx();
        let ctx_ref = &*ctx;
        if let Some(crate::heap::HeapObj::Array(a)) = ctx_ref.heap.get(heap_idx) {
            let idx = if key.is_int() {
                key.as_int() as usize
            } else {
                key.to_i32() as usize
            };
            return a.get_vm(idx).unwrap_or(VmValue::null());
        }
    }
    // Slow path: objects, strings, ranges, SSO strings — fall back to handler
    let ctx_ref = &mut *ctx;
    match crate::exec::collections::array_get_index(obj, key, &mut ctx_ref.heap) {
        Ok(v) => v,
        Err(e) => super::ctx_jit_values::jit_propagate_error(ctx_ref, e),
    }
}

pub unsafe extern "C" fn jit_array_set_fast(
    ctx: *mut ExecCtx,
    obj: VmValue,
    key: VmValue,
    val: VmValue,
) {
    // Fast path: heap array with integer key — no string allocation, no dispatch
    if obj.is_heap() {
        let heap_idx = obj.as_heap_idx();
        let ctx_ref = &mut *ctx;
        if let Some(crate::heap::HeapObj::Array(a)) = ctx_ref.heap.get_mut(heap_idx) {
            let idx = if key.is_int() {
                key.as_int() as usize
            } else {
                key.to_i32() as usize
            };
            let len = a.len();
            if idx < len {
                a.set_vm(idx, val);
            } else if idx == len {
                a.push_vm(val);
            } else {
                while a.len() < idx {
                    a.push_vm(VmValue::null());
                }
                a.push_vm(val);
            }
            ctx_ref.heap.write_barrier(heap_idx, val);
            return;
        }
    }
    // Slow path: objects and other types
    let ctx_ref = &mut *ctx;
    if let Err(e) = crate::exec::collections::array_set_index(obj, key, val, &mut ctx_ref.heap) {
        super::ctx_jit_values::jit_propagate_error(ctx_ref, e);
    }
}

pub extern "C" fn jit_typeof_val(ctx: *mut ExecCtx, v: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let s = crate::exec::advanced::typeof_val(v, &ctx_ref.heap);
        ctx_ref.heap.alloc_str(s)
    }
}

/// CLIF static-call IC miss path: the guarded callee bits didn't match the
/// linked target (rebound global, or GC-moved closure), so dispatch the
/// actual runtime callee through the interpreter/JIT with the (already
/// boxed) argument values. Up to 4 args — the lowering bails a `Call` with
/// more, so `a0..a3` beyond `argc` are ignored.
///
/// Errors propagate through the same longjmp path as every other JIT
/// helper, unwinding to the outer `setjmp` in `execute_jit_frame`.
#[allow(clippy::too_many_arguments)]
pub extern "C" fn clif_call_fallback(
    ctx: *mut ExecCtx,
    callee: VmValue,
    argc: usize,
    a0: VmValue,
    a1: VmValue,
    a2: VmValue,
    a3: VmValue,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let all = [a0, a1, a2, a3];
        // `argc` slots starting at all[0] are already staged (including the null
        // placeholder for regular calls, or the receiver for extension calls).
        // call_vm_staged mirrors the interpreter's exec_call_reg fallback:
        // push the staged args directly and call prepare_call(callee, argc).
        let staged = &all[..argc.min(4)];
        match ctx_ref.call_vm_staged(callee, staged, argc) {
            Ok(v) => v,
            Err(msg) => jit_propagate_error(ctx_ref, crate::error::RuntimeError::new(msg)),
        }
    }
}

pub extern "C" fn jit_instanceof(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &*ctx;
        let r = crate::exec::advanced::instanceof(a, b, &ctx_ref.heap);
        VmValue::from_bool(r)
    }
}

pub extern "C" fn jit_array_length(ctx: *mut ExecCtx, arr: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_array_length(arr) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_array_push(ctx: *mut ExecCtx, arr: VmValue, val: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_array_push(arr, val) {
            Ok(()) => {}
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_array_pop(ctx: *mut ExecCtx, arr: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_array_pop(arr) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_array_extend(ctx: *mut ExecCtx, arr: VmValue, src: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_array_extend(arr, src) {
            Ok(()) => {}
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_str_concat(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        crate::exec::strings::str_concat(a, b, &mut ctx_ref.heap)
    }
}

pub extern "C" fn jit_str_slice(ctx: *mut ExecCtx, s: VmValue, idx: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_str_slice(s, idx) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_str_length(ctx: *mut ExecCtx, v: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_str_length(v) {
            Ok(len) => len,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_bitand(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe { crate::exec::arith::bit_and(a, b, &mut (*ctx).heap) }
}

pub extern "C" fn jit_bitor(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe { crate::exec::arith::bit_or(a, b, &mut (*ctx).heap) }
}

pub extern "C" fn jit_bitxor(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe { crate::exec::arith::bit_xor(a, b, &mut (*ctx).heap) }
}

pub extern "C" fn jit_shl(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe { crate::exec::arith::shl(a, b, &mut (*ctx).heap) }
}

pub extern "C" fn jit_shr(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe { crate::exec::arith::shr(a, b, &mut (*ctx).heap) }
}

pub extern "C" fn jit_ushr(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe { crate::exec::arith::ushr(a, b, &mut (*ctx).heap) }
}

pub extern "C" fn jit_load_module(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    const_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let spec_nv = closure_ref.constants[const_idx];
        let spec = match ctx_ref.heap.str_val(spec_nv) {
            Some(s) => s,
            None => return VmValue::null(),
        };
        match ctx_ref.load_module_from_source(&spec, &closure_ref.proto.chunk.source_file) {
            Ok(v) => v,
            Err(e) => panic!("JIT load_module failed: {:?}", e),
        }
    }
}

pub extern "C" fn jit_load_module_slot(
    ctx: *mut ExecCtx,
    module_val: VmValue,
    slot_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        if !module_val.is_heap() {
            return VmValue::null();
        }
        if let Some(crate::heap::HeapObj::Module(m)) = ctx_ref.heap.get(module_val.as_heap_idx()) {
            m.get_slot(slot_idx).unwrap_or(VmValue::null())
        } else {
            VmValue::null()
        }
    }
}

pub extern "C" fn jit_store_module_slot(ctx: *mut ExecCtx, slot_idx: usize, val_nv: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;

        let exports_nv = if let Some(nv) = ctx_ref.module_exports.get(&frame_idx).copied() {
            nv
        } else {
            panic!("OpStoreModuleSlot: no active module object for current frame");
        };

        if !exports_nv.is_heap() {
            panic!("OpStoreModuleSlot: active module object is not a heap object");
        }

        if let Some(crate::heap::HeapObj::Module(m)) =
            ctx_ref.heap.get_mut(exports_nv.as_heap_idx())
        {
            let m = std::rc::Rc::make_mut(m);
            m.set_slot(slot_idx, val_nv);
        } else {
            panic!("OpStoreModuleSlot: active module object is not a ModuleObj");
        }
    }
}

pub extern "C" fn jit_spawn(ctx: *mut ExecCtx, task_val: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.exec_spawn(task_val).unwrap()
    }
}

pub extern "C" fn jit_push_try(ctx: *mut ExecCtx, catch_ip: usize, err_reg: u32) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_depth = ctx_ref.frames.len();
        crate::exec::exceptions::push_try(
            &mut ctx_ref.try_handlers,
            catch_ip,
            frame_depth,
            err_reg as u8,
        );
    }
}

pub extern "C" fn jit_pop_try(ctx: *mut ExecCtx) {
    unsafe {
        let ctx_ref = &mut *ctx;
        crate::exec::exceptions::pop_try(&mut ctx_ref.try_handlers);
    }
}

pub extern "C" fn jit_throw(ctx: *mut ExecCtx, error: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let err =
            crate::exec::exceptions::build_thrown_error(error, &ctx_ref.heap, &ctx_ref.frames);
        let handler = ctx_ref.try_handlers.pop();
        ctx_ref.jit_panic_exception_handler = handler;
        ctx_ref.jit_panic_exception_error = Some(err.thrown.unwrap_or(VmValue::null()));
        ctx_ref.jit_panic_exception_err_obj = Some(err);

        let buf = ctx_ref.jit_jmp_buf;
        if !buf.is_null() {
            super::ctx::my_longjmp(buf, 1);
        } else {
            panic!("JIT exception triggered but no jump buffer registered");
        }
    }
}

pub extern "C" fn jit_await(ctx: *mut ExecCtx, fut: VmValue, dest_reg: u32, resume_ip: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        ctx_ref.frames[frame_idx].ip = resume_ip;
        ctx_ref.vm_suspend = Some(crate::exec::VmSuspend::Await {
            value: ctx_ref.heap.extract(fut),
            dest_reg: dest_reg as u16,
        });
        ctx_ref.jit_panic_suspend_resume_ip = Some(resume_ip);

        let buf = ctx_ref.jit_jmp_buf;
        if !buf.is_null() {
            super::ctx::my_longjmp(buf, 2);
        } else {
            panic!("JIT suspend triggered but no jump buffer registered");
        }
    }
}

pub extern "C" fn jit_yield(ctx: *mut ExecCtx, val: VmValue, dest_reg: u32, resume_ip: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        ctx_ref.frames[frame_idx].ip = resume_ip;
        ctx_ref.vm_suspend = Some(crate::exec::VmSuspend::Yield {
            value: val,
            dest_reg: dest_reg as u8,
        });
        ctx_ref.jit_panic_suspend_resume_ip = Some(resume_ip);

        let buf = ctx_ref.jit_jmp_buf;
        if !buf.is_null() {
            super::ctx::my_longjmp(buf, 2);
        } else {
            panic!("JIT suspend triggered but no jump buffer registered");
        }
    }
}

pub extern "C" fn jit_build_object_with_shape(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    base: usize,
    start_reg: usize,
    shape_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let Some(shape) = closure_ref.proto.resolved_shape(shape_idx) else {
            return VmValue::null();
        };
        let count = shape.property_names.len();

        let required = base + start_reg + count;
        if ctx_ref.stack.len() < required {
            ctx_ref.stack.resize(required, VmValue::null());
        }
        crate::exec::collections::build_object_with_shape(
            &ctx_ref.stack,
            base + start_reg,
            shape,
            &mut ctx_ref.heap,
        )
    }
}

pub extern "C" fn jit_range(
    ctx: *mut ExecCtx,
    start_reg: usize,
    end_reg: usize,
    flag: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let base = ctx_ref.frames[frame_idx].base;
        let start_val = ctx_ref.stack[base + start_reg];
        let end_val = ctx_ref.stack[base + end_reg];

        let mut temp = vec![start_val, end_val];
        match crate::exec::advanced::invoke_runtime_static(
            "__range__",
            &mut temp,
            &mut ctx_ref.heap,
            flag as u16,
        ) {
            Ok(v) => v,
            Err(e) => panic!("JIT range failed: {:?}", e),
        }
    }
}

#[repr(C)]
pub struct JitClassMemberArgs {
    pub class_val: VmValue,
    pub fn_val: VmValue,
    pub name_idx: usize,
    pub kind: u8,
}

pub extern "C" fn jit_assert_not_null(ctx: *mut ExecCtx, val: VmValue) {
    if let Err(e) = crate::exec::advanced::assert_not_null(val) {
        unsafe {
            let ctx_ref = &mut *ctx;
            jit_propagate_error(ctx_ref, e);
        }
    }
}

pub extern "C" fn jit_close_upvalue(ctx: *mut ExecCtx, lowest: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let base = ctx_ref.frames[frame_idx].base;
        ctx_ref.close_upvalues_above(base + lowest);
    }
}

pub extern "C" fn jit_get_enum_tag(ctx: *mut ExecCtx, val: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match crate::exec::advanced::get_enum_tag(val, &ctx_ref.heap) {
            Ok(tag_val) => tag_val,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_is_array_stub(ctx: *mut ExecCtx, val: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &*ctx;
        VmValue::from_bool(crate::exec::advanced::is_array(val, &ctx_ref.heap))
    }
}

pub extern "C" fn jit_wrap_spread_stub(ctx: *mut ExecCtx, val: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let extracted = ctx_ref.heap.extract(val);
        ctx_ref
            .heap
            .intern(varn_types::Value::Spread(Box::new(extracted)))
    }
}

pub extern "C" fn jit_object_keys_stub(ctx: *mut ExecCtx, val: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match crate::exec::collections::object_keys(val, &mut ctx_ref.heap) {
            Ok(keys_val) => keys_val,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_op_in_stub(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &*ctx;
        VmValue::from_bool(crate::exec::advanced::op_in(a, b, &ctx_ref.heap))
    }
}

pub extern "C" fn jit_object_merge_stub(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match crate::exec::collections::object_merge(a, b, &mut ctx_ref.heap) {
            Ok(res) => res,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_get_fixed_field(ctx: *mut ExecCtx, obj: VmValue, slot: usize) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match crate::exec::props::get_fixed_field(obj, slot, &mut ctx_ref.heap) {
            Ok(val) => val,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_set_fixed_field(ctx: *mut ExecCtx, obj: VmValue, slot: usize, val: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        if let Err(e) = crate::exec::props::set_fixed_field(obj, slot, val, &mut ctx_ref.heap) {
            jit_propagate_error(ctx_ref, e);
        }
    }
}

pub extern "C" fn jit_get_property_maybe_stub(
    ctx: *mut ExecCtx,
    obj: VmValue,
    name_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let name_nv = closure_ref.constants[name_idx];
        let name = ctx_ref.heap.str_val(name_nv).expect("non-string const");
        crate::exec::props::get_property_maybe(obj, &name, &mut ctx_ref.heap)
    }
}

pub extern "C" fn jit_get_super(ctx: *mut ExecCtx, name_idx: usize) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let base = ctx_ref.frames[frame_idx].base;
        let this_val = ctx_ref.stack[base];
        let name_nv = closure_ref.constants[name_idx];
        let name = ctx_ref.heap.str_val(name_nv).expect("non-string const");

        let cls = ctx_ref.frames[frame_idx]
            .current_class
            .clone()
            .or_else(|| crate::exec::props::get_class(this_val, &ctx_ref.heap))
            .expect("GetSuper: 'this' has no class");
        let class_nv = ctx_ref.heap.intern(varn_types::Value::Class(cls));
        match crate::exec::class::op_get_super(class_nv, &name, this_val, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_get_symbol(ctx: *mut ExecCtx, obj: VmValue, sym_idx: usize) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let sym_nv = closure_ref.constants[sym_idx];
        let sym_val = ctx_ref.heap.extract(sym_nv);
        match sym_val {
            varn_types::Value::Symbol(s) => {
                match crate::exec::advanced::get_symbol_property(obj, s, &mut ctx_ref.heap) {
                    Ok(v) => v,
                    Err(e) => jit_propagate_error(ctx_ref, e),
                }
            }
            _ => panic!("GetSymbol: non-symbol constant"),
        }
    }
}

pub extern "C" fn jit_bind_method(ctx: *mut ExecCtx, obj: VmValue, name_idx: usize) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let key_nv = closure_ref.constants[name_idx];
        let key = ctx_ref.heap.str_val(key_nv).expect("non-string const");
        let method = match crate::exec::props::get_property(obj, &key, &mut ctx_ref.heap) {
            Ok(m) => m,
            Err(e) => jit_propagate_error(ctx_ref, e),
        };
        match crate::exec::advanced::bind_method(obj, method, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_define_global(ctx: *mut ExecCtx, src: VmValue, name_idx: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let name_nv = closure_ref.constants[name_idx];
        let name = ctx_ref.heap.str_val(name_nv).expect("non-string const");
        ctx_ref.globals.define(&name, src);
    }
}

pub extern "C" fn jit_store_global(ctx: *mut ExecCtx, src: VmValue, name_idx: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let name_nv = closure_ref.constants[name_idx];
        let name = ctx_ref.heap.str_val(name_nv).expect("non-string const");
        ctx_ref.globals.set_by_name(&name, src);
    }
}

pub extern "C" fn jit_declare_field(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    class_val: VmValue,
    name_idx: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let key_nv = closure_ref.constants[name_idx];
        let key = ctx_ref.heap.str_val(key_nv).expect("non-string const");
        if let Err(e) = crate::exec::class::op_declare_field(class_val, &key, &mut ctx_ref.heap) {
            jit_propagate_error(ctx_ref, e);
        }
    }
}

pub extern "C" fn jit_make_class(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    super_val: VmValue,
    name_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let name_nv = closure_ref.constants[name_idx];
        let name = ctx_ref.heap.str_val(name_nv).expect("non-string const");
        let cls = crate::exec::class::op_class(&name, &mut ctx_ref.heap);
        if !super_val.is_null() {
            if let Err(e) = crate::exec::class::op_inherit(cls, super_val, &mut ctx_ref.heap) {
                jit_propagate_error(ctx_ref, e);
            }
        }
        cls
    }
}

pub extern "C" fn jit_inherit(ctx: *mut ExecCtx, class_val: VmValue, super_val: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        if let Err(e) = crate::exec::class::op_inherit(class_val, super_val, &mut ctx_ref.heap) {
            jit_propagate_error(ctx_ref, e);
        }
    }
}

pub extern "C" fn jit_class_member_op(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    args: *const std::ffi::c_void,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = &*(args as *const JitClassMemberArgs);
        let closure_ref = &*closure;
        let key_nv = closure_ref.constants[args.name_idx];
        let key = ctx_ref.heap.str_val(key_nv).expect("non-string const");

        let res = match args.kind {
            0 => {
                crate::exec::class::op_method(args.class_val, &key, args.fn_val, &mut ctx_ref.heap)
            }
            1 => crate::exec::class::op_define_static(
                args.class_val,
                &key,
                args.fn_val,
                &mut ctx_ref.heap,
            ),
            2 => crate::exec::class::op_define_getter(
                args.class_val,
                &key,
                args.fn_val,
                &mut ctx_ref.heap,
            ),
            3 => crate::exec::class::op_define_setter(
                args.class_val,
                &key,
                args.fn_val,
                &mut ctx_ref.heap,
            ),
            4 => crate::exec::class::op_define_static_getter(
                args.class_val,
                &key,
                args.fn_val,
                &mut ctx_ref.heap,
            ),
            5 => crate::exec::class::op_define_static_setter(
                args.class_val,
                &key,
                args.fn_val,
                &mut ctx_ref.heap,
            ),
            _ => panic!("Unknown class member op kind: {}", args.kind),
        };
        if let Err(e) = res {
            jit_propagate_error(ctx_ref, e);
        }
    }
}

pub extern "C" fn jit_build_object(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    base: usize,
    ip_before: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let code = &closure_ref.proto.chunk.code;

        let mut temp_ip = ip_before;
        let w1 = code[temp_ip];
        temp_ip += 1;
        let count = (w1 & 0xFF) as usize;
        let obj_nv = ctx_ref.heap.alloc_object();
        for _ in 0..count {
            let k_idx = code[temp_ip] as usize;
            temp_ip += 1;
            let w = code[temp_ip];
            temp_ip += 1;
            let val_reg = (w >> 8) as usize;
            let key_nv = closure_ref.constants[k_idx];
            let key = ctx_ref
                .heap
                .str_val(key_nv)
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    closure_ref.proto.chunk.constants[k_idx]
                        .as_str()
                        .unwrap_or("")
                        .to_string()
                });
            let val = ctx_ref.stack[base + val_reg];
            if let Err(e) = crate::exec::props::set_property(obj_nv, &key, val, &mut ctx_ref.heap) {
                jit_propagate_error(ctx_ref, e);
            }
        }
        obj_nv
    }
}

pub extern "C" fn jit_object_rest(ctx: *mut ExecCtx, ip_before: usize) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let base = ctx_ref.frames[frame_idx].base;
        let code = &closure_ref.proto.chunk.code;

        let mut temp_ip = ip_before;
        let w1 = code[temp_ip];
        temp_ip += 1;
        let w2 = code[temp_ip];
        temp_ip += 1;
        let src = (w1 & 0xFF) as usize;
        let skip_count = (w2 >> 8) as usize;
        let mut skip_keys = Vec::with_capacity(skip_count);
        for _ in 0..skip_count {
            let k_idx = code[temp_ip] as usize;
            temp_ip += 1;
            let key_nv = closure_ref.constants[k_idx];
            skip_keys.push(ctx_ref.heap.str_val(key_nv).unwrap_or_else(|| {
                closure_ref.proto.chunk.constants[k_idx]
                    .as_str()
                    .unwrap_or("")
                    .into()
            }));
        }
        let obj = ctx_ref.stack[base + src];
        match ctx_ref.exec_object_rest(obj, &skip_keys) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_make_enum_variant(ctx: *mut ExecCtx, ip_before: usize) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let base = ctx_ref.frames[frame_idx].base;
        let code = &closure_ref.proto.chunk.code;

        let mut temp_ip = ip_before;
        let w1 = code[temp_ip];
        temp_ip += 1;
        let name_idx = code[temp_ip] as usize;
        let tag_reg = (w1 & 0xFF) as usize;
        let name_nv = closure_ref.constants[name_idx];
        let name = ctx_ref.heap.str_val(name_nv).expect("non-string const");
        let tag = ctx_ref.stack[base + tag_reg].as_i32() as u8;

        let name_str = name.as_ref();
        let (name_part, fields_part) = match name_str.find(':') {
            Some(idx) => (&name_str[..idx], &name_str[idx + 1..]),
            None => (name_str, ""),
        };
        let (enum_name_str, variant_name_str) = match name_part.rfind('.') {
            Some(idx) => (&name_part[..idx], &name_part[idx + 1..]),
            None => ("", name_part),
        };
        let fields: Vec<std::rc::Rc<str>> = if fields_part.is_empty() {
            vec![]
        } else {
            fields_part.split(',').map(std::rc::Rc::from).collect()
        };

        let variant =
            varn_types::Value::EnumVariant(Box::new(varn_types::value::EnumVariantData {
                enum_name: std::rc::Rc::from(enum_name_str),
                variant_name: std::rc::Rc::from(variant_name_str),
                variant_tag: tag,
                fields,
                payload: varn_types::Value::Object(varn_types::value::ObjRef::empty()),
            }));
        ctx_ref.heap.intern(variant)
    }
}

pub extern "C" fn jit_call_spread(ctx: *mut ExecCtx, args: *const std::ffi::c_void) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = &*(args as *const varn_jit::JitCallArgs);
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        let base = ctx_ref.frames[frame_idx].base;

        ctx_ref.frames[frame_idx].ip = args.ip;

        let res = ctx_ref.exec_call_spread_reg(
            args.callee,
            base,
            args.arg_start,
            args.arg_count,
            args.dest,
            frame_idx,
        );

        match res {
            Ok(true) => {
                ctx_ref.run_until_inner(caller_depth).unwrap();
            }
            Ok(false) => {}
            Err(e) => {
                jit_propagate_error(ctx_ref, e);
            }
        }

        ctx_ref.stack[base + args.dest]
    }
}
pub extern "C" fn jit_load_module_by_idx(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    spec_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let spec_nv = closure_ref.constants[spec_idx];
        let spec = match ctx_ref.heap.str_val(spec_nv) {
            Some(s) => s,
            None => return VmValue::null(),
        };
        match ctx_ref.load_module_from_source(&spec, &closure_ref.proto.chunk.source_file) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "JIT load_module_by_idx spec='{}', src='{}', err={:?}",
                    spec, closure_ref.proto.chunk.source_file, e
                );
                panic!("JIT load_module failed: {:?}", e);
            }
        }
    }
}
