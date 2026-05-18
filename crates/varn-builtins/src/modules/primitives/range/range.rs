#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_getter, varn_method, varn_module, varn_static};
use varn_types::value::RangeData;
use varn_types::{NativeCtx, Value, VmValue};

fn get_range(ctx: &dyn NativeCtx, this: VmValue) -> Option<RangeData> {
    if let Value::Range(r) = ctx.extract(this) {
        Some(*r)
    } else {
        None
    }
}

#[varn_module("globals")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_class("Range")]
    pub mod range_class {
        use super::*;

        #[varn_getter("start")]
        pub fn range_start(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<VmValue, String> {
            if let Some(r) = get_range(ctx, this) {
                return Ok(VmValue::from_int(r.start));
            }
            Ok(VmValue::from_int(0))
        }

        #[varn_getter("end")]
        pub fn range_end(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<VmValue, String> {
            if let Some(r) = get_range(ctx, this) {
                return Ok(VmValue::from_int(r.end));
            }
            Ok(VmValue::from_int(0))
        }

        #[varn_getter("length")]
        pub fn range_length(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<VmValue, String> {
            if let Some(r) = get_range(ctx, this) {
                let len = if r.inclusive {
                    ((r.end - r.start) / r.step + 1).max(0)
                } else {
                    ((r.end - r.start + r.step - 1) / r.step).max(0)
                };
                return Ok(VmValue::from_int(len));
            }
            Ok(VmValue::from_int(0))
        }

        #[varn_method("contains")]
        pub fn range_contains(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(&val_nv) = args.first() {
                if let Some(r) = get_range(ctx, this) {
                    if let Value::Int(n) = ctx.extract(val_nv) {
                        let in_range = if r.inclusive {
                            n >= r.start && n <= r.end
                        } else {
                            n >= r.start && n < r.end
                        };
                        let aligned = r.step == 1 || (n - r.start) % r.step == 0;
                        return Ok(VmValue::from_bool(in_range && aligned));
                    }
                }
            }
            Ok(VmValue::from_bool(false))
        }

        #[varn_method("toArray")]
        pub fn range_to_array(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(r) = get_range(ctx, this) {
                let end = if r.inclusive { r.end + 1 } else { r.end };
                let mut i = r.start;
                let mut vals = Vec::new();
                while i < end {
                    vals.push(ctx.intern(Value::Int(i)));
                    i += r.step;
                }
                return Ok(ctx.alloc_array(vals));
            }
            Ok(ctx.alloc_array(vec![]))
        }

        #[varn_method("step")]
        pub fn range_step(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            if let Some(&step_nv) = args.first() {
                if let Some(r) = get_range(ctx, this) {
                    let step = if let Value::Int(s) = ctx.extract(step_nv) {
                        s.max(1)
                    } else {
                        1
                    };
                    let end = if r.inclusive { r.end + 1 } else { r.end };
                    let mut i = r.start;
                    let mut vals = Vec::new();
                    while i < end {
                        vals.push(ctx.intern(Value::Int(i)));
                        i += step;
                    }
                    return Ok(ctx.alloc_array(vals));
                }
            }
            Ok(ctx.alloc_array(vec![]))
        }

        #[varn_static("from")]
        pub fn range_from(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
            let start = args
                .first()
                .and_then(|&v| {
                    if let Value::Int(n) = ctx.extract(v) {
                        Some(n)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            let end = args
                .get(1)
                .and_then(|&v| {
                    if let Value::Int(n) = ctx.extract(v) {
                        Some(n)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            Ok(ctx.alloc_range(start, end, false))
        }

        #[varn_static("fromInclusive")]
        pub fn range_from_inclusive(
            ctx: &mut dyn NativeCtx,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            let start = args
                .first()
                .and_then(|&v| {
                    if let Value::Int(n) = ctx.extract(v) {
                        Some(n)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            let end = args
                .get(1)
                .and_then(|&v| {
                    if let Value::Int(n) = ctx.extract(v) {
                        Some(n)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            Ok(ctx.alloc_range(start, end, true))
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
