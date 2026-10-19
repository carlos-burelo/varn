use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_types::PoolEntry;

impl ExecCtx {
    pub(crate) fn make_enum_variant(
        &mut self,
        constants: &[PoolEntry],
        def_idx: usize,
    ) -> VmResult<VmValue> {
        let Some(PoolEntry::Variant(def)) = constants.get(def_idx) else {
            return Err(RuntimeError::new(
                "MakeEnumVariant: constant is not a variant",
            ));
        };
        let data = varn_types::value::EnumVariantData::unit(def.clone());
        Ok(self.heap.alloc_enum_variant_vm(data))
    }
}
