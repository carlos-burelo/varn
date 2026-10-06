use super::*;

pub(super) fn value_estimate_capacity(ctx: &ExecCtx, val: VmValue) -> usize {
    if ctx.is_array(val) {
        ctx.array_len(val) * 80
    } else if val.is_heap() {
        if let Some(HeapObj::Object(o) | HeapObj::Record(o)) = ctx.heap.get(val.as_heap()) {
            o.borrow().len() * 48 + 32
        } else if let Some(HeapObj::Map(m)) = ctx.heap.get(val.as_heap()) {
            m.borrow().len() * 48 + 32
        } else {
            128
        }
    } else {
        64
    }
}

#[inline(always)]
pub(super) fn write_int(n: i64, out: &mut String) {
    let mut buf = [0u8; crate::strbuf::INT_MAX_DIGITS];
    let s = crate::strbuf::itoa(n, &mut buf);
    out.push_str(s);
}














pub(super) fn write_json_vm(ctx: &ExecCtx, val: VmValue, out: &mut String) {
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
        match ctx.heap.get(val.as_heap()) {
            Some(HeapObj::Str(h)) => {
                write_json_str(h.as_str(), out);
                return;
            }
            Some(HeapObj::Array(a) | HeapObj::Tuple(a)) => {
                out.push('[');
                match a.repr() {
                    varn_types::ArrayRepr::Boxed(items) => {
                        let items = items.as_vec();
                        for (i, &elem) in items.iter().enumerate() {
                            if i > 0 {
                                out.push(',');
                            }
                            write_json_vm(ctx, elem, out);
                        }
                    }
                    varn_types::ArrayRepr::I64(items) => {
                        for (i, &elem) in items.iter().enumerate() {
                            if i > 0 {
                                out.push(',');
                            }
                            write_int(elem, out);
                        }
                    }
                    varn_types::ArrayRepr::F64(items) => {
                        for (i, &elem) in items.iter().enumerate() {
                            if i > 0 {
                                out.push(',');
                            }
                            if elem.is_finite() {
                                out.push_str(ryu::Buffer::new().format(elem));
                            } else {
                                out.push_str("null");
                            }
                        }
                    }
                }
                out.push(']');
                return;
            }
            Some(HeapObj::Object(o) | HeapObj::Record(o)) => {
                let obj = o.borrow();
                out.push('{');
                let shape = obj.shape();
                let prefixes = shape.json_prefixes();
                let inline = obj.inline_slice();
                if inline.len() >= prefixes.len() {
                    for (slot, prefix) in prefixes.iter().enumerate() {
                        out.push_str(prefix);
                        write_json_vm(ctx, inline[slot].get(), out);
                    }
                } else {
                    for (slot, prefix) in prefixes.iter().enumerate() {
                        out.push_str(prefix);
                        write_json_vm(ctx, obj.field_at(slot).unwrap_or_else(VmValue::null), out);
                    }
                }
                out.push('}');
                return;
            }
            Some(HeapObj::Map(m)) => {
                let map = m.borrow();
                out.push('{');
                let mut first = true;
                for (k, &v) in map.iter() {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    let key_s = ctx.heap.str_repr(k.0);
                    write_json_str(&key_s, out);
                    out.push(':');
                    write_json_vm(ctx, v, out);
                }
                out.push('}');
                return;
            }
            _ => {}
        }
    }
    out.push_str("null");
}

pub(super) fn write_json_str(s: &str, out: &mut String) {
    out.push('"');
    let bytes = s.as_bytes();
    
    
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
