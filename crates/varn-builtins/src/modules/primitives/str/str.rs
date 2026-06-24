use varn_op_macros::varn_contract;
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
            let mut parts = Vec::with_capacity(len);
            for i in 0..len {
                let elem = arr.get(ctx, i).unwrap_or_else(VmValue::null);
                parts.push(ctx.str_repr(elem));
            }
            parts.join(sep)
        }


        fn length(_ctx: &mut dyn NativeCtx, this: &str) -> i64 { this.chars().count() as i64 }


        fn toString(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.to_string() }
        fn toStr(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.to_string() }
        fn valueOf(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.to_string() }
        fn toLowerCase(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.to_lowercase() }
        fn toUpperCase(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.to_uppercase() }
        fn trim(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.trim().to_string() }
        fn trimStart(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.trim_start().to_string() }
        fn trimEnd(_ctx: &mut dyn NativeCtx, this: &str) -> String { this.trim_end().to_string() }


        fn includes(_ctx: &mut dyn NativeCtx, this: &str, search: &str) -> bool { this.contains(search) }
        fn contains(_ctx: &mut dyn NativeCtx, this: &str, search: &str) -> bool { this.contains(search) }
        fn startsWith(_ctx: &mut dyn NativeCtx, this: &str, search: &str) -> bool { this.starts_with(search) }
        fn endsWith(_ctx: &mut dyn NativeCtx, this: &str, search: &str) -> bool { this.ends_with(search) }

        fn indexOf(_ctx: &mut dyn NativeCtx, this: &str, search: &str) -> i64 {
            let chars: Vec<char> = this.chars().collect();
            let sc: Vec<char> = search.chars().collect();
            if sc.is_empty() { return 0; }
            chars.windows(sc.len()).position(|w| w == sc.as_slice()).map(|i| i as i64).unwrap_or(-1)
        }
        fn lastIndexOf(_ctx: &mut dyn NativeCtx, this: &str, search: &str) -> i64 {
            let chars: Vec<char> = this.chars().collect();
            let sc: Vec<char> = search.chars().collect();
            if sc.is_empty() { return chars.len() as i64; }
            chars.windows(sc.len()).rposition(|w| w == sc.as_slice()).map(|i| i as i64).unwrap_or(-1)
        }


        fn substring(_ctx: &mut dyn NativeCtx, this: &str, start: i64, end: Option<i64>) -> String {
            let chars: Vec<char> = this.chars().collect();
            let len = chars.len();
            let s = (start.max(0) as usize).min(len);
            let e = end.map(|e| e.max(0) as usize).unwrap_or(len).min(len);
            let (si, ei) = if s <= e { (s, e) } else { (e, s) };
            chars[si..ei].iter().collect()
        }
        fn slice(_ctx: &mut dyn NativeCtx, this: &str, start: i64, end: Option<i64>) -> String {
            let chars: Vec<char> = this.chars().collect();
            let len = chars.len() as i64;
            let si = normalize_idx(start, len).min(chars.len());
            let ei = normalize_idx(end.unwrap_or(len), len).min(chars.len()).max(si);
            chars[si..ei].iter().collect()
        }
        fn at(_ctx: &mut dyn NativeCtx, this: &str, index: i64) -> Option<String> {
            let chars: Vec<char> = this.chars().collect();
            let len = chars.len() as i64;
            let idx = if index < 0 { len + index } else { index };
            if idx >= 0 && (idx as usize) < chars.len() {
                Some(chars[idx as usize].to_string())
            } else {
                None
            }
        }
        fn substr(_ctx: &mut dyn NativeCtx, this: &str, start: i64, length: Option<i64>) -> String {
            let chars: Vec<char> = this.chars().collect();
            let len = chars.len();
            let st = if start < 0 { (len as i64 + start).max(0) as usize } else { (start as usize).min(len) };
            let count = length.map(|c| c.max(0) as usize).unwrap_or(len - st);
            let end = (st + count).min(len);
            chars[st..end].iter().collect()
        }


        fn replace(_ctx: &mut dyn NativeCtx, this: &str, from: &str, to: &str) -> String {
            this.replacen(from, to, 1)
        }
        fn replaceAll(_ctx: &mut dyn NativeCtx, this: &str, from: &str, to: &str) -> String {
            this.replace(from, to)
        }
        fn split(ctx: &mut dyn NativeCtx, this: &str, separator: Option<&str>) -> Vec<VmValue> {
            match separator {
                Some(sep) => this.split(sep).map(|p| ctx.alloc_str_owned(p.to_owned())).collect(),
                None => this.chars().map(|c| ctx.alloc_str_owned(c.to_string())).collect(),
            }
        }
        fn lines(ctx: &mut dyn NativeCtx, this: &str) -> Vec<VmValue> {
            this.lines().map(|l| ctx.alloc_str_owned(l.to_owned())).collect()
        }
        fn words(ctx: &mut dyn NativeCtx, this: &str) -> Vec<VmValue> {
            this.split_whitespace().map(|w| ctx.alloc_str_owned(w.to_owned())).collect()
        }


        fn charCodeAt(_ctx: &mut dyn NativeCtx, this: &str, pos: i64) -> i64 {
            this.chars().nth(pos.max(0) as usize).map(|c| c as i64).unwrap_or(-1)
        }
        fn charCode(_ctx: &mut dyn NativeCtx, this: &str) -> i64 {
            this.chars().next().map(|c| c as i64).unwrap_or(-1)
        }
        fn codePointAt(_ctx: &mut dyn NativeCtx, this: &str, pos: i64) -> i64 {
            this.chars().nth(pos.max(0) as usize).map(|c| c as i64).unwrap_or(-1)
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

fn pad_start(s: &str, target: i64, pad: Option<&str>) -> String {
    let target = target.max(0) as usize;
    let pad = pad.filter(|p| !p.is_empty()).unwrap_or(" ");
    let chars: Vec<char> = s.chars().collect();
    if chars.len() >= target {
        return s.to_string();
    }
    let needed = target - chars.len();
    let pad_chars: Vec<char> = pad.chars().collect();
    let prefix: String = pad_chars.iter().cycle().take(needed).collect();
    format!("{prefix}{s}")
}

fn pad_end(s: &str, target: i64, pad: Option<&str>) -> String {
    let target = target.max(0) as usize;
    let pad = pad.filter(|p| !p.is_empty()).unwrap_or(" ");
    let chars: Vec<char> = s.chars().collect();
    if chars.len() >= target {
        return s.to_string();
    }
    let needed = target - chars.len();
    let pad_chars: Vec<char> = pad.chars().collect();
    let suffix: String = pad_chars.iter().cycle().take(needed).collect();
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
