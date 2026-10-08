use super::context::{Builder, Result};
use crate::hir::{HirType, LocalId};
use crate::ssa::ir::{InstKind, Terminator, Value, VarId};
use crate::OptError;
use varn_tir::{Resolution, TirExpr, TirExprKind};

impl<'m> Builder<'m> {
    pub(super) fn lower_assign(&mut self, target: &TirExpr, value: Value) -> Result<()> {
        match &target.kind {
            TirExprKind::Var => match &target.res {
                Resolution::Local(id) => {
                    let var = VarId::Local(LocalId(id.0));
                    let value = match self.var_ty.get(&var).copied() {
                        Some(declared) => self.coerce(value, declared),
                        None => value,
                    };
                    self.store_var(var, value);
                }
                Resolution::Param(i) => {
                    self.store_var(VarId::Param(*i), value);
                }
                Resolution::Upvalue(uv) => {
                    self.emit_effect(InstKind::StoreUpvalue { index: *uv, value });
                }
                Resolution::ByName { name, .. } => {
                    self.emit_effect(InstKind::StoreGlobal {
                        name: name.clone(),
                        value,
                    });
                }
                Resolution::GlobalSlot(n) => {
                    self.emit_effect(InstKind::StoreGlobalIdx { slot: *n, value });
                }
                Resolution::None
                | Resolution::NativeGlobal(_)
                | Resolution::ModuleSlot { .. }
                | Resolution::FieldSlot(_)
                | Resolution::StaticField(_)
                | Resolution::VtableSlot(_)
                | Resolution::DirectFn(_)
                | Resolution::Intrinsic(_)
                | Resolution::NativeOp(_)
                | Resolution::EnumVariant { .. } => {
                    return Err(OptError::Unsupported("from_tir: assign target var"))
                }
            },
            TirExprKind::Field { object, name } => {
                let obj = self.lower_expr(object)?;
                let kind = match &target.res {
                    Resolution::FieldSlot(slot) => match self.compact_field(object.ty, *slot) {
                        Some((offset, tag)) => InstKind::SetFixedField {
                            object: obj,
                            value,
                            slot: *slot,
                            offset,
                            tag,
                        },
                        None => InstKind::SetProperty {
                            object: obj,
                            name: name.clone(),
                            value,
                        },
                    },
                    Resolution::None
                    | Resolution::Local(_)
                    | Resolution::Param(_)
                    | Resolution::Upvalue(_)
                    | Resolution::GlobalSlot(_)
                    | Resolution::NativeGlobal(_)
                    | Resolution::ModuleSlot { .. }
                    | Resolution::StaticField(_)
                    | Resolution::VtableSlot(_)
                    | Resolution::DirectFn(_)
                    | Resolution::Intrinsic(_)
                    | Resolution::NativeOp(_)
                    | Resolution::EnumVariant { .. }
                    | Resolution::ByName { .. } => InstKind::SetProperty {
                        object: obj,
                        name: name.clone(),
                        value,
                    },
                };
                self.emit_effect(kind);
            }
            TirExprKind::Index { object, index } => {
                let obj = self.lower_expr(object)?;
                let idx = self.lower_expr(index)?;
                let kind = if matches!(self.value_ty(obj), HirType::Array(_)) {
                    InstKind::ArraySetIndex {
                        object: obj,
                        index: idx,
                        value,
                    }
                } else if matches!(self.value_ty(obj), HirType::Map(_, _)) {
                    InstKind::MapSetIndex {
                        object: obj,
                        index: idx,
                        value,
                    }
                } else {
                    InstKind::SetIndex {
                        object: obj,
                        index: idx,
                        value,
                    }
                };
                self.emit_effect(kind);
            }
            TirExprKind::IntLit(_)
            | TirExprKind::FloatLit(_)
            | TirExprKind::BoolLit(_)
            | TirExprKind::StrLit(_)
            | TirExprKind::CharLit(_)
            | TirExprKind::NullLit
            | TirExprKind::Binary { .. }
            | TirExprKind::Unary { .. }
            | TirExprKind::Call { .. }
            | TirExprKind::MethodCall { .. }
            | TirExprKind::Assign { .. }
            | TirExprKind::ArrayLit(_)
            | TirExprKind::TupleLit(_)
            | TirExprKind::ObjectLit { .. }
            | TirExprKind::RecordLit { .. }
            | TirExprKind::Await { .. }
            | TirExprKind::Yield { .. }
            | TirExprKind::Discriminant { .. }
            | TirExprKind::VariantPayload { .. }
            | TirExprKind::TypeTest { .. }
            | TirExprKind::Cast { .. }
            | TirExprKind::Closure { .. }
            | TirExprKind::New { .. }
            | TirExprKind::MakeVariant { .. }
            | TirExprKind::Select { .. }
            | TirExprKind::Seq { .. }
            | TirExprKind::ObjectKeys { .. }
            | TirExprKind::IterInit { .. }
            | TirExprKind::SuperCall { .. }
            | TirExprKind::SuperMethodCall { .. }
            | TirExprKind::DecimalLit(_)
            | TirExprKind::BigIntLit(_)
            | TirExprKind::RangeLit { .. }
            | TirExprKind::ObjectRest { .. }
            | TirExprKind::ExtensionCall { .. } => {
                return Err(OptError::Unsupported("from_tir: assign target"))
            }
        }
        Ok(())
    }

