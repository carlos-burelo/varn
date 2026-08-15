use crate::exec::ctx::ExecCtx;
use crate::heap::HeapObj;
use crate::value::VmValue;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::Deserializer;
use std::borrow::Cow;
use varn_types::{NativeCtx, Value};

thread_local! {
    static JSON_SHAPE_CACHE: ShapeCache = const { std::cell::RefCell::new(None) };
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

/// Serialize one VM value.
///
/// Scalars are decided from the NaN-box tag alone; everything else takes
/// **one** heap lookup and matches the object it found. The chain this
/// replaced asked `is_string`, then `is_array`, then `is_object`, and each of
/// those resolved the heap slot on its own before the branch body resolved it
/// a fourth time — four walks per object where the value's own tag already
/// says which one can succeed.
///
/// Matching the variant directly also removes a silent hole: `is_array`
/// accepts `Tuple` and `is_object` accepts `Record`, but the branch bodies
/// only destructured `Array` and `Object`, so a tuple serialized as `[]` and a
/// record as `{}` — valid JSON that had lost its contents.
fn write_json_vm(ctx: &ExecCtx, val: VmValue, out: &mut String) {
    if val.is_null() {
        out.push_str("null");
        return;
    }
    if val.is_bool() {
        out.push_str(if val.as_bool() { "true" } else { "false" });
        return;
    }
    if val.is_int() {
        write_int(val.as_int(), out);
        return;
    }
    if val.is_f64() {
        let f = val.as_f64();
        if f.is_finite() {
            out.push_str(ryu::Buffer::new().format(f));
        } else {
            out.push_str("null");
        }
        return;
    }
    if val.is_sso() {
        let mut buf = [0u8; 5];
        write_json_str(val.sso_as_str(&mut buf), out);
        return;
    }
    if val.is_heap() {
        match ctx.heap.get(val.as_heap_idx()) {
            Some(HeapObj::Str(h)) => {
                write_json_str(h.as_str(), out);
                return;
            }
            Some(HeapObj::Array(a) | HeapObj::Tuple(a)) => {
                out.push('[');
                for i in 0..a.len() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_json_vm(ctx, a.get_vm(i).unwrap_or_else(VmValue::null), out);
                }
                out.push(']');
                return;
            }
            Some(HeapObj::Object(o) | HeapObj::Record(o)) => {
                // Walked through the shape's ordered names instead of
                // `ObjRef::iter()`: the pair list that would materialise here
                // is one allocation and one `Rc` clone per field, per object.
                let obj = o.borrow();
                out.push('{');
                for (slot, key) in obj.shape().ordered_names().iter().enumerate() {
                    if slot > 0 {
                        out.push(',');
                    }
                    write_json_str(key, out);
                    out.push(':');
                    write_json_vm(ctx, obj.field_at(slot).unwrap_or_else(VmValue::null), out);
                }
                out.push('}');
                return;
            }
            _ => {}
        }
    }
    let extracted = ctx.extract(val);
    write_value_json(&extracted, ctx, out);
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

/// The last object shape a parse produced, with the key list it came from.
///
/// Both halves are behind `Rc` so an object can take a SNAPSHOT of the pair
/// for the duration of its own `visit_map` — two refcount bumps — instead of
/// reaching through the thread-local and the `RefCell` once per field.
///
/// Taking it once also closes a hazard rather than just saving work: the
/// previous code re-read the thread-local per key AND once more at the end, so
/// the entry it finally used need not be the one it compared the keys against
/// — a nested object parsed mid-loop replaces the cache. **This is not a
/// demonstrated bug**; the obvious reproducer (a nested object of the same
/// arity between two siblings, pinned in `tests/73-str-charcode-json-shape.vn`)
/// produces correct output on the previous code too. It is removed because a
/// match against one entry and a build from another is a coincidence to rely
/// on, not an invariant.
type CacheEntry = (std::rc::Rc<Vec<String>>, std::rc::Rc<varn_types::Shape>);
type ShapeCache = std::cell::RefCell<Option<CacheEntry>>;

/// The current cache entry, if any. Cheap enough to call once per object.
fn cache_snapshot() -> Option<CacheEntry> {
    JSON_SHAPE_CACHE.with(|c| c.borrow().clone())
}

/// The snapshot's first `n` keys, owned — how an object that matched up to a
/// point and then diverged recovers the names it never had to materialise.
fn key_prefix(cached: &Option<CacheEntry>, n: usize) -> Vec<String> {
    match cached {
        Some((keys, _)) => keys.iter().take(n).cloned().collect(),
        None => Vec::new(),
    }
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

    /// Reads an object's fields straight into a fixed buffer, checking each key
    /// against the cached shape as it arrives.
    ///
    /// This used to collect keys and values into two `Vec`s, then compare the
    /// whole key list against the cache — two throwaway allocations per object,
    /// plus a third inside `alloc_object_with_shape`, which copies the values
    /// into the object's inline storage and drops the `Vec` again. A document
    /// of 50 000 objects paid that 50 000 times per parse.
    ///
    /// Matching incrementally means a hit never materialises the keys at all:
    /// the keys that matched ARE the cached ones. Only a mismatch has to
    /// recover them, and it recovers the matched prefix from the cache rather
    /// than from the parse.
    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        /// Fields held without allocating. Objects wider than this fall back to
        /// the growable path; JSON documents in the shape this matters for
        /// (records in an array) are far narrower.
        const INLINE_FIELDS: usize = 16;

        let mut inline = [VmValue::null(); INLINE_FIELDS];
        let mut spilled: Vec<VmValue> = Vec::new();
        let mut n = 0usize;

        // Taken once: the loop below parses nested values, which can replace
        // the cache underneath it.
        let cached = cache_snapshot();
        let cached_keys = |k: usize| cached.as_ref().and_then(|(keys, _)| keys.get(k));

        // How many keys so far are the snapshot's, in order. `None` once a key
        // has diverged — from then on keys are collected as owned strings.
        let mut matched: Option<usize> = Some(0);
        let mut owned_keys: Vec<String> = Vec::new();

        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            let val = map.next_value_seed(VmSeed(self.0))?;
            if n < INLINE_FIELDS {
                inline[n] = val;
            } else {
                if spilled.is_empty() {
                    spilled.extend_from_slice(&inline);
                }
                spilled.push(val);
            }
            n += 1;

            match matched {
                Some(k) if cached_keys(k).map(String::as_str) == Some(key.as_ref()) => {
                    matched = Some(k + 1);
                }
                Some(k) => {
                    // Diverged at `k`: the first `k` keys are the snapshot's.
                    owned_keys = key_prefix(&cached, k);
                    owned_keys.push(key.into_owned());
                    matched = None;
                }
                None => owned_keys.push(key.into_owned()),
            }
        }

        let values: &[VmValue] = if spilled.is_empty() {
            &inline[..n]
        } else {
            &spilled
        };

        // A prefix match is not a match: the object must also have ENDED where
        // the snapshot's key list does.
        if matched == Some(n) {
            if let Some((keys, shape)) = &cached {
                if keys.len() == n {
                    return Ok(self.0.heap.alloc_object_with_shape_slice(shape, values));
                }
            }
        }

        // No cached shape, or this object has a different one: build the object
        // field by field so the shape is derived, then cache it for the objects
        // that follow.
        if let Some(k) = matched {
            owned_keys = key_prefix(&cached, k);
        }
        let obj = self.0.alloc_object();
        for (k, v) in owned_keys.iter().zip(values.iter()) {
            self.0.set_field(obj, k, *v);
        }
        if let Some(shape) = self.0.get_object_shape(obj) {
            let entry = (std::rc::Rc::new(owned_keys), shape);
            JSON_SHAPE_CACHE.with(|c| *c.borrow_mut() = Some(entry));
        }
        Ok(obj)
    }
}
