use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::FunctionBuilder;

use super::super::emit::{box_or_pass, call_helper, call_helper_void};
use super::super::kinds::K;
use super::safepoints::{
    def_result, flush_boxed, frame_base_addr, live_boxed, reload_boxed, store_home, AllocCtx,
};

/// `shape_ptr` es el `Shape` ya resuelto en compilación y `may_hold_closure`
/// dice si algún campo puede ser una closure. Ambos se resuelven aquí porque el
/// helper los repetía por objeto: eran 5,9 ns de los 41,5 que cuesta crear uno.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_build_object_with_shape(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    count: usize,
    is_record: bool,
    shape_ptr: usize,
    may_hold_closure: bool,
) {
    let w1 = code[ip + 1];
    let dest = (w1 >> 8) as usize;
    let start = (w1 & 0xFF) as usize;

    let fb = frame_base_addr(b, actx);
    for i in 0..count {
        store_home(b, actx, state, fb, start + i);
    }
    let start_v = b.ins().iconst(types::I64, start as i64);
    let shape_v = b.ins().iconst(types::I64, shape_ptr as i64);
    let flags_v = b
        .ins()
        .iconst(types::I64, if may_hold_closure { 1 } else { 0 });
    let helper = if is_record {
        actx.helpers.build_record_with_shape
    } else {
        actx.helpers.build_object_with_shape
    };
    let res = call_helper(
        b,
        actx.cc,
        helper,
        &[actx.exec_ctx, actx.base, start_v, shape_v, flags_v],
    );
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