    pub(super) fn store_var(&mut self, var: VarId, value: Value) {
        if self.pinned.contains(&var) {
            self.var_ty.insert(var, self.value_ty(value));
            self.emit_effect(InstKind::StoreCaptured { var, value });
        } else {
            let cur = self.current;
            self.write_var(var, cur, value);
        }
    }

    pub(super) fn load_var(&mut self, var: VarId, ty: HirType) -> Result<Value> {
        if self.pinned.contains(&var) {
            let ty = self.var_ty.get(&var).copied().unwrap_or(ty);
            Ok(self.emit(InstKind::LoadCaptured { var }, ty))
        } else {
            self.read_var(var, self.current)
        }
    }

    pub(super) fn lower_var(&mut self, res: &Resolution, ty: HirType) -> Result<Value> {
        match res {
            Resolution::Local(id) => self.load_var(VarId::Local(LocalId(id.0)), ty),
            Resolution::Param(i) => {
                if let Some(frame) = self.inlining.last() {
                    if let Some(&arg_v) = frame.params.get(*i as usize) {
                        return Ok(arg_v);
                    }
                }
                self.load_var(VarId::Param(*i), ty)
            }
            Resolution::Upvalue(uv) => Ok(self.emit(InstKind::LoadUpvalue(*uv), ty)),
            Resolution::GlobalSlot(n) => Ok(self.emit(InstKind::LoadGlobalIdx(*n), ty)),
            Resolution::NativeGlobal(n) => Ok(self.emit(InstKind::LoadNativeGlobalIdx(*n), ty)),
            Resolution::ModuleSlot { .. } => Err(OptError::Unsupported("from_tir: module slot")),
            Resolution::ByName { name, .. } => {
                Ok(self.emit(InstKind::LoadGlobal(name.clone()), ty))
            }
            Resolution::DirectFn(f) => {
                let name = self
                    .tir
                    .function(*f)
                    .map(|tf| tf.name.clone())
                    .ok_or(OptError::Unsupported("from_tir: DirectFn var out of range"))?;
                Ok(self.emit(self.global_load(&name), ty))
            }
            Resolution::None => match self.inlining.last().and_then(|frame| frame.this) {
                Some(this) => Ok(this),
                None => Ok(self.emit(InstKind::This, ty)),
            },
            Resolution::FieldSlot(_)
            | Resolution::StaticField(_)
            | Resolution::VtableSlot(_)
            | Resolution::Intrinsic(_)
            | Resolution::NativeOp(_)
            | Resolution::EnumVariant { .. } => {
                Err(OptError::Unsupported("from_tir: var resolution"))
            }
        }
    }

    pub(super) fn lower_select(
        &mut self,
        cond: Value,
        then_val: &TirExpr,
        else_val: &TirExpr,
        ty: HirType,
    ) -> Result<Value> {
        let then_blk = self.new_block();
        let else_blk = self.new_block();
        let join = self.new_block();
        let from = self.current;
        self.set_term(Terminator::Branch {
            cond,
            then_blk,
            then_args: vec![],
            else_blk,
            else_args: vec![],
        });
        self.add_pred(then_blk, from);
        self.add_pred(else_blk, from);
        self.seal_block(then_blk);
        self.seal_block(else_blk);

        let phi = self.add_block_param(join, ty);

        self.current = then_blk;
        let tv = self.lower_expr(then_val)?;
        let tv = self.coerce(tv, ty);
        let tfrom = self.current;
        self.set_term(Terminator::Jump {
            target: join,
            args: vec![tv],
        });
        self.add_pred(join, tfrom);

        self.current = else_blk;
        let ev = self.lower_expr(else_val)?;
        let ev = self.coerce(ev, ty);
        let efrom = self.current;
        self.set_term(Terminator::Jump {
            target: join,
            args: vec![ev],
        });
        self.add_pred(join, efrom);

        self.seal_block(join);
        self.current = join;
        Ok(phi)
    }
}
