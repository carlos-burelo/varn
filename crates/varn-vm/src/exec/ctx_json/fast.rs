use super::*;

use super::number::fast_parse_f64;

pub(super) fn fast_parse_json(ctx: &mut ExecCtx, text: &str) -> Option<VmValue> {
    let cached_shape = cache_snapshot();
    let mut parser = FastJsonParser {
        bytes: text.as_bytes(),
        pos: 0,
        ctx,
        cached_shape,
    };
    let val = parser.parse_value()?;
    parser.skip_whitespace();
    if parser.pos == parser.bytes.len() {
        Some(val)
    } else {
        None
    }
}

pub(super) struct FastJsonParser<'a, 'ctx> {
    bytes: &'a [u8],
    pos: usize,
    ctx: &'ctx mut ExecCtx,
    cached_shape: Option<CacheEntry>,
}

impl<'a, 'ctx> FastJsonParser<'a, 'ctx> {
    #[inline(always)]
    fn skip_whitespace(&mut self) {
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    #[inline(always)]
    fn peek(&mut self) -> Option<u8> {
        self.skip_whitespace();
        self.bytes.get(self.pos).copied()
    }

    fn parse_value(&mut self) -> Option<VmValue> {
        match self.peek()? {
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b'"' => self.parse_string(),
            b't' => self.parse_true(),
            b'f' => self.parse_false(),
            b'n' => self.parse_null(),
            b'0'..=b'9' | b'-' => self.parse_number(),
            _ => None,
        }
    }

    #[inline(always)]
    fn parse_true(&mut self) -> Option<VmValue> {
        if self.bytes.get(self.pos..self.pos + 4) == Some(b"true") {
            self.pos += 4;
            Some(VmValue::bool_true())
        } else {
            None
        }
    }

    #[inline(always)]
    fn parse_false(&mut self) -> Option<VmValue> {
        if self.bytes.get(self.pos..self.pos + 5) == Some(b"false") {
            self.pos += 5;
            Some(VmValue::bool_false())
        } else {
            None
        }
    }

    #[inline(always)]
    fn parse_null(&mut self) -> Option<VmValue> {
        if self.bytes.get(self.pos..self.pos + 4) == Some(b"null") {
            self.pos += 4;
            Some(VmValue::null())
        } else {
            None
        }
    }

    #[inline]
    fn parse_string(&mut self) -> Option<VmValue> {
        self.pos += 1;
        let start = self.pos;
        let rem = &self.bytes[start..];
        let end_rel = memchr::memchr(b'"', rem)?;
        let candidate = &rem[..end_rel];
        if memchr::memchr(b'\\', candidate).is_none() {
            self.pos = start + end_rel + 1;
            let s = std::str::from_utf8(candidate).ok()?;
            if let Some(sso) = VmValue::try_from_sso(s) {
                return Some(sso);
            }
            return Some(self.ctx.heap.alloc_str_dynamic(s));
        }

        let mut out = String::with_capacity(end_rel);
        let mut i = start;
        while i < self.bytes.len() {
            let b = self.bytes[i];
            if b == b'"' {
                self.pos = i + 1;
                return Some(self.ctx.heap.alloc_str_dynamic(&out));
            } else if b == b'\\' {
                i += 1;
                let esc = self.bytes.get(i).copied()?;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\x08'),
                    b'f' => out.push('\x0c'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        if i + 4 >= self.bytes.len() {
                            return None;
                        }
                        let hex_str = std::str::from_utf8(&self.bytes[i + 1..i + 5]).ok()?;
                        let code = u32::from_str_radix(hex_str, 16).ok()?;
                        let c = std::char::from_u32(code)?;
                        out.push(c);
                        i += 4;
                    }
                    _ => return None,
                }
                i += 1;
            } else {
                out.push(b as char);
                i += 1;
            }
        }
        None
    }

    #[inline]
    fn parse_number(&mut self) -> Option<VmValue> {
        let start = self.pos;
        let mut i = start;
        let mut is_float = false;
        let negative = if i < self.bytes.len() && self.bytes[i] == b'-' {
            i += 1;
            true
        } else {
            false
        };
        let digits_start = i;
        let mut int_val: i64 = 0;
        let mut overflow = false;
        while i < self.bytes.len() {
            let b = self.bytes[i];
            if b.is_ascii_digit() {
                if !is_float && !overflow {
                    let d = (b - b'0') as i64;
                    if let Some(next) = int_val.checked_mul(10).and_then(|v| v.checked_add(d)) {
                        int_val = next;
                    } else {
                        overflow = true;
                    }
                }
                i += 1;
            } else if b == b'.' || b == b'e' || b == b'E' || b == b'+' || b == b'-' {
                is_float = true;
                i += 1;
            } else {
                break;
            }
        }
        if i == digits_start {
            return None;
        }
        self.pos = i;
        if !is_float && !overflow {
            let val = if negative { -int_val } else { int_val };
            return Some(VmValue::from_int(val));
        }
        let f = fast_parse_f64(&self.bytes[start..i])?;
        Some(VmValue::from_f64(f))
    }

