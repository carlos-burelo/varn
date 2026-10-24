use std::sync::Arc;
use varn_core::ast::{ExprId, ExprKind, TypeNode};
use varn_core::TypeKind;
use varn_sem::bind::ClassParent;
use varn_sem::types::Type;

impl<'r> super::Binder<'r> {
    pub(crate) fn record_interfaces(&mut self, owner: &Arc<str>, nodes: &[TypeNode]) {
        let edges: Vec<ClassParent> = nodes
            .iter()
            .filter_map(|node| self.heritage_edge(node))
            .collect();
        if !edges.is_empty() {
            self.class_interfaces.insert(owner.clone(), edges);
        }
    }

    pub(crate) fn heritage_type(&mut self, expr: ExprId) -> Type {
        if matches!(
            self.ast_arena.expr(expr).kind,
            ExprKind::Identifier { .. } | ExprKind::Member { .. }
        ) {
            self.infer_expr_type_self(expr)
        } else {
            Type::Dynamic
        }
    }

    fn heritage_edge(&mut self, node: &TypeNode) -> Option<ClassParent> {
        let ty = self.resolve_type(node);
        let (name, origin, args) = match self.ty_table.get(ty.0) {
            TypeKind::Named(name, origin) => (name, origin, Vec::new()),
            TypeKind::Generic(name, args, origin) => {
                let args = self
                    .ty_table
                    .get_list(args)
                    .iter()
                    .map(|id| Type::resolved(*id))
                    .collect();
                (name, origin, args)
            }
            _ => return None,
        };
        let origin = origin
            .map(|o| self.name_text(o))
            .filter(|o| o.as_ref() != self.source_file.as_ref());
        Some(ClassParent {
            name: self.name_text(name),
            origin,
            args,
        })
    }
}
