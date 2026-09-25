//! Class-construction lowering for the SSA backend: `MakeClass`, the member
//! definitions (`Method`/`DefineStatic`/accessors), `DeclareField` and
//! `GetSuper`.
//!
//! The runtime helpers are the same the bytecode lowering uses; because a
//! frame-aware SSA body keeps every value in its home, no explicit
//! flush/reload list is needed around them. The member-definition helper takes
//! a small `[class, member, name_idx, kind]` struct, staged on the native
//! stack exactly as the bytecode lowering does.

use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::FunctionBuilder;

use super::heap::boxed_parts;
use super::props::str_idx;
use super::Ctx;

use super::super::emit::{box_null, call_helper_void};

/// `MakeClass name [super]` — a heap class object.
pub(super) fn emit_make_class(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<cranelift_codegen::ir::Value>],
    name: &str,
    super_class: Option<u32>,
) -> Result<cranelift_codegen::ir::Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: MakeClass without a frame")?;
    let name_idx = str_idx(ctx, name)?;
    let (st, sp) = match super_class {
        Some(v) => boxed_parts(b, ctx, values, v)?,
        None => {
            let null = box_null(b);
            b.ins().isplit(null)
        }
    };
    let ectx = frame.exec_ctx;
    let name_v = b.ins().iconst(types::I64, name_idx as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.make_class,
        &[ectx, frame.closure, st, sp, name_v],
    );
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlags::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

/// `DeclareField class, name` — no result.
pub(super) fn emit_declare_field(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<cranelift_codegen::ir::Value>],
    class: u32,
    name: &str,
    tag: Option<varn_core::RuntimeKind>,
) -> Result<(), String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: DeclareField without a frame")?;
    let (ct, cp) = boxed_parts(b, ctx, values, class)?;
    let name_idx = str_idx(ctx, name)?;
    let ectx = frame.exec_ctx;
    let name_v = b.ins().iconst(types::I64, name_idx as i64);
    let tag_v = b
        .ins()
        .iconst(types::I64, varn_core::RuntimeKind::encode(tag) as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.declare_field,
        &[ectx, frame.closure, ct, cp, name_v, tag_v],
    );
    Ok(())
}

/// A member definition (`Method`=0, `DefineStatic`=1, `DefineGetter`=2,
/// `DefineSetter`=3, `DefineStaticGetter`=4, `DefineStaticSetter`=5).
pub(super) fn emit_define_member(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<cranelift_codegen::ir::Value>],
    class: u32,
    name: &str,
    member: u32,
    kind: i64,
) -> Result<(), String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: class member op without a frame")?;
    let (ct, cp) = boxed_parts(b, ctx, values, class)?;
    let (mt, mp) = boxed_parts(b, ctx, values, member)?;
    let name_idx = str_idx(ctx, name)?;
    let ectx = frame.exec_ctx;

    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        48,
        3,
    ));
    b.ins().stack_store(ct, slot, 0);
    b.ins().stack_store(cp, slot, 8);
    b.ins().stack_store(mt, slot, 16);
    b.ins().stack_store(mp, slot, 24);
    let name_v = b.ins().iconst(types::I64, name_idx as i64);
    b.ins().stack_store(name_v, slot, 32);
    let kind_v = b.ins().iconst(types::I64, kind);
    b.ins().stack_store(kind_v, slot, 40);
    let args = b.ins().stack_addr(types::I64, slot, 0);

    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.class_member_op,
        &[ectx, frame.closure, args],
    );
    Ok(())
}

/// `GetSuper name` — a heap result.
pub(super) fn emit_get_super(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    name: &str,
) -> Result<cranelift_codegen::ir::Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: GetSuper without a frame")?;
    let name_idx = str_idx(ctx, name)?;
    let ectx = frame.exec_ctx;
    let name_v = b.ins().iconst(types::I64, name_idx as i64);
    call_helper_void(b, ctx.cc, ctx.helpers.get_super, &[ectx, name_v]);
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlags::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}
