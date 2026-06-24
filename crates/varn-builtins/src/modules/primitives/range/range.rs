use varn_op_macros::varn_contract;
use varn_types::value::RangeData;
use varn_types::{NativeCtx, Value, VmValue};

pub struct Range;

fn get_range(ctx: &dyn NativeCtx, this: VmValue) -> Option<RangeData> {
    if let Value::Range(r) = ctx.extract(this) {
        Some(*r)
    } else {
        None
    }
}

fn range_end_exclusive(r: &RangeData) -> i64 {
    if r.inclusive {
        r.end + 1
    } else {
        r.end
    }
}

varn_contract! {
    module: "globals",
    class: "Range",
    contract: "src/modules/primitives/range/range.vn",
    impl Range {

        fn start(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            get_range(ctx, this).map(|r| r.start).unwrap_or(0)
        }
        fn end(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            get_range(ctx, this).map(|r| r.end).unwrap_or(0)
        }
        fn inclusive(ctx: &mut dyn NativeCtx, this: VmValue) -> bool {
            get_range(ctx, this).map(|r| r.inclusive).unwrap_or(false)
        }
        fn length(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            match get_range(ctx, this) {
                Some(r) => {
                    if r.inclusive {
                        ((r.end - r.start) / r.step + 1).max(0)
                    } else {
                        ((r.end - r.start + r.step - 1) / r.step).max(0)
                    }
                }
                None => 0,
            }
        }


        fn toString(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            match get_range(ctx, this) {
                Some(r) if r.inclusive => format!("{}..={}", r.start, r.end),
                Some(r) => format!("{}..{}", r.start, r.end),
                None => "0..0".to_string(),
            }
        }
        fn contains(ctx: &mut dyn NativeCtx, this: VmValue, val: i64) -> bool {
            match get_range(ctx, this) {
                Some(r) => {
                    let in_range = if r.inclusive {
                        val >= r.start && val <= r.end
                    } else {
                        val >= r.start && val < r.end
                    };
                    let aligned = r.step == 1 || (val - r.start) % r.step == 0;
                    in_range && aligned
                }
                None => false,
            }
        }
        fn toArray(ctx: &mut dyn NativeCtx, this: VmValue) -> Vec<VmValue> {
            let mut vals = Vec::new();
            if let Some(r) = get_range(ctx, this) {
                let end = range_end_exclusive(&r);
                let mut i = r.start;
                while i < end {
                    vals.push(ctx.intern(Value::Int(i)));
                    i += r.step;
                }
            }
            vals
        }
        fn step(ctx: &mut dyn NativeCtx, this: VmValue, n: i64) -> Vec<VmValue> {
            let mut vals = Vec::new();
            if let Some(r) = get_range(ctx, this) {
                let step = n.max(1);
                let end = range_end_exclusive(&r);
                let mut i = r.start;
                while i < end {
                    vals.push(ctx.intern(Value::Int(i)));
                    i += step;
                }
            }
            vals
        }
        fn forEach(ctx: &mut dyn NativeCtx, this: VmValue, callback: VmValue) {
            if let Some(r) = get_range(ctx, this) {
                let end = range_end_exclusive(&r);
                let mut i = r.start;
                while i < end {
                    let arg = ctx.intern(Value::Int(i));
                    let _ = ctx.call_vm(callback, &[arg]);
                    i += r.step;
                }
            }
        }
        fn map(ctx: &mut dyn NativeCtx, this: VmValue, callback: VmValue) -> Vec<VmValue> {
            let mut out = Vec::new();
            if let Some(r) = get_range(ctx, this) {
                let end = range_end_exclusive(&r);
                let mut i = r.start;
                while i < end {
                    let arg = ctx.intern(Value::Int(i));
                    if let Ok(v) = ctx.call_vm(callback, &[arg]) {
                        out.push(v);
                    }
                    i += r.step;
                }
            }
            out
        }


        fn from(ctx: &mut dyn NativeCtx, start: i64, end: i64) -> VmValue {
            ctx.alloc_range(start, end, false)
        }
        fn fromInclusive(ctx: &mut dyn NativeCtx, start: i64, end: i64) -> VmValue {
            ctx.alloc_range(start, end, true)
        }
    }
}

pub fn range_op(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    let get_int = |i: usize| -> i64 {
        args.get(i)
            .and_then(|&v| {
                if let Value::Int(n) = ctx.extract(v) {
                    Some(n)
                } else {
                    None
                }
            })
            .unwrap_or(if i == 2 { 1 } else { 0 })
    };
    let start = get_int(0);
    let end = get_int(1);
    let step = get_int(2).max(1);
    let mut i = start;
    let mut vals = Vec::new();
    while i < end {
        vals.push(ctx.intern(Value::Int(i)));
        i += step;
    }
    Ok(ctx.alloc_array(vals))
}
