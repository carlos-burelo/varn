use super::context::{Builder, Result};
use crate::ssa::ir::{InstKind, Value};
use std::sync::Arc;
use varn_tir::TirClassDef;
use varn_tir::{BackendTy, DynReason};

impl<'m> Builder<'m> {
    fn apply_member_decorator(
        &mut self,
        mv: Value,
        deco: &varn_tir::TirExpr,
        key: &Arc<str>,
        is_static: bool,
        is_private: bool,
    ) -> Result<Value> {
        let deco_v = self.lower_expr(deco)?;
        let n = self.emit(InstKind::ConstStr(key.clone()), BackendTy::Str);
        let st = self.emit(InstKind::ConstBool(is_static), BackendTy::Bool);
        let pv = self.emit(InstKind::ConstBool(is_private), BackendTy::Bool);
        let ctx = self.emit(
            InstKind::BuildObject {
                pairs: vec![
                    (Arc::from(varn_core::MemberKey::Name.as_str()), n),
                    (Arc::from(varn_core::MemberKey::IsStatic.as_str()), st),
                    (Arc::from(varn_core::MemberKey::IsPrivate.as_str()), pv),
                ],
            },
            BackendTy::Dynamic(DynReason::Runtime),
        );
        let result = self.emit(
            InstKind::Call {
                callee: deco_v,
                args: vec![mv, ctx],
            },
            BackendTy::Dynamic(DynReason::Runtime),
        );
        let isnull = self.emit(InstKind::IsNull { operand: result }, BackendTy::Bool);
        self.select_value(isnull, mv, result, BackendTy::Dynamic(DynReason::Runtime))
    }

    pub(super) fn build_class_methods(&mut self, def: &TirClassDef, class_v: Value) -> Result<()> {
        for m in &def.methods {
            let mut mv = self.emit(
                InstKind::MakeClosure {
                    func: m.func.0,
                    upvalues_src: vec![],
                },
                BackendTy::Dynamic(DynReason::Runtime),
            );
            for deco in m.decorators.iter().rev() {
                mv = self.apply_member_decorator(mv, deco, &m.key, m.is_static, m.is_private)?;
            }
            self.emit_effect(InstKind::DefineMethod {
                class: class_v,
                name: m.key.clone(),
                method: mv,
                is_static: m.is_static,
            });
        }
        Ok(())
    }

    pub(super) fn build_class_decorators(
        &mut self,
        def: &TirClassDef,
        class_v: Value,
    ) -> Result<Value> {
        let mut class_v = class_v;
        for deco in def.decorators.iter().rev() {
            let deco_v = self.lower_expr(deco)?;
            let result = self.emit(
                InstKind::Call {
                    callee: deco_v,
                    args: vec![class_v],
                },
                BackendTy::Dynamic(DynReason::Runtime),
            );
            let isnull = self.emit(InstKind::IsNull { operand: result }, BackendTy::Bool);
            class_v = self.select_value(
                isnull,
                class_v,
                result,
                BackendTy::Dynamic(DynReason::Runtime),
            )?;
        }
        Ok(class_v)
    }

    pub(super) fn build_class_accessors(
        &mut self,
        def: &TirClassDef,
        class_v: Value,
    ) -> Result<()> {
        for a in &def.accessors {
            let mut av = self.emit(
                InstKind::MakeClosure {
                    func: a.func.0,
                    upvalues_src: vec![],
                },
                BackendTy::Dynamic(DynReason::Runtime),
            );
            for deco in a.decorators.iter().rev() {
                av = self.apply_member_decorator(av, deco, &a.key, a.is_static, false)?;
            }
            self.emit_effect(InstKind::DefineAccessor {
                class: class_v,
                name: a.key.clone(),
                accessor: av,
                is_getter: a.is_getter,
                is_static: a.is_static,
            });
        }
        Ok(())
    }

    pub(super) fn build_property_decorators(
        &mut self,
        def: &TirClassDef,
        class_v: Value,
    ) -> Result<()> {
        for prop in &def.property_decorators {
            for deco in prop.decorators.iter().rev() {
                let deco_v = self.lower_expr(deco)?;
                let n = self.emit(InstKind::ConstStr(prop.key.clone()), BackendTy::Str);
                let st = self.emit(InstKind::ConstBool(prop.is_static), BackendTy::Bool);
                let pv = self.emit(InstKind::ConstBool(false), BackendTy::Bool);
                let ctx = self.emit(
                    InstKind::BuildObject {
                        pairs: vec![
                            (Arc::from(varn_core::MemberKey::Name.as_str()), n),
                            (Arc::from(varn_core::MemberKey::IsStatic.as_str()), st),
                            (Arc::from(varn_core::MemberKey::IsPrivate.as_str()), pv),
                        ],
                    },
                    BackendTy::Dynamic(DynReason::Runtime),
                );
                self.emit(
                    InstKind::Call {
                        callee: deco_v,
                        args: vec![class_v, ctx],
                    },
                    BackendTy::Dynamic(DynReason::Runtime),
                );
            }
        }
        Ok(())
    }
}
