use varn_op_macros::varn_contract;
use varn_types::str_util::{
    byte_to_char_idx, char_len, char_range_to_bytes, find_bytes, rfind_bytes,
};
use varn_types::{NativeCtx, NativeFnResult, VmValue};

pub struct Str;

fn normalize_idx(idx: i64, len: i64) -> usize {
    if idx < 0 {
        (len + idx).max(0) as usize
    } else {
        idx as usize
    }
}

varn_contract! {
    module: "globals",
    class: "str",
    contract: "src/modules/primitives/str/str.vn",
    impl Str {

        fn EMPTY(_ctx: &mut dyn NativeCtx) -> String { String::new() }

        fn fromCharCode(_ctx: &mut dyn NativeCtx, codes: &[VmValue]) -> String {
            let mut res = String::new();
            for &v in codes {
                if v.is_int() {
                    if let Some(c) = std::char::from_u32(v.as_int() as u32) {
                        res.push(c);
                    }
                }
            }
            res
        }

        fn from(ctx: &mut dyn NativeCtx, val: VmValue) -> String { ctx.str_repr(val) }
        fn parse(ctx: &mut dyn NativeCtx, val: VmValue) -> String { ctx.str_repr(val) }

        fn join(ctx: &mut dyn NativeCtx, arr: ::varn_types::VnArray, sep: Option<&str>) -> String {
            let sep = sep.unwrap_or("");
            let len = arr.len(ctx);
            let mut vals = Vec::with_capacity(len);
            for i in 0..len {
                vals.push(arr.get(ctx, i).unwrap_or_else(VmValue::null));
            }
            let mut out = String::new();
            for (i, &v) in vals.iter().enumerate() {
                if i > 0 {
                    out.push_str(sep);
                }
                out.push_str(&ctx.str_repr_borrowed(v));
            }
            out
        }


        fn length(_ctx: &mut dyn NativeCtx, this: &str) -> i64 {
            char_len(this, this.is_ascii()) as i64
        }


        fn toString(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.to_string() }
        fn toStr(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.to_string() }
        fn valueOf(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.to_string() }
        fn toLowerCase(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.to_lowercase() }
        fn toUpperCase(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.to_uppercase() }
        fn trim(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.trim().to_string() }
        fn trimStart(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.trim_start().to_string() }
        fn trimEnd(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.trim_end().to_string() }


        fn includes(_ctx: &mut dyn NativeCtx, this: &str, search: &str) -> bool { find_bytes(this, search).is_some() }
        fn contains(_ctx: &mut dyn NativeCtx, this: &str, search: &str) -> bool { find_bytes(this, search).is_some() }
        fn startsWith(_ctx: &mut dyn NativeCtx, this: &str, search: &str) -> bool { this.starts_with(search) }
        fn endsWith(_ctx: &mut dyn NativeCtx, this: &str, search: &str) -> bool { this.ends_with(search) }

        fn indexOf(_ctx: &mut dyn NativeCtx, this: &str, search: &str) -> i64 {
            if search.is_empty() { return 0; }
            let ascii = this.is_ascii();
            find_bytes(this, search).map(|b| byte_to_char_idx(this, ascii, b)).unwrap_or(-1)
        }
        fn lastIndexOf(_ctx: &mut dyn NativeCtx, this: &str, search: &str) -> i64 {
            let ascii = this.is_ascii();
            if search.is_empty() { return char_len(this, ascii) as i64; }
            rfind_bytes(this, search).map(|b| byte_to_char_idx(this, ascii, b)).unwrap_or(-1)
        }


        fn substring(_ctx: &mut dyn NativeCtx, this: &str, start: i64, end: Option<i64>) -> String {
            let b = this.as_bytes();
            let st = start.max(0) as usize;
            let en = end.map(|e| e.max(0) as usize).unwrap_or(b.len());
            let (s, e) = if st <= en { (st, en) } else { (en, st) };
            if e <= b.len() && b[..e].is_ascii() {
                return this[s..e].to_owned();
            }
            let ascii = this.is_ascii();
            let len = char_len(this, ascii);
            let si = s.min(len);
            let ei = e.min(len);
            let (bs, be) = char_range_to_bytes(this, ascii, si, ei);
            this[bs..be].to_owned()
        }
        fn slice(_ctx: &mut dyn NativeCtx, this: &str, start: i64, end: Option<i64>) -> String {
            let b = this.as_bytes();
            if start >= 0 && end.map_or(true, |e| e >= 0) {
                let st = start as usize;
                let en = end.map_or(b.len(), |e| e as usize);
                if st <= en && en <= b.len() && b[..en].is_ascii() {
                    return this[st..en].to_owned();
                }
            }
            let ascii = this.is_ascii();
            let len = char_len(this, ascii);
            let si = normalize_idx(start, len as i64).min(len);
            let ei = normalize_idx(end.unwrap_or(len as i64), len as i64).min(len).max(si);
            let (bs, be) = char_range_to_bytes(this, ascii, si, ei);
            this[bs..be].to_owned()
        }
        fn at(_ctx: &mut dyn NativeCtx, this: &str, index: i64) -> Option<String> {
            let b = this.as_bytes();
            if index >= 0 {
                let idx = index as usize;
                if idx < b.len() && b[..=idx].is_ascii() {
                    return Some(this[idx..idx + 1].to_owned());
                }
            }
            let ascii = this.is_ascii();
            let len = char_len(this, ascii) as i64;
            let idx = if index < 0 { len + index } else { index };
            if idx < 0 || idx >= len {
                return None;
            }
            let (bs, be) = char_range_to_bytes(this, ascii, idx as usize, idx as usize + 1);
            Some(this[bs..be].to_owned())
        }
        fn substr(_ctx: &mut dyn NativeCtx, this: &str, start: i64, length: Option<i64>) -> String {
            let b = this.as_bytes();
            if start >= 0 && length.map_or(true, |l| l >= 0) {
                let st = start as usize;
                let count = length.map_or(b.len().saturating_sub(st), |l| l as usize);
                let en = (st + count).min(b.len());
                if st <= b.len() && b[..en].is_ascii() {
                    return this[st..en].to_owned();
                }
            }
            let ascii = this.is_ascii();
            let len = char_len(this, ascii);
            let st = if start < 0 { (len as i64 + start).max(0) as usize } else { (start as usize).min(len) };
            let count = length.map(|c| c.max(0) as usize).unwrap_or(len - st);
            let end = (st + count).min(len);
            let (bs, be) = char_range_to_bytes(this, ascii, st, end);
            this[bs..be].to_owned()
        }


        fn replace(_ctx: &mut dyn NativeCtx, this: &str, from: &str, to: &str) -> String {
            this.replacen(from, to, 1)
        }
        fn replaceAll(_ctx: &mut dyn NativeCtx, this: &str, from: &str, to: &str) -> String {
            this.replace(from, to)
        }
        fn split(ctx: &mut dyn NativeCtx, this: &str, separator: Option<&str>) -> Vec<VmValue> {
            match separator {
                Some(sep) => this.split(sep).map(|p| ctx.alloc_str(p)).collect(),
                None => {
                    let mut buf = [0u8; 4];
                    this.chars().map(|c| ctx.alloc_str(c.encode_utf8(&mut buf))).collect()
                }
            }
        }
        fn lines(ctx: &mut dyn NativeCtx, this: &str) -> Vec<VmValue> {
            this.lines().map(|l| ctx.alloc_str(l)).collect()
        }
        fn words(ctx: &mut dyn NativeCtx, this: &str) -> Vec<VmValue> {
            this.split_whitespace().map(|w| ctx.alloc_str(w)).collect()
        }


        fn charCodeAt(_ctx: &mut dyn NativeCtx, this: &str, pos: i64) -> i64 {
            char_code_at(this, pos)
        }
        fn charCode(_ctx: &mut dyn NativeCtx, this: &str) -> i64 {
            this.chars().next().map(|c| c as i64).unwrap_or(-1)
        }
        fn codePointAt(_ctx: &mut dyn NativeCtx, this: &str, pos: i64) -> i64 {
            char_code_at(this, pos)
        }


        fn repeat(_ctx: &mut dyn NativeCtx, this: &str, n: i64) -> String {
            this.repeat(n.max(0) as usize)
        }
        fn padStart(_ctx: &mut dyn NativeCtx, this: &str, target: i64, pad: Option<&str>) -> String {
            pad_start(this, target, pad)
        }
        fn padLeft(_ctx: &mut dyn NativeCtx, this: &str, target: i64, pad: Option<&str>) -> String {
            pad_start(this, target, pad)
        }
        fn padEnd(_ctx: &mut dyn NativeCtx, this: &str, target: i64, pad: Option<&str>) -> String {
            pad_end(this, target, pad)
        }
        fn padRight(_ctx: &mut dyn NativeCtx, this: &str, target: i64, pad: Option<&str>) -> String {
            pad_end(this, target, pad)
        }
        fn concat(_ctx: &mut dyn NativeCtx, this: &str, other: &str) -> String {
            format!("{this}{other}")
        }


        fn isEmpty(_ctx: &mut dyn NativeCtx, this: &str) -> bool { this.is_empty() }
        fn isBlank(_ctx: &mut dyn NativeCtx, this: &str) -> bool { this.trim().is_empty() }
        fn isDigit(_ctx: &mut dyn NativeCtx, this: &str) -> bool {
            !this.is_empty() && this.chars().all(|c| c.is_ascii_digit())
        }
        fn isLetter(_ctx: &mut dyn NativeCtx, this: &str) -> bool {
            !this.is_empty() && this.chars().all(|c| c.is_alphabetic())
        }
        fn isWhitespace(_ctx: &mut dyn NativeCtx, this: &str) -> bool {
            !this.is_empty() && this.chars().all(|c| c.is_whitespace())
        }


        fn reverse(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.chars().rev().collect() }
        fn capitalize(_ctx: &mut dyn NativeCtx, this: &str) -> String {
            let mut chars = this.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            }
        }
        fn toInt(_ctx: &mut dyn NativeCtx, this: &str) -> i64 {
            this.trim().parse::<i64>().unwrap_or(0)
        }
        fn toFloat(_ctx: &mut dyn NativeCtx, this: &str) -> f64 {
            this.trim().parse::<f64>().unwrap_or(0.0)
        }
    }
}

