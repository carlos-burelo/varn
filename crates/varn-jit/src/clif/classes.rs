//! Class-construction lowering for CLIF: `MakeClass`, the six member-definition
//! opcodes, `DeclareField`, `Inherit` and `GetSuper`.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use varn_core::OpCode;

use super::alloc::{box_or_load_home, def_result, flush_boxed, live_boxed, reload_boxed, AllocCtx};
use super::emit::call_helper_void;
use super::kinds::K;

pub(super) fn emit_make_class(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let super_reg = (code[ip + 1] >> 8) as usize;
    let name_idx = code[ip + 2] as usize;
    let super_val = if super_reg == 0 {
        super::emit::box_null(b)
    } else {
        box_or_load_home(b, actx, state, super_reg)
    };
    let (super_tag, super_payload) = b.ins().isplit(super_val);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let idx_v = b.ins().iconst(types::I64, name_idx as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.make_class,
        &[actx.exec_ctx, actx.closure, super_tag, super_payload, idx_v],
    );
    reload_boxed(b, actx, state, &regs);
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    );
    def_result(b, actx, dest, res);
}

pub(super) fn emit_class_member_op(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    op: OpCode,
    code: &[u16],
    ip: usize,
) -> Result<(), String> {
    let class_reg = (code[ip + 1] >> 8) as usize;
    let class_val = box_or_load_home(b, actx, state, class_reg);
    let regs = live_boxed(actx, state);

    if op == OpCode::Inherit {
        let super_val = box_or_load_home(b, actx, state, (code[ip + 1] & 0xFF) as usize);
        let (class_tag, class_payload) = b.ins().isplit(class_val);
        let (super_tag, super_payload) = b.ins().isplit(super_val);
        flush_boxed(b, actx, state, &regs);
        call_helper_void(
            b,
            actx.cc,
            actx.helpers.inherit,
            &[actx.exec_ctx, class_tag, class_payload, super_tag, super_payload],
        );
        reload_boxed(b, actx, state, &regs);
        return Ok(());
    }

    let name_idx = code[ip + 2] as usize;
    let idx_v = b.ins().iconst(types::I64, name_idx as i64);

    if op == OpCode::DeclareField {
        let (class_tag, class_payload) = b.ins().isplit(class_val);
        flush_boxed(b, actx, state, &regs);
        call_helper_void(
            b,
            actx.cc,
            actx.helpers.declare_field,
            &[actx.exec_ctx, actx.closure, class_tag, class_payload, idx_v],
        );
        reload_boxed(b, actx, state, &regs);
        return Ok(());
    }

    let kind = match op {
        OpCode::Method => 0,
        OpCode::DefineStatic => 1,
        OpCode::DefineGetter => 2,
        OpCode::DefineSetter => 3,
        OpCode::DefineStaticGetter => 4,
        OpCode::DefineStaticSetter => 5,
        _ => return Err(format!("clif: not a class member op ({op:?})")),
    };
    let fn_val = box_or_load_home(b, actx, state, (code[ip + 1] & 0xFF) as usize);
    let kind_v = b.ins().iconst(types::I64, kind);
    let (class_tag, class_payload) = b.ins().isplit(class_val);
    let (fn_tag, fn_payload) = b.ins().isplit(fn_val);

    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        48,
        3,
    ));
    b.ins().stack_store(class_tag, slot, 0);
    b.ins().stack_store(class_payload, slot, 8);
    b.ins().stack_store(fn_tag, slot, 16);
    b.ins().stack_store(fn_payload, slot, 24);
    b.ins().stack_store(idx_v, slot, 32);
    b.ins().stack_store(kind_v, slot, 40);
    let args = b.ins().stack_addr(types::I64, slot, 0);

    flush_boxed(b, actx, state, &regs);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.class_member_op,
        &[actx.exec_ctx, actx.closure, args],
    );
    reload_boxed(b, actx, state, &regs);
    Ok(())
}

pub(super) fn emit_get_super(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let name_idx = code[ip + 1] as usize;
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let idx_v = b.ins().iconst(types::I64, name_idx as i64);
    call_helper_void(b, actx.cc, actx.helpers.get_super, &[actx.exec_ctx, idx_v]);
    reload_boxed(b, actx, state, &regs);
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    );
    def_result(b, actx, dest, res);
}
