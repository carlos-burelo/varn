pub fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970u64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let month_days: [u64; 12] = [
        31,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u64;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, days + 1)
}

pub fn ymd_to_days(y: u64, m: u64, d: u64) -> u64 {
    let mut days = 0;
    for year in 1970..y {
        days += if is_leap(year) { 366 } else { 365 };
    }
    let month_days = [
        31,
        if is_leap(y) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    for i in 0..((m - 1).min(11)) {
        days += month_days[i as usize];
    }
    days + (d.saturating_sub(1))
}

pub fn calendar_to_secs(y: u64, mo: u64, d: u64, h: u64, min: u64, s: u64) -> u64 {
    let days = ymd_to_days(y, mo, d);
    days * 86400 + h * 3600 + min * 60 + s
}

pub fn calendar_to_millis(y: u64, mo: u64, d: u64, h: u64, min: u64, s: u64, ms: u64) -> u64 {
    let secs = calendar_to_secs(y, mo, d, h, min, s);
    secs * 1000 + ms
}

pub fn secs_to_calendar(secs: u64) -> (u64, u64, u64, u64, u64, u64, u64) {
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    let (y, mo, d) = days_to_ymd(days);

    let weekday = ((days + 3) % 7) + 1;

    (y, mo, d, h, m, s, weekday)
}

pub fn unix_to_iso(secs: u64, millis: u64) -> String {
    let (y, mo, d, h, min, s, _) = secs_to_calendar(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{min:02}:{s:02}.{millis:03}Z")
}

pub fn iso_to_unix(iso: &str) -> Option<(u64, u64)> {
    let mut parts = iso.split(['T', ' ', 'Z', '[', '+']);
    let date_part = parts.next()?;
    let time_part = parts.next().unwrap_or("00:00:00");

    let date_sub: Vec<&str> = date_part.split('-').collect();
    if date_sub.len() < 3 {
        return None;
    }
    let y = date_sub[0].parse::<u64>().ok()?;
    let mo = date_sub[1].parse::<u64>().ok()?;
    let d = date_sub[2].parse::<u64>().ok()?;

    let mut time_sub = time_part.split(':');
    let h = time_sub
        .next()
        .unwrap_or("00")
        .parse::<u64>()
        .ok()
        .unwrap_or(0);
    let min = time_sub
        .next()
        .unwrap_or("00")
        .parse::<u64>()
        .ok()
        .unwrap_or(0);
    let rest = time_sub.next().unwrap_or("00");

    let (s, ms) = if let Some(dot_idx) = rest.find('.') {
        let s_part = &rest[..dot_idx];
        let mut ms_part = &rest[dot_idx + 1..];
        if ms_part.len() > 3 {
            ms_part = &ms_part[..3];
        }
        let ms_val = ms_part.parse::<u64>().ok().unwrap_or(0);
        let s_val = s_part.parse::<u64>().ok().unwrap_or(0);

        let ms_final = match ms_part.len() {
            1 => ms_val * 100,
            2 => ms_val * 10,
            _ => ms_val,
        };
        (s_val, ms_final)
    } else {
        (rest.parse::<u64>().ok().unwrap_or(0), 0)
    };

    let secs = calendar_to_secs(y, mo, d, h, min, s);
    Some((secs, ms))
}

pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn now_secs_f64() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
