use varn_op_macros::varn_module;
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

#[varn_module("runtime:time")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn("timeNowMs", cap = "time.now")]
    pub fn raw_now_ms(_ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Ok(VmValue::from_int(ms))
    }

    #[varn_fn("timeParseIsoDuration")]
    pub fn raw_parse_iso_duration(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let s = args.get(0).map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let ms = parse_iso_duration(&s);
        Ok(VmValue::from_int(ms))
    }

    #[varn_fn("timeMsToParts")]
    pub fn raw_ms_to_parts(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let ms = args.first().and_then(|&v| {
            if let Value::Int(n) = ctx.extract(v) {
                Some(n)
            } else {
                None
            }
        }).unwrap_or(0);

        use chrono::{TimeZone, Utc, Datelike, Timelike};
        let dt = Utc.timestamp_millis_opt(ms).unwrap();

        let obj = ctx.alloc_object();
        set_int(ctx, obj, "year", dt.year() as i64);
        set_int(ctx, obj, "month", dt.month() as i64);
        set_int(ctx, obj, "day", dt.day() as i64);
        set_int(ctx, obj, "hour", dt.hour() as i64);
        set_int(ctx, obj, "minute", dt.minute() as i64);
        set_int(ctx, obj, "second", dt.second() as i64);
        set_int(ctx, obj, "millisecond", (ms % 1000) as i64);
        Ok(obj)
    }
}
