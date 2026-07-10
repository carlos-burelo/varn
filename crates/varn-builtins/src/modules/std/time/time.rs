use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, Value, VmValue};

pub struct TimeRuntime;

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

varn_contract! {
    module: "runtime:time",
    contract: "src/modules/host/time/time_runtime.vn",
    impl TimeRuntime {
        fn timeNowMs(_ctx: &mut dyn NativeCtx) -> Result<i64, String> {
            Ok(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0))
        }
        fn timeParseIsoDuration(_ctx: &mut dyn NativeCtx, s: &str) -> Result<i64, String> {
            Ok(parse_iso_duration(s))
        }
        fn timeMsToParts(ctx: &mut dyn NativeCtx, ms: i64) -> Result<VmValue, String> {
            use chrono::{Datelike, TimeZone, Timelike, Utc};
            let dt = Utc
                .timestamp_millis_opt(ms)
                .single()
                .ok_or_else(|| "invalid timestamp".to_string())?;
            let obj = ctx.alloc_object();
            set_int(ctx, obj, "year", dt.year() as i64);
            set_int(ctx, obj, "month", dt.month() as i64);
            set_int(ctx, obj, "day", dt.day() as i64);
            set_int(ctx, obj, "hour", dt.hour() as i64);
            set_int(ctx, obj, "minute", dt.minute() as i64);
            set_int(ctx, obj, "second", dt.second() as i64);
            set_int(ctx, obj, "millisecond", ms.rem_euclid(1000));
            Ok(obj)
        }
        fn timePartsToMs(ctx: &mut dyn NativeCtx, parts: VmValue) -> Result<i64, String> {
            let get_int = |ctx: &mut dyn NativeCtx, key: &str| -> i64 {
                ctx.get_field(parts, key)
                    .and_then(|v| if let Value::Int(n) = ctx.extract(v) { Some(n) } else { None })
                    .unwrap_or(0)
            };
            let year = get_int(ctx, "year") as i32;
            let month = get_int(ctx, "month") as u32;
            let day = get_int(ctx, "day") as u32;
            let hour = get_int(ctx, "hour") as u32;
            let minute = get_int(ctx, "minute") as u32;
            let second = get_int(ctx, "second") as u32;
            let millisecond = get_int(ctx, "millisecond") as u32;

            use chrono::{TimeZone, Timelike, Utc};
            let dt = Utc
                .with_ymd_and_hms(year, month, day, hour, minute, second)
                .single()
                .ok_or_else(|| "Invalid date parts".to_string())?
                .with_nanosecond(millisecond * 1_000_000)
                .ok_or_else(|| "Invalid millisecond".to_string())?;
            Ok(dt.timestamp_millis())
        }
    }
}
