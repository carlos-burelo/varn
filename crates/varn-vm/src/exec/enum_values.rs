use crate::error::{RuntimeError, VmResult};
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_core::CoreSum;
use varn_types::NativeCtx;

type TemplateKey = (usize, usize, usize);

fn template_key(
    module: &'static str,
    enum_name: &'static str,
    variant: &'static str,
) -> TemplateKey {
    (
        module.as_ptr() as usize,
        enum_name.as_ptr() as usize,
        variant.as_ptr() as usize,
    )
}

impl ExecCtx {
    pub(crate) fn enum_variant(
        &mut self,
        module: &'static str,
        enum_name: &'static str,
        variant: &'static str,
        payload: &[VmValue],
    ) -> VmResult<VmValue> {
        let key = template_key(module, enum_name, variant);
        if !self.enum_templates.contains_key(&key) {
            let owner = self.load_host_module(module)?;
            let class = NativeCtx::get_field(self, owner, enum_name).ok_or_else(|| {
                RuntimeError::new(format!("{module} does not export {enum_name}"))
            })?;
            let template = self.enum_variant_template(class, variant).ok_or_else(|| {
                RuntimeError::new(format!("{enum_name} has no variant {variant}"))
            })?;
            self.enum_templates.insert(key, template);
        }
        let template = &self.enum_templates[&key];
        let data = if payload.is_empty() {
            template.clone()
        } else {
            template.instantiate(payload)
        };
        Ok(self.heap.alloc_enum_variant_vm(data))
    }

    pub(crate) fn make_option(&mut self, value: Option<VmValue>) -> VmResult<VmValue> {
        let sum = CoreSum::Option;
        match value {
            Some(v) => self.enum_variant(sum.module(), sum.name(), sum.success_variant(), &[v]),
            None => self.enum_variant(sum.module(), sum.name(), sum.failure_variant(), &[]),
        }
    }
}
