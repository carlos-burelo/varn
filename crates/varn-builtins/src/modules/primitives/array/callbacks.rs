use varn_types::{NativeCtx, NativeFnResult, VmValue};

pub mod callbacks {
    use super::*;

    pub fn map(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let (Some(&arr), Some(&cb)) = (args.first(), args.get(1)) {
            let len = ctx.array_len(arr);
            let mut out = Vec::with_capacity(len);
            for i in 0..len {
                let v = ctx.array_get(arr, i).unwrap_or(VmValue::null());
                let r = ctx.call_vm(cb, &[v, VmValue::from_int(i as i64), arr])?;
                out.push(r);
            }
            return Ok(ctx.alloc_array(out));
        }
        Ok(ctx.alloc_array(vec![]))
    }

    pub fn filter(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let (Some(&arr), Some(&cb)) = (args.first(), args.get(1)) {
            let len = ctx.array_len(arr);
            let mut out = Vec::new();
            for i in 0..len {
                let v = ctx.array_get(arr, i).unwrap_or(VmValue::null());
                let keep = ctx.call_vm(cb, &[v, VmValue::from_int(i as i64), arr])?;
                if keep.is_truthy() {
                    out.push(v);
                }
            }
            return Ok(ctx.alloc_array(out));
        }
        Ok(ctx.alloc_array(vec![]))
    }

    pub fn reduce(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let (Some(&arr), Some(&cb)) = (args.first(), args.get(1)) {
            let len = ctx.array_len(arr);
            let (mut acc, start) = if let Some(&init) = args.get(2) {
                (init, 0usize)
            } else if len > 0 {
                (ctx.array_get(arr, 0).unwrap_or(VmValue::null()), 1usize)
            } else {
                return Err("reduce of empty array with no initial value".into());
            };
            for i in start..len {
                let v = ctx.array_get(arr, i).unwrap_or(VmValue::null());
                acc = ctx.call_vm(cb, &[acc, v, VmValue::from_int(i as i64), arr])?;
            }
            return Ok(acc);
        }
        Ok(VmValue::null())
    }

    pub fn find(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let (Some(&arr), Some(&cb)) = (args.first(), args.get(1)) {
            let len = ctx.array_len(arr);
            for i in 0..len {
                let v = ctx.array_get(arr, i).unwrap_or(VmValue::null());
                let r = ctx.call_vm(cb, &[v, VmValue::from_int(i as i64), arr])?;
                if r.is_truthy() {
                    return Ok(v);
                }
            }
        }
        Ok(VmValue::null())
    }

    pub fn find_index(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let (Some(&arr), Some(&cb)) = (args.first(), args.get(1)) {
            let len = ctx.array_len(arr);
            for i in 0..len {
                let v = ctx.array_get(arr, i).unwrap_or(VmValue::null());
                let r = ctx.call_vm(cb, &[v, VmValue::from_int(i as i64), arr])?;
                if r.is_truthy() {
                    return Ok(VmValue::from_int(i as i64));
                }
            }
        }
        Ok(VmValue::from_int(-1))
    }

    pub fn for_each(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let (Some(&arr), Some(&cb)) = (args.first(), args.get(1)) {
            let len = ctx.array_len(arr);
            for i in 0..len {
                if let Some(v) = ctx.array_get(arr, i) {
                    ctx.call_vm(cb, &[v, VmValue::from_int(i as i64), arr])?;
                }
            }
        }
        Ok(VmValue::null())
    }

    pub fn some(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let (Some(&arr), Some(&cb)) = (args.first(), args.get(1)) {
            let len = ctx.array_len(arr);
            for i in 0..len {
                let v = ctx.array_get(arr, i).unwrap_or(VmValue::null());
                let r = ctx.call_vm(cb, &[v, VmValue::from_int(i as i64), arr])?;
                if r.is_truthy() {
                    return Ok(VmValue::from_bool(true));
                }
            }
        }
        Ok(VmValue::from_bool(false))
    }

    pub fn every(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let (Some(&arr), Some(&cb)) = (args.first(), args.get(1)) {
            let len = ctx.array_len(arr);
            for i in 0..len {
                let v = ctx.array_get(arr, i).unwrap_or(VmValue::null());
                let r = ctx.call_vm(cb, &[v, VmValue::from_int(i as i64), arr])?;
                if !r.is_truthy() {
                    return Ok(VmValue::from_bool(false));
                }
            }
            return Ok(VmValue::from_bool(true));
        }
        Ok(VmValue::from_bool(true))
    }
}

pub use callbacks::{
    every as array_every, filter as array_filter, find as array_find,
    find_index as array_find_index, for_each as array_for_each, map as array_map,
    reduce as array_reduce, some as array_some,
};
