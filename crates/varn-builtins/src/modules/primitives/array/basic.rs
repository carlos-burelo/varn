use varn_op_macros::varn_module;
use varn_types::{NativeCtx, NativeFnResult, Value, VmValue};

fn vm_eq(ctx: &dyn NativeCtx, a: VmValue, b: VmValue) -> bool {
    if a == b {
        return true;
    }
    if a.is_sso() && b.is_sso() {
        let mut ba = [0u8; 5];
        let mut bb = [0u8; 5];
        return a.sso_as_str(&mut ba) == b.sso_as_str(&mut bb);
    }
    if ctx.is_string(a) && ctx.is_string(b) {
        return ctx.str_owned(a) == ctx.str_owned(b);
    }
    if a.is_f64() && b.is_f64() {
        return a.as_f64() == b.as_f64();
    }
    false
}

pub mod basic {
    use super::*;

    pub fn is_array(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let Some(&v) = args.first() {
            return Ok(VmValue::from_bool(ctx.is_array(v)));
        }
        Ok(VmValue::bool_false())
    }

    pub fn length(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let Some(&v) = args.first() {
            let len = ctx.array_len(v);
            return Ok(VmValue::from_int(len as i64));
        }
        Ok(VmValue::from_int(0))
    }

    pub fn push(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let Some(&arr) = args.first() {
            if let Some(&item) = args.get(1) {
                ctx.array_push(arr, item);
                return Ok(VmValue::from_int(ctx.array_len(arr) as i64));
            }
        }
        Ok(VmValue::null())
    }

    pub fn pop(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let Some(&arr) = args.first() {
            return Ok(ctx.array_pop(arr).unwrap_or(VmValue::null()));
        }
        Ok(VmValue::null())
    }

    pub fn index_of(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let (Some(&arr), Some(&target)) = (args.first(), args.get(1)) {
            let len = ctx.array_len(arr);
            for i in 0..len {
                if let Some(v) = ctx.array_get(arr, i) {
                    if vm_eq(ctx, v, target) {
                        return Ok(VmValue::from_int(i as i64));
                    }
                }
            }
        }
        Ok(VmValue::from_int(-1))
    }

    pub fn includes(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let (Some(&arr), Some(&target)) = (args.first(), args.get(1)) {
            let len = ctx.array_len(arr);
            for i in 0..len {
                if let Some(v) = ctx.array_get(arr, i) {
                    if vm_eq(ctx, v, target) {
                        return Ok(VmValue::from_bool(true));
                    }
                }
            }
        }
        Ok(VmValue::from_bool(false))
    }

    pub fn slice(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let Some(&arr) = args.first() {
            let len = ctx.array_len(arr) as i64;
            let start = args
                .get(1)
                .and_then(|&a| if a.is_int() { Some(a.as_int()) } else { None })
                .unwrap_or(0);
            let end = args
                .get(2)
                .and_then(|&a| if a.is_int() { Some(a.as_int()) } else { None })
                .unwrap_or(len);
            let si = norm_idx(start, len);
            let ei = norm_idx(end, len).max(si);
            let mut items = Vec::with_capacity(ei - si);
            for i in si..ei {
                if let Some(v) = ctx.array_get(arr, i) {
                    items.push(v);
                }
            }
            return Ok(ctx.alloc_array(items));
        }
        Ok(ctx.alloc_array(vec![]))
    }

    pub fn concat(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let Some(&arr) = args.first() {
            let mut items = Vec::new();
            ctx.array_for_each(arr, &mut |v, _| items.push(v));
            if let Some(&other) = args.get(1) {
                ctx.array_for_each(other, &mut |v, _| items.push(v));
            }
            return Ok(ctx.alloc_array(items));
        }
        Ok(ctx.alloc_array(vec![]))
    }

    pub fn join(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let Some(&arr) = args.first() {
            let sep = args
                .get(1)
                .and_then(|&a| ctx.str_owned(a))
                .unwrap_or_else(|| ",".to_owned());
            let len = ctx.array_len(arr);
            let mut parts = Vec::with_capacity(len);
            for i in 0..len {
                let v = ctx.array_get(arr, i).unwrap_or(VmValue::null());
                parts.push(ctx.str_repr(v));
            }
            return Ok(ctx.alloc_str_owned(parts.join(&sep)));
        }
        Ok(ctx.alloc_str(""))
    }

    pub fn unshift(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let (Some(&arr), Some(&item)) = (args.first(), args.get(1)) {
            let len = ctx.array_len(arr);
            let mut items = Vec::with_capacity(len + 1);
            items.push(item);
            for i in 0..len {
                if let Some(v) = ctx.array_get(arr, i) {
                    items.push(v);
                }
            }
            for (i, v) in items.into_iter().enumerate() {
                ctx.array_set(arr, i, v);
            }
            return Ok(VmValue::from_int(ctx.array_len(arr) as i64));
        }
        Ok(VmValue::null())
    }

    pub fn shift(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let Some(&arr) = args.first() {
            let len = ctx.array_len(arr);
            if len == 0 {
                return Ok(VmValue::null());
            }
            let first = ctx.array_get(arr, 0).unwrap_or(VmValue::null());
            for i in 1..len {
                let v = ctx.array_get(arr, i).unwrap_or(VmValue::null());
                ctx.array_set(arr, i - 1, v);
            }
            ctx.array_pop(arr);
            return Ok(first);
        }
        Ok(VmValue::null())
    }

