mod build;
mod extra_shared;
mod invoke;
mod modules;
mod props;
mod suspend;

use super::store::Out;
use super::Ctx;
use cranelift_codegen::ir::Value;
use cranelift_frontend::FunctionBuilder;
use varn_types::ssa::SsaOp;

pub(super) fn try_emit(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    op: &SsaOp,
    dest: Option<u32>,
) -> Result<Option<Option<Out>>, String> {
    match op {
        SsaOp::LoadGlobal(name) => modules::load_global(b, ctx, name),
        SsaOp::StoreGlobal { name, value } => modules::store_global(b, ctx, values, name, *value),
        SsaOp::BuildTuple { elements } => build::tuple(b, ctx, values, elements),
        SsaOp::BuildArraySpread { elements } => build::array_spread(b, ctx, values, elements),
        SsaOp::BuildObjectSpread { parts, cs_base } => {
            build::object_spread(b, ctx, values, parts, *cs_base)
        }
        SsaOp::ObjectMerge { target, source } => {
            build::object_merge(b, ctx, values, *target, *source)
        }
        SsaOp::ObjectRest { object, skip_keys } => {
            build::object_rest(b, ctx, values, *object, skip_keys)
        }
        SsaOp::GetPropertyMaybe { object, name } => {
            props::property_maybe(b, ctx, values, *object, name)
        }
        SsaOp::AssertNotNull { operand } => props::assert_not_null(b, ctx, values, *operand),
        SsaOp::BindMethod { object, name } => props::bind_method(b, ctx, values, *object, name),
        SsaOp::ArrayExtend { array, source } => {
            props::array_extend(b, ctx, values, *array, *source)
        }
        SsaOp::WrapSpread { operand } => props::wrap_spread(b, ctx, values, *operand),
        SsaOp::Range {
            start,
            end,
            inclusive,
        } => props::range(b, ctx, values, *start, *end, *inclusive),
        SsaOp::GetSymbol { object, is_async } => {
            props::get_symbol(b, ctx, values, *object, *is_async)
        }
        SsaOp::IterCall { callee, recv } => invoke::iter_call(b, ctx, values, *callee, *recv),
        SsaOp::SuperCall { args } => invoke::super_call(b, ctx, values, args),
        SsaOp::SuperMethodCall { name, args } => {
            invoke::super_method_call(b, ctx, values, name, args)
        }
        SsaOp::ExtensionCall {
            func,
            slot,
            recv,
            args,
        } => invoke::extension_call(b, ctx, values, func, *slot, *recv, args),
        SsaOp::CallSpread { callee, args } => invoke::call_spread(b, ctx, values, *callee, args),
        SsaOp::LoadModule {
            source,
            own_ip,
            live,
        } => modules::load_module(b, ctx, values, source, *own_ip, live),
        SsaOp::ModuleSlot { object, slot } => modules::module_slot(b, ctx, values, *object, *slot),
        SsaOp::StoreModuleSlot { slot, value } => {
            modules::store_module_slot(b, ctx, values, *slot, *value)
        }
        SsaOp::Await {
            operand,
            resume_ip,
            live,
        } => suspend::await_(b, ctx, values, *operand, *resume_ip, live, dest),
        SsaOp::Spawn { operand } => suspend::spawn(b, ctx, values, *operand),
        SsaOp::Yield {
            operand,
            resume_ip,
            live,
        } => suspend::yield_(b, ctx, values, *operand, *resume_ip, live, dest),
        SsaOp::Dispose { var, is_await, cs } => {
            suspend::dispose(b, ctx, values, *var, *is_await, *cs)
        }
        SsaOp::ConstInt(_) | SsaOp::ConstFloat(_) | SsaOp::ConstBool(_) | SsaOp::ConstNull | SsaOp::ConstStr(_) | SsaOp::Binary { .. } | SsaOp::Unary { .. } | SsaOp::SelfCall { .. } | SsaOp::Call { .. } | SsaOp::AllocInstance { .. } | SsaOp::LoadGlobalIdx(_) | SsaOp::ArrayGetIndex { .. } | SsaOp::ArraySetIndex { .. } | SsaOp::ConstChar(_) | SsaOp::ConstBigInt(_) | SsaOp::ConstDecimal(_) | SsaOp::MakeEnumVariant { .. } | SsaOp::IntrinsicCall { .. } | SsaOp::LoadNativeGlobalIdx(_) | SsaOp::Try { .. } | SsaOp::PopTry | SsaOp::CatchParam { .. } | SsaOp::StoreGlobalIdx { .. } | SsaOp::MakeClosure { .. } | SsaOp::LoadCaptured { .. } | SsaOp::StoreCaptured { .. } | SsaOp::LoadUpvalue(_) | SsaOp::StoreUpvalue { .. } | SsaOp::CloseUpvalues { .. } | SsaOp::Cast { .. } | SsaOp::Convert { .. } | SsaOp::IsNull { .. } | SsaOp::Typeof { .. } | SsaOp::ToString { .. } | SsaOp::IsArray { .. } | SsaOp::GetEnumTag { .. } | SsaOp::ObjectKeys { .. } | SsaOp::BuildStr { .. } | SsaOp::BuildArray { .. } | SsaOp::BuildMap { .. } | SsaOp::BuildObject { .. } | SsaOp::GetProperty { .. } | SsaOp::SetProperty { .. } | SsaOp::GetIndex { .. } | SsaOp::SetIndex { .. } | SsaOp::ArrayLength { .. } | SsaOp::StrLength { .. } | SsaOp::BytesLength { .. } | SsaOp::MethodCall { .. } | SsaOp::CallNativeOp { .. } | SsaOp::ArrayPush { .. } | SsaOp::This | SsaOp::GetFixedField { .. } | SsaOp::SetFixedField { .. } | SsaOp::MakeClass { .. } | SsaOp::DeclareLayout { .. } | SsaOp::DefineMethod { .. } | SsaOp::GetSuper { .. } => Ok(None),
    }
}
