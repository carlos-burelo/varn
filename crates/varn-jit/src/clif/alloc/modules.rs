use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::FunctionBuilder;

use super::super::emit::{box_or_pass, call_helper, call_helper_void};
use super::super::kinds::K;
use super::safepoints::{
    def_result, flush_boxed, frame_base_addr, live_boxed, reload_boxed, store_home, AllocCtx,
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
