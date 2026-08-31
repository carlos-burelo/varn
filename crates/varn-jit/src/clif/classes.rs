//! Class-construction lowering for CLIF: `MakeClass`, the six member-definition
//! opcodes, `DeclareField`, `Inherit` and `GetSuper`.
//!
//! Split out of `clif::alloc`, which is the allocation-path lowering and had
//! grown past this repo's refactor threshold. These opcodes allocate, so they
//! keep the same flush/reload discipline, but class construction is its own
//! domain: it runs once per class at definition time, never in a hot loop, and
//! shares nothing with the array/string/object arms beyond that discipline.

use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::FunctionBuilder;
use varn_core::OpCode;

use super::alloc::{args_struct, def_result, flush_boxed, live_boxed, reload_boxed, AllocCtx};
use super::emit::{box_or_pass, call_helper, call_helper_void};
use super::kinds::K;

/// `MakeClass dest, super_reg, name_idx` — allocate the class object and, when
/// a superclass register is named, inherit from it. `super_reg == 0` means "no
/// superclass" (register 0 is the callee/`this` slot, never a class operand),
/// exactly as the interpreter reads it.
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
        b.ins()
            .iconst(types::I64, varn_types::VmValue::null().raw_tag() as i64)
    } else {
        box_or_pass(b, actx.vars, state, super_reg)
    };
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let idx_v = b.ins().iconst(types::I64, name_idx as i64);
    let res = call_helper(
        b,
        actx.cc,
        actx.helpers.make_class,
        &[actx.exec_ctx, actx.closure, super_val, idx_v],
    );
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}

/// `Method|DefineStatic|DefineGetter|DefineSetter|DefineStaticGetter|
/// DefineStaticSetter class_reg, fn_reg, name_idx` — attach one member to a
/// class. `DeclareField` and `Inherit` share the shape family but have their
/// own helpers (the member helper only knows the six method kinds).
pub(super) fn emit_class_member_op(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    op: OpCode,
    code: &[u16],
    ip: usize,
) -> Result<(), String> {
    let class_reg = (code[ip + 1] >> 8) as usize;
    let class_val = box_or_pass(b, actx.vars, state, class_reg);
    let regs = live_boxed(actx, state);

    if op == OpCode::Inherit {
        // `Inherit class_reg, super_reg` — no name constant.
        let super_val = box_or_pass(b, actx.vars, state, (code[ip + 1] & 0xFF) as usize);
        flush_boxed(b, actx, state, &regs);
        call_helper_void(
            b,
            actx.cc,
            actx.helpers.inherit,
            &[actx.exec_ctx, class_val, super_val],
        );
        reload_boxed(b, actx, state, &regs);
        return Ok(());
    }

    let name_idx = code[ip + 2] as usize;
    let idx_v = b.ins().iconst(types::I64, name_idx as i64);

    if op == OpCode::DeclareField {
        flush_boxed(b, actx, state, &regs);
        call_helper_void(
            b,
            actx.cc,
            actx.helpers.declare_field,
            &[actx.exec_ctx, actx.closure, class_val, idx_v],
        );
        reload_boxed(b, actx, state, &regs);
        return Ok(());
    }

    // `JitClassMemberArgs { class_val, fn_val, name_idx, kind }` — the kind
    // discriminant the helper switches on.
    let kind = match op {
        OpCode::Method => 0,
        OpCode::DefineStatic => 1,
        OpCode::DefineGetter => 2,
        OpCode::DefineSetter => 3,
        OpCode::DefineStaticGetter => 4,
        OpCode::DefineStaticSetter => 5,
        _ => return Err(format!("clif: not a class member op ({op:?})")),
    };
    let fn_val = box_or_pass(b, actx.vars, state, (code[ip + 1] & 0xFF) as usize);
    let kind_v = b.ins().iconst(types::I64, kind);
    let args = args_struct(b, &[class_val, fn_val, idx_v, kind_v]);
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
    let res = call_helper(b, actx.cc, actx.helpers.get_super, &[actx.exec_ctx, idx_v]);
    reload_boxed(b, actx, state, &regs);
    def_result(b, actx, dest, res);
}
