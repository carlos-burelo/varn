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
            K::Union(list) | K::Intersection(list) | K::Tuple(list) => list.iter().collect(),
            K::Fn((params, ret)) => params
                .iter()
                .filter_map(|p| p.constraint.as_ref())
                .chain(std::iter::once(ret.as_ref()))
                .collect(),
            _ => vec![],
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
