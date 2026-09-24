use varn_op_macros::varn_contract;
use varn_types::value::{RangeData, RangeElem};
use varn_types::{NativeCtx, NativeError, Value, VmValue};

pub struct Range;

fn get_range(ctx: &dyn NativeCtx, this: VmValue) -> Option<RangeData> {
    if let Value::Range(r) = ctx.extract(this) {
        Some(*r)
    } else {
        None
    }
}

/// Every element of `r`, in order.
fn elements(ctx: &mut dyn NativeCtx, r: &RangeData) -> Vec<VmValue> {
    (0..r.len())
        .filter_map(|i| r.nth(i))
        .map(|raw| ctx.intern(r.element(raw)))
        .collect()
}

/// The raw bound `val` stands for in `r`'s domain, if it belongs to it.
fn raw_of(ctx: &dyn NativeCtx, r: &RangeData, val: VmValue) -> Option<i64> {
    match (r.elem, ctx.extract(val)) {
        (RangeElem::Int, Value::Int(n)) => Some(n),
        (RangeElem::Char, Value::Char(c)) => Some(c as i64),
        _ => None,
    }
}

varn_contract! {
    module: "globals",
    class: "Range",
    contract: "src/modules/primitives/range/range.vn",
    impl Range {

        fn start(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            match get_range(ctx, this) {
                Some(r) => ctx.intern(r.element(r.start)),
                None => VmValue::null(),
            }
        }
        fn end(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            match get_range(ctx, this) {
                Some(r) => ctx.intern(r.element(r.end)),
                None => VmValue::null(),
            }
        }
        fn inclusive(ctx: &mut dyn NativeCtx, this: VmValue) -> bool {
            get_range(ctx, this).map(|r| r.inclusive).unwrap_or(false)
        }
        fn length(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            get_range(ctx, this).map(|r| r.len()).unwrap_or(0)
        }
        fn toString(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            get_range(ctx, this).map(|r| r.to_string()).unwrap_or_default()
        }
        fn contains(ctx: &mut dyn NativeCtx, this: VmValue, val: VmValue) -> bool {
            match get_range(ctx, this) {
                Some(r) => raw_of(ctx, &r, val).is_some_and(|raw| r.contains(raw)),
                None => false,
            }
        }
        fn toArray(ctx: &mut dyn NativeCtx, this: VmValue) -> Vec<VmValue> {
            match get_range(ctx, this) {
                Some(r) => elements(ctx, &r),
                None => Vec::new(),
            }
        }
        fn step(ctx: &mut dyn NativeCtx, this: VmValue, n: i64) -> Result<VmValue, NativeError> {
            if n <= 0 {
                return Err(NativeError::from(format!("range step must be positive, got {n}")));
            }
            let r = get_range(ctx, this).ok_or_else(|| NativeError::from("range.step: not a range"))?;
            Ok(ctx.intern(Value::Range(Box::new(r.with_step(r.step * n)))))
        }
        fn forEach(ctx: &mut dyn NativeCtx, this: VmValue, callback: VmValue) -> Result<(), NativeError> {
            if let Some(r) = get_range(ctx, this) {
                for item in elements(ctx, &r) {
                    ctx.call_vm(callback, &[item])?;
                }
            }
            Ok(())
        }
        fn map(ctx: &mut dyn NativeCtx, this: VmValue, callback: VmValue) -> Result<Vec<VmValue>, NativeError> {
            let Some(r) = get_range(ctx, this) else {
                return Ok(Vec::new());
            };
            elements(ctx, &r)
                .into_iter()
                .map(|item| ctx.call_vm(callback, &[item]))
                .collect()
        }

        fn from(ctx: &mut dyn NativeCtx, start: i64, end: i64) -> VmValue {
            ctx.alloc_range(start, end, false)
        }
        fn fromInclusive(ctx: &mut dyn NativeCtx, start: i64, end: i64) -> VmValue {
            ctx.alloc_range(start, end, true)
        }
    }
}
