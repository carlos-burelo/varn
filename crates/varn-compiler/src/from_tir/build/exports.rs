use super::context::Builder;
use crate::hir::{HirBinOp, HirType};
use crate::ssa::ir::{InstKind, Terminator, Value};
use std::sync::Arc;

impl<'m> Builder<'m> {
    pub(super) fn build_exports(&mut self, export_slots: &[Arc<str>]) {
        let slot_of = |name: &str| export_slots.iter().position(|n| n.as_ref() == name);
        for exp in &self.tir.exports {
            let Some(slot) = slot_of(&exp.exported) else {
                continue;
            };
            let value = match &exp.reexport_from {
                None => self.emit(self.global_load(&exp.local), HirType::Dynamic),
                Some(src) => {
                    let m = self.emit(
                        InstKind::LoadModule {
                            source: src.clone(),
                        },
                        HirType::Ref,
                    );
                    if exp.namespace {
                        m
                    } else {
                        self.emit(
                            InstKind::GetProperty {
                                object: m,
                                name: exp.local.clone(),
                            },
                            HirType::Dynamic,
                        )
                    }
                }
            };
            self.emit_effect(InstKind::StoreModuleSlot {
                value,
                slot: slot as u16,
            });
        }
    }

    pub(super) fn build_imports(&mut self) {
        for imp in &self.tir.imports {
            if imp.is_type_only {
                continue;
            }
            let mod_v = self.emit(
                InstKind::LoadModule {
                    source: imp.source.clone(),
                },
                HirType::Ref,
            );
            for spec in &imp.specs {
                let imported_name: Option<Arc<str>> = match &spec.kind {
                    varn_tir::TirImportKind::Namespace => None,
                    varn_tir::TirImportKind::Default => Some(Arc::from("default")),
                    varn_tir::TirImportKind::Named(n) => Some(n.clone()),
                };
                let val = match (imported_name, spec.slot) {
                    (Some(name), Some(slot)) => {
                        let slot_v = self.emit(
                            InstKind::ModuleSlot {
                                object: mod_v,
                                slot,
                            },
                            HirType::Dynamic,
                        );
                        let name_v = self.emit(
                            InstKind::GetProperty {
                                object: mod_v,
                                name: name.clone(),
                            },
                            HirType::Dynamic,
                        );
                        self.emit_slot_guard(&imp.source, &name, slot, slot_v, name_v);
                        slot_v
                    }
                    _ => {
                        let name: Arc<str> = match &spec.kind {
                            varn_tir::TirImportKind::Namespace => {
                                let store = self.global_store(&spec.local, mod_v);
                                self.emit_effect(store);
                                continue;
                            }
                            varn_tir::TirImportKind::Default => Arc::from("default"),
                            varn_tir::TirImportKind::Named(n) => n.clone(),
                        };
                        self.emit(
                            InstKind::GetProperty {
                                object: mod_v,
                                name,
                            },
                            HirType::Dynamic,
                        )
                    }
                };
                let store = self.global_store(&spec.local, val);
                self.emit_effect(store);
            }
        }
    }

    fn emit_slot_guard(
        &mut self,
        source: &Arc<str>,
        name: &Arc<str>,
        slot: u16,
        slot_v: Value,
        name_v: Value,
    ) {
        let cond = self.emit(
            InstKind::Binary {
                op: HirBinOp::Eq,
                lhs: slot_v,
                rhs: name_v,
                ty: HirType::Dynamic,
            },
            HirType::Bool,
        );
        let throw_blk = self.new_block();
        let join_blk = self.new_block();
        let from = self.current;
        self.set_term(Terminator::Branch {
            cond,
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
        let msg = self.emit(
            InstKind::ConstStr(Arc::from(format!(
                "E_SLOT_MISMATCH: '{name}' from '{source}' is not at slot {slot} anymore"
            ))),
            HirType::Str,
        );
        self.set_term(Terminator::Throw(msg));
        self.current = join_blk;
    }
}
