use super::type_inference::infer_expr_type;
use super::type_resolution::resolve_type_node;
use super::Binder;
use crate::types::Type;
use varn_core::ast::TypeNode;

impl<'r> Binder<'r> {
    pub(crate) fn resolve_type(&mut self, node: &TypeNode) -> Type {
        self.reject_forbidden_type_forms(node);
        let mut table = std::mem::take(std::sync::Arc::make_mut(&mut self.ty_table));
        let result = resolve_type_node(node, Some(self), &mut table);
        self.ty_table = std::sync::Arc::new(table);
        result
    }

    fn reject_forbidden_type_forms(&mut self, node: &TypeNode) {
        use varn_core::TypeKind as K;
        let children: Vec<&TypeNode> = match &node.kind {
            K::Generic(name, args, _) => {
                if self.interner.try_resolve(*name) == Some(varn_core::well_known::RECORD)
                    && self.reported_type_forms.insert(node.range.start.offset)
                {
                    self.emit(
                        varn_core::Diagnostic::error(
                            varn_core::ErrorCode::ForbiddenRecordGeneric,
                            "`Record<K, V>` is not a type: use `Map<K, V>` for a keyed collection or `{ [key: K]: V }` for an indexable object",
                        )
                        .with_range(node.range),
                    );
                }
                args.iter().collect()
            }
            K::Array(inner) | K::KeyOf(inner) => vec![inner.as_ref()],
            K::Union(list) | K::Intersection(list) | K::Tuple(list) | K::TemplateLiteral(list) => {
                list.iter().collect()
            }
            K::Fn((params, ret)) => params
                .iter()
                .filter_map(|p| p.constraint.as_ref())
                .chain(std::iter::once(ret.as_ref()))
                .collect(),
            K::Object(members) => members.iter().flat_map(interface_member_types).collect(),
            K::IndexedAccess { object, index } => vec![object.as_ref(), index.as_ref()],
            K::Mapped { source, value, .. } => vec![source.as_ref(), value.as_ref()],
            K::Conditional {
                check,
                true_type,
                false_type,
                ..
            } => vec![check.as_ref(), true_type.as_ref(), false_type.as_ref()],
            K::TypePredicate { target_type, .. } => vec![target_type.as_ref()],
            K::EnumVariant {
                type_args,
                payload_ty,
                ..
            } => type_args
                .iter()
                .chain(std::iter::once(payload_ty.as_ref()))
                .collect(),
            K::Infer(_) => {
                if self.reported_type_forms.insert(node.range.start.offset) {
                    self.emit(
                        varn_core::Diagnostic::error(
                            varn_core::ErrorCode::InferOutsideConditional,
                            "`infer` is only valid in the `extends` clause of a conditional type",
                        )
                        .with_range(node.range),
                    );
                }
                vec![]
            }
            K::Primitive(_)
            | K::Builtin(_)
            | K::Literal(_)
            | K::This
            | K::Named(..)
            | K::Typeof(_) => {
                vec![]
            }
        };
        for child in children {
            self.reject_forbidden_type_forms(child);
        }
    }

    pub(crate) fn infer_expr_type_self(&mut self, expr: varn_core::ast::ExprId) -> Type {
        let mut table = std::mem::take(std::sync::Arc::make_mut(&mut self.ty_table));
        let arena = self.ast_arena;
        let result = infer_expr_type(expr, arena, Some(self), &mut table);
        self.ty_table = std::sync::Arc::new(table);
        result
    }
}

fn interface_member_types(m: &varn_core::ast::InterfaceMember) -> Vec<&TypeNode> {
    use varn_core::ast::InterfaceMember as M;
    match m {
        M::Property { type_ann, .. } => vec![type_ann],
        M::Method {
            params,
            return_type,
            ..
        } => params
            .iter()
            .filter_map(|p| p.type_ann.as_ref())
            .chain(return_type.as_ref())
            .collect(),
        M::Index {
            param, return_type, ..
        } => param
            .type_ann
            .iter()
            .chain(std::iter::once(return_type))
            .collect(),
        M::Callable {
            params,
            return_type,
            ..
        } => params
            .iter()
            .filter_map(|p| p.type_ann.as_ref())
            .chain(std::iter::once(return_type))
            .collect(),
    }
}
