use super::context::Builder;
use crate::hir::HirBinOp;
use crate::ssa::ir::{InstKind, Terminator, Value};
use std::sync::Arc;
use varn_tir::{BackendTy, DynReason};

impl<'m> Builder<'m> {
    pub(super) fn build_exports(&mut self, export_slots: &[Arc<str>]) {
        let slot_of = |name: &str| export_slots.iter().position(|n| n.as_ref() == name);
        for exp in &self.tir.exports {
            let Some(slot) = slot_of(&exp.exported) else {
                continue;
            };
            let value = match &exp.reexport_from {
                None => self.emit(
                    self.global_load(&exp.local),
                    BackendTy::Dynamic(DynReason::Boxed),
                ),
                Some(src) => {
                    let m = self.emit(
                        InstKind::LoadModule {
                            source: src.clone(),
                        },
                        BackendTy::Dynamic(DynReason::Runtime),
                    );
                    if exp.namespace {
                        m
                    } else {
                        self.emit(
                            InstKind::GetProperty {
                                object: m,
                                name: exp.local.clone(),
                            },
                            BackendTy::Dynamic(DynReason::Boxed),
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
                BackendTy::Dynamic(DynReason::Runtime),
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
                            BackendTy::Dynamic(DynReason::Boxed),
                        );
                        let name_v = self.emit(
                            InstKind::GetProperty {
                                object: mod_v,
                                name: name.clone(),
                            },
                            BackendTy::Dynamic(DynReason::Boxed),
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
                            BackendTy::Dynamic(DynReason::Boxed),
                        )
                    }
                };
                let store = self.global_store(&spec.local, val);
                self.emit_effect(store);
                self.bind_imported_variants(&spec.local, val);
            }
        }
    }

    fn bind_imported_variants(&mut self, local: &Arc<str>, enum_v: Value) {
        let slots: Vec<(Arc<str>, u32)> = self
            .tir
            .enums
            .iter()
            .filter(|info| info.binding.as_ref() == Some(local))
            .flat_map(|info| {
                info.variants.iter().filter_map(|v| {
                    let slot = self.gslot(&info.variant_global(&v.name)?)?;
                    Some((v.name.clone(), slot))
                })
            })
            .collect();
        if slots.is_empty() {
            return;
        }
        let absent = self.emit(InstKind::IsNull { operand: enum_v }, BackendTy::Bool);
        let bind_blk = self.new_block();
        let join_blk = self.new_block();
        let from = self.current;
        self.set_term(Terminator::Branch {
            cond: absent,
            then_blk: join_blk,
            then_args: vec![],
            else_blk: bind_blk,
            else_args: vec![],
        });
        self.add_pred(bind_blk, from);
        self.seal_block(bind_blk);
        self.current = bind_blk;
        for (name, slot) in slots {
            let value = self.emit(
                InstKind::GetProperty {
                    object: enum_v,
                    name,
                },
                BackendTy::Dynamic(DynReason::Runtime),
            );
            self.emit_effect(InstKind::StoreGlobalIdx { slot, value });
        }
        let end = self.current;
        self.set_term(Terminator::Jump {
            target: join_blk,
            args: vec![],
        });
        self.add_pred(join_blk, from);
        self.add_pred(join_blk, end);
        self.seal_block(join_blk);
        self.current = join_blk;
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
                ty: BackendTy::Dynamic(DynReason::Boxed),
            },
            BackendTy::Bool,
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
            BackendTy::Str,
        );
        self.set_term(Terminator::Throw(msg));
        self.current = join_blk;
    }
}
