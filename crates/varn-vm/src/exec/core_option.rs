use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::CoreSum;
use varn_types::value::EnumVariantData;
use varn_types::NativeCtx;

pub struct CoreOption {
    some: EnumVariantData,
    none: EnumVariantData,
}

impl ExecCtx {
    fn core_option_templates(&mut self) -> VmResult<&CoreOption> {
        if self.core_option.is_none() {
            let module = self.load_module(CoreSum::Option.module())?;
            let class = NativeCtx::get_field(self, module, CoreSum::Option.name())
                .ok_or_else(|| RuntimeError::new("core Option is not exported"))?;
            let template = |ctx: &Self, variant: &str| {
                ctx.enum_variant_template(class, variant)
                    .ok_or_else(|| RuntimeError::new(format!("core Option has no {variant}")))
            };
            let some = template(self, CoreSum::Option.success_variant())?;
            let none = template(self, CoreSum::Option.failure_variant())?;
            self.core_option = Some(CoreOption { some, none });
        }
        Ok(self
            .core_option
            .as_ref()
            .expect("core option templates were just cached"))
    }

    pub(crate) fn make_option(&mut self, value: Option<VmValue>) -> VmResult<VmValue> {
        let templates = self.core_option_templates()?;
        let data = match value {
            Some(v) => templates.some.instantiate(&[v]),
            None => templates.none.clone(),
        };
        Ok(self.heap.alloc_enum_variant_vm(data))
    }
}
