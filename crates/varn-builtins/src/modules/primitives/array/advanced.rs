use varn_types::{NativeCtx, NativeFnResult, VmValue};

pub mod advanced {
    use super::*;

    pub fn reverse(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let Some(&arr) = args.first() {
            if ctx.is_array(arr) {
                let len = ctx.array_len(arr);
                let mut items = Vec::with_capacity(len);
                for i in (0..len).rev() {
                    items.push(ctx.array_get(arr, i).unwrap_or(VmValue::null()));
                }
                for (i, v) in items.into_iter().enumerate() {
                    ctx.array_set(arr, i, v);
                }
                return Ok(arr);
            }
        }
        Ok(VmValue::null())
    }

    pub fn sort(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let Some(&arr) = args.first() {
            let len = ctx.array_len(arr);
            let cb = args.get(1).copied();
            let mut items: Vec<VmValue> = (0..len)
                .map(|i| ctx.array_get(arr, i).unwrap_or(VmValue::null()))
                .collect();

            if let Some(cb) = cb.filter(|v| !v.is_null()) {
                for i in 0..len {
                    for j in 0..len - 1 - i {
                        let a = items[j];
                        let b = items[j + 1];
                        let r = ctx.call_vm(cb, &[a, b])?;
                        let cmp = if r.is_int() { r.as_int() } else { 0 };
                        if cmp > 0 {
                            items.swap(j, j + 1);
                        }
                    }
                }
            } else {
                items.sort_by(|a, b| {
                    let sa = ctx.str_repr(*a);
                    let sb = ctx.str_repr(*b);
                    sa.cmp(&sb)
                });
            }

            for (i, v) in items.into_iter().enumerate() {
                ctx.array_set(arr, i, v);
            }
            return Ok(arr);
        }
        Ok(VmValue::null())
    }

    pub fn flat(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let Some(&arr) = args.first() {
            let depth = args
                .get(1)
                .and_then(|&a| if a.is_int() { Some(a.as_int()) } else { None })
                .unwrap_or(1);
            let mut out = Vec::new();
            flatten(ctx, arr, depth, &mut out);
            return Ok(ctx.alloc_array(out));
        }
        Ok(ctx.alloc_array(vec![]))
    }

    fn flatten(ctx: &mut dyn NativeCtx, arr: VmValue, depth: i64, out: &mut Vec<VmValue>) {
        let len = ctx.array_len(arr);
        for i in 0..len {
            let v = ctx.array_get(arr, i).unwrap_or(VmValue::null());
            if depth > 0 && ctx.is_array(v) {
                flatten(ctx, v, depth - 1, out);
            } else {
                out.push(v);
            }
        }
    }

    pub fn flat_map(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let (Some(&arr), Some(&cb)) = (args.first(), args.get(1)) {
            let len = ctx.array_len(arr);
            let mut out = Vec::new();
            for i in 0..len {
                let v = ctx.array_get(arr, i).unwrap_or(VmValue::null());
                let r = ctx.call_vm(cb, &[v, VmValue::from_int(i as i64), arr])?;
                if ctx.is_array(r) {
                    let rlen = ctx.array_len(r);
                    for j in 0..rlen {
                        out.push(ctx.array_get(r, j).unwrap_or(VmValue::null()));
                    }
                } else {
                    out.push(r);
                }
            }
            return Ok(ctx.alloc_array(out));
        }
        Ok(ctx.alloc_array(vec![]))
    }

    pub fn splice(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
        if let Some(&arr) = args.first() {
            let len = ctx.array_len(arr) as i64;
            let start = args
                .get(1)
                .and_then(|&a| if a.is_int() { Some(a.as_int()) } else { None })
                .unwrap_or(0);
            let start = if start < 0 {
                (len + start).max(0) as usize
            } else {
                (start as usize).min(len as usize)
            };
            let delete_count = args
                .get(2)
                .and_then(|&a| {
                    if a.is_int() {
                        Some(a.as_int().max(0) as usize)
                    } else {
                        None
                    }
                })
                .unwrap_or((len as usize) - start);
            let delete_count = delete_count.min((len as usize) - start);
            let inserts: Vec<VmValue> = args.iter().skip(3).copied().collect();

            let mut items: Vec<VmValue> = (0..len as usize)
                .map(|i| ctx.array_get(arr, i).unwrap_or(VmValue::null()))
                .collect();
            let removed: Vec<VmValue> = items.drain(start..start + delete_count).collect();
            for (i, v) in inserts.into_iter().enumerate() {
                items.insert(start + i, v);
            }

            let new_len = items.len();
            let old_len = len as usize;
            for (i, v) in items.into_iter().enumerate() {
                ctx.array_set(arr, i, v);
            }

            for _ in new_len..old_len {
                ctx.array_pop(arr);
            }
            return Ok(ctx.alloc_array(removed));
        }
        Ok(ctx.alloc_array(vec![]))
    }
}

pub use advanced::{
    flat as array_flat, flat_map as array_flat_map, reverse as array_reverse, sort as array_sort,
    splice as array_splice,
};
