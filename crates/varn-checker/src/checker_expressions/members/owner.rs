use crate::checker::Checker;
use std::sync::Arc;
use varn_sem::bind::BindResult;

impl Checker<'_> {
    pub(crate) fn parent_type(
        &mut self,
        parent: &varn_sem::bind::ClassParent,
    ) -> varn_sem::types::Type {
        varn_sem::types::Type::named_with_origin(
            parent.name.clone(),
            parent.origin.clone(),
            std::sync::Arc::make_mut(&mut self.ty_table),
        )
    }

    pub(super) fn foreign_owner(
        &self,
        bind: &BindResult,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Arc<BindResult>> {
        let origin = origin.filter(|o| *o != bind.source_file.as_ref())?;
        let owner = self
            .resolver
            .module_bind(origin)
            .or_else(|| self.resolver.stdlib_bind(origin))?;
        (!std::ptr::eq(owner.as_ref(), bind) && owner.declares_type(name)).then_some(owner)
    }
}
