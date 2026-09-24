//! How `Array.sort` orders values: by the natural order of their domain, a
//! `Comparable` instance by its own `compare`, or a caller's comparator. A
//! comparator's exception propagates; a comparator that is not a consistent
//! order can shuffle elements but never crash the sort.

use std::cmp::Ordering;
use varn_types::{NativeCtx, NativeError, Value, VmValue};

/// `a` against `b` with no comparator: numbers numerically, text and chars by
/// code point, booleans `false < true`, a `Comparable` by `a.compare(b)`.
pub(super) fn natural_cmp(
    ctx: &mut dyn NativeCtx,
    a: VmValue,
    b: VmValue,
) -> Result<Ordering, NativeError> {
    let order = match (ctx.extract(a), ctx.extract(b)) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(&y)),
        (Value::Float(x), Value::Float(y)) => Some(x.total_cmp(&y)),
        (Value::Int(x), Value::Float(y)) => Some((x as f64).total_cmp(&y)),
        (Value::Float(x), Value::Int(y)) => Some(x.total_cmp(&(y as f64))),
        (Value::Str(x), Value::Str(y)) => Some(x.cmp(&y)),
        (Value::Char(x), Value::Char(y)) => Some(x.cmp(&y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(&y)),
        (Value::BigInt(x), Value::BigInt(y)) => Some(x.cmp(&y)),
        (Value::Decimal(x), Value::Decimal(y)) => Some(x.cmp(&y)),
        _ => None,
    };
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

/// The ordering a comparator's `int` result stands for.
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

/// Stable merge sort, O(n log n). Hand-written because `slice::sort_by`
/// may panic on a comparator that is not a total order — and this one runs
/// user code.
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
                let take_right = j < hi && (i >= mid || cmp(items[i], items[j])? == Ordering::Greater);
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
