use crate::heap::{ascii_flag, HeapInner, HeapObj, HeapStr, INLINE_STR_CAP};
use crate::value::VmValue;

impl HeapInner {
    pub(crate) fn alloc_str_concat_inline(&mut self, a: VmValue, b: VmValue) -> Option<VmValue> {
        use crate::strbuf::{itoa, INT_MAX_DIGITS};

        let mut a_sso_buf = [0u8; 5];
        let a_bytes: &[u8] = if a.is_sso() {
            a.sso_as_str(&mut a_sso_buf).as_bytes()
        } else if a.is_heap() {
            match self.get(a.as_heap()) {
                Some(HeapObj::Str(HeapStr::Ext { .. })) => return None,
                Some(HeapObj::Str(hs)) => hs.as_str().as_bytes(),
                Some(
                    HeapObj::Array(_)
                    | HeapObj::Tuple(_)
                    | HeapObj::Object(_)
                    | HeapObj::Record(_)
                    | HeapObj::Buffer(_)
                    | HeapObj::Module(_)
                    | HeapObj::FrozenModule(_)
                    | HeapObj::VmClosure(_)
                    | HeapObj::Class(_)
                    | HeapObj::NativeFn(..)
                    | HeapObj::BoundMethod(_)
                    | HeapObj::Map(_)
                    | HeapObj::Set(_)
                    | HeapObj::Task(_)
                    | HeapObj::TaskHandle(_)
                    | HeapObj::Range(_)
                    | HeapObj::Symbol(_)
                    | HeapObj::EnumVariant(_)
                    | HeapObj::BigInt(_)
                    | HeapObj::Decimal(_)
                    | HeapObj::Char(_)
                    | HeapObj::Generator(_)
                    | HeapObj::Spread(_),
                )
                | None => return None,
            }
        } else {
            return None;
        };

        if a_bytes.len() > INLINE_STR_CAP {
            return None;
        }

        let mut b_sso_buf = [0u8; 5];
        let mut digits = [0u8; INT_MAX_DIGITS];
        let b_bytes: &[u8] = if b.is_int() {
            itoa(b.as_int(), &mut digits).as_bytes()
        } else if b.is_sso() {
            b.sso_as_str(&mut b_sso_buf).as_bytes()
        } else if b.is_heap() {
            match self.get(b.as_heap()) {
                Some(HeapObj::Str(HeapStr::Ext { .. })) => return None,
                Some(HeapObj::Str(hs)) => hs.as_str().as_bytes(),
                Some(
                    HeapObj::Array(_)
                    | HeapObj::Tuple(_)
                    | HeapObj::Object(_)
                    | HeapObj::Record(_)
                    | HeapObj::Buffer(_)
                    | HeapObj::Module(_)
                    | HeapObj::FrozenModule(_)
                    | HeapObj::VmClosure(_)
                    | HeapObj::Class(_)
                    | HeapObj::NativeFn(..)
                    | HeapObj::BoundMethod(_)
                    | HeapObj::Map(_)
                    | HeapObj::Set(_)
                    | HeapObj::Task(_)
                    | HeapObj::TaskHandle(_)
                    | HeapObj::Range(_)
                    | HeapObj::Symbol(_)
                    | HeapObj::EnumVariant(_)
                    | HeapObj::BigInt(_)
                    | HeapObj::Decimal(_)
                    | HeapObj::Char(_)
                    | HeapObj::Generator(_)
                    | HeapObj::Spread(_),
                )
                | None => return None,
            }
        } else {
            return None;
        };

        let total = a_bytes.len() + b_bytes.len();
        if total > INLINE_STR_CAP {
            return None;
        }

        let mut bytes = [0u8; INLINE_STR_CAP];
        bytes[..a_bytes.len()].copy_from_slice(a_bytes);
        bytes[a_bytes.len()..total].copy_from_slice(b_bytes);

        if total <= 5 {
            if let Ok(s) = std::str::from_utf8(&bytes[..total]) {
                if let Some(sso) = VmValue::try_from_sso(s) {
                    return Some(sso);
                }
            }
        }

        let ascii = if a_bytes.is_ascii() && b_bytes.is_ascii() {
            ascii_flag::YES
        } else {
            ascii_flag::NO
        };

        Some(self.alloc_str_view(HeapStr::Inline {
            len: total as u8,
            ascii: std::cell::Cell::new(ascii),
            bytes,
        }))
    }
}
