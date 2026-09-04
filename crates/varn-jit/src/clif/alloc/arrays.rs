use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::FunctionBuilder;

use super::super::emit::call_helper_void;
use super::super::kinds::K;
use super::safepoints::{
    box_or_load_home, def_result, flush_boxed, frame_base_addr, live_boxed, reload_boxed,
    store_home, AllocCtx,
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
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.build_array,
        &[actx.exec_ctx, actx.base, start_v, count_v],
    );
    let res = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
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
    let arr = box_or_load_home(b, actx, state, arr_r);
    let val = box_or_load_home(b, actx, state, val_r);
    let (arr_tag, arr_payload) = b.ins().isplit(arr);
    let (val_tag, val_payload) = b.ins().isplit(val);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.array_push,
        &[actx.exec_ctx, arr_tag, arr_payload, val_tag, val_payload],
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
    let arr = box_or_load_home(b, actx, state, arr_r);
    let src = box_or_load_home(b, actx, state, src_r);
    let (arr_tag, arr_payload) = b.ins().isplit(arr);
    let (src_tag, src_payload) = b.ins().isplit(src);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.array_extend,
        &[actx.exec_ctx, arr_tag, arr_payload, src_tag, src_payload],
    );
    reload_boxed(b, actx, state, &regs);
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
    let obj = box_or_load_home(b, actx, state, obj_r);
    let idx = box_or_load_home(b, actx, state, idx_r);
    let dest_v = b.ins().iconst(types::I64, dest as i64);

    let (obj_tag, obj_payload) = b.ins().isplit(obj);
    let (idx_tag, idx_payload) = b.ins().isplit(idx);

    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        48,
        3,
    ));
    b.ins().stack_store(obj_tag, slot, 0);
    b.ins().stack_store(obj_payload, slot, 8);
    b.ins().stack_store(idx_tag, slot, 16);
    b.ins().stack_store(idx_payload, slot, 24);
    b.ins().stack_store(dest_v, slot, 32);
    let args = b.ins().stack_addr(types::I64, slot, 0);

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(b, actx.cc, actx.helpers.get_index, &[actx.exec_ctx, args]);
    let res = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    );
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
    let obj = box_or_load_home(b, actx, state, obj_r);
    let idx = box_or_load_home(b, actx, state, idx_r);
    let val = box_or_load_home(b, actx, state, val_r);
    let (obj_tag, obj_payload) = b.ins().isplit(obj);
    let (idx_tag, idx_payload) = b.ins().isplit(idx);
    let (val_tag, val_payload) = b.ins().isplit(val);

    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        48,
        3,
    ));
    b.ins().stack_store(obj_tag, slot, 0);
    b.ins().stack_store(obj_payload, slot, 8);
    b.ins().stack_store(idx_tag, slot, 16);
    b.ins().stack_store(idx_payload, slot, 24);
    b.ins().stack_store(val_tag, slot, 32);
    b.ins().stack_store(val_payload, slot, 40);
    let args = b.ins().stack_addr(types::I64, slot, 0);

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(b, actx.cc, actx.helpers.set_index, &[actx.exec_ctx, args]);
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
    let val = box_or_load_home(b, actx, state, src);
    let (val_tag, val_payload) = b.ins().isplit(val);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.wrap_spread,
        &[actx.exec_ctx, val_tag, val_payload],
    );
    let res = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}
