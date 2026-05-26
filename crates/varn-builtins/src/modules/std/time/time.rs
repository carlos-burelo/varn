use std::cell::RefCell;
use std::rc::Rc;
#[allow(unused_imports)]
use varn_op_macros::{
    varn_class, varn_constructor, varn_fn, varn_getter, varn_method, varn_module, varn_namespace,
    varn_static,
};
use varn_types::value::{ObjData, ObjRef};
use varn_types::{NativeCtx, Value, VmValue};

fn parse_iso_duration(s: &str) -> i64 {
    let s = s.strip_prefix('P').unwrap_or(s);
    let mut ms: i64 = 0;
    let mut rest = s;

    if let Some(t_pos) = rest.find('T') {
        let date_part = &rest[..t_pos];
        rest = &rest[t_pos + 1..];
        if let Some(d_pos) = date_part.find('D') {
            if let Ok(d) = date_part[..d_pos].parse::<i64>() {
                ms += d * 86400 * 1000;
            }
        }
    } else {
        if let Some(d_pos) = rest.find('D') {
            if let Ok(d) = rest[..d_pos].parse::<i64>() {
                ms += d * 86400 * 1000;
            }
        }
        return ms;
    }

    let mut num_buf = String::new();
    for ch in rest.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            num_buf.push(ch);
        } else {
            if let Ok(n) = num_buf.parse::<f64>() {
                match ch {
                    'H' => ms += (n * 3600.0 * 1000.0) as i64,
                    'M' => ms += (n * 60.0 * 1000.0) as i64,
                    'S' => ms += (n * 1000.0) as i64,
                    _ => {}
                }
            }
            num_buf.clear();
        }
    }
    ms
}

fn set_int(ctx: &mut dyn NativeCtx, obj: VmValue, key: &str, val: i64) {
    let nv = ctx.intern(Value::Int(val));
    ctx.set_field(obj, key, nv);
}

fn make_duration_obj(ctx: &mut dyn NativeCtx, total_ms: i64) -> VmValue {
    let obj = ctx.alloc_object();
    let total_s = total_ms / 1000;
    set_int(ctx, obj, "totalMilliseconds", total_ms);
    set_int(ctx, obj, "hours", total_s / 3600);
    set_int(ctx, obj, "minutes", (total_s % 3600) / 60);
    set_int(ctx, obj, "seconds", total_s);
    set_int(ctx, obj, "days", total_s / 86400);
    obj
}

fn make_instant(ctx: &mut dyn NativeCtx, epoch_ms: i64) -> VmValue {
    if let Some(cls) = ctx.get_class("Instant") {
        let obj = ObjData::new_instance(cls);
        let nv = ctx.intern(Value::Object(ObjRef(Rc::new(RefCell::new(obj)))));
        ctx.set_field(nv, "epochMs", VmValue::from_int(epoch_ms));
        nv
    } else {
        VmValue::null()
    }
}

#[varn_module("std:time")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn("now")]
    pub fn time_now(_ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Ok(VmValue::from_int(ms))
    }

    #[varn_namespace("Duration")]
    pub mod duration_ns {
        use super::*;

        #[varn_fn("from")]
        pub fn duration_from(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
            let s = args.get(0).map(|&v| ctx.str_repr(v)).unwrap_or_default();
            let ms = parse_iso_duration(&s);
            Ok(make_duration_obj(ctx, ms))
        }

        #[varn_fn("ofMilliseconds")]
        pub fn duration_of_ms(
            ctx: &mut dyn NativeCtx,
            args: &[VmValue],
        ) -> Result<VmValue, String> {
            let ms = args
                .get(0)
                .and_then(|&v| {
                    if let Value::Int(n) = ctx.extract(v) {
                        Some(n)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            Ok(make_duration_obj(ctx, ms))
        }
    }

    #[varn_namespace("Now")]
    pub mod now_ns {
        use super::*;

        #[varn_fn("instant")]
        pub fn now_instant_op(
            ctx: &mut dyn NativeCtx,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            Ok(make_instant(ctx, ms))
        }

        #[varn_fn("epochMilliseconds")]
        pub fn now_epoch_ms(
            _ctx: &mut dyn NativeCtx,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            Ok(VmValue::from_int(ms))
        }
    }

    #[varn_class("Instant")]
    pub mod instant_class {
        use super::*;

        #[varn_constructor]
        pub fn constructor(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<(), String> {
            let ms = args.first().copied().unwrap_or(VmValue::null());
            ctx.set_field(this, "epochMs", ms);
            Ok(())
        }

        #[varn_method("toString")]
        pub fn to_string(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            let ms = ctx
                .get_field(this, "epochMs")
                .and_then(|v| {
                    if let varn_types::Value::Int(n) = ctx.extract(v) {
                        Some(n)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            Ok(ctx.alloc_str_owned(format!("Instant({}ms)", ms)))
        }

        #[varn_static("compare")]
        pub fn compare(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
            let a = args.first().copied().unwrap_or(VmValue::null());
            let b = args.get(1).copied().unwrap_or(VmValue::null());
            let a_ms = ctx
                .get_field(a, "epochMs")
                .and_then(|v| {
                    if let varn_types::Value::Int(n) = ctx.extract(v) {
                        Some(n)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            let b_ms = ctx
                .get_field(b, "epochMs")
                .and_then(|v| {
                    if let varn_types::Value::Int(n) = ctx.extract(v) {
                        Some(n)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            Ok(VmValue::from_int(a_ms.cmp(&b_ms) as i64))
        }
    }

    #[varn_class("PlainDateTime")]
    pub mod plain_datetime_class {
        use super::*;

        #[varn_constructor]
        pub fn constructor(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<(), String> {
            let mut ints = [0i64; 7];
            for (i, slot) in ints.iter_mut().enumerate() {
                if let Some(&v) = args.get(i) {
                    if let varn_types::Value::Int(n) = ctx.extract(v) {
                        *slot = n;
                    }
                }
            }
            let nv_year = ctx.intern(varn_types::Value::Int(ints[0]));
            let nv_month = ctx.intern(varn_types::Value::Int(ints[1]));
            let nv_day = ctx.intern(varn_types::Value::Int(ints[2]));
            let nv_hour = ctx.intern(varn_types::Value::Int(ints[3]));
            let nv_min = ctx.intern(varn_types::Value::Int(ints[4]));
            let nv_sec = ctx.intern(varn_types::Value::Int(ints[5]));
            let nv_ms = ctx.intern(varn_types::Value::Int(ints[6]));
            ctx.set_field(this, "year", nv_year);
            ctx.set_field(this, "month", nv_month);
            ctx.set_field(this, "day", nv_day);
            ctx.set_field(this, "hour", nv_hour);
            ctx.set_field(this, "minute", nv_min);
            ctx.set_field(this, "second", nv_sec);
            ctx.set_field(this, "millisecond", nv_ms);
            Ok(())
        }

        #[varn_method("toString")]
        pub fn to_string(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            let mut parts = [0i64; 7];
            let keys = [
                "year",
                "month",
                "day",
                "hour",
                "minute",
                "second",
                "millisecond",
            ];
            for (i, key) in keys.iter().enumerate() {
                if let Some(v) = ctx.get_field(this, key) {
                    if let varn_types::Value::Int(n) = ctx.extract(v) {
                        parts[i] = n;
                    }
                }
            }
            let s = format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}",
                parts[0], parts[1], parts[2], parts[3], parts[4], parts[5], parts[6]
            );
            Ok(ctx.alloc_str_owned(s))
        }
    }
}
