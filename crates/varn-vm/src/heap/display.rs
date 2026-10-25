use super::obj::HeapObj;
use super::structs::HeapInner;
use crate::value::VmValue;
use std::fmt::Write;
use varn_types::ClassObj;

const MAX_DISPLAY_DEPTH: usize = 12;

impl HeapInner {
    pub(crate) fn str_repr(&self, nv: VmValue) -> String {
        let mut out = String::new();
        self.write_repr(nv, &mut out, &mut Vec::new());
        out
    }

    fn write_repr(&self, nv: VmValue, out: &mut String, path: &mut Vec<u64>) {
        if nv.is_null() {
            out.push_str("null");
        } else if nv.is_bool() {
            out.push_str(if nv.as_bool() { "true" } else { "false" });
        } else if nv.is_int() {
            let _ = write!(out, "{}", nv.as_int());
        } else if nv.is_f64() {
            out.push_str(&varn_core::float_to_string(nv.as_f64()));
        } else if nv.is_sso() {
            let mut buf = [0u8; varn_types::vm_value::SSO_MAX_LEN];
            out.push_str(nv.sso_as_str(&mut buf));
        } else if nv.is_heap() {
            self.write_heap_repr(nv, out, path);
        } else {
            out.push_str("null");
        }
    }

    fn write_heap_repr(&self, nv: VmValue, out: &mut String, path: &mut Vec<u64>) {
        let id = nv.raw_payload();
        if path.contains(&id) {
            out.push_str("[Circular]");
            return;
        }
        if path.len() >= MAX_DISPLAY_DEPTH {
            out.push('…');
            return;
        }
        path.push(id);
        self.write_heap_body(nv, out, path);
        path.pop();
    }

    fn write_list(
        &self,
        open: &str,
        close: &str,
        items: impl Iterator<Item = VmValue>,
        out: &mut String,
        path: &mut Vec<u64>,
    ) {
        out.push_str(open);
        for (at, item) in items.enumerate() {
            if at > 0 {
                out.push_str(", ");
            }
            self.write_repr(item, out, path);
        }
        out.push_str(close);
    }

    fn write_fields(
        &self,
        open: &str,
        fields: impl Iterator<Item = (String, VmValue)>,
        out: &mut String,
        path: &mut Vec<u64>,
    ) {
        out.push_str(open);
        let mut any = false;
        for (name, value) in fields {
            out.push_str(if any { ", " } else { " " });
            any = true;
            out.push_str(&name);
            out.push_str(": ");
            self.write_repr(value, out, path);
        }
        out.push_str(if any { " }" } else { "}" });
    }

    fn write_heap_body(&self, nv: VmValue, out: &mut String, path: &mut Vec<u64>) {
        if let Some(inst) = self.instance(nv.as_heap()) {
            let class = ClassObj::find_by_id(inst.class_id);
            let name = class.as_ref().map_or("Object", |c| c.name.as_str());
            let fields: Vec<(String, VmValue)> = match &class {
                Some(c) => c
                    .layout()
                    .fields
                    .iter()
                    .filter_map(|f| inst.read_field(f).map(|v| (f.name.to_string(), v)))
                    .collect(),
                None => Vec::new(),
            };
            out.push_str(name);
            out.push(' ');
            self.write_fields("{", fields.into_iter(), out, path);
            return;
        }
        let Some(obj) = self.get(nv.as_heap()) else {
            out.push_str("null");
            return;
        };
        match obj {
            HeapObj::Str(s) => out.push_str(s.as_ref()),
            HeapObj::Char(c) => out.push(*c),
            HeapObj::Array(a) => {
                let items: Vec<VmValue> = (0..a.len()).filter_map(|i| a.get_vm(i)).collect();
                self.write_list("[", "]", items.into_iter(), out, path);
            }
            HeapObj::Tuple(a) if a.is_empty() => out.push_str("()"),
            HeapObj::Tuple(a) => {
                let items: Vec<VmValue> = (0..a.len()).filter_map(|i| a.get_vm(i)).collect();
                self.write_list("#[", "]", items.into_iter(), out, path);
            }
            HeapObj::Object(_) => {
                let entries = crate::exec::normalize::object_entries(self, nv).unwrap_or_default();
                self.write_fields("{", entries.into_iter(), out, path);
            }
            HeapObj::Record(_) => {
                let entries = crate::exec::normalize::object_entries(self, nv).unwrap_or_default();
                self.write_fields("#{", entries.into_iter(), out, path);
            }
            HeapObj::Map(m) => {
                let entries: Vec<(VmValue, VmValue)> =
                    m.read().iter().map(|(k, v)| (k.0, *v)).collect();
                out.push_str("Map {");
                for (at, (k, v)) in entries.into_iter().enumerate() {
                    out.push_str(if at > 0 { ", " } else { " " });
                    self.write_repr(k, out, path);
                    out.push_str(" => ");
                    self.write_repr(v, out, path);
                }
                out.push_str(" }");
            }
            HeapObj::Set(s) => {
                let items: Vec<VmValue> = s.read().iter().map(|k| k.0).collect();
                self.write_list("Set { ", " }", items.into_iter(), out, path);
            }
            HeapObj::Range(r) => {
                let _ = write!(
                    out,
                    "{}..{}{}",
                    r.start,
                    if r.inclusive { "=" } else { "" },
                    r.end
                );
            }
            HeapObj::VmClosure(nc) => {
                let _ = write!(
                    out,
                    "[Function {}]",
                    nc.proto.name.as_deref().unwrap_or("<anon>")
                );
            }
            HeapObj::NativeFn(_, name) => {
                let _ = write!(out, "[NativeFn: {name}]");
            }
            HeapObj::BoundMethod(method) => match &method.target {
                varn_types::value::BoundMethodTarget::Native { name, .. } => {
                    let _ = write!(out, "[Function {name}]");
                }
                varn_types::value::BoundMethodTarget::Vm { .. } => out.push_str("[BoundMethod]"),
            },
            HeapObj::Class(c) => {
                let _ = write!(out, "[class {}]", c.name);
            }
            HeapObj::BigInt(n) => {
                let _ = write!(out, "{n}");
            }
            HeapObj::Decimal(d) => out.push_str(&d.to_plain_string()),
            HeapObj::EnumVariant(v) if v.def.fields.is_empty() => {
                let _ = write!(out, "{}.{}", v.def.enum_name, v.def.variant_name);
            }
            HeapObj::EnumVariant(v) => {
                let _ = write!(out, "{}.{}(..)", v.def.enum_name, v.def.variant_name);
            }
            HeapObj::Buffer(_) => out.push_str("[Bytes]"),
            HeapObj::Module(_) | HeapObj::FrozenModule(_) => out.push_str("[Module]"),
            HeapObj::Task(_) => out.push_str("[Task]"),
            HeapObj::TaskHandle(_) => out.push_str("[TaskHandle]"),
            HeapObj::Generator(_) => out.push_str("[Generator]"),
            HeapObj::Spread(_) => out.push_str("[Spread]"),
        }
    }
}
