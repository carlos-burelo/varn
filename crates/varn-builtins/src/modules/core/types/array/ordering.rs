use std::cmp::Ordering;
use varn_types::{NativeCtx, NativeError, VmValue};

pub(super) fn natural_cmp(
    ctx: &mut dyn NativeCtx,
    a: VmValue,
    b: VmValue,
) -> Result<Ordering, NativeError> {
    let order = natural_scalar_cmp(ctx, a, b);
    if let Some(order) = order {
        return Ok(order);
    }
    match ctx.method(a, "compare") {
        Some(compare) => sign(ctx, compare, &[b]),
        None => Err(NativeError::from(format!(
            "sort: {} and {} have no natural order; pass a comparator or implement Comparable",
            ctx.str_repr(a),
            ctx.str_repr(b)
        ))),
    }
}

pub(super) fn sign(
    ctx: &mut dyn NativeCtx,
    compare: VmValue,
    args: &[VmValue],
) -> Result<Ordering, NativeError> {
    let result = ctx.call_vm(compare, args)?;
    if !result.is_int() {
        return Err(NativeError::from("sort: a comparator must return int"));
    }
    Ok(result.as_int().cmp(&0))
}

pub(super) fn merge_sort(
    items: &mut Vec<VmValue>,
    cmp: &mut dyn FnMut(VmValue, VmValue) -> Result<Ordering, NativeError>,
) -> Result<(), NativeError> {
    let len = items.len();
    let mut scratch = items.clone();
    let mut width = 1;
    while width < len {
        let mut lo = 0;
        while lo < len {
            let mid = (lo + width).min(len);
            let hi = (lo + 2 * width).min(len);
            let (mut i, mut j) = (lo, mid);
            for slot in scratch.iter_mut().take(hi).skip(lo) {
                let take_right =
                    j < hi && (i >= mid || cmp(items[i], items[j])? == Ordering::Greater);
                if take_right {
                    *slot = items[j];
                    j += 1;
                } else {
                    *slot = items[i];
                    i += 1;
                }
            }
            lo = hi;
        }
        std::mem::swap(items, &mut scratch);
        width *= 2;
    }
    Ok(())
}

fn natural_scalar_cmp(ctx: &dyn NativeCtx, a: VmValue, b: VmValue) -> Option<Ordering> {
    if ctx.is_int(a) && ctx.is_int(b) {
        return Some(ctx.as_int(a).cmp(&ctx.as_int(b)));
    }
    if a.is_f64() && b.is_f64() {
        return Some(a.as_f64().total_cmp(&b.as_f64()));
    }
    if ctx.is_int(a) && b.is_f64() {
        return Some((ctx.as_int(a) as f64).total_cmp(&b.as_f64()));
    }
    if a.is_f64() && ctx.is_int(b) {
        return Some(a.as_f64().total_cmp(&(ctx.as_int(b) as f64)));
    }
    if ctx.is_string(a) && ctx.is_string(b) {
        return Some(ctx.str_repr_borrowed(a).cmp(&ctx.str_repr_borrowed(b)));
    }
    if a.is_bool() && b.is_bool() {
        return Some(a.as_bool().cmp(&b.as_bool()));
    }
    if let (Some(x), Some(y)) = (ctx.as_char(a), ctx.as_char(b)) {
        return Some(x.cmp(&y));
    }
    if let (Some(x), Some(y)) = (ctx.as_bigint(a), ctx.as_bigint(b)) {
        return Some(x.cmp(&y));
    }
    if let (Some(x), Some(y)) = (ctx.as_decimal(a), ctx.as_decimal(b)) {
        return Some(x.cmp(&y));
    }
    None
}
