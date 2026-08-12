//! Object, array, closure, string, enum, and upvalue allocation emitters for CLIF lowering.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use varn_types::chunk::{Literal, PoolEntry};
use varn_types::register_meta::RegisterMeta;

use super::super::emit::{
    box_bool, box_f64, box_int, box_or_pass, call_helper, call_helper_void, meta_is_float,
    unbox_bool, unbox_f64_coerce, use_f64, use_int, wrap_i48,
};
use super::super::kinds::K;
use super::safepoints::{
    args_struct, def_result, flush_boxed, frame_base_addr, live_boxed, reload_boxed, store_home,
    AllocCtx,
};

pub(crate) fn emit_build_array(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let w1 = code[ip + 1];
    let w2 = code[ip + 2];
    let dest = (w1 >> 8) as usize;
    let start = (w1 & 0xFF) as usize;
    let count = (w2 >> 8) as usize;

    let fb = frame_base_addr(b, actx);
    for i in 0..count {
        store_home(b, actx, state, fb, start + i);
    }
    let start_v = b.ins().iconst(types::I64, start as i64);
    let count_v = b.ins().iconst(types::I64, count as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.build_array,
        &[actx.exec_ctx, actx.base, start_v, count_v],
    );
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_array_push(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let arr_r = (code[ip] >> 8) as usize;
    let val_r = (code[ip + 1] >> 8) as usize;
    let arr = box_or_pass(b, actx.vars, state, arr_r);
    let val = box_or_pass(b, actx.vars, state, val_r);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    call_helper_void(
        b,
        actx.cc,
        actx.helpers.array_push,
        &[actx.exec_ctx, arr, val],
    );

    reload_boxed(b, actx, state, &regs);
}

pub(crate) fn emit_call_native_op(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    _meta: &[RegisterMeta],
    code: &[u16],
    pool: &[PoolEntry],
    ip: usize,
) -> Result<(), String> {
    let dest = (code[ip] >> 8) as usize;
    let cidx = code[ip + 1] as usize;
    let total = code[ip + 2] as usize;
    let op_id = match pool.get(cidx) {
        Some(PoolEntry::Literal(Literal::Int(i))) => *i as u64,
        _ => return Err("clif: native op-id not an int constant".into()),
    };

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let fb = frame_base_addr(b, actx);
    for r in dest..(dest + total).min(actx.nregs) {
        store_home(b, actx, state, fb, r);
    }
    let args_start = b.ins().iadd_imm(actx.base, dest as i64);
    let total_v = b.ins().iconst(types::I64, total as i64);
    let target = (actx.helpers.resolve_native_op)(op_id);
    let (fn_addr, raw_addr, sig_desc) = (target.func_ptr, target.raw_func_ptr, target.signature);
    if raw_addr != 0 {
        let mut sig = cranelift_codegen::ir::Signature::new(actx.cc);
        let argc = sig_desc.param_count as usize;
        let mut raw_args = Vec::with_capacity(argc);

        for i in 0..argc {
            let r = dest + 1 + i;
            let arg_ty = sig_desc.param_types[i];
            match arg_ty {
                varn_types::ArgType::Int => {
                    sig.params
                        .push(cranelift_codegen::ir::AbiParam::new(types::I64));
                    let v = use_int(b, actx.vars, state, r)?;
                    raw_args.push(v);
                }
                varn_types::ArgType::Float => {
                    sig.params
                        .push(cranelift_codegen::ir::AbiParam::new(types::F64));
                    let f = use_f64(b, actx.vars, state, r)?;
                    raw_args.push(f);
                }
                varn_types::ArgType::Bool => {
                    sig.params
                        .push(cranelift_codegen::ir::AbiParam::new(types::I64));
                    let boxed = box_or_pass(b, actx.vars, state, r);
                    let b_val = unbox_bool(b, boxed);
                    raw_args.push(b_val);
                }
                _ => {
                    sig.params
                        .push(cranelift_codegen::ir::AbiParam::new(types::I64));
                    let boxed = box_or_pass(b, actx.vars, state, r);
                    raw_args.push(boxed);
                }
            }
        }

        match sig_desc.return_type {
            varn_types::ArgType::Float => {
                sig.returns
                    .push(cranelift_codegen::ir::AbiParam::new(types::F64));
            }
            varn_types::ArgType::Void => {}
            _ => {
                sig.returns
                    .push(cranelift_codegen::ir::AbiParam::new(types::I64));
            }
        }

        let sig_ref = b.import_signature(sig);
        let raw_fn_v = b.ins().iconst(types::I64, raw_addr as i64);
        let call = b.ins().call_indirect(sig_ref, raw_fn_v, &raw_args);

        let res = match sig_desc.return_type {
            varn_types::ArgType::Float => {
                let f_res = b.inst_results(call)[0];
                box_f64(b, f_res)
            }
            varn_types::ArgType::Int => {
                let i_res = b.inst_results(call)[0];
                let w = wrap_i48(b, i_res);
                box_int(b, w)
            }
            varn_types::ArgType::Bool => {
                let b_res = b.inst_results(call)[0];
                box_bool(b, b_res)
            }
            varn_types::ArgType::Void => b.ins().iconst(types::I64, 0),
            _ => b.inst_results(call)[0],
        };

        reload_boxed(b, actx, state, &regs);
        def_result(b, actx, dest, res);
        return Ok(());
    }

    let res = if fn_addr != 0 {
        let fn_v = b.ins().iconst(types::I64, fn_addr as i64);
        call_helper(
            b,
            actx.cc,
            actx.helpers.jit_call_native_fnptr,
            &[actx.exec_ctx, fn_v, args_start, total_v],
        )
    } else {
        let op_id_v = b.ins().iconst(types::I64, op_id as i64);
        call_helper(
            b,
            actx.cc,
            actx.helpers.jit_call_native_op,
            &[actx.exec_ctx, op_id_v, args_start, total_v],
        )
    };
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
    Ok(())
}

pub(crate) fn emit_build_object_with_shape(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    count: usize,
    is_record: bool,
) {
    let w1 = code[ip + 1];
    let dest = (w1 >> 8) as usize;
    let start = (w1 & 0xFF) as usize;
    let shape_idx = code[ip + 2] as usize;

    let fb = frame_base_addr(b, actx);
    for i in 0..count {
        store_home(b, actx, state, fb, start + i);
    }
    let start_v = b.ins().iconst(types::I64, start as i64);
    let shape_v = b.ins().iconst(types::I64, shape_idx as i64);
    let helper = if is_record {
        actx.helpers.build_record_with_shape
    } else {
        actx.helpers.build_object_with_shape
    };
    let res = call_helper(
        b,
        actx.cc,
        helper,
        &[actx.exec_ctx, actx.closure, actx.base, start_v, shape_v],
    );
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_make_enum_variant(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip + 1] >> 8) as usize;
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let tag_reg = (code[ip + 1] & 0xFF) as usize;
    let fb = frame_base_addr(b, actx);
    store_home(b, actx, state, fb, tag_reg);

    let ip_v = b.ins().iconst(types::I64, (ip + 1) as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.make_enum_variant,
        &[actx.exec_ctx, ip_v],
    );

    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_get_enum_tag(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;

    let val = box_or_pass(b, actx.vars, state, src);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let res = call_helper(b, actx.cc, actx.helpers.get_enum_tag, &[actx.exec_ctx, val]);

    reload_boxed(b, actx, state, &regs);

    if meta_is_float(meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else {
        b.def_var(actx.vars[dest], res);
    }
}

pub(crate) fn emit_build_str(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let count = (code[ip + 1] >> 8) as usize;

    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        (count * 8) as u32,
        3,
    ));
    let parts_ptr = b.ins().stack_addr(types::I64, slot, 0);

    let vals: Vec<_> = (0..count)
        .map(|i| {
            let r = (code[ip + 2 + i] >> 8) as usize;
            box_or_pass(b, actx.vars, state, r)
        })
        .collect();

    for (i, val) in vals.into_iter().enumerate() {
        b.ins()
            .store(MemFlags::trusted(), val, parts_ptr, (i * 8) as i32);
    }

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let count_v = b.ins().iconst(types::I64, count as i64);

    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.build_str,
        &[actx.exec_ctx, parts_ptr, count_v],
    );

    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_intrinsic(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let w1 = code[ip + 1];
    let wire_byte = (w1 >> 8) as usize;
    let arg_count = (w1 & 0xFF) as usize;

    let fb = frame_base_addr(b, actx);
    for r in dest..dest + arg_count {
        store_home(b, actx, state, fb, r);
    }

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let wire_v = b.ins().iconst(types::I64, wire_byte as i64);
    let start_v = b.ins().iadd_imm(actx.base, dest as i64);
    let count_v = b.ins().iconst(types::I64, arg_count as i64);

    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.dispatch_intrinsic,
        &[actx.exec_ctx, wire_v, start_v, count_v],
    );

    reload_boxed(b, actx, state, &regs);

    if meta_is_float(meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else {
        b.def_var(actx.vars[dest], res);
    }
}

