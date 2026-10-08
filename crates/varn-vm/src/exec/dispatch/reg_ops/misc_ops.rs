use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::heap::HeapObj;
use crate::value::VmValue;
use std::sync::Arc;

impl ExecCtx {
    #[inline(always)]
    pub(in crate::exec::dispatch) fn exec_get_fixed_field(
        &mut self,
        obj: VmValue,
        slot: usize,
        offset: u16,
        tag: u8,
    ) -> VmResult<VmValue> {
        if let varn_core::FieldAccess::Compact(kind) = varn_core::FieldAccess::decode(tag) {
            if let Ok(v) =
                crate::exec::props::get_fixed_field_at(obj, offset as u32, kind, &self.heap)
            {
                return Ok(v);
            }
        }

        crate::exec::props::get_fixed_field(obj, slot, &mut self.heap)
    }

    #[inline(always)]
    pub(in crate::exec::dispatch) fn exec_set_fixed_field(
        &mut self,
        obj: VmValue,
        slot: usize,
        offset: u16,
        tag: u8,
        val: VmValue,
    ) -> VmResult<()> {
        if let varn_core::FieldAccess::Compact(kind) = varn_core::FieldAccess::decode(tag) {
            if crate::exec::props::set_fixed_field_at(obj, offset as u32, kind, val, &mut self.heap)
                .is_ok()
            {
                return Ok(());
            }
        }
        crate::exec::props::set_fixed_field(obj, slot, val, &mut self.heap)
    }

