//! The intrinsic methods a method call answers before any lookup: `push` /
//! `pop` on an array and `startsWith` / `endsWith` / `indexOf` on a string —
//! no method table, no bound method, no indirect call.

use crate::exec::ctx::ExecCtx;
use crate::exec::method_args::MethodArgs;
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use varn_core::MemberKey;
use varn_types::vm_value::SSO_MAX_LEN;

/// The string methods answered directly; each takes one string argument.
enum StrOp {
    StartsWith,
    EndsWith,
    IndexOf,
}

/// The text of `v` when it is a string: inline (into `buf`) or on the heap.
fn str_of<'a>(v: VmValue, heap: &'a Heap, buf: &'a mut [u8; SSO_MAX_LEN]) -> Option<&'a str> {
    if v.is_sso() {
        return Some(v.sso_as_str(buf));
    }
    if v.is_heap() {
        if let Some(HeapObj::Str(hs)) = heap.get(v.as_heap_idx()) {
            return Some(hs.as_str());
        }
    }
    None
}

impl ExecCtx {
    /// The value of `this_val.name(args)` when it is an intrinsic the call
    /// answers directly; `None` sends the call on to the general resolution.
    pub(super) fn intrinsic_method(
        &mut self,
        this_val: VmValue,
        name: &str,
        args: MethodArgs<'_>,
    ) -> Option<VmValue> {
        let arg_count = args.len();
        if this_val.is_heap() {
            if let Some(HeapObj::Array(arr)) = self.heap.get(this_val.as_heap_idx()) {
                if name == MemberKey::Push.as_str() && arg_count == 1 {
                    let val = args.get(&self.stack, 0);
                    arr.push_vm(val);
                    self.heap.write_barrier(this_val.as_heap_idx(), val);
                    self.record_ic_hit_callmethod();
                    self.record_call_native(|_, _| Ok(VmValue::null()), Some("push"));
                    return Some(VmValue::null());
                }
                if name == MemberKey::Pop.as_str() && arg_count == 0 {
                    let val = arr.pop_vm().unwrap_or(VmValue::null());
                    self.record_ic_hit_callmethod();
                    self.record_call_native(|_, _| Ok(VmValue::null()), Some("pop"));
                    return Some(val);
                }
                return None;
            }
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
            StrOp::IndexOf => VmValue::from_int(char_index_of(s, p)),
        };
        self.record_ic_hit_callmethod();
        Some(result)
    }
}

/// `s.indexOf(p)` in characters: `0` for an empty `p`, `-1` when absent.
fn char_index_of(s: &str, p: &str) -> i64 {
    if p.is_empty() {
        return 0;
    }
    match s.find(p) {
        Some(byte_idx) if s.is_ascii() => byte_idx as i64,
        Some(byte_idx) => s[..byte_idx].chars().count() as i64,
        None => -1,
    }
}
