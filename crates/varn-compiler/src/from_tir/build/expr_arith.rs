use super::context::{Builder, Result};
use crate::ssa::ir::{InstKind, Value};
use varn_tir::{BackendTy, DynReason};
use varn_tir::{TirBinOp, TirExpr, TirUnOp};

impl<'m> Builder<'m> {
    pub(super) fn lower_binary(
        &mut self,
        op: TirBinOp,
        lhs: &TirExpr,
        rhs: &TirExpr,
        ty: BackendTy,
    ) -> Result<Value> {
        let mut l = self.lower_expr(lhs)?;
        let mut r = self.lower_expr(rhs)?;
        let is_cmp = matches!(
            op,
            TirBinOp::Eq | TirBinOp::Ne | TirBinOp::Lt | TirBinOp::Le | TirBinOp::Gt | TirBinOp::Ge
        );
        if !is_cmp && matches!(ty, BackendTy::Int | BackendTy::Float | BackendTy::Str) {
            l = self.coerce(l, ty);
            r = self.coerce(r, ty);
        }
        let (lt, rt) = (self.value_ty(l), self.value_ty(r));
        let op_ty = if lt == rt
            && matches!(
                lt,
                BackendTy::Int | BackendTy::Float | BackendTy::Bool | BackendTy::Str
            ) {
            lt
        } else {
            BackendTy::Dynamic(DynReason::Boxed)
        };
        let result_ty = if is_cmp && op_ty != BackendTy::Dynamic(DynReason::Boxed) {
            BackendTy::Bool
        } else if op_ty != BackendTy::Dynamic(DynReason::Boxed) {
            op_ty
        } else {
            ty
        };
        Ok(self.emit(
            InstKind::Binary {
                op: super::ops::bin_op(op),
                lhs: l,
                rhs: r,
                ty: op_ty,
            },
            result_ty,
        ))
    }

    pub(super) fn lower_unary(
        &mut self,
        op: TirUnOp,
        operand: &TirExpr,
        ty: BackendTy,
    ) -> Result<Value> {
        let v = self.lower_expr(operand)?;
        match op {
            TirUnOp::IsNull => match self.value_ty(v) {
                BackendTy::Int | BackendTy::Float | BackendTy::Bool => {
                    Ok(self.emit(InstKind::ConstBool(false), BackendTy::Bool))
                }
                BackendTy::Str
                | BackendTy::Char
                | BackendTy::Bytes
                | BackendTy::Enum(_)
                | BackendTy::Fn(_)
                | BackendTy::Dynamic(_)
                | BackendTy::Decimal
                | BackendTy::BigInt
                | BackendTy::Void
                | BackendTy::Never
                | BackendTy::Array(_)
                | BackendTy::Map(..)
                | BackendTy::Dict(_)
                | BackendTy::Set(_)
                | BackendTy::Tuple(_)
                | BackendTy::Class(_)
                | BackendTy::Nullable(_) => {
                    Ok(self.emit(InstKind::IsNull { operand: v }, BackendTy::Bool))
                }
            },
            TirUnOp::Present => {
                self.emit_effect(InstKind::AssertNotNull { operand: v });
                Ok(v)
            }
            TirUnOp::ToStr => Ok(self.emit(
                InstKind::Cast {
                    operand: v,
                    ty: BackendTy::Str,
                },
                BackendTy::Str,
            )),
            TirUnOp::Neg | TirUnOp::Not | TirUnOp::BitNot | TirUnOp::Typeof => {
                let result = self.emit(
                    InstKind::Unary {
                        op: super::ops::un_op(op),
                        operand: v,
                        ty,
                    },
                    ty,
                );
                Ok(result)
            }
        }
    }

    fn lower_checked_class(
        &mut self,
        v: Value,
        class: varn_tir::ClassId,
        nullable: bool,
        ty: BackendTy,
    ) -> Result<Value> {
        let name =
            self.tir
                .class(class)
                .map(|ci| ci.name.clone())
                .ok_or(crate::OptError::Unsupported(
                    "from_tir: Checked to an unknown class",
                ))?;
        let cls = self.emit(
            self.global_load(&name),
            BackendTy::Dynamic(DynReason::Runtime),
        );
        let is_instance = self.emit(
            InstKind::Binary {
                op: crate::hir::HirBinOp::Instanceof,
                lhs: v,
                rhs: cls,
                ty: BackendTy::Dynamic(DynReason::Boxed),
            },
            BackendTy::Bool,
        );
        let holds = if nullable {
            let is_null = self.emit(InstKind::IsNull { operand: v }, BackendTy::Bool);
            let not_null = self.emit(
                InstKind::Unary {
                    op: crate::hir::HirUnOp::Not,
                    operand: is_null,
                    ty: BackendTy::Bool,
                },
                BackendTy::Bool,
            );
            self.emit(
                InstKind::Binary {
                    op: crate::hir::HirBinOp::Eq,
                    lhs: is_instance,
                    rhs: not_null,
                    ty: BackendTy::Bool,
                },
                BackendTy::Bool,
            )
        } else {
            is_instance
        };
        let expected = if nullable {
            format!("{name}?")
        } else {
            name.to_string()
        };
        self.throw_type_mismatch_unless(holds, v, &expected)?;
        Ok(self.emit(InstKind::Cast { operand: v, ty }, ty))
    }

