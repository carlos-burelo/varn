#[inline(always)]
pub(super) fn fast_parse_f64(s: &[u8]) -> Option<f64> {
    if s.is_empty() {
        return None;
    }
    let (neg, s) = if s[0] == b'-' {
        (true, &s[1..])
    } else {
        (false, s)
    };
    if s.is_empty() {
        return None;
    }
    let mut dot_pos = None;
    let mut int_val: u64 = 0;
    let mut frac_val: u64 = 0;
    let mut frac_len: u32 = 0;
    let mut idx = 0;
    while idx < s.len() {
        let b = s[idx];
        if b == b'.' {
            if dot_pos.is_some() {
                return None;
            }
            dot_pos = Some(idx);
            idx += 1;
            break;
        } else if b.is_ascii_digit() {
            int_val = int_val.checked_mul(10)?.checked_add((b - b'0') as u64)?;
            idx += 1;
        } else {
            let s_full = std::str::from_utf8(s).ok()?;
            let f = s_full.parse::<f64>().ok()?;
            return Some(if neg { -f } else { f });
        }
    }
    if dot_pos.is_some() {
        while idx < s.len() {
            let b = s[idx];
            if b.is_ascii_digit() {
                if frac_len < 15 {
                    frac_val = frac_val * 10 + (b - b'0') as u64;
                    frac_len += 1;
                }
                idx += 1;
            } else {
                let s_full = std::str::from_utf8(s).ok()?;
                let f = s_full.parse::<f64>().ok()?;
                return Some(if neg { -f } else { f });
            }
        }
        const POW10: [f64; 16] = [
            1.0,
            10.0,
            100.0,
            1000.0,
            10000.0,
            100000.0,
            1000000.0,
            10000000.0,
            100000000.0,
            1000000000.0,
            10000000000.0,
            100000000000.0,
            1000000000000.0,
            10000000000000.0,
            100000000000000.0,
            1000000000000000.0,
        ];
        let frac = (frac_val as f64) / POW10[frac_len as usize];
        let val = (int_val as f64) + frac;
        Some(if neg { -val } else { val })
    } else {
        Some(if neg {
            -(int_val as f64)
        } else {
            int_val as f64
        })
    }
}
