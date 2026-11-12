use crate::checker::Checker;
use vn_core::ast::TypeNode;
use vn_core::source::SourceRange;
use vn_core::{Diagnostic, ErrorCode, TypeKind};
use vn_sem::bind::{BindResult, BindView};
use vn_sem::types::TypeContext;

impl<'r> Checker<'r> {
    pub(super) fn check_is_target(
        &mut self,
        type_ann: &TypeNode,
        range: SourceRange,
        bind: &BindResult,
    ) {
        if !self.is_shape_only(type_ann, bind) {
            return;
        }
        self.emit(
            Diagnostic::error(
                ErrorCode::IsNeedsRuntimeType,
                "`is` tests a class, an enum, a builtin type or a union of them; an interface or a record has no identity at run time: validate its shape with `as`",
            )
            .with_range(range),
        );
    }

    fn is_shape_only(&mut self, type_ann: &TypeNode, bind: &BindResult) -> bool {
        let ty = self.resolve_type_node_cached(type_ann, bind);
        match self.ty_table.get(ty.0) {
            TypeKind::Record(_) | TypeKind::Interface(_) => true,
            TypeKind::Named(name, origin) | TypeKind::Generic(name, _, origin) => {
                let name = self.resolve_bind_atom(bind, name);
                let origin = origin.map(|o| self.resolve_bind_atom(bind, o));
                let view = BindView::new(bind, self.resolver);
                if view.get_class_members(&name, origin.as_deref()).is_some() {
                    return false;
                }
                if view
                    .get_interface_members(&name, origin.as_deref())
                    .is_some()
                {
                    return true;
                }
                view.resolve_type_alias(&name, origin.as_deref())
                    .is_some_and(|expanded| {
                        matches!(
                            self.ty_table.get(expanded.0),
                            TypeKind::Record(_) | TypeKind::Interface(_)
                        )
                    })
            }
            TypeKind::Union(members) => {
                let members = self.ty_table.get_list(members).to_vec();
                members.into_iter().any(|member| {
                    matches!(
                        self.ty_table.get(member),
                        TypeKind::Record(_) | TypeKind::Interface(_)
                    )
                })
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => false,
        }
    }
}
