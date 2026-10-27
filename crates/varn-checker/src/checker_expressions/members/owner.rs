use crate::checker::Checker;
use std::sync::Arc;
use varn_sem::bind::BindResult;

impl Checker<'_> {
    pub(super) fn member_input(
        &self,
        ty: varn_sem::types::Type,
        bind: &BindResult,
    ) -> varn_sem::types::Type {
        let view = varn_sem::bind::BindView::new(bind, self.resolver);
        let view = if bind.source_file.as_ref() == self.source_file.as_ref() {
            view.at(self.current_scope)
        } else {
            view
        };
        varn_sem::resolve::expand_alias(ty, Some(&view), &self.ty_table)
    }

    pub(crate) fn parent_type(
        &mut self,
        parent: &varn_sem::bind::ClassParent,
    ) -> varn_sem::types::Type {
        let table = std::sync::Arc::make_mut(&mut self.ty_table);
        if parent.args.is_empty() {
            varn_sem::types::Type::named_with_origin(
                parent.name.clone(),
                parent.origin.clone(),
                table,
            )
        } else {
            varn_sem::types::Type::generic_with_origin(
                parent.name.clone(),
                parent.args.clone(),
                parent.origin.clone(),
                table,
            )
        }
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
