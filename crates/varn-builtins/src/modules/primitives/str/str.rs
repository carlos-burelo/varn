#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_const, varn_getter, varn_method, varn_module, varn_static};
use varn_types::{NativeCtx, NativeFnResult, VmValue};

fn normalize_idx(idx: i64, len: i64) -> usize {
    if idx < 0 {
        (len + idx).max(0) as usize
    } else {
        idx as usize
    }
}

#[varn_module("globals")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_class("str")]
    pub mod str_class {
        use super::*;

        #[varn_getter("length")]
        pub fn length(ctx: &mut dyn NativeCtx, this: VmValue) -> NativeFnResult {
            if this.is_sso() {
                return Ok(VmValue::from_int(this.sso_len() as i64));
            }
            if let Some(s) = ctx.str_owned(this) {
                return Ok(VmValue::from_int(s.chars().count() as i64));
            }
            Ok(VmValue::from_int(0))
        }

        #[varn_method("toLowerCase")]
        pub fn to_lower(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                return Ok(ctx.alloc_str_owned(s.to_lowercase()));
            }
            Ok(VmValue::null())
        }

        #[varn_method("toUpperCase")]
        pub fn to_upper(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                return Ok(ctx.alloc_str_owned(s.to_uppercase()));
            }
            Ok(VmValue::null())
        }

        #[varn_method("trim")]
        pub fn trim(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                return Ok(ctx.alloc_str_owned(s.trim().to_owned()));
            }
            Ok(VmValue::null())
        }

        #[varn_method("trimStart")]
        pub fn trim_start(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                return Ok(ctx.alloc_str_owned(s.trim_start().to_owned()));
            }
            Ok(VmValue::null())
        }

        #[varn_method("trimEnd")]
        pub fn trim_end(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                return Ok(ctx.alloc_str_owned(s.trim_end().to_owned()));
            }
            Ok(VmValue::null())
        }

        #[varn_method("includes")]
        pub fn includes(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(&search_v) = args.first() {
                if let (Some(s), Some(search)) = (ctx.str_owned(this), ctx.str_owned(search_v)) {
                    return Ok(VmValue::from_bool(s.contains(&search)));
                }
            }
            Ok(VmValue::from_bool(false))
        }

        #[varn_method("contains")]
        pub fn contains(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> NativeFnResult {
            includes(ctx, this, args)
        }

        #[varn_method("startsWith")]
        pub fn starts_with(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(&search_v) = args.first() {
                if let (Some(s), Some(search)) = (ctx.str_owned(this), ctx.str_owned(search_v)) {
                    return Ok(VmValue::from_bool(s.starts_with(&search)));
                }
            }
            Ok(VmValue::from_bool(false))
        }

        #[varn_method("endsWith")]
        pub fn ends_with(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(&search_v) = args.first() {
                if let (Some(s), Some(search)) = (ctx.str_owned(this), ctx.str_owned(search_v)) {
                    return Ok(VmValue::from_bool(s.ends_with(&search)));
                }
            }
            Ok(VmValue::from_bool(false))
        }

        #[varn_method("indexOf")]
        pub fn index_of(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(&search_v) = args.first() {
                if let (Some(s), Some(search)) = (ctx.str_owned(this), ctx.str_owned(search_v)) {
                    let chars: Vec<char> = s.chars().collect();
                    let search_chars: Vec<char> = search.chars().collect();
                    let result = chars
                        .windows(search_chars.len())
                        .position(|w| w == search_chars.as_slice())
                        .map(|i| i as i64)
                        .unwrap_or(-1);
                    return Ok(VmValue::from_int(result));
                }
            }
            Ok(VmValue::from_int(-1))
        }

        #[varn_method("lastIndexOf")]
        pub fn last_index_of(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(&search_v) = args.first() {
                if let (Some(s), Some(search)) = (ctx.str_owned(this), ctx.str_owned(search_v)) {
                    let chars: Vec<char> = s.chars().collect();
                    let search_chars: Vec<char> = search.chars().collect();
                    let result = chars
                        .windows(search_chars.len())
                        .rposition(|w| w == search_chars.as_slice())
                        .map(|i| i as i64)
                        .unwrap_or(-1);
                    return Ok(VmValue::from_int(result));
                }
            }
            Ok(VmValue::from_int(-1))
        }

        #[varn_method("slice")]
        pub fn slice(ctx: &mut dyn NativeCtx, this: VmValue, args: &[VmValue]) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                let chars: Vec<char> = s.chars().collect();
                let len = chars.len() as i64;
                let start = args
                    .first()
                    .and_then(|&a| if a.is_int() { Some(a.as_int()) } else { None })
                    .unwrap_or(0);
                let end = args
                    .get(1)
                    .and_then(|&a| if a.is_int() { Some(a.as_int()) } else { None })
                    .unwrap_or(len);
                let si = normalize_idx(start, len).min(chars.len());
                let ei = normalize_idx(end, len).min(chars.len()).max(si);
                return Ok(ctx.alloc_str_owned(chars[si..ei].iter().collect()));
            }
            Ok(ctx.alloc_str(""))
        }

        #[varn_method("substring")]
        pub fn substring(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                let chars: Vec<char> = s.chars().collect();
                let len = chars.len();
                let start = args
                    .first()
                    .and_then(|&a| {
                        if a.is_int() {
                            Some(a.as_int().max(0) as usize)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0)
                    .min(len);
                let end = args
                    .get(1)
                    .and_then(|&a| {
                        if a.is_int() {
                            Some(a.as_int().max(0) as usize)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(len)
                    .min(len);
                let (si, ei) = if start <= end {
                    (start, end)
                } else {
                    (end, start)
                };
                return Ok(ctx.alloc_str_owned(chars[si..ei].iter().collect()));
            }
            Ok(ctx.alloc_str(""))
        }

        #[varn_method("replace")]
        pub fn replace(ctx: &mut dyn NativeCtx, this: VmValue, args: &[VmValue]) -> NativeFnResult {
            if let (Some(&from_v), Some(&to_v)) = (args.first(), args.get(1)) {
                if let (Some(s), Some(from), Some(to)) = (
                    ctx.str_owned(this),
                    ctx.str_owned(from_v),
                    ctx.str_owned(to_v),
                ) {
                    return Ok(ctx.alloc_str_owned(s.replacen(&from, &to, 1)));
                }
            }
            if let Some(s) = ctx.str_owned(this) {
                return Ok(ctx.alloc_str_owned(s));
            }
            Ok(VmValue::null())
        }

        #[varn_method("replaceAll")]
        pub fn replace_all(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> NativeFnResult {
            if let (Some(&from_v), Some(&to_v)) = (args.first(), args.get(1)) {
                if let (Some(s), Some(from), Some(to)) = (
                    ctx.str_owned(this),
                    ctx.str_owned(from_v),
                    ctx.str_owned(to_v),
                ) {
                    return Ok(ctx.alloc_str_owned(s.replace(&from, &to)));
                }
            }
            if let Some(s) = ctx.str_owned(this) {
                return Ok(ctx.alloc_str_owned(s));
            }
            Ok(VmValue::null())
        }

        #[varn_method("split")]
        pub fn split(ctx: &mut dyn NativeCtx, this: VmValue, args: &[VmValue]) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                let parts: Vec<VmValue> =
                    if let Some(sep) = args.first().and_then(|&a| ctx.str_owned(a)) {
                        s.split(&sep)
                            .map(|p| ctx.alloc_str_owned(p.to_owned()))
                            .collect()
                    } else {
                        s.chars()
                            .map(|c| {
                                let mut buf = [0u8; 4];
                                ctx.alloc_str_owned(c.encode_utf8(&mut buf).to_owned())
                            })
                            .collect()
                    };
                return Ok(ctx.alloc_array(parts));
            }
            Ok(ctx.alloc_array(vec![]))
        }

        #[varn_method("repeat")]
        pub fn repeat(ctx: &mut dyn NativeCtx, this: VmValue, args: &[VmValue]) -> NativeFnResult {
            if let Some(&n_v) = args.first() {
                if let Some(s) = ctx.str_owned(this) {
                    let n = if n_v.is_int() {
                        n_v.as_int().max(0) as usize
                    } else {
                        0
                    };
                    return Ok(ctx.alloc_str_owned(s.repeat(n)));
                }
            }
            Ok(ctx.alloc_str(""))
        }

        #[varn_method("padStart")]
        pub fn pad_start(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                let target = args
                    .first()
                    .and_then(|&a| {
                        if a.is_int() {
                            Some(a.as_int().max(0) as usize)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                let pad = args
                    .get(1)
                    .and_then(|&a| ctx.str_owned(a))
                    .unwrap_or_else(|| " ".to_owned());
                let pad = if pad.is_empty() { " ".to_owned() } else { pad };
                let chars: Vec<char> = s.chars().collect();
                if chars.len() >= target {
                    return Ok(ctx.alloc_str_owned(s));
                }
                let needed = target - chars.len();
                let pad_chars: Vec<char> = pad.chars().collect();
                let prefix: String = pad_chars.iter().cycle().take(needed).collect();
                return Ok(ctx.alloc_str_owned(format!("{}{}", prefix, s)));
            }
            Ok(ctx.alloc_str(""))
        }

        #[varn_method("padLeft")]
        pub fn pad_left(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> NativeFnResult {
            pad_start(ctx, this, args)
        }

        #[varn_method("padEnd")]
        pub fn pad_end(ctx: &mut dyn NativeCtx, this: VmValue, args: &[VmValue]) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                let target = args
                    .first()
                    .and_then(|&a| {
                        if a.is_int() {
                            Some(a.as_int().max(0) as usize)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                let pad = args
                    .get(1)
                    .and_then(|&a| ctx.str_owned(a))
                    .unwrap_or_else(|| " ".to_owned());
                let pad = if pad.is_empty() { " ".to_owned() } else { pad };
                let chars: Vec<char> = s.chars().collect();
                if chars.len() >= target {
                    return Ok(ctx.alloc_str_owned(s));
                }
                let needed = target - chars.len();
                let pad_chars: Vec<char> = pad.chars().collect();
                let suffix: String = pad_chars.iter().cycle().take(needed).collect();
                return Ok(ctx.alloc_str_owned(format!("{}{}", s, suffix)));
            }
            Ok(ctx.alloc_str(""))
        }

        #[varn_method("padRight")]
        pub fn pad_right(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> NativeFnResult {
            pad_end(ctx, this, args)
        }

        #[varn_method("at")]
        pub fn at(ctx: &mut dyn NativeCtx, this: VmValue, args: &[VmValue]) -> NativeFnResult {
            if let Some(&idx_v) = args.first() {
                if let Some(s) = ctx.str_owned(this) {
                    let chars: Vec<char> = s.chars().collect();
                    let len = chars.len() as i64;
                    let idx = if idx_v.is_int() {
                        idx_v.as_int()
                    } else {
                        return Ok(VmValue::null());
                    };
                    let idx = if idx < 0 { len + idx } else { idx };
                    if idx >= 0 && (idx as usize) < chars.len() {
                        let mut buf = [0u8; 4];
                        return Ok(ctx.alloc_str_owned(
                            chars[idx as usize].encode_utf8(&mut buf).to_owned(),
                        ));
                    }
                }
            }
            Ok(VmValue::null())
        }

        #[varn_method("charCode")]
        pub fn char_code(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                if let Some(c) = s.chars().next() {
                    return Ok(VmValue::from_int(c as i64));
                }
            }
            Ok(VmValue::from_int(-1))
        }

        #[varn_method("charCodeAt")]
        pub fn char_code_at(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(&idx_v) = args.first() {
                if let Some(s) = ctx.str_owned(this) {
                    let idx = if idx_v.is_int() {
                        idx_v.as_int().max(0) as usize
                    } else {
                        0
                    };
                    if let Some(c) = s.chars().nth(idx) {
                        return Ok(VmValue::from_int(c as i64));
                    }
                }
            }
            Ok(VmValue::from_int(-1))
        }

        #[varn_method("codePointAt")]
        pub fn code_point_at(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> NativeFnResult {
            char_code_at(ctx, this, args)
        }

        #[varn_method("concat")]
        pub fn concat(ctx: &mut dyn NativeCtx, this: VmValue, args: &[VmValue]) -> NativeFnResult {
            if let Some(&other_v) = args.first() {
                if let (Some(s), Some(other)) = (ctx.str_owned(this), ctx.str_owned(other_v)) {
                    return Ok(ctx.alloc_str_owned(format!("{}{}", s, other)));
                }
            }
            if let Some(s) = ctx.str_owned(this) {
                return Ok(ctx.alloc_str_owned(s));
            }
            Ok(ctx.alloc_str(""))
        }

        #[varn_method("substr")]
        pub fn substr(ctx: &mut dyn NativeCtx, this: VmValue, args: &[VmValue]) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                let chars: Vec<char> = s.chars().collect();
                let len = chars.len();
                let start = args
                    .first()
                    .and_then(|&a| {
                        if a.is_int() {
                            let i = a.as_int();
                            Some(if i < 0 {
                                (len as i64 + i).max(0) as usize
                            } else {
                                (i as usize).min(len)
                            })
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                let count = args
                    .get(1)
                    .and_then(|&a| {
                        if a.is_int() {
                            Some(a.as_int().max(0) as usize)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(len - start);
                let end = (start + count).min(len);
                return Ok(ctx.alloc_str_owned(chars[start..end].iter().collect()));
            }
            Ok(ctx.alloc_str(""))
        }

        #[varn_method("isEmpty")]
        pub fn is_empty(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if this.is_sso() {
                return Ok(VmValue::from_bool(this.sso_len() == 0));
            }
            if let Some(s) = ctx.str_owned(this) {
                return Ok(VmValue::from_bool(s.is_empty()));
            }
            Ok(VmValue::from_bool(true))
        }

        #[varn_method("isBlank")]
        pub fn is_blank(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                return Ok(VmValue::from_bool(s.trim().is_empty()));
            }
            Ok(VmValue::from_bool(true))
        }

        #[varn_method("isDigit")]
        pub fn is_digit(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                return Ok(VmValue::from_bool(
                    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()),
                ));
            }
            Ok(VmValue::from_bool(false))
        }

        #[varn_method("isLetter")]
        pub fn is_letter(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                return Ok(VmValue::from_bool(
                    !s.is_empty() && s.chars().all(|c| c.is_alphabetic()),
                ));
            }
            Ok(VmValue::from_bool(false))
        }

        #[varn_method("isWhitespace")]
        pub fn is_whitespace(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                return Ok(VmValue::from_bool(
                    !s.is_empty() && s.chars().all(|c| c.is_whitespace()),
                ));
            }
            Ok(VmValue::from_bool(false))
        }

        #[varn_method("reverse")]
        pub fn reverse(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                return Ok(ctx.alloc_str_owned(s.chars().rev().collect()));
            }
            Ok(ctx.alloc_str(""))
        }

        #[varn_method("capitalize")]
        pub fn capitalize(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                let mut chars = s.chars();
                let result = match chars.next() {
                    None => String::new(),
                    Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                };
                return Ok(ctx.alloc_str_owned(result));
            }
            Ok(ctx.alloc_str(""))
        }

        #[varn_method("lines")]
        pub fn lines(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                let parts: Vec<VmValue> = s
                    .lines()
                    .map(|l| ctx.alloc_str_owned(l.to_owned()))
                    .collect();
                return Ok(ctx.alloc_array(parts));
            }
            Ok(ctx.alloc_array(vec![]))
        }

        #[varn_method("words")]
        pub fn words(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                let parts: Vec<VmValue> = s
                    .split_whitespace()
                    .map(|w| ctx.alloc_str_owned(w.to_owned()))
                    .collect();
                return Ok(ctx.alloc_array(parts));
            }
            Ok(ctx.alloc_array(vec![]))
        }

        #[varn_method("toString")]
        pub fn to_str(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            Ok(ctx.alloc_str_owned(ctx.str_repr(this)))
        }

        #[varn_method("toStr")]
        pub fn to_str2(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            Ok(ctx.alloc_str_owned(ctx.str_repr(this)))
        }

        #[varn_method("valueOf")]
        pub fn value_of(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            Ok(ctx.alloc_str_owned(ctx.str_repr(this)))
        }

        #[varn_method("toInt")]
        pub fn to_int(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                if let Ok(n) = s.trim().parse::<i64>() {
                    return Ok(VmValue::from_int(n));
                }
            }
            Ok(VmValue::null())
        }

        #[varn_method("toFloat")]
        pub fn to_float(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(s) = ctx.str_owned(this) {
                if let Ok(f) = s.trim().parse::<f64>() {
                    return Ok(VmValue::from_f64(f));
                }
            }
            Ok(VmValue::null())
        }

        #[varn_const("EMPTY")]
        pub const STR_EMPTY: &str = "";

        #[varn_static("fromCharCode")]
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

        #[varn_static("from")]
        pub fn str_from_value(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
            if let Some(&v) = args.first() {
                return Ok(ctx.alloc_str_owned(ctx.str_repr(v)));
            }
            Ok(ctx.alloc_str(""))
        }

        #[varn_static("parse")]
        pub fn str_parse(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
            if let Some(&v) = args.first() {
                return Ok(ctx.alloc_str_owned(ctx.str_repr(v)));
            }
            Ok(ctx.alloc_str(""))
        }

        #[varn_static("join")]
        pub fn str_join(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
            let arr = args.first().copied().unwrap_or(VmValue::null());
            let sep = args
                .get(1)
                .and_then(|&a| ctx.str_owned(a))
                .unwrap_or_default();
            let len = ctx.array_len(arr);
            let mut parts = Vec::with_capacity(len);
            for i in 0..len {
                let elem = ctx.array_get(arr, i).unwrap_or(VmValue::null());
                parts.push(ctx.str_repr(elem));
            }
            Ok(ctx.alloc_str_owned(parts.join(&sep)))
        }
    }
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

pub fn str_join(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    let arr = args.first().copied().unwrap_or(VmValue::null());
    let sep = args
        .get(1)
        .and_then(|&a| ctx.str_owned(a))
        .unwrap_or_default();
    let len = ctx.array_len(arr);
    let mut parts = Vec::with_capacity(len);
    for i in 0..len {
        let elem = ctx.array_get(arr, i).unwrap_or(VmValue::null());
        parts.push(ctx.str_repr(elem));
    }
    Ok(ctx.alloc_str_owned(parts.join(&sep)))
}
