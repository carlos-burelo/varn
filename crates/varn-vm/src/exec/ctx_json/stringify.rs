use varn_core::text_repr::write_quoted_str;

use super::*;

pub(super) fn value_estimate_capacity(ctx: &ExecCtx, val: VmValue) -> usize {
    if ctx.is_array(val) {
        ctx.array_len(val) * 80
    } else if val.is_heap() {
        if let Some(HeapObj::Object(o) | HeapObj::Record(o)) = ctx.heap.get(val.as_heap()) {
            o.borrow().len() * 48 + 32
        } else if let Some(HeapObj::Map(m)) = ctx.heap.get(val.as_heap()) {
            m.read().len() * 48 + 32
        } else {
            128
        }
    } else {
        64
    }
}

const MAX_NESTING: usize = 128;

thread_local! {
    static FAULT: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

pub(super) fn take_fault() -> Option<&'static str> {
    FAULT.with(|fault| fault.take())
}

fn raise(message: &'static str) {
    FAULT.with(|fault| {
        if fault.get().is_none() {
            fault.set(Some(message));
        }
    });
}

fn write_json_float(f: f64, out: &mut String) {
    if f.is_finite() {
        out.push_str(ryu::Buffer::new().format(f));
    } else {
        raise("NaN and Infinity have no JSON form");
        out.push_str("null");
    }
}

#[inline(always)]
pub(super) fn write_int(n: i64, out: &mut String) {
    let mut buf = [0u8; crate::strbuf::INT_MAX_DIGITS];
    let s = crate::strbuf::itoa(n, &mut buf);
    out.push_str(s);
}

pub(super) fn write_json_vm(ctx: &ExecCtx, val: VmValue, out: &mut String, depth: usize) {
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
        write_json_float(f, out);
        return;
    }
    if val.is_sso() {
        let mut buf = [0u8; varn_types::vm_value::SSO_MAX_LEN];
        write_quoted_str(val.sso_as_str(&mut buf), out);
        return;
    }
    if val.is_heap() {
        if depth >= MAX_NESTING {
            raise("nesting is deeper than 128 levels or the value contains a cycle");
            out.push_str("null");
            return;
        }
        if let Some(inst) = ctx.heap.instance(val.as_heap())
            && let Some(class) = varn_types::ClassObj::find_by_id(inst.class_id)
        {
            out.push('{');
            let mut first = true;
            for field in class.layout().fields.iter() {
                if !first {
                    out.push(',');
                }
                first = false;
                write_quoted_str(field.name.as_ref(), out);
                out.push(':');
                let value = inst.read_field(field).unwrap_or_else(VmValue::null);
                write_json_vm(ctx, value, out, depth + 1);
            }
            out.push('}');
            return;
        }
        match ctx.heap.get(val.as_heap()) {
            Some(HeapObj::Str(h)) => {
                write_quoted_str(h.as_str(), out);
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
                            write_json_vm(ctx, elem, out, depth + 1);
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
                            write_json_float(elem, out);
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
                        write_json_vm(ctx, inline[slot].get(), out, depth + 1);
                    }
                } else {
                    for (slot, prefix) in prefixes.iter().enumerate() {
                        out.push_str(prefix);
                        write_json_vm(
                            ctx,
                            obj.field_at(slot).unwrap_or_else(VmValue::null),
                            out,
                            depth + 1,
                        );
                    }
                }
                out.push('}');
                return;
            }
            Some(HeapObj::Map(m)) => {
                let map = m.read();
                out.push('{');
                let mut first = true;
                for (k, &v) in map.iter() {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    let key_s = ctx.heap.str_repr(k.0);
                    write_quoted_str(&key_s, out);
                    out.push(':');
                    write_json_vm(ctx, v, out, depth + 1);
                }
                out.push('}');
                return;
            }
            Some(
                HeapObj::Buffer(_)
                | HeapObj::Module(_)
                | HeapObj::FrozenModule(_)
                | HeapObj::VmClosure(_)
                | HeapObj::Class(_)
                | HeapObj::NativeFn(..)
                | HeapObj::BoundMethod(_)
                | HeapObj::Set(_)
                | HeapObj::Task(_)
                | HeapObj::TaskHandle(_)
                | HeapObj::Range(_)
                | HeapObj::EnumVariant(_)
                | HeapObj::BigInt(_)
                | HeapObj::Decimal(_)
                | HeapObj::Char(_)
                | HeapObj::Generator(_)
                | HeapObj::Spread(_),
            )
            | None => {}
        }
    }
    out.push_str("null");
}