    fn throw_type_mismatch_unless(&mut self, holds: Value, v: Value, expected: &str) -> Result<()> {
        let throw_blk = self.new_block();
        let join_blk = self.new_block();
        let from = self.current;
        self.set_term(crate::ssa::ir::Terminator::Branch {
            cond: holds,
            then_blk: join_blk,
            then_args: vec![],
            else_blk: throw_blk,
            else_args: vec![],
        });
        self.add_pred(join_blk, from);
        self.add_pred(throw_blk, from);
        self.seal_block(join_blk);
        self.seal_block(throw_blk);
        self.current = throw_blk;
        let got = self.emit(
            InstKind::Unary {
                op: crate::hir::HirUnOp::Typeof,
                operand: v,
                ty: BackendTy::Str,
            },
            BackendTy::Str,
        );
        let head = self.emit(
            InstKind::ConstStr(std::sync::Arc::from(format!(
                "type mismatch: expected '{expected}', got '"
            ))),
            BackendTy::Str,
        );
        let tail = self.emit(
            InstKind::ConstStr(std::sync::Arc::from("'")),
            BackendTy::Str,
        );
        let mut message = head;
        for part in [got, tail] {
            message = self.emit(
                InstKind::Binary {
                    op: crate::hir::HirBinOp::Add,
                    lhs: message,
                    rhs: part,
                    ty: BackendTy::Str,
                },
                BackendTy::Str,
            );
        }
        let type_error = varn_core::abi::native_global_index(
            varn_core::RuntimeErrorKind::TypeError.class_name(),
        )
        .ok_or(crate::OptError::Unsupported(
            "from_tir: TypeError is not a native global",
        ))?;
        let ctor = self.emit(
            InstKind::LoadNativeGlobalIdx(type_error),
            BackendTy::Dynamic(DynReason::Runtime),
        );
        let error = self.emit(
            InstKind::Call {
                callee: ctor,
                args: vec![message],
            },
            BackendTy::Dynamic(DynReason::Runtime),
        );
        self.set_term(crate::ssa::ir::Terminator::Throw(error));
        self.current = join_blk;
        Ok(())
    }