pub(crate) fn emit_to_string(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    meta: &[RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;

    let v = box_or_pass(b, actx.vars, state, src);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let res = call_helper(b, actx.cc, actx.helpers.to_string, &[actx.exec_ctx, v]);

    reload_boxed(b, actx, state, &regs);

    if meta_is_float(meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else {
        b.def_var(actx.vars[dest], res);
    }
}

pub(crate) fn emit_load_upvalue(b: &mut FunctionBuilder, actx: &AllocCtx, code: &[u16], ip: usize) {
    let dest = (code[ip + 1] >> 8) as usize;
    let uv_idx = (code[ip + 1] & 0xFF) as usize;
    let uv_v = b.ins().iconst(types::I64, uv_idx as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.load_upvalue,
        &[actx.exec_ctx, actx.closure, uv_v],
    );
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_store_upvalue(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let uv_idx = (code[ip + 1] >> 8) as usize;
    let src_r = (code[ip + 1] & 0xFF) as usize;
    let uv_v = b.ins().iconst(types::I64, uv_idx as i64);
    let val = box_or_pass(b, actx.vars, state, src_r);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.store_upvalue,
        &[actx.exec_ctx, actx.closure, uv_v, val],
    );
}

pub(crate) fn emit_close_upvalue(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let reg = (code[ip + 1] >> 8) as usize;
    let fb = frame_base_addr(b, actx);
    store_home(b, actx, state, fb, reg);
    let reg_v = b.ins().iconst(types::I64, reg as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.close_upvalue,
        &[actx.exec_ctx, reg_v],
    );
}

pub(crate) fn emit_make_closure(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip + 1] >> 8) as usize;
    let uv_count = (code[ip + 1] & 0xFF) as usize;
    let fb = frame_base_addr(b, actx);
    for i in 0..uv_count {
        let uv_desc = code[ip + 3 + i];
        let is_local = (uv_desc >> 8) != 0;
        let index = (uv_desc & 0xFF) as usize;
        if is_local {
            store_home(b, actx, state, fb, index);
        }
    }
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let ip_v = b.ins().iconst(types::I64, ip as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.make_closure,
        &[actx.exec_ctx, actx.closure, ip_v, actx.base],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_load_static_fn(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let proto_idx = code[ip + 1] as usize;
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let idx_v = b.ins().iconst(types::I64, proto_idx as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.load_static_fn,
        &[actx.exec_ctx, actx.closure, idx_v],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_load_module(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let spec_idx = code[ip + 1] as usize;
    let fb = frame_base_addr(b, actx);
    for r in 0..actx.vars.len() {
        store_home(b, actx, state, fb, r);
    }
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let spec_v = b.ins().iconst(types::I64, spec_idx as i64);
    let own_ip_v = b.ins().iconst(types::I64, ip as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.load_module,
        &[actx.exec_ctx, actx.closure, spec_v, own_ip_v],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_load_module_slot(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src_r = (code[ip + 1] >> 8) as usize;
    let slot_idx = code[ip + 2] as usize;
    let mod_val = box_or_pass(b, actx.vars, state, src_r);
    let slot_v = b.ins().iconst(types::I64, slot_idx as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.load_module_slot,
        &[actx.exec_ctx, mod_val, slot_v],
    );
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_store_module_slot(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let val_r = (code[ip] >> 8) as usize;
    let slot_idx = code[ip + 1] as usize;
    let val = box_or_pass(b, actx.vars, state, val_r);
    let slot_v = b.ins().iconst(types::I64, slot_idx as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.store_module_slot,
        &[actx.exec_ctx, slot_v, val],
    );
}

pub(crate) fn emit_get_index(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let obj_r = (code[ip + 1] >> 8) as usize;
    let idx_r = (code[ip + 1] & 0xFF) as usize;
    let obj = box_or_pass(b, actx.vars, state, obj_r);
    let idx = box_or_pass(b, actx.vars, state, idx_r);
    let dest_v = b.ins().iconst(types::I64, dest as i64);
    let args = args_struct(b, &[obj, idx, dest_v]);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(b, actx.cc, actx.helpers.get_index, &[actx.exec_ctx, args]);
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_set_index(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let obj_r = (code[ip] >> 8) as usize;
    let idx_r = (code[ip + 1] >> 8) as usize;
    let val_r = (code[ip + 1] & 0xFF) as usize;
    let obj = box_or_pass(b, actx.vars, state, obj_r);
    let idx = box_or_pass(b, actx.vars, state, idx_r);
    let val = box_or_pass(b, actx.vars, state, val_r);
    let args = args_struct(b, &[obj, idx, val]);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(b, actx.cc, actx.helpers.set_index, &[actx.exec_ctx, args]);
    reload_boxed(b, actx, state, &regs);
}

pub(crate) fn emit_try_push(b: &mut FunctionBuilder, actx: &AllocCtx, code: &[u16], ip: usize) {
    let err_reg = (code[ip + 1] >> 8) as u32;
    let offset_hi = code[ip + 2] as usize;
    let offset_lo = code[ip + 3] as usize;
    let catch_offset = (offset_hi << 16) | offset_lo;
    let catch_ip = ip + 4 + catch_offset;
    let ip_v = b.ins().iconst(types::I64, catch_ip as i64);
    let err_v = b.ins().iconst(types::I64, err_reg as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.try_push,
        &[actx.exec_ctx, ip_v, err_v],
    );
}

pub(crate) fn emit_try_pop(b: &mut FunctionBuilder, actx: &AllocCtx) {
    call_helper_void(b, actx.cc, actx.helpers.try_pop, &[actx.exec_ctx]);
}

pub(crate) fn emit_throw(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let src = (code[ip + 1] >> 8) as usize;
    let val = box_or_pass(b, actx.vars, state, src);
    call_helper_void(b, actx.cc, actx.helpers.throw, &[actx.exec_ctx, val]);
}

pub(crate) fn emit_yield(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest_reg = (code[ip + 1] >> 8) as u32;
    let src = (code[ip + 1] & 0xFF) as usize;
    let val = box_or_pass(b, actx.vars, state, src);
    let fb = frame_base_addr(b, actx);
    for r in 0..actx.vars.len() {
        store_home(b, actx, state, fb, r);
    }
    let dest_v = b.ins().iconst(types::I64, dest_reg as i64);
    let resume_v = b.ins().iconst(types::I64, (ip + 2) as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.yield_helper,
        &[actx.exec_ctx, val, dest_v, resume_v],
    );
}

pub(crate) fn emit_await(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let val = box_or_pass(b, actx.vars, state, src);
    let fb = frame_base_addr(b, actx);
    for r in 0..actx.vars.len() {
        store_home(b, actx, state, fb, r);
    }
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let dest_v = b.ins().iconst(types::I64, dest as i64);
    let resume_v = b.ins().iconst(types::I64, (ip + 2) as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.await_helper,
        &[actx.exec_ctx, val, dest_v, resume_v],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_spawn(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let val = box_or_pass(b, actx.vars, state, src);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(b, actx.cc, actx.helpers.spawn, &[actx.exec_ctx, val]);
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_build_object(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip + 1] >> 8) as usize;
    let count = (code[ip + 1] & 0xFF) as usize;
    let fb = frame_base_addr(b, actx);
    let mut cur_ip = ip + 2;
    for _ in 0..count {
        cur_ip += 1;
        let val_reg = (code[cur_ip] >> 8) as usize;
        cur_ip += 1;
        store_home(b, actx, state, fb, val_reg);
    }
    let ipv = b.ins().iconst(types::I64, (ip + 1) as i64);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.build_object,
        &[actx.exec_ctx, actx.closure, actx.base, ipv],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_object_rest(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip + 1] >> 8) as usize;
    let src = (code[ip + 1] & 0xFF) as usize;
    let fb = frame_base_addr(b, actx);
    store_home(b, actx, state, fb, src);
    let ipv = b.ins().iconst(types::I64, (ip + 1) as i64);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(b, actx.cc, actx.helpers.object_rest, &[actx.exec_ctx, ipv]);
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_object_keys(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let obj = box_or_pass(b, actx.vars, state, src);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(b, actx.cc, actx.helpers.object_keys, &[actx.exec_ctx, obj]);
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_object_merge(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest_reg = (code[ip] >> 8) as usize;
    let src_reg = (code[ip + 1] >> 8) as usize;
    let dest_val = box_or_pass(b, actx.vars, state, dest_reg);
    let src_val = box_or_pass(b, actx.vars, state, src_reg);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.object_merge,
        &[actx.exec_ctx, dest_val, src_val],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest_reg, res);
}

pub(crate) fn emit_get_property_maybe(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let obj_r = (code[ip + 1] >> 8) as usize;
    let name_idx = code[ip + 2] as usize;
    let obj = box_or_pass(b, actx.vars, state, obj_r);
    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.get_property_maybe,
        &[actx.exec_ctx, obj, ni],
    );
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_bind_method(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip + 1] >> 8) as usize;
    let obj_r = (code[ip + 1] & 0xFF) as usize;
    let name_idx = code[ip + 2] as usize;
    let obj = box_or_pass(b, actx.vars, state, obj_r);
    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.bind_method,
        &[actx.exec_ctx, obj, ni],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_assert_not_null(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let src = (code[ip + 1] >> 8) as usize;
    let val = box_or_pass(b, actx.vars, state, src);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.assert_not_null,
        &[actx.exec_ctx, val],
    );
}

pub(crate) fn emit_array_extend(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let arr_r = (code[ip] >> 8) as usize;
    let src_r = (code[ip + 1] >> 8) as usize;
    let arr = box_or_pass(b, actx.vars, state, arr_r);
    let src = box_or_pass(b, actx.vars, state, src_r);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.array_extend,
        &[actx.exec_ctx, arr, src],
    );
    reload_boxed(b, actx, state, &regs);
}

pub(crate) fn emit_wrap_spread(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let src = (code[ip + 1] >> 8) as usize;
    let val = box_or_pass(b, actx.vars, state, src);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let res = call_helper(b, actx.cc, actx.helpers.wrap_spread, &[actx.exec_ctx, val]);
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

pub(crate) fn emit_call_spread(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    meta: &[varn_types::register_meta::RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let w1 = code[ip + 1];
    let w2 = code[ip + 2];
    let dest = (w1 >> 8) as usize;
    let callee_reg = (w1 & 0xFF) as usize;
    let argc = (w2 >> 8) as usize;
    let arg_start = (w2 & 0xFF) as usize;

    let callee = box_or_pass(b, actx.vars, state, callee_reg);
    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        40,
        0,
    ));

    let arg_start_v = b.ins().iconst(types::I64, arg_start as i64);
    let argc_v = b.ins().iconst(types::I64, argc as i64);
    let dest_v = b.ins().iconst(types::I64, dest as i64);
    let next_ip_v = b.ins().iconst(types::I64, (ip + 3) as i64);

    b.ins().stack_store(callee, slot, 0);
    b.ins().stack_store(arg_start_v, slot, 8);
    b.ins().stack_store(argc_v, slot, 16);
    b.ins().stack_store(dest_v, slot, 24);
    b.ins().stack_store(next_ip_v, slot, 32);

    let slot_addr = b.ins().stack_addr(types::I64, slot, 0);

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let fb = frame_base_addr(b, actx);
    for r in arg_start..(arg_start + argc).min(actx.nregs) {
        store_home(b, actx, state, fb, r);
    }

    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.call_spread,
        &[actx.exec_ctx, slot_addr],
    );

    reload_boxed(b, actx, state, &regs);

    if meta.get(dest).map_or(false, |m| {
        m.kind == varn_types::register_meta::SlotKind::Int
    }) {
        let s = b.ins().ishl_imm(res, 16);
        let un = b.ins().sshr_imm(s, 16);
        b.def_var(actx.vars[dest], un);
    } else {
        b.def_var(actx.vars[dest], res);
    }
}

pub(crate) fn emit_invoke_runtime_static(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) -> Result<(), String> {
    let dest = (code[ip + 1] >> 8) as usize;
    let start_reg = (code[ip + 3] & 0xFF) as usize;
    let end_reg = (code[ip + 4] >> 8) as usize;
    let flag = (code[ip + 4] & 0xFF) as usize;

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let fb = frame_base_addr(b, actx);
    for r in [start_reg, end_reg] {
        store_home(b, actx, state, fb, r);
    }

    let start_v = b.ins().iconst(types::I64, start_reg as i64);
    let end_v = b.ins().iconst(types::I64, end_reg as i64);
    let flag_v = b.ins().iconst(types::I64, flag as i64);

    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.range,
        &[actx.exec_ctx, start_v, end_v, flag_v],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
    Ok(())
}
