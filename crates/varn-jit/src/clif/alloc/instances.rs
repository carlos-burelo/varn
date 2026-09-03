use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::FunctionBuilder;

use super::super::emit::call_helper_void;
use super::super::kinds::K;
use super::safepoints::{
    box_or_load_home, def_result, flush_boxed, frame_base_addr, live_boxed, reload_boxed,
    store_home, AllocCtx,
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
    call_helper_void(
        b,
        actx.cc,
        helper,
        &[actx.exec_ctx, actx.base, start_v, shape_v, flags_v],
    );
    let res = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
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
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.build_object,
        &[actx.exec_ctx, actx.closure, actx.base, ipv],
    );
    reload_boxed(b, actx, state, &regs);
    let res = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    );
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
    call_helper_void(b, actx.cc, actx.helpers.object_rest, &[actx.exec_ctx, ipv]);
    let res = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    );
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
    let obj = box_or_load_home(b, actx, state, src);
    let (obj_tag, obj_payload) = b.ins().isplit(obj);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.object_keys,
        &[actx.exec_ctx, obj_tag, obj_payload],
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

pub(crate) fn emit_object_merge(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest_reg = (code[ip] >> 8) as usize;
    let src_reg = (code[ip + 1] >> 8) as usize;
    let dest_val = box_or_load_home(b, actx, state, dest_reg);
    let src_val = box_or_load_home(b, actx, state, src_reg);
    let (dest_tag, dest_payload) = b.ins().isplit(dest_val);
    let (src_tag, src_payload) = b.ins().isplit(src_val);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.object_merge,
        &[actx.exec_ctx, dest_tag, dest_payload, src_tag, src_payload],
    );
    let res = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
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
    let obj = box_or_load_home(b, actx, state, obj_r);
    let (obj_tag, obj_payload) = b.ins().isplit(obj);
    let ni = b.ins().iconst(types::I64, name_idx as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.get_property_maybe,
        &[actx.exec_ctx, obj_tag, obj_payload, ni],
    );
    let res = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
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
    let obj = box_or_load_home(b, actx, state, obj_r);
    let (obj_tag, obj_payload) = b.ins().isplit(obj);
    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.bind_method,
        &[actx.exec_ctx, obj_tag, obj_payload, ni],
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

pub(crate) fn emit_assert_not_null(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let src = (code[ip + 1] >> 8) as usize;
    let val = box_or_load_home(b, actx, state, src);
    let (val_tag, val_payload) = b.ins().isplit(val);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.assert_not_null,
        &[actx.exec_ctx, val_tag, val_payload],
    );
}
