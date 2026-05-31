#![allow(dead_code)]

use varn_core::OpCode;
use crate::codegen::CodegenCtx;

pub(crate) mod class;
pub(crate) mod objects;
pub(crate) mod globals;
pub(crate) mod helpers;

pub(super) const NULL_BITS: u64 = 0x7FF9_0000_0000_0000;

pub(crate) fn emit_misc_ops(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
    match op {
        OpCode::AssertNotNull => helpers::emit_assert_not_null(ctx, first_reg),
        OpCode::CloseUpvalue => helpers::emit_close_upvalue(ctx, first_reg),
        OpCode::GetEnumTag => helpers::emit_get_enum_tag(ctx, first_reg),
        OpCode::IsArray => helpers::emit_is_array(ctx, first_reg),
        OpCode::WrapSpread => helpers::emit_wrap_spread(ctx, first_reg),
        OpCode::ObjectKeys => helpers::emit_object_keys(ctx, first_reg),
        OpCode::ObjectMerge => helpers::emit_object_merge(ctx, first_reg),
        OpCode::In => helpers::emit_op_in(ctx, first_reg),
        OpCode::GetFixedField => objects::emit_get_fixed_field(ctx, first_reg),
        OpCode::SetFixedField => objects::emit_set_fixed_field(ctx, first_reg),
        OpCode::GetPropertyMaybe => objects::emit_get_property_maybe(ctx, first_reg),
        OpCode::GetSuper => objects::emit_get_super(ctx, first_reg),
        OpCode::GetSymbol => objects::emit_get_symbol(ctx, first_reg),
        OpCode::BindMethod => helpers::emit_bind_method(ctx, first_reg),
        OpCode::DefineGlobal => globals::emit_define_global(ctx, first_reg),
        OpCode::StoreGlobal => globals::emit_store_global(ctx, first_reg),
        OpCode::DeclareField => class::emit_declare_field(ctx, first_reg),
        OpCode::MakeClass => class::emit_make_class(ctx, first_reg),
        OpCode::Inherit => class::emit_inherit(ctx, first_reg),
        OpCode::Method => class::emit_class_member_op(ctx, first_reg, 0),
        OpCode::DefineStatic => class::emit_class_member_op(ctx, first_reg, 1),
        OpCode::DefineGetter => class::emit_class_member_op(ctx, first_reg, 2),
        OpCode::DefineSetter => class::emit_class_member_op(ctx, first_reg, 3),
        OpCode::DefineStaticGetter => class::emit_class_member_op(ctx, first_reg, 4),
        OpCode::DefineStaticSetter => class::emit_class_member_op(ctx, first_reg, 5),
        OpCode::BuildObject => objects::emit_build_object(ctx, first_reg),
        OpCode::ObjectRest => objects::emit_object_rest(ctx, first_reg),
        OpCode::MakeEnumVariant => helpers::emit_make_enum_variant(ctx, first_reg),
        OpCode::CallSpread => helpers::emit_call_spread(ctx, first_reg),
        OpCode::LoadModule => helpers::emit_load_module(ctx, first_reg),
        OpCode::StoreModuleSlot => helpers::emit_store_module_slot(ctx, first_reg),
        OpCode::Spawn => helpers::emit_spawn(ctx, first_reg),
        OpCode::Try => helpers::emit_try(ctx, first_reg),
        OpCode::PopTry => helpers::emit_pop_try(ctx),
        OpCode::Throw => helpers::emit_throw(ctx),
        OpCode::Await => helpers::emit_await(ctx, first_reg),
        OpCode::Yield => helpers::emit_yield(ctx, first_reg),
        _ => unreachable!("emit_misc_ops called with {:?}", op),
    }
    Ok(())
}
