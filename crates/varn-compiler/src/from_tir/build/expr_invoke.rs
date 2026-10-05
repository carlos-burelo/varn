use super::context::{Builder, Result};
use crate::hir::HirType;
use crate::ssa::ir::{InstKind, Value};
use crate::OptError;
use varn_tir::{Resolution, TirExpr};

impl<'m> Builder<'m> {
    pub(super) fn lower_call_expr(
        &mut self,
        callee: &TirExpr,
        args: &[varn_tir::TirArg],
        res: &Resolution,
        ty: HirType,
    ) -> Result<Value> {
        if let Resolution::Intrinsic(wire) = res {
            let object = self.emit(InstKind::ConstNull, HirType::Dynamic);
            let argv = self.lower_args(args)?;
            return Ok(self.emit(
                InstKind::IntrinsicCall {
                    object,
                    args: argv,
                    wire_byte: *wire as u8,
                },
                ty,
            ));
        }
        if let Resolution::DirectFn(f) = res {
            if Some(*f) == self.self_fn {
                if let Some(v) = self.try_lower_self_call(args, ty)? {
                    return Ok(v);
                }
            }
            if let Some(v) = self.try_inline_direct_call(*f, args, ty)? {
                return Ok(v);
            }
        }
        let cv = match res {
            Resolution::DirectFn(f) => {
                let name = self
                    .tir
                    .function(*f)
                    .map(|tf| tf.name.clone())
                    .ok_or(OptError::Unsupported("from_tir: DirectFn out of range"))?;
                self.emit(self.global_load(&name), HirType::Ref)
            }
            Resolution::NativeGlobal(n) => {
                self.emit(InstKind::LoadNativeGlobalIdx(*n), HirType::Ref)
            }
            _ => self.lower_expr(callee)?,
        };
        self.lower_call(cv, args, ty)
    }

    pub(super) fn lower_method_call(
        &mut self,
        recv: &TirExpr,
        name: &std::sync::Arc<str>,
        args: &[varn_tir::TirArg],
        res: &Resolution,
        ty: HirType,
    ) -> Result<Value> {
        let r = self.lower_expr(recv)?;
        if args
            .iter()
            .any(|a| matches!(a, varn_tir::TirArg::Spread(_)))
        {
            let bound = self.emit(
                InstKind::GetProperty {
                    object: r,
                    name: name.clone(),
                },
                HirType::Ref,
            );
            return self.lower_call(bound, args, ty);
        }
        let argv = self.lower_args(args)?;
        if let Resolution::NativeOp(op_id) = res {
            if *op_id == varn_core::op_id::array_push_op_id() && argv.len() == 1 {
                self.emit_effect(InstKind::ArrayPush {
                    array: r,
                    value: argv[0],
                });
                return Ok(self.emit(InstKind::ConstNull, ty));
            }
            return Ok(self.emit(
                InstKind::CallNativeOp {
                    object: r,
                    args: argv,
                    op_id: *op_id,
                },
                ty,
            ));
        }
        Ok(self.emit(
            InstKind::MethodCall {
                recv: r,
                name: name.clone(),
                args: argv,
            },
            ty,
        ))
    }

    pub(super) fn lower_make_variant(
        &mut self,
        args: &[varn_tir::TirArg],
        res: &Resolution,
        ty: HirType,
    ) -> Result<Value> {
        let (enum_id, tag) = match res {
            Resolution::EnumVariant { enum_id, tag } => (*enum_id, *tag),
            _ => return Err(OptError::Unsupported("from_tir: MakeVariant without res")),
        };
        let ei = self
            .tir
            .enum_info(enum_id)
            .ok_or(OptError::Unsupported("from_tir: enum out of range"))?;
        let vname = ei
            .variants
            .iter()
            .find(|v| v.tag == tag)
            .map(|v| v.name.clone())
            .ok_or(OptError::Unsupported("from_tir: variant out of range"))?;
        let enum_val = self.emit(self.global_load(&ei.name), HirType::Ref);
        let variant = self.emit(
            InstKind::GetProperty {
                object: enum_val,
                name: vname,
            },
            HirType::Ref,
        );
        if args.is_empty() {
            Ok(variant)
        } else {
            self.lower_call(variant, args, ty)
        }
    }
}
