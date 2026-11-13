use super::context::{Builder, Result};
use crate::ssa::ir::{InstKind, Value};
use crate::OptError;
use vn_tir::{BackendTy, DynReason};
use vn_tir::{Resolution, TirExpr};

impl<'m> Builder<'m> {
    pub(super) fn lower_call_expr(
        &mut self,
        callee: &TirExpr,
        args: &[vn_tir::TirArg],
        res: &Resolution,
        ty: BackendTy,
    ) -> Result<Value> {
        if let Resolution::Intrinsic(wire) = res {
            let object = self.emit(InstKind::ConstNull, BackendTy::Dynamic(DynReason::Boxed));
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
        if let Resolution::NativeGlobal(n) = &callee.res
            && let Some(map) = self.try_lower_map_construction(*n, args, ty)?
        {
            return Ok(map);
        }
        if let Resolution::DirectFn(f) = res {
            if Some(*f) == self.self_fn
                && let Some(v) = self.try_lower_self_call(args, ty)?
            {
                return Ok(v);
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
                self.emit(
                    self.global_load(&name),
                    BackendTy::Dynamic(DynReason::Runtime),
                )
            }
            Resolution::NativeGlobal(n) => self.emit(
                InstKind::LoadNativeGlobalIdx(*n),
                BackendTy::Dynamic(DynReason::Runtime),
            ),
            Resolution::None
            | Resolution::Local(_)
            | Resolution::Param(_)
            | Resolution::Upvalue(_)
            | Resolution::GlobalSlot(_)
            | Resolution::ModuleSlot { .. }
            | Resolution::FieldSlot(_)
            | Resolution::StaticField(_)
            | Resolution::VtableSlot(_)
            | Resolution::Intrinsic(_)
            | Resolution::NativeOp(_)
            | Resolution::EnumVariant { .. }
            | Resolution::ByName { .. } => self.lower_expr(callee)?,
        };
        self.lower_call(cv, args, ty)
    }

    fn try_lower_map_construction(
        &mut self,
        native: u32,
        args: &[vn_tir::TirArg],
        ty: BackendTy,
    ) -> Result<Option<Value>> {
        use vn_core::abi::NATIVE_GLOBALS;
        use vn_core::op_id::KnownOp;
        use vn_core::BuiltinType;
        let Some(global) = NATIVE_GLOBALS.get(native as usize) else {
            return Ok(None);
        };
        let factory = [
            (BuiltinType::Map, KnownOp::MapNew),
            (BuiltinType::Set, KnownOp::SetNew),
            (BuiltinType::Array, KnownOp::ArrayNew),
        ]
        .into_iter()
        .find(|(class, _)| class.name() == global.name);
        let Some((class, op)) = factory else {
            return Ok(None);
        };
        if class != BuiltinType::Map || !matches!(ty, BackendTy::Map(..)) {
            return self.lower_factory_construction(op, args, ty);
        }
        let Some(entries) = map_entries(args) else {
            return self.lower_factory_construction(KnownOp::MapNew, args, ty);
        };
        let by_value = |key: &TirExpr| {
            matches!(
                key.ty.representation(),
                BackendTy::Str | BackendTy::Int | BackendTy::Bool
            )
        };
        if !entries.iter().all(|(key, _)| by_value(key)) {
            return self.lower_factory_construction(KnownOp::MapNew, args, ty);
        }
        let mut lowered = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            let key = self.lower_expr(key)?;
            let value = self.lower_expr(value)?;
            lowered.push((key, value));
        }
        Ok(Some(self.emit(InstKind::BuildMap { pairs: lowered }, ty)))
    }

