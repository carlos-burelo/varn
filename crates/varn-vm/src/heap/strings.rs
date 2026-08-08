//! String allocation and rendering.
//!
//! Four allocation paths, and the difference between them is not cosmetic:
//! `alloc_str` interns, `alloc_str_dynamic` does not (a runtime-produced
//! string would otherwise be hashed in full and retained), `alloc_substring`
//! and `alloc_str_view` build views over an existing buffer instead of
//! copying.

use super::core::alloc_into;
use super::obj::HeapObj;
use super::str::{ascii_flag, HeapStr, INLINE_STR_CAP};
use super::structs::HeapInner;
use crate::nursery::{old_idx_raw, pack_old_idx};
use crate::value::VmValue;
use std::rc::Rc;
use varn_types::RuntimeString;

const SLICE_LEN_MASK: u32 = 0x3FFF_FFFF;

impl HeapInner {
    pub(crate) fn alloc_str(&mut self, s: impl AsRef<str>) -> VmValue {
        let s_ref = s.as_ref();
        if let Some(sso) = VmValue::try_from_sso(s_ref) {
            return sso;
        }

        let rs: RuntimeString = Rc::from(s_ref);
        if let Some(&packed) = self.string_interner.get(&rs) {
            let raw = old_idx_raw(packed);
            if self
                .objects
                .get(raw as usize)
                .map(|o| o.is_some())
                .unwrap_or(false)
            {
                return VmValue::from_heap_idx(packed);
            }
            self.string_interner.remove(&rs);
        }

        let idx = match self
            .nursery
            .try_alloc(HeapObj::Str(HeapStr::shared(rs.clone())))
        {
            Ok(ni) => ni,
            Err(obj) => {
                let oi = alloc_into(
                    &mut self.objects,
                    &mut self.free,
                    &mut self.alloc_count,
                    &mut self.gc_alloc_since_collect,
                    obj,
                );
                let packed = pack_old_idx(oi);
                self.string_interner.insert(rs, packed);
                packed
            }
        };
        VmValue::from_heap_idx(idx)
    }

    pub(crate) fn alloc_str_interned(&mut self, s: impl AsRef<str>) -> VmValue {
        let s_ref = s.as_ref();
        if let Some(sso) = VmValue::try_from_sso(s_ref) {
            return sso;
        }
        if let Some(&packed) = self.string_interner.get(s_ref) {
            let raw = old_idx_raw(packed);
            if self
                .objects
                .get(raw as usize)
                .map(|o| o.is_some())
                .unwrap_or(false)
            {
                return VmValue::from_heap_idx(packed);
            }
            self.string_interner.remove(s_ref);
        }
        let rs: RuntimeString = Rc::from(s_ref);
        let oi = alloc_into(
            &mut self.objects,
            &mut self.free,
            &mut self.alloc_count,
            &mut self.gc_alloc_since_collect,
            HeapObj::Str(HeapStr::shared(rs.clone())),
        );
        let packed = pack_old_idx(oi);
        self.string_interner.insert(rs, packed);
        VmValue::from_heap_idx(packed)
    }

    pub(crate) fn alloc_str_dynamic(&mut self, s: impl AsRef<str>) -> VmValue {
        let s_ref = s.as_ref();
        if let Some(sso) = VmValue::try_from_sso(s_ref) {
            return sso;
        }
        if s_ref.len() <= INLINE_STR_CAP {
            return self.alloc_str_view(HeapStr::inline(s_ref));
        }

        let rs: RuntimeString = Rc::from(s_ref);
        let idx = match self.nursery.try_alloc(HeapObj::Str(HeapStr::shared(rs))) {
            Ok(ni) => ni,
            Err(obj) => pack_old_idx(alloc_into(
                &mut self.objects,
                &mut self.free,
                &mut self.alloc_count,
                &mut self.gc_alloc_since_collect,
                obj,
            )),
        };
        VmValue::from_heap_idx(idx)
    }

