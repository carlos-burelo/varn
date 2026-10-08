use crate::checker::Checker;
use crate::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn is_subclass_or_same(
        &self,
        candidate: &str,
        target: &str,
        bind: &crate::binder::BindResult,
    ) -> bool {
        let mut visited: Vec<crate::binder::ClassParent> = Vec::new();
        let mut current = crate::binder::ClassParent {
            name: std::sync::Arc::from(candidate),
            origin: None,
        };
        loop {
            if current.name.as_ref() == target {
                return true;
            }
            if visited.contains(&current) {
                return false;
            }
            visited.push(current.clone());
            match self.class_parent_step(&current, bind) {
                Some(next) => current = next,
                None => return false,
            }
        }
    }

    fn class_parent_step(
        &self,
        class: &crate::binder::ClassParent,
        bind: &crate::binder::BindResult,
    ) -> Option<crate::binder::ClassParent> {
        if let Some(owner) = class.origin.as_deref().and_then(|o| {
            self.resolver
                .module_bind(o)
                .or_else(|| self.resolver.stdlib_bind(o))
        }) {
            let parent = owner.get_class_parent(&class.name)?;
            return Some(crate::binder::ClassParent {
                name: parent.name.clone(),
                origin: parent.origin.clone().or_else(|| class.origin.clone()),
            });
        }
        if let Some(parent) = bind.get_class_parent(&class.name) {
            return Some(parent.clone());
        }
        for spec in varn_modules::std_module_ids() {
            if let Some(rb) = self.resolver.stdlib_bind(spec) {
                if let Some(parent) = rb.class_parents.get(class.name.as_ref()) {
                    return Some(crate::binder::ClassParent {
                        name: parent.name.clone(),
                        origin: parent
                            .origin
                            .clone()
                            .or_else(|| Some(rb.source_file.clone())),
                    });
                }
            }
        }
        None
    }

    pub(crate) fn is_throwable(&self, ty: &Type, bind: &crate::binder::BindResult) -> bool {
        match self.ty_table.get(ty.0) {
            varn_core::TypeKind::Named(name, _) => {
                self.is_subclass_or_same(bind.interner.resolve(name), "Error", bind)
            }
            varn_core::TypeKind::Generic(name, _, _) => {
                self.is_subclass_or_same(bind.interner.resolve(name), "Error", bind)
            }
            varn_core::TypeKind::Union(list) => self
                .ty_table
                .get_list(list)
                .iter()
                .all(|id| self.is_throwable(&Type::resolved(*id), bind)),
            varn_core::TypeKind::This => self
                .current_class
                .as_deref()
                .is_some_and(|c| self.is_subclass_or_same(c, "Error", bind)),
            _ => ty.is_dynamic(),
        }
    }
}
