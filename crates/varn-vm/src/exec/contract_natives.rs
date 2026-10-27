use crate::value::VmValue;
use varn_types::{NativeCtx, NativeError, NativeFnResult};

pub(super) fn spread_items(ctx: &dyn NativeCtx, flat: VmValue) -> Vec<VmValue> {
    (0..ctx.array_len(flat))
        .filter_map(|i| ctx.array_get(flat, i))
        .collect()
}

pub(super) fn verify(
    ctx: &mut dyn NativeCtx,
    callee: VmValue,
    args: &[VmValue],
) -> Result<(), NativeError> {
    match ctx.boundary_mut() {
        Some(boundary) => boundary.verify_call(callee, args),
        None => Ok(()),
    }
}

pub(super) fn verify_positional(
    ctx: &mut dyn NativeCtx,
    callee: VmValue,
    args: &[VmValue],
) -> NativeFnResult {
    verify(ctx, callee, args)?;
    Ok(VmValue::null())
}

pub(super) fn verify_flat(
    ctx: &mut dyn NativeCtx,
    callee: VmValue,
    flat: VmValue,
) -> NativeFnResult {
    let items = spread_items(ctx, flat);
    verify(ctx, callee, &items)?;
    Ok(VmValue::null())
}

pub(super) fn method_of(ctx: &dyn NativeCtx, recv: VmValue, name: VmValue) -> VmValue {
    match ctx.boundary() {
        Some(boundary) => boundary.method_callee(recv, &ctx.str_repr_borrowed(name)),
        None => VmValue::null(),
    }
}

pub(super) fn inherited_of(ctx: &dyn NativeCtx, class: VmValue, name: VmValue) -> VmValue {
    match ctx.boundary() {
        Some(boundary) => boundary.class_method(class, &ctx.str_repr_borrowed(name)),
        None => VmValue::null(),
    }
}

pub(super) fn verify_call_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    match args {
        [callee, rest @ ..] => verify_positional(ctx, *callee, rest),
        [] => Ok(VmValue::null()),
    }
}

pub(super) fn verify_spread_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    match args {
        [callee, flat] => verify_flat(ctx, *callee, *flat),
        _ => Ok(VmValue::null()),
    }
}

pub(super) fn verify_method_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    match args {
        [recv, name, rest @ ..] => {
            let callee = method_of(ctx, *recv, *name);
            verify_positional(ctx, callee, rest)
        }
        _ => Ok(VmValue::null()),
    }
}

pub(super) fn verify_method_spread_native(
    ctx: &mut dyn NativeCtx,
    args: &[VmValue],
) -> NativeFnResult {
    match args {
        [recv, name, flat] => {
            let callee = method_of(ctx, *recv, *name);
            verify_flat(ctx, callee, *flat)
        }
        _ => Ok(VmValue::null()),
    }
}

pub(super) fn verify_super_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    match args {
        [class, name, rest @ ..] => {
            let callee = inherited_of(ctx, *class, *name);
            verify_positional(ctx, callee, rest)
        }
        _ => Ok(VmValue::null()),
    }
}

pub(super) fn verify_super_spread_native(
    ctx: &mut dyn NativeCtx,
    args: &[VmValue],
) -> NativeFnResult {
    match args {
        [class, name, flat] => {
            let callee = inherited_of(ctx, *class, *name);
            verify_flat(ctx, callee, *flat)
        }
        _ => Ok(VmValue::null()),
    }
}