    pub(crate) fn alloc_substring(&mut self, handle: &HeapStr, bs: usize, be: usize) -> VmValue {
        let sub = &handle.as_str()[bs..be];
        if let Some(sso) = VmValue::try_from_sso(sub) {
            return sso;
        }
        let flag = if handle.is_ascii_cached() {
            ascii_flag::YES
        } else {
            ascii_flag::UNKNOWN
        };
        let len = be - bs;
        if len as u64 <= SLICE_LEN_MASK as u64 {
            match handle {
                HeapStr::Shared(rc, _) => {
                    let hs = HeapStr::slice_of(Rc::clone(rc), bs, len, flag);
                    return self.alloc_str_view(hs);
                }
                HeapStr::Slice { src, off, .. } => {
                    let hs = HeapStr::slice_of(Rc::clone(src), *off as usize + bs, len, flag);
                    return self.alloc_str_view(hs);
                }
                HeapStr::Ext { .. } | HeapStr::Inline { .. } => {}
            }
        }
        self.alloc_str_dynamic(sub)
    }

    pub(crate) fn alloc_str_view(&mut self, hs: HeapStr) -> VmValue {
        let idx = match self.nursery.try_alloc(HeapObj::Str(hs)) {
            Ok(ni) => ni,
            Err(obj) => pack_old_idx(alloc_into(
                &mut self.objects,
                &mut self.free,
                &mut self.alloc_count,
                &mut self.gc_alloc_since_collect,
                obj,
            )),
        };
        VmValue::from_heap_idx(idx)
    }

    pub(crate) fn str_val(&self, nv: VmValue) -> Option<RuntimeString> {
        if nv.is_sso() {
            let mut buf = [0u8; 5];
            let s = nv.sso_as_str(&mut buf);
            return Some(Rc::from(s));
        }
        if !nv.is_heap() {
            return None;
        }
        if let Some(HeapObj::Str(s)) = self.get_by_idx(nv.as_heap_idx()) {
            return Some(s.to_shared());
        }
        None
    }

    pub(crate) fn is_string(&self, nv: VmValue) -> bool {
        if nv.is_sso() {
            return true;
        }
        if nv.is_heap() {
            return matches!(self.get_by_idx(nv.as_heap_idx()), Some(HeapObj::Str(_)));
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
            if let Some(HeapObj::Str(s)) = self.get_by_idx(nv.as_heap_idx()) {
                return Some(s.to_string());
            }
        }
        None
    }

    pub(crate) fn str_repr_borrowed<'a>(&'a self, nv: VmValue) -> std::borrow::Cow<'a, str> {
        if nv.is_heap() {
            if let Some(HeapObj::Str(s)) = self.get_by_idx(nv.as_heap_idx()) {
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
            if let Some(HeapObj::Str(s)) = self.get_by_idx(nv.as_heap_idx()) {
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
            return match self.get_by_idx(nv.as_heap_idx()) {
                Some(HeapObj::Str(s)) => s.to_string(),
                Some(HeapObj::Char(c)) => c.to_string(),
                Some(HeapObj::Array(a)) => {
                    let parts: Vec<_> = (0..a.len())
                        .map(|i| self.str_repr(a.get_vm(i).unwrap()))
                        .collect();
                    format!("[{}]", parts.join(", "))
                }
                Some(HeapObj::Object(_)) => "[object Object]".into(),
                Some(HeapObj::VmClosure(nc)) => format!(
                    "[Function {}]",
                    nc.proto.name.as_deref().unwrap_or("<anon>")
                ),
                Some(HeapObj::NativeFn(name, _)) => format!("[NativeFn: {}]", name),
                Some(HeapObj::BoundMethod(method)) => match &method.target {
                    varn_types::value::BoundMethodTarget::Native { name, .. } => {
                        format!("[Function {}]", name)
                    }
                    varn_types::value::BoundMethodTarget::Vm { .. } => "[BoundMethod]".into(),
                },
                Some(HeapObj::Class(c)) => format!("[class {}]", c.name),
                Some(HeapObj::BigInt(n)) => n.to_string(),
                Some(HeapObj::Decimal(d)) => d.to_string(),
                _ => "[object]".into(),
            };
        }
        "null".into()
    }
}
