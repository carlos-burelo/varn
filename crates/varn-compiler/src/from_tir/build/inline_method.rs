use super::context::{Builder, Result};
use crate::ssa::ir::Value;
use varn_tir::{BackendTy, ClassId, FnId, TirArg, TirClassDef, TirExpr};

impl<'m> Builder<'m> {
    pub(super) fn try_inline_method_call(
        &mut self,
        recv: &TirExpr,
        receiver: Value,
        name: &str,
        args: &[TirArg],
        call_ty: BackendTy,
    ) -> Result<Option<Value>> {
        let BackendTy::Class(class) = recv.ty else {
            return Ok(None);
        };
        if self.inlining.len() >= 4 {
            return Ok(None);
        }
        let Some(func) = self.devirtualized_method(class, name) else {
            return Ok(None);
        };
        if self.inlining.iter().any(|frame| frame.func == func) {
            return Ok(None);
        }
        self.try_inline_body_call(func, args, call_ty, false, Some(receiver))
    }

    fn devirtualized_method(&self, class: ClassId, name: &str) -> Option<FnId> {
        let def = self.class_def_of(class)?;
        if !def.decorators.is_empty() || self.is_exported_class(def) {
            return None;
        }
        let mut owner = def;
        let method = loop {
            if let Some(m) = owner
                .methods
                .iter()
                .find(|m| !m.is_static && m.key.as_ref() == name)
            {
                break m;
            }
            owner = self.class_def_of(owner.parent?)?;
            if !owner.decorators.is_empty() || self.is_exported_class(owner) {
                return None;
            }
        };
        if !method.decorators.is_empty() || self.subclass_overrides(class, name) {
            return None;
        }
        Some(method.func)
    }

    fn class_def_of(&self, class: ClassId) -> Option<&'m TirClassDef> {
        self.tir
            .class_defs
            .iter()
            .find(|d| d.class_id == Some(class))
    }

    fn is_exported_class(&self, def: &TirClassDef) -> bool {
        self.tir.exports.iter().any(|e| e.local == def.name)
    }

    fn subclass_overrides(&self, class: ClassId, name: &str) -> bool {
        self.tir.class_defs.iter().any(|d| {
            d.class_id != Some(class)
                && d.methods
                    .iter()
                    .any(|m| !m.is_static && m.key.as_ref() == name)
                && self.descends_from(d, class)
        })
    }

    fn descends_from(&self, def: &TirClassDef, class: ClassId) -> bool {
        let mut cur = def.parent;
        for _ in 0..=self.tir.class_defs.len() {
            let Some(p) = cur else {
                return false;
            };
            if p == class {
                return true;
            }
            cur = self.class_def_of(p).and_then(|d| d.parent);
        }
        false
    }
}
