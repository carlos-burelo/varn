use cranelift_codegen::ir::{types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;

use super::super::emit::call_helper_void;
use super::super::kinds::K;
use super::safepoints::{
    box_or_load_home, def_result, flush_boxed, frame_base_addr, live_boxed, reload_boxed, store_home,
    AllocCtx,
};

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
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.load_module,
        &[actx.exec_ctx, actx.closure, spec_v, own_ip_v],
    );
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
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
    let mod_val = box_or_load_home(b, actx, state, src_r);
    let (mod_tag, mod_payload) = b.ins().isplit(mod_val);
    let slot_v = b.ins().iconst(types::I64, slot_idx as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.load_module_slot,
        &[actx.exec_ctx, mod_tag, mod_payload, slot_v],
    );
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
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
    let val = box_or_load_home(b, actx, state, val_r);
    let (val_tag, val_payload) = b.ins().isplit(val);
    let slot_v = b.ins().iconst(types::I64, slot_idx as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.store_module_slot,
        &[actx.exec_ctx, slot_v, val_tag, val_payload],
    );
}
