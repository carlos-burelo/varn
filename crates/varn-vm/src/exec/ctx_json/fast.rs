use super::*;

use super::number::fast_parse_f64;

pub(super) fn fast_parse_json(ctx: &mut ExecCtx, text: &str) -> Option<VmValue> {
    let mut parser = FastJsonParser {
        bytes: text.as_bytes(),
        pos: 0,
        ctx,
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

    fn read_string_text(&mut self) -> Option<std::borrow::Cow<'a, str>> {
        let bytes: &'a [u8] = self.bytes;
        let start = self.pos + 1;
        let first = memchr::memchr2(b'"', b'\\', &bytes[start..])? + start;
        if bytes[first] == b'"' {
            self.pos = first + 1;
            return Some(std::borrow::Cow::Borrowed(Self::plain_text(
                &bytes[start..first],
            )?));
        }
        let mut out = String::with_capacity(first - start + 16);
        let mut chunk = start;
        let mut at = first;
        loop {
            out.push_str(Self::plain_text(&bytes[chunk..at])?);
            if bytes[at] == b'"' {
                self.pos = at + 1;
                return Some(std::borrow::Cow::Owned(out));
            }
            let mut next = at + 2;
            match *bytes.get(at + 1)? {
                b'"' => out.push('"'),
                b'\\' => out.push('\\'),
                b'/' => out.push('/'),
                b'b' => out.push('\x08'),
                b'f' => out.push('\x0c'),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'u' => {
                    let high = Self::hex4(bytes, at + 2)?;
                    next = at + 6;
                    let code = if (0xD800..0xDC00).contains(&high) {
                        if bytes.get(next) != Some(&b'\\') || bytes.get(next + 1) != Some(&b'u') {
                            return None;
                        }
                        let low = Self::hex4(bytes, next + 2)?;
                        if !(0xDC00..0xE000).contains(&low) {
                            return None;
                        }
                        next += 6;
                        0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)
                    } else {
                        high
                    };
                    out.push(std::char::from_u32(code)?);
                }
                _ => return None,
            }
            chunk = next;
            at = memchr::memchr2(b'"', b'\\', &bytes[chunk..])? + chunk;
        }
    }

    fn plain_text(span: &[u8]) -> Option<&str> {
        if span.iter().any(|&b| b < 0x20) {
            return None;
        }
        std::str::from_utf8(span).ok()
    }

    fn hex4(bytes: &[u8], at: usize) -> Option<u32> {
        let digits = bytes.get(at..at + 4)?;
        std::str::from_utf8(digits)
            .ok()
            .filter(|text| text.bytes().all(|b| b.is_ascii_hexdigit()))
            .and_then(|text| u32::from_str_radix(text, 16).ok())
    }

    #[inline]
    fn parse_string(&mut self) -> Option<VmValue> {
        let text = self.read_string_text()?;
        if let Some(sso) = VmValue::try_from_sso(&text) {
            return Some(sso);
        }
        Some(self.ctx.heap.alloc_str_dynamic(&text))
    }

    #[inline]
    fn parse_number(&mut self) -> Option<VmValue> {
        let start = self.pos;
        let mut i = start;
        let negative = self.bytes.get(i) == Some(&b'-');
        if negative {
            i += 1;
        }
        let digits_start = i;
        while self.bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        let int_end = i;
        if int_end == digits_start
            || (self.bytes[digits_start] == b'0' && int_end - digits_start > 1)
        {
            return None;
        }
        let mut is_float = false;
        if self.bytes.get(i) == Some(&b'.') {
            is_float = true;
            i += 1;
            let frac = i;
            while self.bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            if i == frac {
                return None;
            }
        }
        if matches!(self.bytes.get(i), Some(b'e' | b'E')) {
            is_float = true;
            i += 1;
            if matches!(self.bytes.get(i), Some(b'+' | b'-')) {
                i += 1;
            }
            let exp = i;
            while self.bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            if i == exp {
                return None;
            }
        }
        self.pos = i;
        if !is_float {
            let mut value: i64 = 0;
            let mut fits = true;
            for &b in &self.bytes[digits_start..int_end] {
                let d = i64::from(b - b'0');
                let next = value.checked_mul(10).and_then(|v| {
                    if negative {
                        v.checked_sub(d)
                    } else {
                        v.checked_add(d)
                    }
                });
                match next {
                    Some(n) => value = n,
                    None => {
                        fits = false;
                        break;
                    }
                }
            }
            if fits {
                return Some(VmValue::from_int(value));
            }
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

        let known = self.ctx.json_shapes.len();
        let mut candidates: u64 = if known >= 64 {
            u64::MAX
        } else {
            (1u64 << known) - 1
        };
        let mut owned_keys: Vec<String> = Vec::new();
        let mut cold = candidates == 0;

        loop {
            self.skip_whitespace();
            if self.pos >= self.bytes.len() || self.bytes[self.pos] != b'"' {
                return None;
            }
            let key_text = self.read_string_text()?;
            let key_str: &str = &key_text;

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

            if cold {
                owned_keys.push(key_str.to_owned());
            } else {
                let shapes = &self.ctx.json_shapes;
                let mut next = 0u64;
                let mut rest = candidates;
                while rest != 0 {
                    let i = rest.trailing_zeros() as usize;
                    rest &= rest - 1;
                    if shapes[i].0.get(n).map(String::as_str) == Some(key_str) {
                        next |= 1u64 << i;
                    }
                }
                if next == 0 {
                    let pivot = candidates.trailing_zeros() as usize;
                    owned_keys = shapes[pivot].0[..n].to_vec();
                    owned_keys.push(key_str.to_owned());
                    cold = true;
                } else {
                    candidates = next;
                }
            }
            n += 1;

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

        if !cold {
            let shapes = &self.ctx.json_shapes;
            let mut rest = candidates;
            while rest != 0 {
                let i = rest.trailing_zeros() as usize;
                rest &= rest - 1;
                if shapes[i].0.len() == n {
                    let shape = std::rc::Rc::clone(&shapes[i].1);
                    return Some(self.ctx.heap.alloc_object_with_shape_slice(&shape, values));
                }
            }
            let pivot = candidates.trailing_zeros() as usize;
            owned_keys = shapes[pivot].0[..n].to_vec();
        }
        let obj = self.ctx.alloc_object();
        for (k, v) in owned_keys.iter().zip(values.iter()) {
            self.ctx.set_field(obj, k, *v);
        }
        if self.ctx.json_shapes.len() < JSON_SHAPE_CAP
            && let Some(shape) = self.ctx.get_object_shape(obj)
            && shape.property_names.len() == owned_keys.len()
        {
            self.ctx
                .json_shapes
                .push((std::rc::Rc::new(owned_keys), shape));
        }
        Some(obj)
    }
}
