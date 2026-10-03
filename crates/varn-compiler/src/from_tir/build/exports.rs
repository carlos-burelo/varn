use super::context::Builder;
use crate::hir::HirType;
use crate::ssa::ir::InstKind;
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
                let val = match &spec.kind {
                    varn_tir::TirImportKind::Namespace => mod_v,
                    varn_tir::TirImportKind::Default => self.emit(
                        InstKind::GetProperty {
                            object: mod_v,
                            name: Arc::from("default"),
                        },
                        HirType::Dynamic,
                    ),
                    varn_tir::TirImportKind::Named(n) => self.emit(
                        InstKind::GetProperty {
                            object: mod_v,
                            name: n.clone(),
                        },
                        HirType::Dynamic,
                    ),
                };
                let store = self.global_store(&spec.local, val);
                self.emit_effect(store);
            }
        }
    }
}
