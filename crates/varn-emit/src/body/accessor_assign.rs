use super::context::FnEmitter;
use super::small_utils::assign_bin_op;
use std::sync::Arc;
use varn_core::ast::{operators::AssignOp, ExprId};
use varn_tir::{BackendTy, Resolution, Span, TirArg, TirExpr, TirExprKind, TirStmt};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_accessor_assign(
        &mut self,
        op: AssignOp,
        target: ExprId,
        value: ExprId,
        span: Span,
    ) -> Option<TirExpr> {
        let varn_core::ast::ExprKind::Member {
            object,
            property,
            computed: false,
            optional: false,
        } = self.ast_arena.expr(target).kind
        else {
            return None;
        };
        if matches!(
            self.ast_arena.expr(object).kind,
            varn_core::ast::ExprKind::Super
        ) {
            return None;
        }
        let combine = assign_bin_op(op).ok()?;
        let name = Self::member_name(self.ast_arena, property, self.m.interner)?;
        let class_ty = self.expr_ty(object);
        let ci = self.class_of(class_ty)?;
        if ci.field(&name).is_some() || ci.static_slot(&name).is_some() {
            return None;
        }
        let setter = varn_core::AccessorKind::Set.key(&name);
        let getter = varn_core::AccessorKind::Get.key(&name);
        let set_slot = ci.method_slot(&setter)?;
        let get_slot = match combine {
            Some(_) => Some(ci.method_slot(&getter)?),
            None => None,
        };
        let param = ci
            .method_at(set_slot)
            .and_then(|entry| self.signatures.get(entry.sig.0 as usize))
            .and_then(|sig| sig.params.first().copied())?;
        let obj = self.lower_expr(object);
        if Self::is_class_ref(&obj, &ci.name, self.m.globals) {
            return None;
        }
        let v = self.lower_expr(value);
        let v = match combine {
            None => self.widened(v, param),
            Some(_) => v,
        };
        let obj = self.pin(obj);
        let new = match (combine, get_slot) {
            (Some(bop), Some(get_slot)) => {
                let read = TirExpr {
                    kind: TirExprKind::MethodCall {
                        recv: Box::new(obj.clone()),
                        name: Arc::from(getter),
                        args: Vec::new(),
                    },
                    ty: param,
                    res: Resolution::VtableSlot(get_slot),
                    span,
                };
                let (lhs, rhs, nty) = self.coerce_binary_operands(bop, read, v, param);
                TirExpr {
                    kind: TirExprKind::Binary {
                        op: bop,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    ty: nty,
                    res: Resolution::None,
                    span,
                }
            }
            _ => v,
        };
        let new = self.guard_dynamic(new, param);
        let new = self.hoist(new);
        let call = TirExpr {
            kind: TirExprKind::MethodCall {
                recv: Box::new(obj),
                name: Arc::from(setter),
                args: vec![TirArg::Expr(new.clone())],
            },
            ty: BackendTy::Void,
            res: Resolution::VtableSlot(set_slot),
            span,
        };
        self.pending.push(TirStmt::Expr(call));
        Some(new)
    }
}
