//! Value-level helpers: constants, globals, upvalues, closures, and the
//! arithmetic and comparison operators compiled code cannot inline.
//!
//! Everything here is a pure value operation over the running `ExecCtx` —
//! no frame is pushed and no call is made.

use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

pub(crate) extern "C" fn jit_load_const(
    closure: *const crate::closure::VmClosure,
    idx: usize,
) -> VmValue {
    unsafe {
        let closure_ref = &*closure;
        closure_ref.constants[idx]
    }
}

pub(crate) extern "C" fn jit_load_global_idx(ctx: *mut ExecCtx, idx: usize) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.globals.get_by_index(idx).unwrap_or(VmValue::null())
    }
}

pub(crate) extern "C" fn jit_store_global_idx(ctx: *mut ExecCtx, idx: usize, val: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.globals.set_by_index(idx, val);
    }
}

pub(crate) extern "C" fn jit_define_global_idx(ctx: *mut ExecCtx, idx: usize, val: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.globals.set_by_index(idx, val);
    }
}

pub(crate) extern "C" fn jit_eq(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let res = crate::exec::compare::eq(a, b, &ctx_ref.heap);
        VmValue::from_bool(res)
    }
}

pub(crate) extern "C" fn jit_neq(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let res = crate::exec::compare::neq(a, b, &ctx_ref.heap);
        VmValue::from_bool(res)
    }
}

pub(crate) extern "C" fn jit_lt(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let res = crate::exec::compare::lt_heap(a, b, &ctx_ref.heap);
        VmValue::from_bool(res)
    }
}

pub(crate) extern "C" fn jit_lte(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let res = crate::exec::compare::lte_heap(a, b, &ctx_ref.heap);
        VmValue::from_bool(res)
    }
}

pub(crate) extern "C" fn jit_gt(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let res = crate::exec::compare::gt_heap(a, b, &ctx_ref.heap);
        VmValue::from_bool(res)
    }
}

pub(crate) extern "C" fn jit_gte(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let res = crate::exec::compare::gte_heap(a, b, &ctx_ref.heap);
        VmValue::from_bool(res)
    }
}

pub(crate) extern "C" fn jit_add(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        crate::exec::arith::add(a, b, &mut ctx_ref.heap)
    }
}

pub(crate) extern "C" fn jit_sub(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        crate::exec::arith::sub(a, b, &mut ctx_ref.heap)
    }
}

pub(crate) extern "C" fn jit_mul(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        crate::exec::arith::mul(a, b, &mut ctx_ref.heap)
    }
}

pub(crate) extern "C" fn jit_to_string(ctx: *mut ExecCtx, v: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        // Same implementation the interpreter's `ToString` runs; the two
        // tiers must not disagree about how a coercion allocates.
        crate::exec::strings::to_string(v, &mut ctx_ref.heap)
    }
}

pub(crate) extern "C" fn jit_load_global(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    name_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let name_nv = closure_ref.constants[name_idx];
        let name = ctx_ref.heap.str_val(name_nv).unwrap();
        ctx_ref
            .globals
            .get_by_name(&name)
            .unwrap_or(VmValue::null())
    }
}

pub(crate) extern "C" fn jit_load_upvalue(
    ctx: *mut ExecCtx,

    closure: *const crate::closure::VmClosure,

    uv_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &*ctx;

        let closure_ref = &*closure;

        let val = closure_ref.upvalues[uv_idx].read(&ctx_ref.stack);
        val
    }
}

pub(crate) extern "C" fn jit_store_upvalue(
    ctx: *mut ExecCtx,

    closure: *const crate::closure::VmClosure,

    uv_idx: usize,

    val: VmValue,
) {
    unsafe {
        let ctx_ref = &mut *ctx;

        let closure_ref = &*closure;

        closure_ref.upvalues[uv_idx].write(val, &mut ctx_ref.stack);
    }
}

pub(crate) extern "C" fn jit_make_closure(
    ctx: *mut ExecCtx,

    closure: *const crate::closure::VmClosure,

    ip_offset: usize,

    base: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let code = &closure_ref.proto.chunk.code;
        let mut ip = ip_offset + 1;

        let w1 = code[ip];

        ip += 1;

        let proto_idx = code[ip] as usize;

        ip += 1;

        let uv_count = (w1 & 0xFF) as usize;

        let proto = match closure_ref.proto.chunk.constants.get(proto_idx) {
            Some(varn_types::PoolEntry::Function(p)) => p.clone(),
            other => panic!("MakeClosure: invalid function proto at ip_offset={} proto_idx={} consts_len={} got={:?} in fn={:?}", ip_offset, proto_idx, closure_ref.proto.chunk.constants.len(), other, closure_ref.proto.name),
        };

        let proto_ptr = std::rc::Rc::as_ptr(&proto) as usize;
        if uv_count == 0 {
            if let Some(&(_, cached_val)) = ctx_ref.static_closures.get(&proto_ptr) {
                return cached_val;
            }
        }

        let mut upvalues = Vec::with_capacity(uv_count);

        for ___ in 0..uv_count {
            let uv_desc = code[ip];

            ip += 1;

            let is_local = (uv_desc >> 8) != 0;

            let index = (uv_desc & 0xFF) as usize;

            if is_local {
                let slot = base + index;

                let captured = ctx_ref.capture_upvalue(slot);

                upvalues.push(captured);
            } else {
                let captured = closure_ref.upvalues[index].clone();

                upvalues.push(captured);
            }
        }

        let constants = ctx_ref
            .proto_constants
            .entry(proto_ptr)
            .or_insert_with(|| {
                let resolved = std::rc::Rc::new(crate::exec::calls::resolve_constants(
                    &proto,
                    &mut ctx_ref.heap,
                ));
                (proto.clone(), resolved)
            })
            .1
            .clone();

        let new_closure = crate::closure::VmClosure::with_upvalues(
            proto.clone(),
            upvalues,
            constants,
            ctx_ref.settings,
        );
        let val = ctx_ref.heap.alloc_vm_closure(std::rc::Rc::new(new_closure));
        if uv_count == 0 {
            ctx_ref.static_closures.insert(proto_ptr, (proto, val));
        }
        val
    }
}

pub(crate) extern "C" fn jit_define_global(ctx: *mut ExecCtx, src: VmValue, name_idx: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let name_nv = closure_ref.constants[name_idx];
        let name = ctx_ref.heap.str_val(name_nv).expect("non-string const");
        ctx_ref.globals.define(&name, src);
    }
}

pub(crate) extern "C" fn jit_store_global(ctx: *mut ExecCtx, src: VmValue, name_idx: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let name_nv = closure_ref.constants[name_idx];
        let name = ctx_ref.heap.str_val(name_nv).expect("non-string const");
        ctx_ref.globals.set_by_name(&name, src);
    }
}
