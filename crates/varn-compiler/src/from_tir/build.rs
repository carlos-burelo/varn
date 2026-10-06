










use std::sync::Arc;

use crate::hir::{HirBinOp, HirType};
use crate::ssa::ir::{InstKind, Value};
use varn_tir::{TirExpr, TirExprKind};

mod assign;
mod call;
mod class_def;
mod classdef_methods;
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
        let ty = self.ty(e.ty);
        match &e.kind {
            
            
            TirExprKind::IntLit(n) => Ok(self.emit(InstKind::ConstInt(*n), HirType::Int)),
            TirExprKind::FloatLit(f) => Ok(self.emit(InstKind::ConstFloat(*f), HirType::Float)),
            TirExprKind::BoolLit(b) => Ok(self.emit(InstKind::ConstBool(*b), HirType::Bool)),
            TirExprKind::StrLit(s) => Ok(self.emit(InstKind::ConstStr(s.clone()), HirType::Str)),
            
            
            
            
            TirExprKind::CharLit(c) => Ok(self.emit(InstKind::ConstChar(*c), HirType::Ref)),
            TirExprKind::NullLit => Ok(self.emit(InstKind::ConstNull, HirType::Dynamic)),
            TirExprKind::DecimalLit(s) => {
                let d = s.parse().unwrap_or_default();
                Ok(self.emit(InstKind::ConstDecimal(d), HirType::Dynamic))
            }
            TirExprKind::BigIntLit(n) => {
                Ok(self.emit(InstKind::ConstBigInt(n.clone()), HirType::Dynamic))
            }
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
                    None => self.emit(InstKind::ConstNull, HirType::Dynamic),
                };
                Ok(self.emit(InstKind::Yield { operand: v }, ty))
            }

            TirExprKind::Discriminant { value } => {
                let v = self.lower_expr(value)?;
                Ok(self.emit(InstKind::GetEnumTag { operand: v }, HirType::Int))
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
                let cls = self.emit(self.global_load(&cname), HirType::Ref);
                Ok(self.emit(
                    InstKind::Binary {
                        op: HirBinOp::Instanceof,
                        lhs: v,
                        rhs: cls,
                        ty: HirType::Dynamic,
                    },
                    HirType::Bool,
                ))
            }

            TirExprKind::ObjectKeys { operand } => {
                let o = self.lower_expr(operand)?;
                Ok(self.emit(InstKind::ObjectKeys { operand: o }, ty))
            }

            TirExprKind::IterInit { source, is_async } => {
                let src = self.lower_expr(source)?;
                let sym = self.emit(
                    InstKind::GetSymbol {
                        object: src,
                        is_async: *is_async,
                    },
                    HirType::Ref,
                );
                Ok(self.emit(
                    InstKind::IterCall {
                        callee: sym,
                        recv: src,
                    },
                    ty,
                ))
            }

            TirExprKind::SuperCall { args } => {
                let argv = self.lower_args(args)?;
                Ok(self.emit(InstKind::SuperCall { args: argv }, ty))
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
                    HirType::Ref,
                ))
            }
        }
    }
}

pub(crate) use functions::defaulted_param_mask;
pub use functions::{build_function, build_module, build_top_level};