/// `charCodeAt`/`codePointAt`: the ASCII-prefix fast path indexes bytes
/// directly (char index == byte index while the prefix is ASCII); otherwise
/// one forward char scan.
fn char_code_at(s: &str, pos: i64) -> i64 {
    if pos < 0 {
        return -1;
    }
    let pos = pos as usize;
    let b = s.as_bytes();
    if pos >= b.len() {
        return -1;
    }
    if b[..=pos].is_ascii() {
        return b[pos] as i64;
    }
    s.chars().nth(pos).map(|c| c as i64).unwrap_or(-1)
}

fn pad_start(s: &str, target: i64, pad: Option<&str>) -> String {
    let target = target.max(0) as usize;
    let pad = pad.filter(|p| !p.is_empty()).unwrap_or(" ");
    let len = char_len(s, s.is_ascii());
    if len >= target {
        return s.to_string();
    }
    let needed = target - len;
    let prefix: String = pad.chars().cycle().take(needed).collect();
    format!("{prefix}{s}")
}

fn pad_end(s: &str, target: i64, pad: Option<&str>) -> String {
    let target = target.max(0) as usize;
    let pad = pad.filter(|p| !p.is_empty()).unwrap_or(" ");
    let len = char_len(s, s.is_ascii());
    if len >= target {
        return s.to_string();
    }
    let needed = target - len;
    let suffix: String = pad.chars().cycle().take(needed).collect();
    format!("{s}{suffix}")
}

pub fn str_from_char_code(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    let mut res = String::new();
    for &v in args {
        if v.is_int() {
            if let Some(c) = std::char::from_u32(v.as_int() as u32) {
                res.push(c);
            }
        }
    }
    Ok(ctx.alloc_str_owned(res))
}

pub fn str_from_value(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    if let Some(&v) = args.first() {
        return Ok(ctx.alloc_str_owned(ctx.str_repr(v)));
    }
    Ok(ctx.alloc_str(""))
}
