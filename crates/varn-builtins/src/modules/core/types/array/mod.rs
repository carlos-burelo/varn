#![allow(non_upper_case_globals)]

mod ordering;

use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, NativeError, VmValue, VnArray};

pub struct Array;

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

fn norm_idx(idx: i64, len: i64) -> usize {
    if idx < 0 {
        (len + idx).max(0) as usize
    } else {
        (idx as usize).min(len as usize)
    }
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

varn_contract! {
    module: "globals",
    class: "Array",
    contract: "src/modules/core/types/array/array.vn",
    impl Array {

        fn length(ctx: &mut dyn NativeCtx, this: VnArray) -> i64 {
            this.len(ctx) as i64
        }


        fn push(ctx: &mut dyn NativeCtx, this: VnArray, item: VmValue) {
            this.push(ctx, item);
        }
        fn pop(ctx: &mut dyn NativeCtx, this: VnArray) -> VmValue {
            this.pop(ctx).unwrap_or_else(VmValue::null)
        }
        fn shift(ctx: &mut dyn NativeCtx, this: VnArray) -> VmValue {
            let len = this.len(ctx);
            if len == 0 {
                return VmValue::null();
            }
            let first = this.get(ctx, 0).unwrap_or_else(VmValue::null);
            for i in 1..len {
                let v = this.get(ctx, i).unwrap_or_else(VmValue::null);
                this.set(ctx, i - 1, v);
            }
            this.pop(ctx);
            first
        }
        fn unshift(ctx: &mut dyn NativeCtx, this: VnArray, item: VmValue) {
            let len = this.len(ctx);
            let mut items = Vec::with_capacity(len + 1);
            items.push(item);
            for i in 0..len {
                items.push(this.get(ctx, i).unwrap_or_else(VmValue::null));
            }
            this.push(ctx, VmValue::null());
            for (i, v) in items.into_iter().enumerate() {
                this.set(ctx, i, v);
            }
        }


        fn join(ctx: &mut dyn NativeCtx, this: VnArray, separator: Option<&str>) -> String {
            let sep = separator.unwrap_or(",");
            let len = this.len(ctx);
            if len == 0 {
                return String::new();
            }
            let mut out = String::with_capacity(len * (sep.len() + 8));
            let mut first = true;
            let mut buf = [0u8; 5];
            for i in 0..len {
                if !first {
                    out.push_str(sep);
                } else {
                    first = false;
                }
                if let Some(v) = this.get(ctx, i) {
                    if v.is_sso() {
                        out.push_str(v.sso_as_str(&mut buf));
                    } else {
                        out.push_str(&ctx.str_repr_borrowed(v));
                    }
                }
            }
            out
        }
        fn toString(ctx: &mut dyn NativeCtx, this: VnArray) -> String {
            let len = this.len(ctx);
            let mut parts = Vec::with_capacity(len);
            for i in 0..len {
                let v = this.get(ctx, i).unwrap_or_else(VmValue::null);
                parts.push(ctx.str_repr(v));
            }
            parts.join(",")
        }
        fn slice(ctx: &mut dyn NativeCtx, this: VnArray, start: Option<i64>, end: Option<i64>) -> Vec<VmValue> {
            let len = this.len(ctx) as i64;
            let si = norm_idx(start.unwrap_or(0), len);
            let ei = norm_idx(end.unwrap_or(len), len).max(si);
            let mut items = Vec::with_capacity(ei.saturating_sub(si));
            for i in si..ei {
                if let Some(v) = this.get(ctx, i) {
                    items.push(v);
                }
            }
            items
        }
        fn at(ctx: &mut dyn NativeCtx, this: VnArray, index: i64) -> Option<VmValue> {
            let len = this.len(ctx) as i64;
            let idx = if index < 0 { len + index } else { index };
            if idx >= 0 && (idx as usize) < this.len(ctx) {
                this.get(ctx, idx as usize)
            } else {
                None
            }
        }
        fn includes(ctx: &mut dyn NativeCtx, this: VnArray, search: VmValue) -> bool {
            let len = this.len(ctx);
            for i in 0..len {
                if let Some(v) = this.get(ctx, i) {
                    if vm_eq(ctx, v, search) {
                        return true;
                    }
                }
            }
            false
        }
        fn indexOf(ctx: &mut dyn NativeCtx, this: VnArray, search: VmValue) -> i64 {
            let len = this.len(ctx);
            for i in 0..len {
                if let Some(v) = this.get(ctx, i) {
                    if vm_eq(ctx, v, search) {
                        return i as i64;
                    }
                }
            }
            -1
        }
        fn lastIndexOf(ctx: &mut dyn NativeCtx, this: VnArray, search: VmValue) -> i64 {
            let len = this.len(ctx);
            for i in (0..len).rev() {
                if let Some(v) = this.get(ctx, i) {
                    if vm_eq(ctx, v, search) {
                        return i as i64;
                    }
                }
            }
            -1
        }


        fn concat(ctx: &mut dyn NativeCtx, this: VnArray, items: &[VmValue]) -> Vec<VmValue> {
            let mut out = this.to_vec(ctx);
            for &other in items {
                if ctx.is_array(other) {
                    let olen = ctx.array_len(other);
                    for j in 0..olen {
                        out.push(ctx.array_get(other, j).unwrap_or_else(VmValue::null));
                    }
                } else {
                    out.push(other);
                }
            }
            out
        }
        fn fill(ctx: &mut dyn NativeCtx, this: VnArray, value: VmValue, start: Option<i64>, end: Option<i64>) -> Vec<VmValue> {
            let len = this.len(ctx) as i64;
            let si = norm_idx(start.unwrap_or(0), len);
            let ei = norm_idx(end.unwrap_or(len), len);
            for i in si..ei {
                this.set(ctx, i, value);
            }
            this.to_vec(ctx)
        }
        fn flat(ctx: &mut dyn NativeCtx, this: VnArray, depth: Option<i64>) -> Vec<VmValue> {
            let d = depth.unwrap_or(1);
            let mut out = Vec::new();
            flatten(ctx, this.raw(), d, &mut out);
            out
        }
        fn reverse(ctx: &mut dyn NativeCtx, this: VnArray) -> Vec<VmValue> {
            let mut items = this.to_vec(ctx);
            items.reverse();
            for (i, v) in items.iter().enumerate() {
                this.set(ctx, i, *v);
            }
            items
        }


        fn map(ctx: &mut dyn NativeCtx, this: VnArray, callback: VmValue) -> Result<Vec<VmValue>, NativeError> {
            let arr = this.raw();
            let len = this.len(ctx);
            let mut out = Vec::with_capacity(len);
            for i in 0..len {
                let v = this.get(ctx, i).unwrap_or_else(VmValue::null);
                out.push(ctx.call_vm(callback, &[v, VmValue::from_int(i as i64), arr])?);
            }
            Ok(out)
        }
        fn filter(ctx: &mut dyn NativeCtx, this: VnArray, predicate: VmValue) -> Result<Vec<VmValue>, NativeError> {
            let arr = this.raw();
            let len = this.len(ctx);
            let mut out = Vec::new();
            for i in 0..len {
                let v = this.get(ctx, i).unwrap_or_else(VmValue::null);
                if ctx.call_vm(predicate, &[v, VmValue::from_int(i as i64), arr])?.is_truthy() {
                    out.push(v);
                }
            }
            Ok(out)
        }
        fn find(ctx: &mut dyn NativeCtx, this: VnArray, predicate: VmValue) -> Result<Option<VmValue>, NativeError> {
            let arr = this.raw();
            let len = this.len(ctx);
            for i in 0..len {
                let v = this.get(ctx, i).unwrap_or_else(VmValue::null);
                if ctx.call_vm(predicate, &[v, VmValue::from_int(i as i64), arr])?.is_truthy() {
                    return Ok(Some(v));
                }
            }
            Ok(None)
        }
        fn findIndex(ctx: &mut dyn NativeCtx, this: VnArray, predicate: VmValue) -> Result<i64, NativeError> {
            let arr = this.raw();
            let len = this.len(ctx);
            for i in 0..len {
                let v = this.get(ctx, i).unwrap_or_else(VmValue::null);
                if ctx.call_vm(predicate, &[v, VmValue::from_int(i as i64), arr])?.is_truthy() {
                    return Ok(i as i64);
                }
            }
            Ok(-1)
        }
        fn forEach(ctx: &mut dyn NativeCtx, this: VnArray, callback: VmValue) -> Result<(), NativeError> {
            let arr = this.raw();
            let len = this.len(ctx);
            for i in 0..len {
                let v = this.get(ctx, i).unwrap_or_else(VmValue::null);
                ctx.call_vm(callback, &[v, VmValue::from_int(i as i64), arr])?;
            }
            Ok(())
        }
        fn flatMap(ctx: &mut dyn NativeCtx, this: VnArray, callback: VmValue) -> Result<Vec<VmValue>, NativeError> {
            let arr = this.raw();
            let len = this.len(ctx);
            let mut out = Vec::new();
            for i in 0..len {
                let v = this.get(ctx, i).unwrap_or_else(VmValue::null);
                let r = ctx.call_vm(callback, &[v, VmValue::from_int(i as i64), arr])?;
                if ctx.is_array(r) {
                    let rlen = ctx.array_len(r);
                    for j in 0..rlen {
                        out.push(ctx.array_get(r, j).unwrap_or_else(VmValue::null));
                    }
                } else {
                    out.push(r);
                }
            }
            Ok(out)
        }
        fn reduce(ctx: &mut dyn NativeCtx, this: VnArray, callback: VmValue, initial: Option<VmValue>) -> Result<VmValue, NativeError> {
            let arr = this.raw();
            let len = this.len(ctx);
            let (mut acc, start) = match initial {
                Some(init) => (init, 0usize),
                None if len > 0 => (this.get(ctx, 0).unwrap_or_else(VmValue::null), 1usize),
                None => return Err(NativeError::from("reduce of an empty array with no initial value")),
            };
            for i in start..len {
                let v = this.get(ctx, i).unwrap_or_else(VmValue::null);
                acc = ctx.call_vm(callback, &[acc, v, VmValue::from_int(i as i64), arr])?;
            }
            Ok(acc)
        }
        fn sort(ctx: &mut dyn NativeCtx, this: VnArray, compareFn: Option<VmValue>) -> Result<Vec<VmValue>, NativeError> {
            let mut items = this.to_vec(ctx);
            match compareFn.filter(|v| !v.is_null()) {
                Some(cb) => ordering::merge_sort(&mut items, &mut |a, b| ordering::sign(ctx, cb, &[a, b]))?,
                None => ordering::merge_sort(&mut items, &mut |a, b| ordering::natural_cmp(ctx, a, b))?,
            }
            for (i, v) in items.iter().enumerate() {
                this.set(ctx, i, *v);
            }
            Ok(items)
        }
        fn splice(ctx: &mut dyn NativeCtx, this: VnArray, start: i64, deleteCount: Option<i64>, items: &[VmValue]) -> Vec<VmValue> {
            let len = this.len(ctx) as i64;
            let st = if start < 0 { (len + start).max(0) as usize } else { (start as usize).min(len as usize) };
            let del = deleteCount
                .map(|d| d.max(0) as usize)
                .unwrap_or((len as usize) - st)
                .min((len as usize) - st);

            let mut all = this.to_vec(ctx);
            let removed: Vec<VmValue> = all.drain(st..st + del).collect();
            for (i, &v) in items.iter().enumerate() {
                all.insert(st + i, v);
            }

            let new_len = all.len();
            let old_len = len as usize;
            for (i, v) in all.into_iter().enumerate() {
                this.set(ctx, i, v);
            }
            for _ in new_len..old_len {
                this.pop(ctx);
            }
            removed
        }
        fn every(ctx: &mut dyn NativeCtx, this: VnArray, predicate: VmValue) -> Result<bool, NativeError> {
            let arr = this.raw();
            let len = this.len(ctx);
            for i in 0..len {
                let v = this.get(ctx, i).unwrap_or_else(VmValue::null);
                if !ctx.call_vm(predicate, &[v, VmValue::from_int(i as i64), arr])?.is_truthy() {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        fn some(ctx: &mut dyn NativeCtx, this: VnArray, predicate: VmValue) -> Result<bool, NativeError> {
            let arr = this.raw();
            let len = this.len(ctx);
            for i in 0..len {
                let v = this.get(ctx, i).unwrap_or_else(VmValue::null);
                if ctx.call_vm(predicate, &[v, VmValue::from_int(i as i64), arr])?.is_truthy() {
                    return Ok(true);
                }
            }
            Ok(false)
        }


        fn isArray(ctx: &mut dyn NativeCtx, obj: VmValue) -> bool {
            ctx.is_array(obj)
        }
        fn from(ctx: &mut dyn NativeCtx, value: VmValue) -> Vec<VmValue> {
            if ctx.is_array(value) {
                let len = ctx.array_len(value);
                return (0..len)
                    .map(|i| ctx.array_get(value, i).unwrap_or_else(VmValue::null))
                    .collect();
            }
            if ctx.is_string(value) {
                if let Some(s) = ctx.str_owned(value) {
                    return s.chars().map(|c| ctx.alloc_str_owned(c.to_string())).collect();
                }
            }
            Vec::new()
        }
    }
}
