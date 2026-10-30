use varn_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

pub struct TimeRuntime;

fn parse_iso_duration(text: &str) -> Result<i64, String> {
    let invalid = || format!("invalid ISO-8601 duration '{text}'");
    let body = text.strip_prefix('P').ok_or_else(invalid)?;
    let (date_part, time_part) = match body.split_once('T') {
        Some((date, time)) => (date, Some(time)),
        None => (body, None),
    };
    if date_part.is_empty() && time_part.is_none_or(str::is_empty) {
        return Err(invalid());
    }
    let mut ms = 0.0_f64;
    let sections = [(date_part, 'D'), (time_part.unwrap_or(""), 'T')];
    for (section, kind) in sections {
        let mut number = String::new();
        for ch in section.chars() {
            if ch.is_ascii_digit() || ch == '.' {
                number.push(ch);
                continue;
            }
            let value: f64 = number.parse().map_err(|_| invalid())?;
            number.clear();
            ms += value
                * match (kind, ch) {
                    ('D', 'W') => 7.0 * 86_400_000.0,
                    ('D', 'D') => 86_400_000.0,
                    ('T', 'H') => 3_600_000.0,
                    ('T', 'M') => 60_000.0,
                    ('T', 'S') => 1_000.0,
                    _ => return Err(invalid()),
                };
        }
        if !number.is_empty() {
            return Err(invalid());
        }
    }
    if !ms.is_finite() || ms > i64::MAX as f64 {
        return Err(invalid());
    }
    Ok(ms.round() as i64)
}

fn parse_iso_instant(text: &str) -> Result<i64, String> {
    chrono::DateTime::parse_from_rfc3339(text)
        .map(|dt| dt.timestamp_millis())
        .map_err(|e| format!("invalid ISO-8601 instant '{text}': {e}"))
}

fn set_int(ctx: &mut dyn NativeCtx, obj: VmValue, key: &str, val: i64) {
    let nv = VmValue::from_int(val);
    ctx.set_field(obj, key, nv);
}

varn_contract! {
    module: "runtime:time",
    contract: "src/modules/runtime/time/time_runtime.vn",
    impl TimeRuntime {
        fn nowMs(_ctx: &mut dyn NativeCtx) -> Result<i64, String> {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .map_err(|e| format!("time.now: the system clock is before the Unix epoch: {e}"))
        }
        fn parseIsoDuration(_ctx: &mut dyn NativeCtx, s: &str) -> Result<i64, String> {
            parse_iso_duration(s)
        }
        fn parseIsoInstant(_ctx: &mut dyn NativeCtx, s: &str) -> Result<i64, String> {
            parse_iso_instant(s)
        }
        fn msToParts(ctx: &mut dyn NativeCtx, ms: i64) -> Result<VmValue, String> {
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
            set_int(ctx, obj, "weekday", dt.weekday().num_days_from_sunday() as i64);
            Ok(obj)
        }
        fn partsToMs(ctx: &mut dyn NativeCtx, parts: VmValue) -> Result<i64, String> {
            let get_int = |ctx: &mut dyn NativeCtx, key: &str| -> i64 {
                ctx.get_field(parts, key)
                    .filter(|v| ctx.is_int(*v))
                    .map(|v| ctx.as_int(v))
                    .unwrap_or(0)
            };
            let invalid = || "Invalid date parts".to_string();
            let year = i32::try_from(get_int(ctx, "year")).map_err(|_| invalid())?;
            let month = u32::try_from(get_int(ctx, "month")).map_err(|_| invalid())?;
            let day = u32::try_from(get_int(ctx, "day")).map_err(|_| invalid())?;
            let hour = u32::try_from(get_int(ctx, "hour")).map_err(|_| invalid())?;
            let minute = u32::try_from(get_int(ctx, "minute")).map_err(|_| invalid())?;
            let second = u32::try_from(get_int(ctx, "second")).map_err(|_| invalid())?;
            let millisecond = u32::try_from(get_int(ctx, "millisecond"))
                .ok()
                .filter(|ms| *ms < 1000)
                .ok_or_else(|| "Invalid millisecond".to_string())?;

            use chrono::{TimeZone, Timelike, Utc};
            let dt = Utc
                .with_ymd_and_hms(year, month, day, hour, minute, second)
                .single()
                .ok_or_else(invalid)?
                .with_nanosecond(millisecond * 1_000_000)
                .ok_or_else(|| "Invalid millisecond".to_string())?;
            Ok(dt.timestamp_millis())
        }
    }
}
