use super::cells::SlotState;
use super::obj::HeapObj;
use super::str::{HeapStr, INLINE_STR_CAP};
use super::structs::HeapInner;
use crate::value::VmValue;
use std::sync::Arc;
use varn_types::RuntimeString;

impl HeapInner {
    pub(crate) fn alloc_str(&mut self, s: impl AsRef<str>) -> VmValue {
        let s_ref = s.as_ref();
        if let Some(sso) = VmValue::try_from_sso(s_ref) {
            return sso;
        }
        if let Some(&idx) = self.string_interner.get(s_ref) {
            return VmValue::from_heap(idx);
        }
        self.alloc_str_view(HeapStr::shared(Arc::from(s_ref)))
    }

    pub(crate) fn alloc_str_interned(&mut self, s: impl AsRef<str>) -> VmValue {
        let s_ref = s.as_ref();
        if let Some(sso) = VmValue::try_from_sso(s_ref) {
            return sso;
        }
        if let Some(&idx) = self.string_interner.get(s_ref) {
            return VmValue::from_heap(idx);
        }
        let rs: RuntimeString = Arc::from(s_ref);
        let idx = self
            .cells
            .alloc(HeapObj::Str(HeapStr::shared(rs.clone())), SlotState::Old);
        self.string_interner.insert(rs, idx);
        VmValue::from_heap(idx)
    }

    pub(crate) fn alloc_str_dynamic(&mut self, s: impl AsRef<str>) -> VmValue {
        let s_ref = s.as_ref();
        if let Some(sso) = VmValue::try_from_sso(s_ref) {
            return sso;
        }
        if s_ref.len() <= INLINE_STR_CAP {
            return self.alloc_str_view(HeapStr::inline(s_ref));
        }
        self.alloc_str_view(HeapStr::shared(Arc::from(s_ref)))
    }

    pub(crate) fn alloc_str_view(&mut self, hs: HeapStr) -> VmValue {
        VmValue::from_heap(self.alloc(HeapObj::Str(hs)))
    }

    pub(crate) fn str_val(&self, nv: VmValue) -> Option<RuntimeString> {
        if nv.is_sso() {
            let mut buf = [0u8; 5];
            let s = nv.sso_as_str(&mut buf);
            return Some(Arc::from(s));
        }
        if !nv.is_heap() {
            return None;
        }
        if let Some(HeapObj::Str(s)) = self.get(nv.as_heap()) {
            return Some(s.to_shared());
        }
        None
    }

    pub(crate) fn is_string(&self, nv: VmValue) -> bool {
        if nv.is_sso() {
            return true;
        }
        if nv.is_heap() {
            return matches!(self.get(nv.as_heap()), Some(HeapObj::Str(_)));
        }
        false
    }

    pub(crate) fn str_owned(&self, nv: VmValue) -> Option<String> {
        if nv.is_sso() {
            let mut buf = [0u8; 5];
            let s = nv.sso_as_str(&mut buf);
            return Some(s.to_owned());
        }
        if nv.is_heap() {
            if let Some(HeapObj::Str(s)) = self.get(nv.as_heap()) {
                return Some(s.to_string());
            }
        }
        None
    }

