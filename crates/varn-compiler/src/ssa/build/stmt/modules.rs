//! Import and export statements: the module-slot stores that link a module to
//! its dependencies and its exported surface.

use std::rc::Rc;

use crate::hir::{HirExportSpec, HirExpr, HirImportKind, HirImportSpec, HirType};
use crate::ssa::ir::{InstKind};

use super::super::{Builder, Result};


impl Builder {
    pub(in crate::ssa::build) fn lower_import(
        &mut self,
        source: &Rc<str>,
        is_type: bool,
        specs: &[HirImportSpec],
    ) -> Result<()> {
        let mod_val = self.emit(
            InstKind::LoadModule {
                source: source.clone(),
            },
            HirType::Ref,
        );
        if !is_type {
            for spec in specs {
                let name = if let Some(ref src) = self.source_file {
                    Rc::from(format!("{}::{}", src.replace('\\', "/"), spec.local))
                } else {
                    spec.local.clone()
                };
                match &spec.kind {
                    HirImportKind::Namespace => {
                        self.emit_effect(InstKind::StoreGlobal {
                            name,
                            value: mod_val,
                        });
                    }
                    HirImportKind::Default | HirImportKind::Named(_) => {
                        let val = if let Some(slot) = spec.slot {
                            self.emit(
                                InstKind::ModuleSlot {
                                    object: mod_val,
                                    slot,
                                },
                                HirType::Dynamic,
                            )
                        } else {
                            let key = match &spec.kind {
                                HirImportKind::Named(n) => n.clone(),
                                _ => Rc::from("default"),
                            };
                            self.emit(
                                InstKind::GetProperty {
                                    object: mod_val,
                                    name: key,
                                },
                                HirType::Dynamic,
                            )
                        };
                        self.emit_effect(InstKind::StoreGlobal { name, value: val });
                    }
                }
            }
        }
        Ok(())
    }

    pub(in crate::ssa::build) fn lower_store_export(&mut self, name: &Rc<str>, slot: u16) -> Result<()> {
        let qualified_name = if let Some(ref src) = self.source_file {
            Rc::from(format!("{}::{}", src.replace('\\', "/"), name))
        } else {
            name.clone()
        };
        let val = self.emit(InstKind::LoadGlobal(qualified_name), HirType::Dynamic);
        self.emit_effect(InstKind::StoreModuleSlot { value: val, slot });
        Ok(())
    }

    pub(in crate::ssa::build) fn lower_export_named(
        &mut self,
        specifiers: &[HirExportSpec],
        source: &Option<Rc<str>>,
    ) -> Result<()> {
        match source {
            Some(src) => {
                let mod_val = self.emit(
                    InstKind::LoadModule {
                        source: src.clone(),
                    },
                    HirType::Ref,
                );
                for spec in specifiers {
                    let val = if let Some(imported_slot) = spec.local_slot {
                        self.emit(
                            InstKind::ModuleSlot {
                                object: mod_val,
                                slot: imported_slot,
                            },
                            HirType::Dynamic,
                        )
                    } else {
                        self.emit(
                            InstKind::GetProperty {
                                object: mod_val,
                                name: spec.local.clone(),
                            },
                            HirType::Dynamic,
                        )
                    };
                    if let Some(exported_slot) = spec.exported_slot {
                        self.emit_effect(InstKind::StoreModuleSlot {
                            value: val,
                            slot: exported_slot,
                        });
                    }
                }
            }
            None => {
                for spec in specifiers {
                    let val = self.load_binding(&spec.binding)?;
                    if let Some(exported_slot) = spec.exported_slot {
                        self.emit_effect(InstKind::StoreModuleSlot {
                            value: val,
                            slot: exported_slot,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    pub(in crate::ssa::build) fn lower_export_all(
        &mut self,
        source: &Rc<str>,
        alias: &Option<Rc<str>>,
        slot: &Option<u16>,
    ) -> Result<()> {
        let mod_val = self.emit(
            InstKind::LoadModule {
                source: source.clone(),
            },
            HirType::Ref,
        );
        if alias.is_some() {
            if let Some(slot_idx) = slot {
                self.emit_effect(InstKind::StoreModuleSlot {
                    value: mod_val,
                    slot: *slot_idx,
                });
            }
        }
        Ok(())
    }

    pub(in crate::ssa::build) fn lower_export_default_expr(&mut self, value: &HirExpr, slot: &Option<u16>) -> Result<()> {
        let val = self.lower_expr(value)?;
        if let Some(slot_idx) = slot {
            self.emit_effect(InstKind::StoreModuleSlot {
                value: val,
                slot: *slot_idx,
            });
        }
        Ok(())
    }

}