    pub(super) fn lower_checked(
        &mut self,
        operand: &TirExpr,
        node_ty: varn_tir::BackendTy,
        ty: BackendTy,
    ) -> Result<Value> {
        let v = self.lower_expr(operand)?;
        match node_ty {
            varn_tir::BackendTy::Class(class) => {
                return self.lower_checked_class(v, class, false, ty);
            }
            varn_tir::BackendTy::Nullable(inner) if self.tir.types.contains(inner) => {
                let inner = self.tir.types.get(inner);
                if let varn_tir::BackendTy::Class(class) = inner {
                    return self.lower_checked_class(v, class, true, ty);
                }
                if super::conform::is_structural(inner) {
                    return self.lower_conform(v, node_ty, ty);
                }
            }
            varn_tir::BackendTy::Array(_)
            | varn_tir::BackendTy::Map(..)
            | varn_tir::BackendTy::Dict(_)
            | varn_tir::BackendTy::Set(_)
            | varn_tir::BackendTy::Tuple(_)
            | varn_tir::BackendTy::Enum(_)
            | varn_tir::BackendTy::Fn(_)
            | varn_tir::BackendTy::Char
            | varn_tir::BackendTy::Bytes
            | varn_tir::BackendTy::Decimal
            | varn_tir::BackendTy::BigInt
            | varn_tir::BackendTy::Dynamic(
                varn_tir::DynReason::Callable | varn_tir::DynReason::Shape(_),
            ) => {
                return self.lower_conform(v, node_ty, ty);
            }
            varn_tir::BackendTy::Int
            | varn_tir::BackendTy::Float
            | varn_tir::BackendTy::Bool
            | varn_tir::BackendTy::Str
            | varn_tir::BackendTy::Nullable(_)
            | varn_tir::BackendTy::Void
            | varn_tir::BackendTy::Never
            | varn_tir::BackendTy::Dynamic(
                varn_tir::DynReason::HostBoundary
                | varn_tir::DynReason::Union
                | varn_tir::DynReason::IndexSignature
                | varn_tir::DynReason::Declared
                | varn_tir::DynReason::Awaitable(_)
                | varn_tir::DynReason::Yields(_)
                | varn_tir::DynReason::NotYetSupported
                | varn_tir::DynReason::Boxed
                | varn_tir::DynReason::Runtime,
            ) => {}
        }
        let conv = match ty {
            BackendTy::Str => varn_core::ValueConv::ExpectStr,
            BackendTy::Bool => varn_core::ValueConv::ExpectBool,
            BackendTy::Int => varn_core::ValueConv::ExpectInt,
            BackendTy::Float => varn_core::ValueConv::ExpectFloat,
            BackendTy::Nullable(inner) => {
                let conv = match self.tir.types.get(inner) {
                    BackendTy::Str => varn_core::ValueConv::ExpectStrOrNull,
                    BackendTy::Bool => varn_core::ValueConv::ExpectBoolOrNull,
                    BackendTy::Int => varn_core::ValueConv::ExpectIntOrNull,
                    BackendTy::Float => varn_core::ValueConv::ExpectFloatOrNull,
                    BackendTy::Dynamic(_)
                    | BackendTy::Char
                    | BackendTy::Bytes
                    | BackendTy::Enum(_)
                    | BackendTy::Fn(_)
                    | BackendTy::Decimal
                    | BackendTy::BigInt
                    | BackendTy::Void
                    | BackendTy::Never
                    | BackendTy::Array(_)
                    | BackendTy::Map(..)
                    | BackendTy::Dict(_)
                    | BackendTy::Set(_)
                    | BackendTy::Tuple(_)
                    | BackendTy::Class(_)
                    | BackendTy::Nullable(_) => {
                        return Err(crate::OptError::Unsupported(
                            "from_tir: Checked to an unsupported nullable type",
                        ));
                    }
                };
                let checked = self.emit(
                    InstKind::Convert { operand: v, conv },
                    BackendTy::Dynamic(DynReason::Boxed),
                );
                return Ok(self.emit(
                    InstKind::Cast {
                        operand: checked,
                        ty,
                    },
                    ty,
                ));
            }
            BackendTy::Dynamic(_)
            | BackendTy::Char
            | BackendTy::Bytes
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Dict(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Class(_) => {
                return Err(crate::OptError::Unsupported(
                    "from_tir: Checked to a non-scalar type",
                ));
            }
        };
        let result_ty = crate::ssa::verify::convert_result_ty(conv);
        Ok(self.emit(InstKind::Convert { operand: v, conv }, result_ty))
    }

    fn static_truthiness(&mut self, v: Value) -> Option<Value> {
        match self.value_ty(v) {
            BackendTy::Bool => Some(v),
            BackendTy::Int
            | BackendTy::Float
            | BackendTy::Str
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Dict(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Class(_) => Some(self.emit(InstKind::ConstBool(true), BackendTy::Bool)),
            BackendTy::Nullable(inner) => match self.tir.types.get(inner) {
                BackendTy::Int
                | BackendTy::Float
                | BackendTy::Str
                | BackendTy::Array(_)
                | BackendTy::Map(..)
                | BackendTy::Dict(_)
                | BackendTy::Set(_)
                | BackendTy::Tuple(_)
                | BackendTy::Class(_) => {
                    let absent = self.emit(InstKind::IsNull { operand: v }, BackendTy::Bool);
                    Some(self.emit(
                        InstKind::Unary {
                            op: crate::hir::HirUnOp::Not,
                            operand: absent,
                            ty: BackendTy::Bool,
                        },
                        BackendTy::Bool,
                    ))
                }
                BackendTy::Bool
                | BackendTy::Dynamic(_)
                | BackendTy::Char
                | BackendTy::Bytes
                | BackendTy::Enum(_)
                | BackendTy::Fn(_)
                | BackendTy::Decimal
                | BackendTy::BigInt
                | BackendTy::Void
                | BackendTy::Never
                | BackendTy::Nullable(_) => None,
            },
            BackendTy::Dynamic(_)
            | BackendTy::Char
            | BackendTy::Bytes
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Void
            | BackendTy::Never => None,
        }
    }

    pub(super) fn lower_cast(
        &mut self,
        operand: &TirExpr,
        node_ty: varn_tir::BackendTy,
        ty: BackendTy,
    ) -> Result<Value> {
        use varn_core::NumericDomain as D;
        let v = self.lower_expr(operand)?;
        if ty == BackendTy::Bool
            && let Some(holds) = self.static_truthiness(v)
        {
            return Ok(holds);
        }
        let from = match self.value_ty(v) {
            BackendTy::Int => Some(D::Int),
            BackendTy::Float => Some(D::Float),
            BackendTy::Bool
            | BackendTy::Str
            | BackendTy::Char
            | BackendTy::Bytes
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Dynamic(_)
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Dict(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Class(_)
            | BackendTy::Nullable(_) => super::ops::numeric_domain(operand.ty)
                .filter(|d| matches!(d, D::BigInt | D::Decimal)),
        };
        let to = super::ops::numeric_domain(node_ty);
        let conv = match (from, to) {
            (Some(f), Some(t)) if f == t => return Ok(v),
            (Some(f), Some(t)) => varn_core::ValueConv::between(f, t),
            (None, Some(D::Int)) => Some(varn_core::ValueConv::DynToInt),
            (None, Some(D::Float)) => Some(varn_core::ValueConv::DynToFloat),
            _ => None,
        };
        if let Some(conv) = conv {
            let result_ty = crate::ssa::verify::convert_result_ty(conv);
            return Ok(self.emit(InstKind::Convert { operand: v, conv }, result_ty));
        }
        Ok(self.emit(InstKind::Cast { operand: v, ty }, ty))
    }
}