    pub(in crate::exec::dispatch) fn exec_get_super_reg(
        &mut self,
        this_val: VmValue,
        name_idx: usize,
        frame_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<VmValue> {
        let name_nv = closure.constants[name_idx];
        let name = self
            .heap
            .str_val(name_nv)
            .ok_or_else(|| RuntimeError::new("GetSuper: non-string const"))?;

        let cls = self.frames[frame_idx]
            .current_class
            .clone()
            .or_else(|| crate::exec::props::get_class(this_val, &self.heap))
            .ok_or_else(|| RuntimeError::new("GetSuper: 'this' has no class"))?;
        crate::exec::class::op_get_super(cls, &name, this_val, &mut self.heap)
    }

    pub(in crate::exec::dispatch) fn exec_get_symbol(
        &mut self,
        obj: VmValue,
        sym_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<VmValue> {
        let sym_nv = closure.constants[sym_idx];
        match self.heap.symbol_of(sym_nv) {
            Some(s) => crate::exec::symbol_iterator::get_symbol_property(obj, s, &mut self.heap),
            None => Err(crate::error::RuntimeError::new(
                "GetSymbol: non-symbol constant",
            )),
        }
    }

    pub(in crate::exec::dispatch) fn exec_assert_not_null(&self, v: VmValue) -> VmResult<()> {
        if v.is_null() {
            Err(RuntimeError::new("assertion failed: value is null"))
        } else {
            Ok(())
        }
    }

    pub(in crate::exec::dispatch) fn exec_get_index(
        &mut self,
        obj: VmValue,
        key_nv: VmValue,
    ) -> VmResult<VmValue> {
        let key_nv = self.hashable_key(key_nv)?;
        crate::exec::collections::get_index(obj, key_nv, &mut self.heap)
    }

    #[inline(always)]
    pub(in crate::exec::dispatch) fn exec_array_get_index(
        &mut self,
        obj: VmValue,
        key_nv: VmValue,
    ) -> VmResult<VmValue> {
        crate::exec::collections::array_get_index(obj, key_nv, &mut self.heap)
    }

    pub(in crate::exec::dispatch) fn exec_set_index(
        &mut self,
        obj: VmValue,
        idx: VmValue,
        val: VmValue,
    ) -> VmResult<()> {
        let idx = self.hashable_key(idx)?;
        crate::exec::collections::set_index(obj, idx, val, &mut self.heap)
    }

    #[inline(always)]
    pub(in crate::exec::dispatch) fn exec_array_set_index(
        &mut self,
        obj: VmValue,
        idx: VmValue,
        val: VmValue,
    ) -> VmResult<()> {
        crate::exec::collections::array_set_index(obj, idx, val, &mut self.heap)
    }

    #[inline(always)]
    pub(in crate::exec::dispatch) fn exec_map_get_index(
        &mut self,
        obj: VmValue,
        key_nv: VmValue,
    ) -> VmResult<VmValue> {
        let key_nv = self.hashable_key(key_nv)?;
        crate::exec::collections::map_get_index(obj, key_nv, &mut self.heap)
    }

    #[inline(always)]
    pub(in crate::exec::dispatch) fn exec_map_set_index(
        &mut self,
        obj: VmValue,
        idx: VmValue,
        val: VmValue,
    ) -> VmResult<()> {
        let idx = self.hashable_key(idx)?;
        crate::exec::collections::map_set_index(obj, idx, val, &mut self.heap)
    }

    pub(crate) fn exec_object_rest(
        &mut self,
        obj: VmValue,
        skip_keys: &[Arc<str>],
    ) -> VmResult<VmValue> {
        let owned: Vec<String> = skip_keys.iter().map(|s| s.to_string()).collect();
        crate::exec::collections::object_rest(obj, &owned, &mut self.heap)
    }

    pub(in crate::exec::dispatch) fn exec_object_keys(
        &mut self,
        obj: VmValue,
    ) -> VmResult<VmValue> {
        crate::exec::collections::object_keys(obj, &mut self.heap)
    }

    pub(crate) fn exec_array_length(&mut self, arr: VmValue) -> VmResult<VmValue> {
        crate::exec::collections::array_length(arr, &self.heap)
    }

    pub(crate) fn exec_bytes_length(&mut self, v: VmValue) -> VmResult<VmValue> {
        crate::exec::collections::bytes_length(v, &self.heap)
    }

    pub(crate) fn exec_array_push(&mut self, arr: VmValue, val: VmValue) -> VmResult<()> {
        crate::exec::collections::array_push(arr, val, &mut self.heap)
    }

    pub(crate) fn exec_array_pop(&mut self, arr: VmValue) -> VmResult<VmValue> {
        crate::exec::collections::array_pop(arr, &mut self.heap)
    }

    pub(crate) fn exec_array_extend(&mut self, arr: VmValue, src: VmValue) -> VmResult<()> {
        crate::exec::collections::array_extend(arr, src, &self.heap)
    }

    pub(crate) fn exec_str_length(&mut self, v: VmValue) -> VmResult<VmValue> {
        if v.is_sso() {
            return Ok(VmValue::from_int(v.sso_len() as i64));
        }
        if v.is_heap() {
            if let Some(HeapObj::Str(s)) = self.heap.get(v.as_heap()) {
                let len = if s.is_ascii() {
                    s.len()
                } else {
                    s.chars().count()
                };
                return Ok(VmValue::from_int(len as i64));
            }
        }
        Ok(VmValue::from_int(0))
    }

    pub(crate) fn exec_str_slice(&mut self, s: VmValue, idx: VmValue) -> VmResult<VmValue> {
        let start = idx.as_i32().max(0) as usize;
        if s.is_heap() {
            if let Some(HeapObj::Str(st)) = self.heap.get(s.as_heap()) {
                if st.is_ascii() {
                    let len = st.byte_len();
                    let off = start.min(len);
                    if off == 0 {
                        return Ok(s);
                    }
                    return Ok(self.heap.alloc_str_view_of(s, off, len - off));
                }
                let text = st.as_str();
                let total = text.len();
                let boff = text
                    .char_indices()
                    .nth(start)
                    .map(|(b, _)| b)
                    .unwrap_or(total);
                if boff == 0 {
                    return Ok(s);
                }
                return Ok(self.heap.alloc_str_view_of(s, boff, total - boff));
            }
        } else if s.is_sso() {
            let mut buf = [0u8; 5];
            let st = s.sso_as_str(&mut buf);
            let start = idx.as_i32().max(0) as usize;
            let sliced: String = st.chars().skip(start).collect();
            return Ok(self.heap.alloc_str(sliced));
        }
        Ok(self.heap.alloc_str(""))
    }
}
