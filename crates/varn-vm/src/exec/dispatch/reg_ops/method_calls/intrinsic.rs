use crate::exec::ctx::ExecCtx;
use crate::exec::method_args::MethodArgs;
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use varn_core::MemberKey;
use varn_types::vm_value::SSO_MAX_LEN;

enum StrOp {
    StartsWith,
    EndsWith,
    IndexOf,
}

fn str_of<'a>(v: VmValue, heap: &'a Heap, buf: &'a mut [u8; SSO_MAX_LEN]) -> Option<&'a str> {
    if v.is_sso() {
        return Some(v.sso_as_str(buf));
    }
    if v.is_heap()
        && let Some(HeapObj::Str(hs)) = heap.get(v.as_heap())
    {
        return Some(hs.as_str());
    }
    None
}

impl ExecCtx {
    pub(super) fn intrinsic_method(
        &mut self,
        this_val: VmValue,
        name: &str,
        args: MethodArgs<'_>,
    ) -> Option<VmValue> {
        let arg_count = args.len();
        if this_val.is_heap()
            && let Some(HeapObj::Array(arr)) = self.heap.get(this_val.as_heap())
        {
            if name == MemberKey::Push.as_str() && arg_count == 1 {
                let val = args.get(&self.stack, 0);
                arr.push_vm(val);
                self.heap.write_barrier(this_val.as_heap(), val);
                self.record_ic_hit_callmethod();
                self.record_call_native(
                    |_, _| Ok(VmValue::null()),
                    Some(varn_core::MemberKey::Push.as_str()),
                );
                return Some(VmValue::null());
            }
            if name == MemberKey::Pop.as_str() && arg_count == 0 {
                let val = arr.pop_vm().unwrap_or(VmValue::null());
                self.record_ic_hit_callmethod();
                self.record_call_native(
                    |_, _| Ok(VmValue::null()),
                    Some(varn_core::MemberKey::Pop.as_str()),
                );
                return Some(val);
            }
            return None;
        }
        if arg_count != 1 {
            return None;
        }
        let op = if name == MemberKey::StartsWith.as_str() {
            StrOp::StartsWith
        } else if name == MemberKey::EndsWith.as_str() {
            StrOp::EndsWith
        } else if name == MemberKey::IndexOf.as_str() {
            StrOp::IndexOf
        } else {
            return None;
        };
        let arg = args.get(&self.stack, 0);
        let (mut buf_s, mut buf_p) = ([0u8; SSO_MAX_LEN], [0u8; SSO_MAX_LEN]);
        let s = str_of(this_val, &self.heap, &mut buf_s)?;
        let p = str_of(arg, &self.heap, &mut buf_p)?;
        let result = match op {
            StrOp::StartsWith => VmValue::from_bool(s.starts_with(p)),
            StrOp::EndsWith => VmValue::from_bool(s.ends_with(p)),
            StrOp::IndexOf => varn_types::str_util::char_index_of(s, p)
                .map_or_else(VmValue::null, |at| VmValue::from_int(at as i64)),
        };
        self.record_ic_hit_callmethod();
        Some(result)
    }
}
