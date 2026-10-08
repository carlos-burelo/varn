use crate::error::VmResult;
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use varn_types::value::RuntimeSymbol;
use varn_types::NativeCtx;

pub(crate) fn get_symbol_property(
    obj: VmValue,
    symbol: RuntimeSymbol,
    heap: &mut Heap,
) -> VmResult<VmValue> {
    let sym_str = symbol.to_string();
    let kind = obj
        .is_heap()
        .then(|| heap.get(obj.as_heap()))
        .flatten()
        .map(|o| match o {
            HeapObj::Array(_) => 1,
            HeapObj::Range(_) => 2,
            HeapObj::Generator(_) => 3,
            HeapObj::Object(_) => 4,
            HeapObj::Str(_)
            | HeapObj::Tuple(_)
            | HeapObj::Record(_)
            | HeapObj::Buffer(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_)
            | HeapObj::VmClosure(_)
            | HeapObj::Class(_)
            | HeapObj::NativeFn(..)
            | HeapObj::BoundMethod(_)
            | HeapObj::Map(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Symbol(_)
            | HeapObj::EnumVariant(_)
            | HeapObj::BigInt(_)
            | HeapObj::Decimal(_)
            | HeapObj::Char(_)
            | HeapObj::Spread(_) => 0,
        })
        .unwrap_or(0);
    let result = match (kind, symbol) {
        (1, RuntimeSymbol::Iterator) => {
            heap.alloc_bound_native(obj, array_symbol_iterator, "[Symbol.iterator]")
        }
        (2, RuntimeSymbol::Iterator) => {
            heap.alloc_bound_native(obj, range_symbol_iterator, "[Symbol.iterator]")
        }

        (3, RuntimeSymbol::Iterator | RuntimeSymbol::AsyncIterator) => {
            heap.alloc_bound_native(obj, generator_symbol_iterator, "[Symbol.iterator]")
        }
        (4, _) => match heap.get(obj.as_heap()) {
            Some(HeapObj::Object(o)) => o.get(sym_str.as_str()).unwrap_or(VmValue::null()),
            Some(
                HeapObj::Str(_)
                | HeapObj::Array(_)
                | HeapObj::Tuple(_)
                | HeapObj::Record(_)
                | HeapObj::Buffer(_)
                | HeapObj::Module(_)
                | HeapObj::FrozenModule(_)
                | HeapObj::VmClosure(_)
                | HeapObj::Class(_)
                | HeapObj::NativeFn(..)
                | HeapObj::BoundMethod(_)
                | HeapObj::Map(_)
                | HeapObj::Set(_)
                | HeapObj::Task(_)
                | HeapObj::TaskHandle(_)
                | HeapObj::Range(_)
                | HeapObj::Symbol(_)
                | HeapObj::EnumVariant(_)
                | HeapObj::BigInt(_)
                | HeapObj::Decimal(_)
                | HeapObj::Char(_)
                | HeapObj::Generator(_)
                | HeapObj::Spread(_),
            )
            | None => VmValue::null(),
        },
        _ => VmValue::null(),
    };
    Ok(result)
}

fn array_symbol_iterator(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> varn_types::NativeFnResult {
    let arr_nv = args.first().copied().unwrap_or(VmValue::null());
    let iter_nv = ctx.alloc_object();
    ctx.set_field(iter_nv, "__arr", arr_nv);
    ctx.set_field(iter_nv, "__idx", VmValue::from_int(0));
    let res_nv = ctx.alloc_object();
    ctx.set_field(iter_nv, "__res", res_nv);
    let next_nv = ctx.alloc_bound_native(iter_nv, array_iter_next, "next");
    ctx.set_field(iter_nv, "next", next_nv);
    Ok(iter_nv)
}

fn array_iter_next(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> varn_types::NativeFnResult {
    let obj_nv = args
        .first()
        .copied()
        .ok_or("array_iter_next: missing receiver")?;
    let arr_nv = ctx.get_field(obj_nv, "__arr").unwrap_or(VmValue::null());
    let idx = ctx.as_int(ctx.get_field(obj_nv, "__idx").unwrap_or(VmValue::null()));
    let arr_len = ctx.array_len(arr_nv);
    let result_nv = ctx
        .get_field(obj_nv, "__res")
        .unwrap_or_else(|| ctx.alloc_object());
    if idx as usize >= arr_len {
        ctx.set_field(result_nv, "value", VmValue::null());
        ctx.set_field(result_nv, "done", VmValue::from_bool(true));
        return Ok(result_nv);
    }
    let item = ctx
        .array_get(arr_nv, idx as usize)
        .unwrap_or(VmValue::null());
    let idx_val = ctx.int_val(idx + 1);
    ctx.set_field(obj_nv, "__idx", idx_val);
    ctx.set_field(result_nv, "value", item);
    ctx.set_field(result_nv, "done", VmValue::from_bool(false));
    Ok(result_nv)
}

fn range_symbol_iterator(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> varn_types::NativeFnResult {
    let range_nv = args
        .first()
        .copied()
        .ok_or("range_symbol_iterator: missing receiver")?;
    if ctx.as_range(range_nv).is_none() {
        return Err("range_symbol_iterator: invalid receiver".into());
    }
    let iter_nv = ctx.alloc_object();
    ctx.set_field(iter_nv, "__range", range_nv);
    ctx.set_field(iter_nv, "__idx", VmValue::from_int(0));
    let res_nv = ctx.alloc_object();
    ctx.set_field(iter_nv, "__res", res_nv);
    let next_nv = ctx.alloc_bound_native(iter_nv, range_iter_next, "next");
    ctx.set_field(iter_nv, "next", next_nv);
    Ok(iter_nv)
}

fn range_iter_next(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> varn_types::NativeFnResult {
    let obj_nv = args
        .first()
        .copied()
        .ok_or("range_iter_next: missing receiver")?;
    let range_nv = ctx.get_field(obj_nv, "__range").unwrap_or(VmValue::null());
    let Some(range) = ctx.as_range(range_nv) else {
        return Err("range_iter_next: invalid range".into());
    };
    let idx = ctx.as_int(ctx.get_field(obj_nv, "__idx").unwrap_or(VmValue::null()));
    let result_nv = ctx
        .get_field(obj_nv, "__res")
        .unwrap_or_else(|| ctx.alloc_object());
    let Some(raw) = range.nth(idx) else {
        ctx.set_field(result_nv, "value", VmValue::null());
        ctx.set_field(result_nv, "done", VmValue::from_bool(true));
        return Ok(result_nv);
    };
    let next_idx = ctx.int_val(idx + 1);
    ctx.set_field(obj_nv, "__idx", next_idx);
    let item = ctx.range_element(&range, raw);
    ctx.set_field(result_nv, "value", item);
    ctx.set_field(result_nv, "done", VmValue::from_bool(false));
    Ok(result_nv)
}

fn generator_symbol_iterator(
    _ctx: &mut dyn NativeCtx,
    args: &[VmValue],
) -> varn_types::NativeFnResult {
    Ok(args.first().copied().unwrap_or(VmValue::null()))
}