    pub fn last_index_of(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let (Some(&arr), Some(&target)) = (args.first(), args.get(1)) {
            let len = ctx.array_len(arr);
            for i in (0..len).rev() {
                if let Some(v) = ctx.array_get(arr, i) {
                    if vm_eq(ctx, v, target) {
                        return Ok(VmValue::from_int(i as i64));
                    }
                }
            }
        }
        Ok(VmValue::from_int(-1))
    }

    pub fn fill(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let (Some(&arr), Some(&val)) = (args.first(), args.get(1)) {
            let len = ctx.array_len(arr) as i64;
            let start = args
                .get(2)
                .and_then(|&a| if a.is_int() { Some(a.as_int()) } else { None })
                .unwrap_or(0);
            let end = args
                .get(3)
                .and_then(|&a| if a.is_int() { Some(a.as_int()) } else { None })
                .unwrap_or(len);
            let si = norm_idx(start, len);
            let ei = norm_idx(end, len);
            for i in si..ei {
                ctx.array_set(arr, i, val);
            }
            return Ok(arr);
        }
        Ok(VmValue::null())
    }

    pub fn at(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let (Some(&arr), Some(&idx_v)) = (args.first(), args.get(1)) {
            if idx_v.is_int() {
                let len = ctx.array_len(arr) as i64;
                let idx = idx_v.as_int();
                let idx = if idx < 0 { len + idx } else { idx };
                if idx >= 0 && (idx as usize) < ctx.array_len(arr) {
                    return Ok(ctx.array_get(arr, idx as usize).unwrap_or(VmValue::null()));
                }
            }
        }
        Ok(VmValue::null())
    }

    pub fn to_str(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        join(ctx, args)
    }

    pub fn array_from(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let Some(&v) = args.first() {
            if ctx.is_array(v) {
                return Ok(v);
            }
            if ctx.is_string(v) {
                if let Some(s) = ctx.str_owned(v) {
                    let chars: Vec<VmValue> = s
                        .chars()
                        .map(|c| {
                            let mut buf = [0u8; 4];
                            ctx.alloc_str_owned(c.encode_utf8(&mut buf).to_owned())
                        })
                        .collect();
                    return Ok(ctx.alloc_array(chars));
                }
            }
        }
        Ok(ctx.alloc_array(vec![]))
    }
}

fn norm_idx(idx: i64, len: i64) -> usize {
    if idx < 0 {
        (len + idx).max(0) as usize
    } else {
        (idx as usize).min(len as usize)
    }
}

pub use basic::{index_of as array_index_of, is_array as array_is_array};

pub fn array_class_builder(
    _ctx: &mut dyn varn_types::NativeCtx,
    _args: &[VmValue],
) -> NativeFnResult {
    use varn_core::IntrinsicType;
    use varn_types::value::ClassObj;

    let cls = ClassObj::new_native_rc(IntrinsicType::Array.as_str());

    cls.add_static("isArray", Value::native(basic::is_array, "isArray"));
    cls.add_static("from", Value::native(basic::array_from, "from"));

    cls.add_method("push", Value::native(basic::push, "push"));
    cls.add_method("pop", Value::native(basic::pop, "pop"));
    cls.add_method("indexOf", Value::native(basic::index_of, "indexOf"));
    cls.add_method(
        "lastIndexOf",
        Value::native(basic::last_index_of, "lastIndexOf"),
    );
    cls.add_method("includes", Value::native(basic::includes, "includes"));
    cls.add_method("slice", Value::native(basic::slice, "slice"));
    cls.add_method("concat", Value::native(basic::concat, "concat"));
    cls.add_method("join", Value::native(basic::join, "join"));
    cls.add_method("unshift", Value::native(basic::unshift, "unshift"));
    cls.add_method("shift", Value::native(basic::shift, "shift"));
    cls.add_method("fill", Value::native(basic::fill, "fill"));
    cls.add_method("at", Value::native(basic::at, "at"));
    cls.add_method("toString", Value::native(basic::to_str, "toString"));

    cls.add_method(
        "map",
        Value::native(super::callbacks::callbacks::map, "map"),
    );
    cls.add_method(
        "filter",
        Value::native(super::callbacks::callbacks::filter, "filter"),
    );
    cls.add_method(
        "reduce",
        Value::native(super::callbacks::callbacks::reduce, "reduce"),
    );
    cls.add_method(
        "find",
        Value::native(super::callbacks::callbacks::find, "find"),
    );
    cls.add_method(
        "findIndex",
        Value::native(super::callbacks::callbacks::find_index, "findIndex"),
    );
    cls.add_method(
        "some",
        Value::native(super::callbacks::callbacks::some, "some"),
    );
    cls.add_method(
        "every",
        Value::native(super::callbacks::callbacks::every, "every"),
    );
    cls.add_method(
        "forEach",
        Value::native(super::callbacks::callbacks::for_each, "forEach"),
    );

    cls.add_method(
        "reverse",
        Value::native(super::advanced::advanced::reverse, "reverse"),
    );
    cls.add_method(
        "sort",
        Value::native(super::advanced::advanced::sort, "sort"),
    );
    cls.add_method(
        "flat",
        Value::native(super::advanced::advanced::flat, "flat"),
    );
    cls.add_method(
        "flatMap",
        Value::native(super::advanced::advanced::flat_map, "flatMap"),
    );
    cls.add_method(
        "splice",
        Value::native(super::advanced::advanced::splice, "splice"),
    );

    cls.add_getter("length", Value::native(basic::length, "length"));

    let cls_val = Value::Class(cls);
    Ok(_ctx.intern(cls_val))
}

#[varn_module("globals")]
mod globals_export {
    use super::*;

    #[varn_fn("Array")]
    pub fn array_export(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        array_class_builder(ctx, args)
    }
}
