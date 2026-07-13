use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::Deserializer;
use std::borrow::Cow;
use std::fmt::Write;
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, Value, VmValue};

pub struct JsonRuntime;

thread_local! {
    static JSON_SHAPE_CACHE: std::cell::RefCell<Option<(Vec<String>, std::rc::Rc<varn_types::Shape>)>> = const { std::cell::RefCell::new(None) };
}

varn_contract! {
    module: "runtime:json",
    contract: "src/modules/host/json/json_runtime.vn",
    impl JsonRuntime {
        fn parse(ctx: &mut dyn NativeCtx, text: &str) -> Result<VmValue, String> {
            JSON_SHAPE_CACHE.with(|c| *c.borrow_mut() = None);
            let mut deserializer = serde_json::Deserializer::from_str(text);
            deserializer
                .deserialize_any(VmVisitor(ctx))
                .map_err(|e| format!("JSON.parse: {e}"))
        }

        fn stringify(ctx: &mut dyn NativeCtx, value: VmValue) -> Result<String, String> {
            let mut out = String::with_capacity(value_estimate_capacity(ctx, value));
            write_json_vm(ctx, value, &mut out);
            Ok(out)
        }
    }
}

fn value_estimate_capacity(ctx: &dyn NativeCtx, val: VmValue) -> usize {
    if ctx.is_array(val) {
        ctx.array_len(val) * 64
    } else {
        1024
    }
}

fn write_json_vm(ctx: &dyn NativeCtx, val: VmValue, out: &mut String) {
    if val.is_null() {
        out.push_str("null");
    } else if val.is_bool() {
        out.push_str(if val.as_bool() { "true" } else { "false" });
    } else if val.is_int() {
        let _ = write!(out, "{}", val.as_int());
    } else if val.is_f64() {
        let f = val.as_f64();
        if f.is_finite() {
            let _ = write!(out, "{}", f);
        } else {
            out.push_str("null");
        }
    } else if ctx.is_string(val) {
        let s = ctx.str_repr_borrowed(val);
        write_json_str(&s, out);
    } else if ctx.is_array(val) {
        out.push('[');
        let mut first = true;
        ctx.array_for_each(val, &mut |item, _| {
            if !first {
                out.push(',');
            }
            first = false;
            write_json_vm(ctx, item, out);
        });
        out.push(']');
    } else if ctx.is_object(val) {
        out.push('{');
        let mut first = true;
        ctx.object_for_each(val, &mut |k, nv| {
            if !first {
                out.push(',');
            }
            first = false;
            write_json_str(k, out);
            out.push(':');
            write_json_vm(ctx, nv, out);
        });
        out.push('}');
    } else {
        let extracted = ctx.extract(val);
        write_value_json(&extracted, ctx, out);
    }
}

fn write_value_json(val: &Value, ctx: &dyn NativeCtx, out: &mut String) {
    match val {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => {
            let _ = write!(out, "{}", i);
        }
        Value::Float(f) => {
            if f.is_finite() {
                let _ = write!(out, "{}", f);
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
            let guard = o.borrow();
            for (k, nv) in guard.inner.iter() {
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

struct VmSeed<'a>(&'a mut dyn NativeCtx);

impl<'de, 'a> DeserializeSeed<'de> for VmSeed<'a> {
    type Value = VmValue;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(VmVisitor(self.0))
    }
}

struct VmVisitor<'a>(&'a mut dyn NativeCtx);

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
        while let Some(key) = map.next_key::<Cow<str>>()? {
            let val = map.next_value_seed(VmSeed(self.0))?;
            keys.push(key.into_owned());
            values.push(val);
        }

        let cached = JSON_SHAPE_CACHE.with(|c| c.borrow().clone());
        if let Some((ref cached_keys, ref cached_shape)) = cached {
            if cached_keys == &keys {
                return Ok(self.0.alloc_object_with_shape(cached_shape, values));
            }
        }

        let obj = self.0.alloc_object();
        for (k, v) in keys.iter().zip(values.into_iter()) {
            self.0.set_field(obj, k, v);
        }
        if let Some(shape) = self.0.get_object_shape(obj) {
            JSON_SHAPE_CACHE.with(|c| *c.borrow_mut() = Some((keys, shape)));
        }
        Ok(obj)
    }
}
