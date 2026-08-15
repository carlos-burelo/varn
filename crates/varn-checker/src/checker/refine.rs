//! Facts proved about a value that the DIAGNOSTIC type deliberately does not
//! carry.
//!
//! A type checker and a prover want opposite failure modes. The checker must
//! never reject a valid program, so when it cannot see a type it says
//! `Dynamic` and moves on. A prover feeding codegen wants the strongest sound
//! fact available, and if it is wrong in the weak direction the cost is a
//! missed optimisation. Forcing both jobs through one type makes one of them
//! wrong: either valid programs stop compiling, or the backend loses
//! information it could have had.
//!
//! So the checker records two lanes per expression — `TypeEntry::ty`, which
//! diagnostics are reported against, and `TypeEntry::refined`, which only
//! codegen reads. This module produces the second.
//!
//! # What is proved today
//!
//! One prover: the evolving empty array (`binder::array_evolve`). The binder
//! watches an unannotated `let x = []`, unifies the element type across every
//! `x.push(e)` / `x[i] = e` in the declaring scope, and escapes the candidate
//! on any use it does not recognise — a blanket escape at every closure
//! boundary, because a false negative there is a soundness bug and not a
//! missed optimisation: the CLIF backend trusts this proof and skips guards.
//! The verdict lands in `BindResult::evolved_array_types`, keyed by the
//! declarator's source offset.
//!
//! # Where it used to live
//!
//! This propagation ran at ANNOTATION time, by calling the binder's syntactic
//! `infer_expr_type` over an overlay environment that shadowed the evolved
//! name. That made the type the backend compiles against come out of a
//! different inference engine than the one that checked the program, and it
//! re-walked those subtrees a second time. Same rules, computed once, in the
//! checker.
//!
//! # Adding a prover
//!
//! Return `Some(t)` only when `t` is a NARROWING of what the checker already
//! decided — telling codegen something *different* rather than something
//! *more* is a miscompile, not an optimisation.

use varn_core::ast::operators::{BinaryOp, UnaryOp};
use varn_core::ast::{Expr, ExprKind};
use varn_core::TypeKind;

use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::Type;

impl Checker {
    /// The strongest proved type for `expr`, when stronger than what the
    /// checker recorded. `None` means "nothing beyond the checked type".
    ///
    /// Structural, and deliberately narrow: it mirrors exactly the shapes
    /// `array_evolve` proves things about. Anything else answers `None`
    /// rather than guessing, because a guess here is trusted by a backend
    /// that removes guards on the strength of it.
    pub(crate) fn refine(&mut self, expr: &Expr, bind: &BindResult) -> Option<Type> {
        match &expr.kind {
            ExprKind::Identifier { name } => self.evolved_array_of(name, bind),

            ExprKind::Paren { expression } => self.refine(expression, bind),

            // Sign and negation preserve the numeric type; `!` does not
            // produce a refinement worth carrying (its type is already known).
            ExprKind::Unary { op, operand, .. } => match op {
                UnaryOp::Minus | UnaryOp::Plus => self.refine(operand, bind),
                _ => None,
            },

            ExprKind::Member {
                object,
                property,
                computed,
                ..
            } => {
                let obj = self.refine(object, bind)?;
                let TypeKind::Array(elem) = &obj.0 else {
                    return None;
                };
                if *computed {
                    // `x[i]` on a proved `Array<T>` is a `T`.
                    Some((**elem).clone())
                } else if matches!(&property.kind, ExprKind::Identifier { name } if name.as_ref() == "length")
                {
                    Some(Type::Int)
                } else {
                    None
                }
            }

            // Arithmetic over refined operands. The operand kinds come from
            // the refinement where there is one and from the checked type
            // otherwise, so `sum + x[i]` refines even though only one side is
            // governed. The result kind is the language's own rule, not a
            // local guess — `varn_core::binary_result_kind` is the same
            // function the checker uses.
            ExprKind::Binary { op, left, right } => {
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
                numeric_result(op, &l, &r)
            }

            _ => None,
        }
    }

    /// The checked type already recorded for `expr`, or `Dynamic` when the
    /// checker has not reached it yet (operands are checked before their
    /// parent, so in practice it is there).
    fn checked_ty(&self, expr: &Expr) -> Type {
        self.expr_table
            .get(&expr.id)
            .map(|e| e.ty.clone())
            .unwrap_or(Type::Dynamic)
    }

    /// `Array<T>` when `name` resolves to a local the binder proved an element
    /// type for. Keyed by the declarator's offset, which is what
    /// `finalize_array_watch` records.
    fn evolved_array_of(&self, name: &str, bind: &BindResult) -> Option<Type> {
        if bind.evolved_array_types.is_empty() {
            return None;
        }
        let scope = bind.scopes.get(self.current_scope);
        let sym_id = scope.resolve(name, &bind.scopes)?;
        let offset = bind.arena.get(sym_id).offset;
        bind.evolved_array_types.get(&offset).cloned()
    }
}

/// Result type of an arithmetic operator over two operand types, following the
/// language's numeric rules (`int / int` is a float; `int` op `int` is an
/// `int`). Anything not both-numeric yields no refinement.
fn numeric_result(op: &BinaryOp, l: &Type, r: &Type) -> Option<Type> {
    use varn_core::{binary_operand_kind, binary_result_kind, NumericOperand, TypeTag};

    let operand = |t: &Type| match &t.0 {
        TypeKind::Intrinsic(TypeTag::Int) => Some(NumericOperand::Int),
        TypeKind::Intrinsic(TypeTag::Float) => Some(NumericOperand::Float),
        TypeKind::Intrinsic(TypeTag::Decimal) => Some(NumericOperand::Decimal),
        _ => None,
    };
    let combined = binary_operand_kind(operand(l), operand(r))?;
    match binary_result_kind(*op, combined) {
        NumericOperand::Int => Some(Type::Int),
        NumericOperand::Float => Some(Type::Float),
        // No refinement for decimal: the annotation pass records no numeric
        // kind for it either, so claiming one here would be the refinement
        // lane telling codegen something the rest of the pipeline does not
        // model.
        NumericOperand::Decimal => None,
    }
}
