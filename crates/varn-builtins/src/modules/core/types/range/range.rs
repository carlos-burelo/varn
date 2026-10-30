use varn_macros::varn_contract;
use varn_types::value::{RangeData, RangeElem};
use varn_types::{NativeCtx, NativeError, VmValue};

pub struct Range;

fn get_range(ctx: &dyn NativeCtx, this: VmValue) -> Option<RangeData> {
    ctx.as_range(this)
}

const MAX_MATERIALIZED: u128 = 1 << 28;

fn length_of(r: &RangeData) -> Result<i64, NativeError> {
    r.len().ok_or_else(|| {
        NativeError::integer_overflow(format!("integer overflow: the length of {r} does not fit int"))
    })
}

fn elements(ctx: &mut dyn NativeCtx, r: &RangeData) -> Result<Vec<VmValue>, NativeError> {
    if r.count() > MAX_MATERIALIZED {
        return Err(NativeError::range_error(format!(
            "{r} has {} elements, more than the {MAX_MATERIALIZED} that can be materialized",
            r.count()
        )));
    }
    let len = length_of(r)?;
    Ok((0..len)
        .filter_map(|i| r.nth(i))
        .map(|raw| ctx.range_element(r, raw))
        .collect())
}

fn raw_of(ctx: &dyn NativeCtx, r: &RangeData, val: VmValue) -> Option<i64> {
    match r.elem {
        RangeElem::Int => ctx.is_int(val).then(|| ctx.as_int(val)),
        RangeElem::Char => ctx.as_char(val).map(|c| c as i64),
    }
}

varn_contract! {
    module: "globals",
    class: "Range",
    contract: "src/modules/core/types/range/range.vn",
    impl Range {

        fn start(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            match get_range(ctx, this) {
                Some(r) => ctx.range_element(&r, r.start),
                None => VmValue::null(),
            }
        }
        fn end(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            match get_range(ctx, this) {
                Some(r) => ctx.range_element(&r, r.end),
                None => VmValue::null(),
            }
        }
        fn inclusive(ctx: &mut dyn NativeCtx, this: VmValue) -> bool {
            get_range(ctx, this).map(|r| r.inclusive).unwrap_or(false)
        }
        fn length(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<i64, NativeError> {
            get_range(ctx, this).map_or(Ok(0), |r| length_of(&r))
        }
        fn stride(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            get_range(ctx, this).map(|r| r.step).unwrap_or(1)
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
        fn toArray(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<Vec<VmValue>, NativeError> {
            match get_range(ctx, this) {
                Some(r) => elements(ctx, &r),
                None => Ok(Vec::new()),
            }
        }
        fn step(ctx: &mut dyn NativeCtx, this: VmValue, n: i64) -> Result<VmValue, NativeError> {
            if n <= 0 {
                return Err(NativeError::range_error(format!("range step must be positive, got {n}")));
            }
            let r = get_range(ctx, this).ok_or_else(|| NativeError::type_error("range.step: not a range"))?;
            let step = r.step.checked_mul(n).ok_or_else(|| {
                NativeError::integer_overflow(format!("integer overflow: the step {} * {n} does not fit int", r.step))
            })?;
            Ok(ctx.alloc_range_data(r.with_step(step)))
        }
        fn forEach(ctx: &mut dyn NativeCtx, this: VmValue, callback: VmValue) -> Result<(), NativeError> {
            if let Some(r) = get_range(ctx, this) {
                for item in elements(ctx, &r)? {
                    ctx.call_vm(callback, &[item])?;
                }
            }
            Ok(())
        }
        fn map(ctx: &mut dyn NativeCtx, this: VmValue, callback: VmValue) -> Result<Vec<VmValue>, NativeError> {
            let Some(r) = get_range(ctx, this) else {
                return Ok(Vec::new());
            };
            elements(ctx, &r)?
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
