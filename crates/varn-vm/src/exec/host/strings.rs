use super::*;

impl ExecCtx {
    pub(super) fn host_str_sub(
        &mut self,
        base: &varn_types::marshal::VnStr,
        start: usize,
        len: usize,
    ) -> VmValue {
        use crate::heap::HeapStr;
        let s = base.as_str();
        let start = start.min(s.len());
        let len = len.min(s.len() - start);
        if len == 0 {
            return VmValue::try_from_sso("").unwrap_or_else(VmValue::null);
        }
        if len <= varn_types::vm_value::SSO_MAX_LEN {
            return self.heap.alloc_str_dynamic(&s[start..start + len]);
        }
        match base {
            varn_types::marshal::VnStr::Shared(arc, ascii) => self.heap.alloc_str_view(
                HeapStr::view(std::sync::Arc::clone(arc), start, len, *ascii),
            ),
            varn_types::marshal::VnStr::Sso { .. } => {
                self.heap.alloc_str_dynamic(&s[start..start + len])
            }
        }
    }
}
