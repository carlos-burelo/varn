use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::FunctionBuilder;

use super::heap::boxed_parts;
use super::props::str_idx;
use super::Ctx;

use super::super::emit::{box_null, call_helper_void};

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
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

pub(super) fn emit_alloc_instance(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<cranelift_codegen::ir::Value>],
    class: u32,
) -> Result<cranelift_codegen::ir::Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: AllocInstance without a frame")?;
    let (ct, cp) = boxed_parts(b, ctx, values, class)?;
    let ectx = frame.exec_ctx;
    call_helper_void(b, ctx.cc, ctx.helpers.alloc_instance, &[ectx, ct, cp]);
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

pub(super) fn emit_declare_layout(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<cranelift_codegen::ir::Value>],
    class: u32,
    layout: &varn_core::layout::ClassLayout,
) -> Result<(), String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: DeclareLayout without a frame")?;
    let (ct, cp) = boxed_parts(b, ctx, values, class)?;
    let layout_idx = ctx
        .proto
        .chunk
        .constants
        .iter()
        .position(|e| matches!(e, varn_types::PoolEntry::Layout(l) if l.as_ref() == layout))
        .ok_or("from_ssa: class layout not in pool")?;
    let ectx = frame.exec_ctx;
    let idx_v = b.ins().iconst(types::I64, layout_idx as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.declare_layout,
        &[ectx, frame.closure, ct, cp, idx_v],
    );
    Ok(())
}

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
    b.ins().stack_store(types::I64, ct, slot, 0);
    b.ins().stack_store(types::I64, cp, slot, 8);
    b.ins().stack_store(types::I64, mt, slot, 16);
    b.ins().stack_store(types::I64, mp, slot, 24);
    let name_v = b.ins().iconst(types::I64, name_idx as i64);
    b.ins().stack_store(types::I64, name_v, slot, 32);
    let kind_v = b.ins().iconst(types::I64, kind);
    b.ins().stack_store(types::I64, kind_v, slot, 40);
    let args = b.ins().stack_addr(types::I64, slot, 0);

    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.class_member_op,
        &[ectx, frame.closure, args],
    );
    Ok(())
}

pub(super) fn emit_get_super(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    name: &str,
) -> Result<cranelift_codegen::ir::Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: GetSuper without a frame")?;
    frame.base.ok_or(super::NEEDS_ACTIVATION)?;
    let name_idx = str_idx(ctx, name)?;
    let ectx = frame.exec_ctx;
    let name_v = b.ins().iconst(types::I64, name_idx as i64);
    call_helper_void(b, ctx.cc, ctx.helpers.get_super, &[ectx, name_v]);
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

pub(super) fn emit_make_enum_variant(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    tag: i64,
    meta: &str,
) -> Result<cranelift_codegen::ir::Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: MakeEnumVariant without a frame")?;
    let meta_v = b.ins().iconst(types::I64, str_idx(ctx, meta)? as i64);
    let tag_v = b.ins().iconst(types::I64, tag);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.make_enum_variant_const,
        &[frame.exec_ctx, frame.closure, tag_v, meta_v],
    );
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        frame.exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}
