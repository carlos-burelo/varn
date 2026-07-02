use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::frame::VmClosure;
use crate::heap::HeapObj;
use crate::value::VmValue;
use std::rc::Rc;

impl ExecCtx {
    #[inline(always)]
    pub(in crate::exec::dispatch) fn exec_get_fixed_field(
        &mut self,
        obj: VmValue,
        slot: usize,
    ) -> VmResult<VmValue> {
        crate::exec::props::get_fixed_field(obj, slot, &mut self.heap)
    }

    #[inline(always)]
    pub(in crate::exec::dispatch) fn exec_set_fixed_field(
        &mut self,
        obj: VmValue,
        slot: usize,
        val: VmValue,
    ) -> VmResult<()> {
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
        let class_nv = self.heap.intern(varn_types::Value::Class(cls));
        crate::exec::class::op_get_super(class_nv, &name, this_val, &mut self.heap)
    }

    pub(in crate::exec::dispatch) fn exec_get_symbol(
        &mut self,
        obj: VmValue,
        sym_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<VmValue> {
        let sym_nv = closure.constants[sym_idx];
        let sym_val = self.heap.extract(sym_nv);
        match sym_val {
            varn_types::Value::Symbol(s) => {
                crate::exec::advanced::get_symbol_property(obj, s, &mut self.heap)
            }
            _ => Err(crate::error::RuntimeError::new(
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

    pub(in crate::exec::dispatch) fn exec_declare_field(
        &mut self,
        obj: VmValue,
        name_idx: usize,
        _frame_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<()> {
        let name_nv = closure.constants[name_idx];
        let name = self
            .heap
            .str_val(name_nv)
            .ok_or_else(|| RuntimeError::new("DeclareField: non-string const"))?;
        crate::exec::class::op_declare_field(obj, &name, &mut self.heap)
    }

    pub(in crate::exec::dispatch) fn exec_get_index_nv(
        &mut self,
        obj: VmValue,
        key_nv: VmValue,
    ) -> VmResult<VmValue> {
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

    pub(in crate::exec::dispatch) fn exec_build_object_with_shape(
        &mut self,
        base: usize,
        start_reg: usize,
        shape_idx: usize,
        closure: &VmClosure,
    ) -> VmResult<VmValue> {
        let shape = closure.proto.resolved_shape(shape_idx).ok_or_else(|| {
            RuntimeError::new("BuildObjectWithShape: invalid shape const")
        })?;
        let count = shape.property_names.len();
        let required = base + start_reg + count;
        if self.stack.len() < required {
            self.stack.resize(required, VmValue::null());
        }
        Ok(crate::exec::collections::build_object_with_shape(
            &self.stack,
            base + start_reg,
            shape,
            &mut self.heap,
        ))
    }

    pub(crate) fn exec_object_rest(
        &mut self,
        obj: VmValue,
        skip_keys: &[Rc<str>],
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
        if v.is_heap() {
            if let Some(HeapObj::Str(s)) = self.heap.get(v.as_heap_idx()) {
                return Ok(VmValue::from_i32(s.chars().count() as i32));
            }
        }
        Ok(VmValue::from_i32(0))
    }

    pub(crate) fn exec_str_slice(&mut self, s: VmValue, idx: VmValue) -> VmResult<VmValue> {
        if s.is_heap() {
            if let Some(HeapObj::Str(st)) = self.heap.get(s.as_heap_idx()) {
                let start = idx.as_i32().max(0) as usize;
                let sliced: String = st.chars().skip(start).collect();
                return Ok(self.heap.alloc_str(sliced));
            }
        }
        Ok(self.heap.alloc_str(""))
    }
}
