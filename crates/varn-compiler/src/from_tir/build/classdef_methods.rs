use super::context::{Builder, Result};
use crate::hir::HirType;
use crate::ssa::ir::{InstKind, Value};
use std::sync::Arc;
use varn_tir::TirClassDef;

impl<'m> Builder<'m> {
    pub(super) fn build_class_methods(&mut self, def: &TirClassDef, class_v: Value) -> Result<()> {
        for m in &def.methods {
            let mut mv = self.emit(
                InstKind::MakeClosure {
                    func: m.func.0,
                    upvalues_src: vec![],
                },
                HirType::Ref,
            );
            for deco in m.decorators.iter().rev() {
                let deco_v = self.lower_expr(deco)?;
                let n = self.emit(InstKind::ConstStr(m.key.clone()), HirType::Str);
                let k = self.emit(InstKind::ConstStr(Arc::from("method")), HirType::Str);
                let st = self.emit(InstKind::ConstBool(m.is_static), HirType::Bool);
                let pv = self.emit(InstKind::ConstBool(m.is_private), HirType::Bool);
                let ctx = self.emit(
                    InstKind::BuildObject {
                        pairs: vec![
                            (Arc::from("name"), n),
                            (Arc::from("kind"), k),
                            (Arc::from("isStatic"), st),
                            (Arc::from("isPrivate"), pv),
                        ],
                    },
                    HirType::Ref,
                );
                let result = self.emit(
                    InstKind::Call {
                        callee: deco_v,
                        args: vec![mv, ctx],
                    },
                    HirType::Ref,
                );
                let isnull = self.emit(InstKind::IsNull { operand: result }, HirType::Bool);
                mv = self.select_value(isnull, mv, result, HirType::Ref)?;
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
                HirType::Ref,
            );
            let isnull = self.emit(InstKind::IsNull { operand: result }, HirType::Bool);
            class_v = self.select_value(isnull, class_v, result, HirType::Ref)?;
        }
        Ok(class_v)
    }

    pub(super) fn build_class_accessors(&mut self, def: &TirClassDef, class_v: Value) {
        for a in &def.accessors {
            let av = self.emit(
                InstKind::MakeClosure {
                    func: a.func.0,
                    upvalues_src: vec![],
                },
                HirType::Ref,
            );
            self.emit_effect(InstKind::DefineAccessor {
                class: class_v,
                name: a.key.clone(),
                accessor: av,
                is_getter: a.is_getter,
                is_static: a.is_static,
            });
        }
    }
}