    #[inline]
    fn parse_array(&mut self) -> Option<VmValue> {
        self.pos += 1;
        let est_capacity = ((self.bytes.len() - self.pos) / 32).clamp(16, 65536);
        let mut items = Vec::with_capacity(est_capacity);
        loop {
            self.skip_whitespace();
            if self.pos < self.bytes.len() && self.bytes[self.pos] == b']' {
                self.pos += 1;
                return Some(self.ctx.alloc_array(items));
            }
            let elem = self.parse_value()?;
            items.push(elem);
            self.skip_whitespace();
            if self.pos < self.bytes.len() {
                let b = self.bytes[self.pos];
                if b == b',' {
                    self.pos += 1;
                } else if b == b']' {
                    self.pos += 1;
                    return Some(self.ctx.alloc_array(items));
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
    }

    #[inline]
    fn parse_object(&mut self) -> Option<VmValue> {
        self.pos += 1;
        self.skip_whitespace();
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'}' {
            self.pos += 1;
            return Some(self.ctx.alloc_object());
        }

        const INLINE_FIELDS: usize = 16;
        let mut inline = [VmValue::null(); INLINE_FIELDS];
        let mut spilled: Vec<VmValue> = Vec::new();
        let mut n = 0usize;

        let mut matched: Option<usize> = Some(0);
        let mut owned_keys: Vec<String> = Vec::new();

        loop {
            self.skip_whitespace();
            if self.pos >= self.bytes.len() || self.bytes[self.pos] != b'"' {
                return None;
            }
            self.pos += 1;
            let k_start = self.pos;
            let rem = &self.bytes[k_start..];
            let k_end_rel = memchr::memchr(b'"', rem)?;
            let k_bytes = &rem[..k_end_rel];
            self.pos = k_start + k_end_rel + 1;

            let key_str = std::str::from_utf8(k_bytes).ok()?;

            self.skip_whitespace();
            if self.pos >= self.bytes.len() || self.bytes[self.pos] != b':' {
                return None;
            }
            self.pos += 1;

            let val = self.parse_value()?;
            if n < INLINE_FIELDS {
                inline[n] = val;
            } else {
                if spilled.is_empty() {
                    spilled.extend_from_slice(&inline);
                }
                spilled.push(val);
            }
            n += 1;

            let cached_match = matched.and_then(|k| {
                self.cached_shape
                    .as_ref()
                    .and_then(|(keys, _)| keys.get(k))
                    .map(|expected| (k, expected.as_str() == key_str))
            });

            match cached_match {
                Some((k, true)) => {
                    matched = Some(k + 1);
                }
                Some((k, false)) => {
                    owned_keys = key_prefix(&self.cached_shape, k);
                    owned_keys.push(key_str.to_owned());
                    matched = None;
                }
                None => {
                    if let Some(k) = matched {
                        owned_keys = key_prefix(&self.cached_shape, k);
                        matched = None;
                    }
                    owned_keys.push(key_str.to_owned());
                }
            }

            self.skip_whitespace();
            if self.pos < self.bytes.len() {
                let b = self.bytes[self.pos];
                if b == b',' {
                    self.pos += 1;
                } else if b == b'}' {
                    self.pos += 1;
                    break;
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }

        let values: &[VmValue] = if spilled.is_empty() {
            &inline[..n]
        } else {
            &spilled
        };

        if matched == Some(n) {
            if let Some((keys, shape)) = &self.cached_shape {
                if keys.len() == n {
                    return Some(self.ctx.heap.alloc_object_with_shape_slice(shape, values));
                }
            }
        }

        if let Some(k) = matched {
            owned_keys = key_prefix(&self.cached_shape, k);
        }
        let obj = self.ctx.alloc_object();
        for (k, v) in owned_keys.iter().zip(values.iter()) {
            self.ctx.set_field(obj, k, *v);
        }
        if let Some(shape) = self.ctx.get_object_shape(obj) {
            let entry = (std::rc::Rc::new(owned_keys), shape);
            self.cached_shape = Some(entry.clone());
            JSON_SHAPE_CACHE.with(|c| *c.borrow_mut() = Some(entry));
        }
        Some(obj)
    }
}
