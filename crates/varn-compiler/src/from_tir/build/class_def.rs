use super::context::{Builder, Result};
use crate::hir::HirType;
use crate::ssa::ir::{InstKind, Value};
use std::sync::Arc;
use varn_tir::{BackendTy, TirClassDef};

impl<'m> Builder<'m> {
    pub(super) fn build_class_def(&mut self, def: &TirClassDef) -> Result<()> {
        for s in &def.prelude {
            self.lower_stmt(s)?;
        }

        let super_v = match &def.super_class {
            Some(e) => Some(self.lower_expr(e)?),
            None => None,
        };
        let mut class_v = self.emit(
            InstKind::MakeClass {
                name: def.name.clone(),
                super_class: super_v,
            },
            HirType::Ref,
        );

        for v in &def.variants {
            let variant_v = self.emit(
                InstKind::MakeEnumVariant {
                    tag: v.tag,
                    meta: v.meta.clone(),
                },
                HirType::Ref,
            );
            self.emit_effect(InstKind::DefineStatic {
                class: class_v,
                name: v.name.clone(),
                value: variant_v,
            });
        }

        let inherited = def
            .parent
            .or_else(|| {
                def.class_id
                    .and_then(|c| self.tir.class(c))
                    .and_then(|ci| ci.parent)
            })
            .and_then(|p| self.tir.class(p))
            .map(|p| p.fields.len())
            .or_else(|| (def.super_class.is_some() && def.parent.is_none()).then_some(3))
            .unwrap_or(0);
        let fields: Vec<(Arc<str>, BackendTy)> = def
            .class_id
            .and_then(|cid| self.tir.class(cid))
            .map(|ci| {
                ci.fields
                    .iter()
                    .skip(inherited)
                    .map(|f| (f.name.clone(), f.ty))
                    .collect()
            })
            .unwrap_or_default();
        for (fname, fty) in fields {
            self.emit_effect(InstKind::DeclareField {
                class: class_v,
                name: fname,
                tag: super::ops::field_kind(fty, &self.tir.types),
            });
        }

        for (sname, init) in &def.statics {
            let val = match init {
                Some(e) => self.lower_expr(e)?,
                None => self.emit(InstKind::ConstNull, HirType::Ref),
            };
            self.emit_effect(InstKind::DefineStatic {
                class: class_v,
                name: sname.clone(),
                value: val,
            });
        }

        self.build_class_methods(def, class_v)?;
        self.build_class_accessors(def, class_v);
        class_v = self.build_class_decorators(def, class_v)?;

        let store = self.global_store(&def.name, class_v);
        self.emit_effect(store);

        for blk in &def.static_blocks {
            let fv = self.emit(
                InstKind::MakeClosure {
                    func: blk.0,
                    upvalues_src: vec![],
                },
                HirType::Ref,
            );
            self.emit(
                InstKind::Call {
                    callee: fv,
                    args: vec![],
                },
                HirType::Dynamic,
            );
        }

        for v in &def.variants {
            if v.const_args.is_empty() {
                continue;
            }
            let recv = self.emit(
                InstKind::GetProperty {
                    object: class_v,
                    name: v.name.clone(),
                },
                HirType::Ref,
            );
            let mut args = Vec::with_capacity(v.const_args.len());
            for a in &v.const_args {
                args.push(self.lower_expr(a)?);
            }
            self.emit(
                InstKind::MethodCall {
                    recv,
                    name: Arc::from("constructor"),
                    args,
                },
                HirType::Dynamic,
            );
        }

        Ok(())
    }

    pub(super) fn select_value(
        &mut self,
        cond: Value,
        then_v: Value,
        else_v: Value,
        ty: HirType,
    ) -> Result<Value> {
        let then_blk = self.new_block();
        let else_blk = self.new_block();
        let join = self.new_block();
        let then_v = self.coerce(then_v, ty);
        let else_v = self.coerce(else_v, ty);
        let from = self.current;
        self.set_term(crate::ssa::ir::Terminator::Branch {
            cond,
            then_blk,
            then_args: vec![then_v],
            else_blk,
            else_args: vec![else_v],
        });
        self.add_pred(then_blk, from);
        self.add_pred(else_blk, from);
        self.seal_block(then_blk);
        self.seal_block(else_blk);
        let tp = self.add_block_param(then_blk, ty);
        let ep = self.add_block_param(else_blk, ty);
        let phi = self.add_block_param(join, ty);
        self.current = then_blk;
        self.set_term(crate::ssa::ir::Terminator::Jump {
            target: join,
            args: vec![tp],
        });
        self.add_pred(join, then_blk);
        self.current = else_blk;
        self.set_term(crate::ssa::ir::Terminator::Jump {
            target: join,
            args: vec![ep],
        });
        self.add_pred(join, else_blk);
        self.seal_block(join);
        self.current = join;
        Ok(phi)
    }
}