    fn lower_factory_construction(
        &mut self,
        op: vn_core::op_id::KnownOp,
        args: &[vn_tir::TirArg],
        ty: BackendTy,
    ) -> Result<Option<Value>> {
        if args
            .iter()
            .any(|arg| matches!(arg, vn_tir::TirArg::Spread(_)))
        {
            return Ok(None);
        }
        let argv = self.lower_args(args)?;
        let object = self.emit(InstKind::ConstNull, BackendTy::Dynamic(DynReason::Boxed));
        Ok(Some(self.emit(
            InstKind::CallNativeOp {
                object,
                args: argv,
                op_id: op.id(),
            },
            ty,
        )))
    }

    pub(super) fn lower_method_call(
        &mut self,
        recv: &TirExpr,
        name: &std::sync::Arc<str>,
        args: &[vn_tir::TirArg],
        res: &Resolution,
        ty: BackendTy,
    ) -> Result<Value> {
        let r = self.lower_expr(recv)?;
        if args.iter().any(|a| matches!(a, vn_tir::TirArg::Spread(_))) {
            let bound = self.emit(
                InstKind::GetProperty {
                    object: r,
                    name: name.clone(),
                },
                BackendTy::Dynamic(DynReason::Runtime),
            );
            return self.lower_call(bound, args, ty);
        }
        if !matches!(res, Resolution::NativeOp(_))
            && let Some(v) = self.try_inline_method_call(recv, r, name, args, ty)?
        {
            return Ok(v);
        }
        let argv = self.lower_args(args)?;
        if let Resolution::NativeOp(op_id) = res {
            if *op_id == vn_core::op_id::KnownOp::ArrayPush.id() && argv.len() == 1 {
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
        args: &[vn_tir::TirArg],
        res: &Resolution,
        ty: BackendTy,
    ) -> Result<Value> {
        let (enum_id, tag) = match res {
            Resolution::EnumVariant { enum_id, tag } => (*enum_id, *tag),
            Resolution::None
            | Resolution::Local(_)
            | Resolution::Param(_)
            | Resolution::Upvalue(_)
            | Resolution::GlobalSlot(_)
            | Resolution::NativeGlobal(_)
            | Resolution::ModuleSlot { .. }
            | Resolution::FieldSlot(_)
            | Resolution::StaticField(_)
            | Resolution::VtableSlot(_)
            | Resolution::DirectFn(_)
            | Resolution::Intrinsic(_)
            | Resolution::NativeOp(_)
            | Resolution::ByName { .. } => {
                return Err(OptError::Unsupported("from_tir: MakeVariant without res"))
            }
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
        let slot = ei
            .variant_global(&vname)
            .and_then(|global| self.gslot(&global))
            .ok_or(OptError::Unsupported("from_tir: variant without a slot"))?;
        let variant = self.emit(
            InstKind::LoadGlobalIdx(slot),
            BackendTy::Dynamic(DynReason::Runtime),
        );
        if args.is_empty() {
            Ok(variant)
        } else {
            self.lower_call(variant, args, ty)
        }
    }
}

fn map_entries(args: &[vn_tir::TirArg]) -> Option<Vec<(&TirExpr, &TirExpr)>> {
    use vn_tir::{TirArg, TirArrayEl, TirExprKind};
    if args.is_empty() {
        return Some(Vec::new());
    }
    let [TirArg::Expr(only)] = args else {
        return None;
    };
    let mut source = only;
    while let TirExprKind::Cast { operand } = &source.kind {
        source = operand;
    }
    if matches!(source.kind, TirExprKind::NullLit) {
        return Some(Vec::new());
    }
    let TirExprKind::ArrayLit(elements) = &source.kind else {
        return None;
    };
    elements
        .iter()
        .map(|element| {
            let TirArrayEl::Expr(entry) = element else {
                return None;
            };
            if let TirExprKind::TupleLit(pair) = &entry.kind
                && let [key, value] = pair.as_slice()
            {
                return Some((key, value));
            }
            if let TirExprKind::ArrayLit(pair) = &entry.kind
                && let [TirArrayEl::Expr(key), TirArrayEl::Expr(value)] = pair.as_slice()
            {
                return Some((key, value));
            }
            None
        })
        .collect()
}
