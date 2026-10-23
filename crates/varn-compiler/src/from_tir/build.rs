use std::sync::Arc;
use varn_tir::{BackendTy, DynReason};

use crate::hir::HirBinOp;
use crate::ssa::ir::{InstKind, Value};
use varn_tir::{TirExpr, TirExprKind};

mod assign;
mod call;
mod class_def;
mod classdef_methods;
mod conform;
mod construct;
mod context;
mod exports;
mod expr_access;
mod expr_arith;
mod expr_collect;
mod expr_invoke;
mod expr_lit;
mod functions;
mod globals;
mod infra;
mod ops;
mod stmt;

pub(super) use context::{Builder, Result};

impl<'m> Builder<'m> {
    fn lower_expr(&mut self, e: &TirExpr) -> Result<Value> {
        let ty = e.ty;
        match &e.kind {
            TirExprKind::IntLit(n) => Ok(self.emit(InstKind::ConstInt(*n), BackendTy::Int)),
            TirExprKind::FloatLit(f) => Ok(self.emit(InstKind::ConstFloat(*f), BackendTy::Float)),
            TirExprKind::BoolLit(b) => Ok(self.emit(InstKind::ConstBool(*b), BackendTy::Bool)),
            TirExprKind::StrLit(s) => Ok(self.emit(InstKind::ConstStr(s.clone()), BackendTy::Str)),

            TirExprKind::CharLit(c) => Ok(self.emit(InstKind::ConstChar(*c), BackendTy::Char)),
            TirExprKind::NullLit => {
                Ok(self.emit(InstKind::ConstNull, BackendTy::Dynamic(DynReason::Boxed)))
            }
            TirExprKind::DecimalLit(s) => {
                let d = s.parse().unwrap_or_default();
                Ok(self.emit(
                    InstKind::ConstDecimal(d),
                    BackendTy::Dynamic(DynReason::Boxed),
                ))
            }
            TirExprKind::BigIntLit(n) => Ok(self.emit(
                InstKind::ConstBigInt(n.clone()),
                BackendTy::Dynamic(DynReason::Boxed),
            )),
            TirExprKind::RangeLit {
                start,
                end,
                inclusive,
            } => self.lower_range_lit(start, end, *inclusive),
            TirExprKind::ObjectRest { object, skip_keys } => {
                self.lower_object_rest(object, skip_keys)
            }
            TirExprKind::ExtensionCall { func, recv, args } => {
                self.lower_extension_call(func, recv, args, ty)
            }

            TirExprKind::Var => self.lower_var(&e.res, ty),

            TirExprKind::Binary { op, lhs, rhs } => self.lower_binary(*op, lhs, rhs, ty),
            TirExprKind::Unary { op, operand } => self.lower_unary(*op, operand, ty),
            TirExprKind::Cast { operand } => self.lower_cast(operand, e.ty, ty),
            TirExprKind::Checked { operand } => self.lower_checked(operand, e.ty, ty),
            TirExprKind::Select {
                cond,
                then_val,
                else_val,
            } => {
                let c = self.lower_expr(cond)?;
                self.lower_select(c, then_val, else_val, ty)
            }
            TirExprKind::Seq { stmts, value } => self.lower_seq(stmts, value),

            TirExprKind::Field { object, name } => self.lower_field(object, name, &e.res, ty),
            TirExprKind::Index { object, index } => self.lower_index(object, index, ty),

            TirExprKind::Call { callee, args } => self.lower_call_expr(callee, args, &e.res, ty),
            TirExprKind::MethodCall { recv, name, args } => {
                self.lower_method_call(recv, name, args, &e.res, ty)
            }
            TirExprKind::New { class, args } => self.lower_new_expr(*class, args, ty),
            TirExprKind::MakeVariant { args } => self.lower_make_variant(args, &e.res, ty),

            TirExprKind::ArrayLit(els) => self.lower_array_lit(els, ty),
            TirExprKind::TupleLit(xs) => self.lower_tuple_lit(xs, ty),
            TirExprKind::RecordLit { fields } => self.lower_record_lit(fields, ty),
            TirExprKind::ObjectLit { entries } => self.lower_object_lit(entries, ty),

            TirExprKind::Assign { target, value } => self.lower_assign_expr(target, value),

            TirExprKind::Await { future } => {
                let v = self.lower_expr(future)?;
                Ok(self.emit(InstKind::Await { operand: v }, ty))
            }
            TirExprKind::Yield { value, .. } => {
                let v = match value {
                    Some(e) => self.lower_expr(e)?,
                    None => self.emit(InstKind::ConstNull, BackendTy::Dynamic(DynReason::Boxed)),
                };
                Ok(self.emit(InstKind::Yield { operand: v }, ty))
            }

            TirExprKind::Discriminant { value } => {
                let v = self.lower_expr(value)?;
                Ok(self.emit(InstKind::GetEnumTag { operand: v }, BackendTy::Int))
            }
            TirExprKind::VariantPayload { value, field, .. } => {
                let v = self.lower_expr(value)?;
                Ok(self.emit(
                    InstKind::GetFixedField {
                        object: v,
                        slot: *field,
                        offset: 0,
                        tag: varn_core::FieldAccess::Slot,
                    },
                    ty,
                ))
            }
            TirExprKind::TypeTest { value, class } => {
                let v = self.lower_expr(value)?;
                let cname = self
                    .tir
                    .class(*class)
                    .map(|ci| ci.name.clone())
                    .unwrap_or_else(|| Arc::from("?"));
                let cls = self.emit(
                    self.global_load(&cname),
                    BackendTy::Dynamic(DynReason::Runtime),
                );
                Ok(self.emit(
                    InstKind::Binary {
                        op: HirBinOp::Instanceof,
                        lhs: v,
                        rhs: cls,
                        ty: BackendTy::Dynamic(DynReason::Boxed),
                    },
                    BackendTy::Bool,
                ))
            }

            TirExprKind::ObjectKeys { operand } => {
                let o = self.lower_expr(operand)?;
                Ok(self.emit(InstKind::ObjectKeys { operand: o }, ty))
            }

            TirExprKind::SuperCall { args }
                if args
                    .iter()
                    .any(|a| matches!(a, varn_tir::TirArg::Spread(_))) =>
            {
                let ctor = self.emit(
                    InstKind::GetSuper {
                        name: Arc::from(varn_core::TokenKind::Constructor.as_str()),
                    },
                    BackendTy::Dynamic(DynReason::Runtime),
                );
                self.lower_call(ctor, args, ty)
            }
            TirExprKind::SuperCall { args } => {
                let argv = self.lower_args(args)?;
                Ok(self.emit(InstKind::SuperCall { args: argv }, ty))
            }
            TirExprKind::SuperMethodCall { name, args }
                if args
                    .iter()
                    .any(|a| matches!(a, varn_tir::TirArg::Spread(_))) =>
            {
                let method = self.emit(
                    InstKind::GetSuper { name: name.clone() },
                    BackendTy::Dynamic(DynReason::Runtime),
                );
                self.lower_call(method, args, ty)
            }
            TirExprKind::SuperMethodCall { name, args } => {
                let argv = self.lower_args(args)?;
                Ok(self.emit(
                    InstKind::SuperMethodCall {
                        name: name.clone(),
                        args: argv,
                    },
                    ty,
                ))
            }

            TirExprKind::Closure { func, upvalues } => {
                let src = upvalues.iter().map(|u| ops::upvalue_src(*u)).collect();
                Ok(self.emit(
                    InstKind::MakeClosure {
                        func: func.0,
                        upvalues_src: src,
                    },
                    BackendTy::Dynamic(DynReason::Runtime),
                ))
            }
        }
    }
}

pub(crate) use functions::defaulted_param_mask;
pub use functions::{build_function, build_module, build_top_level};
