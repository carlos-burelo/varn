use varn_core::ast::operators::{BinaryOp, UnaryOp};
use varn_core::ast::{ExprId, ExprKind};
use varn_core::TypeKind;

use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn refine(&mut self, expr: ExprId, bind: &BindResult) -> Option<Type> {
        let arena = self.ast_arena;
        match &arena.expr(expr).kind {
            ExprKind::Identifier { name } => {
                self.evolved_array_of(bind.interner.resolve(*name), bind)
            }

            ExprKind::Paren { expression } => self.refine(*expression, bind),

            ExprKind::Unary {
                op: UnaryOp::Minus | UnaryOp::Plus,
                operand,
                ..
            } => self.refine(*operand, bind),
            ExprKind::Unary { .. } => None,

            ExprKind::Member {
                object,
                property,
                computed,
                ..
            } => {
                let (object, property, computed) = (*object, *property, *computed);
                let obj = self.refine(object, bind)?;
                let TypeKind::Array(elem) = self.ty_table.get(obj.0) else {
                    return None;
                };
                if computed {
                    Some(Type::resolved(elem))
                } else if matches!(
                    &arena.expr(property).kind,
                    ExprKind::Identifier { name }
                        if bind.interner.resolve(*name) == varn_core::MemberKey::Length.as_str()
                ) {
                    Some(Type::Int)
                } else {
                    None
                }
            }

            ExprKind::Binary { op, left, right } => {
                let (op, left, right) = (*op, *left, *right);
                if !matches!(
                    op,
                    BinaryOp::Add
                        | BinaryOp::Sub
                        | BinaryOp::Mul
                        | BinaryOp::Div
                        | BinaryOp::Mod
                        | BinaryOp::Pow
                ) {
                    return None;
                }
                let l_ref = self.refine(left, bind);
                let r_ref = self.refine(right, bind);
                if l_ref.is_none() && r_ref.is_none() {
                    return None;
                }
                let l = l_ref.unwrap_or_else(|| self.checked_ty(left));
                let r = r_ref.unwrap_or_else(|| self.checked_ty(right));
                numeric_result(&l, &r, &self.ty_table)
            }

            ExprKind::IntLiteral { .. } | ExprKind::FloatLiteral { .. } | ExprKind::BigIntLiteral { .. } | ExprKind::DecimalLiteral { .. } | ExprKind::StrLiteral { .. } | ExprKind::CharLiteral { .. } | ExprKind::BoolLiteral { .. } | ExprKind::NullLiteral | ExprKind::RegexLiteral { .. } | ExprKind::Template { .. } | ExprKind::TaggedTemplate { .. } | ExprKind::Missing | ExprKind::This | ExprKind::Super | ExprKind::Array { .. } | ExprKind::Object { .. } | ExprKind::Tuple { .. } | ExprKind::Record { .. } | ExprKind::Update { .. } | ExprKind::Logical { .. } | ExprKind::Assign { .. } | ExprKind::Conditional { .. } | ExprKind::Call { .. } | ExprKind::New { .. } | ExprKind::Function { .. } | ExprKind::Arrow { .. } | ExprKind::Sequence { .. } | ExprKind::Await { .. } | ExprKind::Spawn { .. } | ExprKind::Yield { .. } | ExprKind::Spread { .. } | ExprKind::Pipeline { .. } | ExprKind::Range { .. } | ExprKind::NonNull { .. } | ExprKind::Try { .. } | ExprKind::As { .. } | ExprKind::Satisfies { .. } | ExprKind::ClassExpr { .. } | ExprKind::Match { .. } | ExprKind::Is { .. } | ExprKind::With { .. } | ExprKind::MetaAccess { .. } => None,
        }
    }

    fn checked_ty(&self, expr: ExprId) -> Type {
        self.expr_table
            .get(&expr.index())
            .map(|e| e.ty)
            .unwrap_or(Type::Dynamic)
    }

    fn evolved_array_of(&self, name: &str, bind: &BindResult) -> Option<Type> {
        if bind.evolved_array_types.is_empty() {
            return None;
        }
        let scope = bind.scopes.get(self.current_scope);
        let atom = bind.interner.get(name)?;
        let sym_id = scope.resolve(atom, &bind.scopes)?;
        let offset = bind.arena.get(sym_id).offset;
        bind.evolved_array_types.get(&offset).cloned()
    }
}

fn numeric_result(l: &Type, r: &Type, table: &crate::types::CheckerTyTable) -> Option<Type> {
    use crate::binder::type_inference::numeric_operand;
    use varn_core::{binary_operand_kind, NumericOperand};

    let combined = binary_operand_kind(numeric_operand(l, table), numeric_operand(r, table))?;
    match combined {
        NumericOperand::Int => Some(Type::Int),
        NumericOperand::Float => Some(Type::Float),

        NumericOperand::Decimal | NumericOperand::BigInt => None,
    }
}
