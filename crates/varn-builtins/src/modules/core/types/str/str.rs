use varn_macros::varn_contract;
use varn_types::str_util::{
    byte_to_char_idx, char_len, char_range_to_bytes, find_bytes, rfind_bytes,
};
use varn_types::{NativeCtx, NativeFnResult, VmValue, VnStr};

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
    contract: "src/modules/core/types/str/str.vn",
    impl Str {

        fn EMPTY(_ctx: &mut dyn NativeCtx) -> String { String::new() }

        fn fromCharCode(_ctx: &mut dyn NativeCtx, codes: &[VmValue]) -> String {
            let mut res = String::new();
            for &v in codes {
                if v.is_int()
                    && let Some(c) = std::char::from_u32(v.as_int() as u32) {
                        res.push(c);
                    }
            }
            res
        }

        fn from(ctx: &mut dyn NativeCtx, val: VmValue) -> String { ctx.str_repr(val) }
        fn parse(ctx: &mut dyn NativeCtx, val: VmValue) -> String { ctx.str_repr(val) }

        fn join(ctx: &mut dyn NativeCtx, arr: ::varn_types::VnArray, sep: Option<&str>) -> String {
            let sep = sep.unwrap_or("");
            let len = arr.len(ctx);
            if len == 0 {
                return String::new();
            }
            let mut out = String::with_capacity(len.saturating_mul(sep.len() + 12));
            for i in 0..len {
                if i > 0 && !sep.is_empty() {
                    out.push_str(sep);
                }
                if let Some(v) = arr.get(ctx, i) {
                    out.push_str(&ctx.str_repr_borrowed(v));
                }
            }
            out
        }


        fn length(_ctx: &mut dyn NativeCtx, this: &VnStr) -> i64 {
            char_len(this, this.is_ascii()) as i64
        }


        fn toString(_ctx: &mut dyn NativeCtx, this: &VnStr) -> String { this.to_string() }
        fn toStr(_ctx: &mut dyn NativeCtx, this: &VnStr) -> String { this.to_string() }
        fn valueOf(_ctx: &mut dyn NativeCtx, this: &VnStr) -> String { this.to_string() }
        fn toLowerCase(_ctx: &mut dyn NativeCtx, this: &VnStr) -> String {
            if this.is_ascii() {
                let mut bytes = this.as_bytes().to_vec();
                bytes.make_ascii_lowercase();
                unsafe { String::from_utf8_unchecked(bytes) }
            } else {
                this.to_lowercase()
            }
        }
        fn toUpperCase(_ctx: &mut dyn NativeCtx, this: &VnStr) -> String {
            if this.is_ascii() {
                let mut bytes = this.as_bytes().to_vec();
                bytes.make_ascii_uppercase();
                unsafe { String::from_utf8_unchecked(bytes) }
            } else {
                this.to_uppercase()
            }
        }
        fn trim(ctx: &mut dyn NativeCtx, this: &VnStr) -> VmValue {
            let s = this.as_str();
            let t = s.trim();
            ctx.str_sub(this, t.as_ptr() as usize - s.as_ptr() as usize, t.len())
        }
        fn trimStart(ctx: &mut dyn NativeCtx, this: &VnStr) -> VmValue {
            let s = this.as_str();
            let t = s.trim_start();
            ctx.str_sub(this, t.as_ptr() as usize - s.as_ptr() as usize, t.len())
        }
        fn trimEnd(ctx: &mut dyn NativeCtx, this: &VnStr) -> VmValue {
            let s = this.as_str();
            let t = s.trim_end();
            ctx.str_sub(this, t.as_ptr() as usize - s.as_ptr() as usize, t.len())
        }


        fn includes(_ctx: &mut dyn NativeCtx, this: &VnStr, search: &str) -> bool { find_bytes(this, search).is_some() }
        fn contains(_ctx: &mut dyn NativeCtx, this: &VnStr, search: &str) -> bool { find_bytes(this, search).is_some() }
        fn startsWith(_ctx: &mut dyn NativeCtx, this: &VnStr, search: &str) -> bool { this.starts_with(search) }
        fn endsWith(_ctx: &mut dyn NativeCtx, this: &VnStr, search: &str) -> bool { this.ends_with(search) }

        fn indexOf(_ctx: &mut dyn NativeCtx, this: &VnStr, search: &str) -> i64 {
            if search.is_empty() { return 0; }
            let ascii = this.is_ascii();
            find_bytes(this, search).map(|b| byte_to_char_idx(this, ascii, b)).unwrap_or(-1)
        }
        fn lastIndexOf(_ctx: &mut dyn NativeCtx, this: &VnStr, search: &str) -> i64 {
            let ascii = this.is_ascii();
            if search.is_empty() { return char_len(this, ascii) as i64; }
            rfind_bytes(this, search).map(|b| byte_to_char_idx(this, ascii, b)).unwrap_or(-1)
        }


        fn substring(ctx: &mut dyn NativeCtx, this: &VnStr, start: i64, end: Option<i64>) -> VmValue {
            let b = this.as_bytes();
            let st = start.max(0) as usize;
            let en = end.map(|e| e.max(0) as usize).unwrap_or(b.len());
            let (s, e) = if st <= en { (st, en) } else { (en, st) };
            if this.is_ascii() {
                let s = s.min(b.len());
                let e = e.min(b.len());
                return ctx.str_sub(this, s, e - s);
            }
            let len = char_len(this, false);
            let si = s.min(len);
            let ei = e.min(len);
            let (bs, be) = char_range_to_bytes(this, false, si, ei);
            ctx.str_sub(this, bs, be - bs)
        }
        fn slice(ctx: &mut dyn NativeCtx, this: &VnStr, start: i64, end: Option<i64>) -> VmValue {
            let b = this.as_bytes();
            if this.is_ascii() {
                let len = b.len() as i64;
                let si = normalize_idx(start, len).min(b.len());
                let ei = normalize_idx(end.unwrap_or(len), len).min(b.len()).max(si);
                return ctx.str_sub(this, si, ei - si);
            }
            let len = char_len(this, false);
            let si = normalize_idx(start, len as i64).min(len);
            let ei = normalize_idx(end.unwrap_or(len as i64), len as i64).min(len).max(si);
            let (bs, be) = char_range_to_bytes(this, false, si, ei);
            ctx.str_sub(this, bs, be - bs)
        }
        fn at(ctx: &mut dyn NativeCtx, this: &VnStr, index: i64) -> Option<VmValue> {
            let b = this.as_bytes();
            if this.is_ascii() {
                let len = b.len() as i64;
                let idx = if index < 0 { len + index } else { index };
                if idx < 0 || idx >= len {
                    return None;
                }
                let idx = idx as usize;
                return Some(ctx.str_sub(this, idx, 1));
            }
            let len = char_len(this, false) as i64;
            let idx = if index < 0 { len + index } else { index };
            if idx < 0 || idx >= len {
                return None;
            }
            let (bs, be) = char_range_to_bytes(this, false, idx as usize, idx as usize + 1);
            Some(ctx.str_sub(this, bs, be - bs))
        }
        fn substr(ctx: &mut dyn NativeCtx, this: &VnStr, start: i64, length: Option<i64>) -> VmValue {
            let b = this.as_bytes();
            if this.is_ascii() {
                let len = b.len() as i64;
                let st = if start < 0 { (len + start).max(0) as usize } else { (start as usize).min(b.len()) };
                let count = length.map(|c| c.max(0) as usize).unwrap_or(b.len() - st);
                let end = (st + count).min(b.len());
                return ctx.str_sub(this, st, end - st);
            }
            let len = char_len(this, false);
            let st = if start < 0 { (len as i64 + start).max(0) as usize } else { (start as usize).min(len) };
            let count = length.map(|c| c.max(0) as usize).unwrap_or(len - st);
            let end = (st + count).min(len);
            let (bs, be) = char_range_to_bytes(this, false, st, end);
            ctx.str_sub(this, bs, be - bs)
        }


        fn replace(_ctx: &mut dyn NativeCtx, this: &VnStr, from: &str, to: &str) -> String {
            this.replacen(from, to, 1)
        }
        fn replaceAll(_ctx: &mut dyn NativeCtx, this: &VnStr, from: &str, to: &str) -> String {
            if from.is_empty() {
                return this.to_string();
            }
            this.replace(from, to)
        }
        fn split(ctx: &mut dyn NativeCtx, this: &VnStr, separator: Option<&str>) -> Vec<VmValue> {
            let s = this.as_str();
            let base = s.as_ptr() as usize;
            match separator {
                Some(sep) => {
                    if sep.len() == 1 {
                        let byte = sep.as_bytes()[0];
                        let count = this.as_bytes().iter().filter(|&&b| b == byte).count() + 1;
                        let mut out = Vec::with_capacity(count);
                        for p in s.split(byte as char) {
                            out.push(ctx.str_sub(this, p.as_ptr() as usize - base, p.len()));
                        }
                        out
                    } else {
                        s.split(sep).map(|p| ctx.str_sub(this, p.as_ptr() as usize - base, p.len())).collect()
                    }
                }
                None => {
                    let count = this.chars().count();
                    let mut out = Vec::with_capacity(count);
                    let mut off = 0usize;
                    for c in s.chars() {
                        let l = c.len_utf8();
                        out.push(ctx.str_sub(this, off, l));
                        off += l;
                    }
                    out
                }
            }
        }
        fn lines(ctx: &mut dyn NativeCtx, this: &VnStr) -> Vec<VmValue> {
            let s = this.as_str();
            let base = s.as_ptr() as usize;
            s.lines().map(|l| ctx.str_sub(this, l.as_ptr() as usize - base, l.len())).collect()
        }
        fn words(ctx: &mut dyn NativeCtx, this: &VnStr) -> Vec<VmValue> {
            let s = this.as_str();
            let base = s.as_ptr() as usize;
            s.split_whitespace().map(|w| ctx.str_sub(this, w.as_ptr() as usize - base, w.len())).collect()
        }


        fn charCodeAt(_ctx: &mut dyn NativeCtx, this: &VnStr, pos: i64) -> i64 {
            this.char_code_at(pos)
        }
        fn charCode(_ctx: &mut dyn NativeCtx, this: &VnStr) -> i64 {
            this.chars().next().map(|c| c as i64).unwrap_or(-1)
        }
        fn codePointAt(_ctx: &mut dyn NativeCtx, this: &VnStr, pos: i64) -> i64 {
            this.char_code_at(pos)
        }


        fn repeat(_ctx: &mut dyn NativeCtx, this: &VnStr, n: i64) -> String {
            this.repeat(n.max(0) as usize)
        }
        fn padStart(_ctx: &mut dyn NativeCtx, this: &VnStr, target: i64, pad: Option<&str>) -> String {
            pad_start(this, target, pad)
        }
        fn padLeft(_ctx: &mut dyn NativeCtx, this: &VnStr, target: i64, pad: Option<&str>) -> String {
            pad_start(this, target, pad)
        }
        fn padEnd(_ctx: &mut dyn NativeCtx, this: &VnStr, target: i64, pad: Option<&str>) -> String {
            pad_end(this, target, pad)
        }
        fn padRight(_ctx: &mut dyn NativeCtx, this: &VnStr, target: i64, pad: Option<&str>) -> String {
            pad_end(this, target, pad)
        }
        fn concat(_ctx: &mut dyn NativeCtx, this: &VnStr, other: &str) -> String {
            format!("{this}{other}")
        }


        fn isEmpty(_ctx: &mut dyn NativeCtx, this: &VnStr) -> bool { this.is_empty() }
        fn isBlank(_ctx: &mut dyn NativeCtx, this: &VnStr) -> bool { this.trim().is_empty() }
        fn isDigit(_ctx: &mut dyn NativeCtx, this: &VnStr) -> bool {
            !this.is_empty() && this.chars().all(|c| c.is_ascii_digit())
        }
        fn isLetter(_ctx: &mut dyn NativeCtx, this: &VnStr) -> bool {
            !this.is_empty() && this.chars().all(|c| c.is_alphabetic())
        }
        fn isWhitespace(_ctx: &mut dyn NativeCtx, this: &VnStr) -> bool {
            !this.is_empty() && this.chars().all(|c| c.is_whitespace())
        }


        fn reverse(_ctx: &mut dyn NativeCtx, this: &VnStr) -> String { this.chars().rev().collect() }
        fn capitalize(_ctx: &mut dyn NativeCtx, this: &VnStr) -> String {
            let mut chars = this.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            }
        }
        fn toInt(_ctx: &mut dyn NativeCtx, this: &VnStr) -> i64 {
            this.trim().parse::<i64>().unwrap_or(0)
        }
        fn toFloat(_ctx: &mut dyn NativeCtx, this: &VnStr) -> f64 {
            this.trim().parse::<f64>().unwrap_or(0.0)
        }
    }
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
        if v.is_int()
            && let Some(c) = std::char::from_u32(v.as_int() as u32)
        {
            res.push(c);
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
