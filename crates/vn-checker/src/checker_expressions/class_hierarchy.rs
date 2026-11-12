use crate::checker::Checker;
use vn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn is_subclass_or_same(
        &self,
        candidate: &str,
        target: &str,
        bind: &vn_sem::bind::BindResult,
    ) -> bool {
        let start = vn_sem::bind::ClassParent {
            name: std::sync::Arc::from(candidate),
            origin: None,
            args: Vec::new(),
        };
        self.ancestry_any(start, bind, |class| class.name.as_ref() == target)
    }

    fn ancestry_any(
        &self,
        start: vn_sem::bind::ClassParent,
        bind: &vn_sem::bind::BindResult,
        found: impl Fn(&vn_sem::bind::ClassParent) -> bool,
    ) -> bool {
        let mut visited: Vec<vn_sem::bind::ClassParent> = Vec::new();
        let mut current = start;
        loop {
            if found(&current) {
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
        class: &vn_sem::bind::ClassParent,
        bind: &vn_sem::bind::BindResult,
    ) -> Option<vn_sem::bind::ClassParent> {
        if let Some(owner) = class.origin.as_deref().and_then(|o| {
            self.resolver
                .module_bind(o)
                .or_else(|| self.resolver.stdlib_bind(o))
        }) {
            let parent = owner.get_class_parent(&class.name)?;
            return Some(vn_sem::bind::ClassParent {
                name: parent.name.clone(),
                origin: parent.origin.clone().or_else(|| class.origin.clone()),
                args: Vec::new(),
            });
        }
        if let Some(parent) = bind.get_class_parent(&class.name) {
            return Some(parent.clone());
        }
        if let Some(rb) = self
            .resolver
            .find_stdlib_bind(&mut |rb| rb.class_parents.contains_key(class.name.as_ref()))
            && let Some(parent) = rb.class_parents.get(class.name.as_ref())
        {
            return Some(vn_sem::bind::ClassParent {
                name: parent.name.clone(),
                origin: parent
                    .origin
                    .clone()
                    .or_else(|| Some(rb.source_file.clone())),
                args: Vec::new(),
            });
        }
        None
    }

    pub(crate) fn is_throwable(&self, ty: &Type, bind: &vn_sem::bind::BindResult) -> bool {
        let is_core_error = |class: &vn_sem::bind::ClassParent| {
            vn_core::RuntimeErrorKind::identify(
                vn_core::Atom::of(&class.name),
                class.origin.as_deref().map(vn_core::Atom::of),
            ) == Some(vn_core::RuntimeErrorKind::Error)
        };
        let start = |name: vn_core::Atom, origin: Option<vn_core::Atom>| {
            let origin = origin
                .map(|o| self.resolve_bind_atom(bind, o))
                .filter(|o| o.as_ref() != bind.source_file.as_ref());
            vn_sem::bind::ClassParent {
                name: self.resolve_bind_atom(bind, name),
                origin,
                args: Vec::new(),
            }
        };
        match self.ty_table.get(ty.0) {
            vn_core::TypeKind::Named(name, origin)
            | vn_core::TypeKind::Generic(name, _, origin) => {
                self.ancestry_any(start(name, origin), bind, is_core_error)
            }
            vn_core::TypeKind::Union(list) => self
                .ty_table
                .get_list(list)
                .iter()
                .all(|id| self.is_throwable(&Type::resolved(*id), bind)),
            vn_core::TypeKind::This => self.current_class.as_deref().is_some_and(|c| {
                let local = vn_sem::bind::ClassParent {
                    name: std::sync::Arc::from(c),
                    origin: None,
                    args: Vec::new(),
                };
                self.ancestry_any(local, bind, is_core_error)
            }),
            vn_core::TypeKind::Primitive(_)
            | vn_core::TypeKind::Literal(_)
            | vn_core::TypeKind::Array(_)
            | vn_core::TypeKind::Intersection(_)
            | vn_core::TypeKind::Tuple(_)
            | vn_core::TypeKind::TemplateLiteral(_)
            | vn_core::TypeKind::Fn(_)
            | vn_core::TypeKind::Record(_)
            | vn_core::TypeKind::Interface(_)
            | vn_core::TypeKind::Typeof(_)
            | vn_core::TypeKind::KeyOf(_)
            | vn_core::TypeKind::IndexedAccess { .. }
            | vn_core::TypeKind::Mapped { .. }
            | vn_core::TypeKind::Conditional { .. }
            | vn_core::TypeKind::Infer(_)
            | vn_core::TypeKind::EnumVariant { .. }
            | vn_core::TypeKind::TypePredicate { .. } => ty.is_unresolved(),
        }
    }
}
