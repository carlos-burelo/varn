//! Which module's tables answer a member lookup: the module declaring the
//! type, found through the type's origin, never a same-named local
//! declaration.

use crate::binder::BindResult;
use crate::checker::Checker;
use std::sync::Arc;

impl Checker<'_> {
    /// The bind of the module declaring type `name`, when its `origin` is
    /// another module that does declare it. `None` for a local type, for an
    /// origin that only names the type (a core module refers to other core
    /// types without declaring them), and for `bind` itself reached again
    /// through the resolver's cache, so a redirected lookup never redirects
    /// twice.
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