    pub(crate) fn str_repr_borrowed<'a>(&'a self, nv: VmValue) -> std::borrow::Cow<'a, str> {
        if nv.is_heap() {
            if let Some(HeapObj::Str(s)) = self.get(nv.as_heap()) {
                return std::borrow::Cow::Borrowed(s.as_ref());
            }
        }
        std::borrow::Cow::Owned(self.str_repr(nv))
    }

    pub(crate) fn str_repr_into<W: std::fmt::Write>(&self, nv: VmValue, out: &mut W) {
        use crate::strbuf::{itoa, INT_MAX_DIGITS};
        if nv.is_null() {
            let _ = out.write_str("null");
        } else if nv.is_bool() {
            let _ = out.write_str(if nv.as_bool() { "true" } else { "false" });
        } else if nv.is_int() {
            let mut buf = [0u8; INT_MAX_DIGITS];
            let _ = out.write_str(itoa(nv.as_int(), &mut buf));
        } else if nv.is_f64() {
            let f = nv.as_f64();
            if f.fract() == 0.0 && f.abs() < 1e15 {
                let mut buf = [0u8; INT_MAX_DIGITS];
                let _ = out.write_str(itoa(f as i64, &mut buf));
            } else {
                let _ = write!(out, "{}", f);
            }
        } else if nv.is_sso() {
            let mut buf = [0u8; 5];
            let _ = out.write_str(nv.sso_as_str(&mut buf));
        } else if nv.is_heap() {
            if let Some(HeapObj::Str(s)) = self.get(nv.as_heap()) {
                let _ = out.write_str(s.as_ref());
                return;
            }
            let _ = out.write_str(&self.str_repr(nv));
        } else {
            let _ = out.write_str(&self.str_repr(nv));
        }
    }

    pub(crate) fn str_repr(&self, nv: VmValue) -> String {
        if nv.is_null() {
            return "null".into();
        }
        if nv.is_bool() {
            return nv.as_bool().to_string();
        }
        if nv.is_int() {
            return nv.as_int().to_string();
        }
        if nv.is_f64() {
            let f = nv.as_f64();
            if f.fract() == 0.0 && f.abs() < 1e15 {
                return format!("{}", f as i64);
            }
            return format!("{}", f);
        }
        if nv.is_sso() {
            let mut buf = [0u8; 5];
            return nv.sso_as_str(&mut buf).to_owned();
        }
        if nv.is_heap() {
            return match self.get(nv.as_heap()) {
                Some(HeapObj::Str(s)) => s.to_string(),
                Some(HeapObj::Char(c)) => c.to_string(),
                Some(HeapObj::Array(a)) => {
                    let parts: Vec<_> = (0..a.len())
                        .map(|i| self.str_repr(a.get_vm(i).unwrap()))
                        .collect();
                    format!("[{}]", parts.join(", "))
                }
                Some(HeapObj::Tuple(a)) if a.is_empty() => "()".into(),
                Some(HeapObj::Tuple(a)) => {
                    let parts: Vec<_> = (0..a.len())
                        .map(|i| self.str_repr(a.get_vm(i).unwrap()))
                        .collect();
                    format!("#[{}]", parts.join(", "))
                }
                Some(HeapObj::Object(_)) => "[object Object]".into(),
                Some(HeapObj::VmClosure(nc)) => format!(
                    "[Function {}]",
                    nc.proto.name.as_deref().unwrap_or("<anon>")
                ),
                Some(HeapObj::NativeFn(_, name)) => format!("[NativeFn: {}]", name),
                Some(HeapObj::BoundMethod(method)) => match &method.target {
                    varn_types::value::BoundMethodTarget::Native { name, .. } => {
                        format!("[Function {}]", name)
                    }
                    varn_types::value::BoundMethodTarget::Vm { .. } => "[BoundMethod]".into(),
                },
                Some(HeapObj::Class(c)) => format!("[class {}]", c.name),
                Some(HeapObj::BigInt(n)) => n.to_string(),
                Some(HeapObj::Decimal(d)) => d.to_plain_string(),
                Some(
                    HeapObj::Record(_)
                    | HeapObj::Buffer(_)
                    | HeapObj::Module(_)
                    | HeapObj::FrozenModule(_)
                    | HeapObj::Map(_)
                    | HeapObj::Set(_)
                    | HeapObj::Task(_)
                    | HeapObj::TaskHandle(_)
                    | HeapObj::Range(_)
                    | HeapObj::Symbol(_)
                    | HeapObj::EnumVariant(_)
                    | HeapObj::Generator(_)
                    | HeapObj::Spread(_),
                )
                | None => "[object]".into(),
            };
        }
        "null".into()
    }
}
