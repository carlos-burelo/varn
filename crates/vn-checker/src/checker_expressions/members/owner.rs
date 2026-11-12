use crate::checker::Checker;
use std::sync::Arc;
use vn_sem::bind::BindResult;

impl Checker<'_> {
    pub(super) fn member_input(
        &self,
        ty: vn_sem::types::Type,
        bind: &BindResult,
    ) -> vn_sem::types::Type {
        let view = vn_sem::bind::BindView::new(bind, self.resolver);
        let view = if bind.source_file.as_ref() == self.source_file.as_ref() {
            view.at(self.current_scope)
        } else {
            view
        };
        vn_sem::resolve::expand_alias(ty, Some(&view), &self.ty_table)
    }

    pub(crate) fn parent_type(
        &mut self,
        parent: &vn_sem::bind::ClassParent,
    ) -> vn_sem::types::Type {
        let table = std::sync::Arc::make_mut(&mut self.ty_table);
        if parent.args.is_empty() {
            vn_sem::types::Type::named_with_origin(
                parent.name.clone(),
                parent.origin.clone(),
                table,
            )
        } else {
            vn_sem::types::Type::generic_with_origin(
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
