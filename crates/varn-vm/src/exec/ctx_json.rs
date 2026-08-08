use crate::exec::ctx::ExecCtx;
use crate::heap::HeapObj;
use crate::value::VmValue;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::Deserializer;
use std::borrow::Cow;
use varn_types::{NativeCtx, Value};

thread_local! {
    static JSON_SHAPE_CACHE: std::cell::RefCell<Option<(Vec<String>, std::rc::Rc<varn_types::Shape>)>> = const { std::cell::RefCell::new(None) };
}

impl ExecCtx {
    pub(crate) fn json_parse(&mut self, text: &str) -> Result<VmValue, String> {
        JSON_SHAPE_CACHE.with(|c| *c.borrow_mut() = None);
        let mut deserializer = serde_json::Deserializer::from_str(text);
        deserializer
            .deserialize_any(VmVisitor(self))
            .map_err(|e| format!("JSON.parse: {e}"))
    }

    pub(crate) fn json_stringify(&self, value: VmValue) -> Result<String, String> {
        let mut out = String::with_capacity(value_estimate_capacity(self, value));
        write_json_vm(self, value, &mut out);
        Ok(out)
    }
}

fn value_estimate_capacity(ctx: &ExecCtx, val: VmValue) -> usize {
    if ctx.is_array(val) {
        ctx.array_len(val) * 64
    } else {
        1024
    }
}

fn write_int(mut n: i64, out: &mut String) {
    if n == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    let negative = n < 0;
    if negative {
        if n == i64::MIN {
            out.push_str("-9223372036854775808");
            return;
        }
        n = -n;
    }
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if negative {
        i -= 1;
        buf[i] = b'-';
    }
    let s = unsafe { std::str::from_utf8_unchecked(&buf[i..]) };
    out.push_str(s);
}

fn write_json_vm(ctx: &ExecCtx, val: VmValue, out: &mut String) {
    if val.is_null() {
        out.push_str("null");
    } else if val.is_bool() {
        out.push_str(if val.as_bool() { "true" } else { "false" });
    } else if val.is_int() {
        write_int(val.as_int(), out);
    } else if val.is_f64() {
        let f = val.as_f64();
        if f.is_finite() {
            out.push_str(ryu::Buffer::new().format(f));
        } else {
            out.push_str("null");
        }
    } else if ctx.is_string(val) {
        let s = ctx.str_repr_borrowed(val);
        write_json_str(&s, out);
    } else if ctx.is_array(val) {
        out.push('[');
        let mut first = true;
        if val.is_heap() {
            if let Some(HeapObj::Array(a)) = ctx.heap.get(val.as_heap_idx()) {
                for i in 0..a.len() {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    write_json_vm(ctx, a.get_vm(i).unwrap(), out);
                }
            }
        }
        out.push(']');
    } else if ctx.is_object(val) {
        out.push('{');
        let mut first = true;
        if val.is_heap() {
            if let Some(HeapObj::Object(o)) = ctx.heap.get(val.as_heap_idx()) {
                for (k, nv) in o.borrow().iter() {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    write_json_str(k.as_ref(), out);
                    out.push(':');
                    write_json_vm(ctx, nv, out);
                }
            }
        }
        out.push('}');
    } else {
        let extracted = ctx.extract(val);
        write_value_json(&extracted, ctx, out);
    }
}

fn write_value_json(val: &Value, ctx: &ExecCtx, out: &mut String) {
    match val {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => {
            write_int(*i, out);
        }
        Value::Float(f) => {
            if f.is_finite() {
                out.push_str(ryu::Buffer::new().format(*f));
            } else {
                out.push_str("null");
            }
        }
        Value::Str(s) => write_json_str(s, out),
        Value::Array(a) => {
            out.push('[');
            let mut first = true;
            for item in a.borrow().iter() {
                if !first {
                    out.push(',');
                }
                first = false;
                write_value_json(item, ctx, out);
            }
            out.push(']');
        }
        Value::Object(o) => {
            out.push('{');
            let mut first = true;
            for (k, nv) in o.borrow().iter() {
                if !first {
                    out.push(',');
                }
                first = false;
                write_json_str(k.as_ref(), out);
                out.push(':');
                write_json_vm(ctx, nv, out);
            }
            out.push('}');
        }
        _ => out.push_str("null"),
    }
}

fn write_json_str(s: &str, out: &mut String) {
    out.push('"');
    let bytes = s.as_bytes();
    // Fast path: for strings without quotes, backslashes or control characters (< 0x20),
    // append directly in one operation without per-byte branch scanning.
    if !bytes.iter().any(|&b| b == b'"' || b == b'\\' || b < 0x20) {
        out.push_str(s);
        out.push('"');
        return;
    }
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let escaped = match b {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            b'\x08' => "\\b",
            b'\x0c' => "\\f",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            _ => continue,
        };
        if start < i {
            out.push_str(&s[start..i]);
        }
        out.push_str(escaped);
        start = i + 1;
    }
    if start < bytes.len() {
        out.push_str(&s[start..]);
    }
    out.push('"');
}

struct VmSeed<'a>(&'a mut ExecCtx);

impl<'de, 'a> DeserializeSeed<'de> for VmSeed<'a> {
    type Value = VmValue;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(VmVisitor(self.0))
    }
}

struct VmVisitor<'a>(&'a mut ExecCtx);

impl<'de, 'a> Visitor<'de> for VmVisitor<'a> {
    type Value = VmValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("any valid JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
        Ok(VmValue::from_bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
        Ok(VmValue::from_int(v))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
        if v <= i64::MAX as u64 {
            Ok(VmValue::from_int(v as i64))
        } else {
            Ok(VmValue::from_f64(v as f64))
        }
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
        Ok(VmValue::from_f64(v))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
        Ok(self.0.alloc_str(v))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
        Ok(self.0.alloc_str_owned(v))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(VmValue::null())
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(VmValue::null())
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(elem) = seq.next_element_seed(VmSeed(self.0))? {
            items.push(elem);
        }
        Ok(self.0.alloc_array(items))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let cap = map.size_hint().unwrap_or(4);
        let mut keys = Vec::with_capacity(cap);
        let mut values = Vec::with_capacity(cap);
        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            let val = map.next_value_seed(VmSeed(self.0))?;
            keys.push(key);
            values.push(val);
        }

        let shape_opt = JSON_SHAPE_CACHE.with(|c| {
            let borrow = c.borrow();
            if let Some((ref cached_keys, ref cached_shape)) = *borrow {
                if cached_keys.len() == keys.len()
                    && cached_keys
                        .iter()
                        .zip(keys.iter())
                        .all(|(ck, k)| ck.as_str() == k.as_ref())
                {
                    return Some(std::rc::Rc::clone(cached_shape));
                }
            }
            None
        });
        if let Some(cached_shape) = shape_opt {
            return Ok(self.0.alloc_object_with_shape(&cached_shape, values));
        }

        let owned_keys: Vec<String> = keys.iter().map(|k| k.as_ref().to_string()).collect();
        let obj = self.0.alloc_object();
        for (k, v) in owned_keys.iter().zip(values.into_iter()) {
            self.0.set_field(obj, k, v);
        }
        if let Some(shape) = self.0.get_object_shape(obj) {
            JSON_SHAPE_CACHE.with(|c| *c.borrow_mut() = Some((owned_keys, shape)));
        }
        Ok(obj)
    }
}
