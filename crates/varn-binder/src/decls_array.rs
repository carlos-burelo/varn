use super::Binder;
use varn_core::ast::{Arg, ExprId, ExprKind};

impl<'r> Binder<'r> {
    pub(super) fn bind_array_push_call(&mut self, callee: ExprId, args: &[Arg]) -> bool {
        let arena = self.ast_arena;
        let ExprKind::Member {
            object,
            property,
            computed,
            ..
        } = &arena.expr(callee).kind
        else {
            return false;
        };
        let (object, property, computed) = (*object, *property, *computed);
        let ExprKind::Identifier { name } = &arena.expr(object).kind else {
            return false;
        };
        let name = *name;
        if !self.array_candidate_active(name) {
            return false;
        }
        if computed {
            self.escape_array_candidate(name);
            self.bind_expr(property);
            self.bind_args(args);
            return true;
        }
        let ExprKind::Identifier { name: prop_name } = &arena.expr(property).kind else {
            return false;
        };
        let prop_name = *prop_name;
        if self.interner.resolve(prop_name) != varn_core::MemberKey::Push.as_str() {
            return false;
        }
        match args {
            [Arg::Positional(value)] => {
                let value = *value;
                let value_ty = self.infer_expr_type_self(value);
                self.record_array_write(name, &value_ty);
                self.bind_expr(value);
            }
            _ => {
                self.escape_array_candidate(name);
                self.bind_args(args);
            }
        }
        true
    }

    pub(super) fn bind_array_index_write(
        &mut self,
        op: varn_core::ast::operators::AssignOp,
        target: ExprId,
        value: ExprId,
    ) -> bool {
        use varn_core::ast::operators::AssignOp;

        let arena = self.ast_arena;
        let ExprKind::Member {
            object,
            property,
            computed,
            ..
        } = &arena.expr(target).kind
        else {
            return false;
        };
        let (object, property, computed) = (*object, *property, *computed);
        let ExprKind::Identifier { name } = &arena.expr(object).kind else {
            return false;
        };
        let name = *name;
        if !self.array_candidate_active(name) {
            return false;
        }
        if !computed {
            self.escape_array_candidate(name);
            self.bind_expr(value);
            return true;
        }
        if op != AssignOp::Assign {
            self.escape_array_candidate(name);
            self.bind_expr(property);
            self.bind_expr(value);
            return true;
        }
        let value_ty = self.infer_expr_type_self(value);
        self.record_array_write(name, &value_ty);
        self.bind_expr(property);
        self.bind_expr(value);
        true
    }

    pub(super) fn bind_array_whitelisted_member(
        &mut self,
        object: ExprId,
        property: ExprId,
        computed: bool,
    ) -> bool {
        let arena = self.ast_arena;
        let ExprKind::Identifier { name } = &arena.expr(object).kind else {
            return false;
        };
        let name = *name;
        if !self.array_candidate_active(name) {
            return false;
        }
        if computed {
            if matches!(&arena.expr(property).kind, ExprKind::StrLiteral { .. }) {
                self.escape_array_candidate(name);
            }
            self.bind_expr(property);
        } else if !matches!(
            &arena.expr(property).kind,
            ExprKind::Identifier { name: p } if self.interner.resolve(*p) == varn_core::MemberKey::Length.as_str()
        ) {
            self.escape_array_candidate(name);
        }
        true
    }
}
