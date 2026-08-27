//! Declarations that shape a module's surface: namespaces, extensions, imports
//! and exports.

use std::rc::Rc;
use varn_core::AnnKey;

use varn_core::ast::decl::{
    ExportDecl, ExportDefaultDecl, ExtensionDecl, ExtensionMember, ImportDecl, ImportSpecifier,
    NamespaceDecl,
};
use varn_core::ast::{Decl, Param, Stmt, TypeNode};

use super::super::*;

impl<'a> Lowerer<'a> {
    pub(in crate::hir::lower) fn lower_namespace(
        &mut self,
        ns: &NamespaceDecl,
        scope: &mut Scope,
        out: &mut Vec<HirStmt>,
    ) -> R<()> {
        let is_global = scope.is_global();
        scope.push_block();
        let was_in_namespace = std::mem::replace(&mut self.in_namespace, true);
        let mut names = Vec::new();
        for member in &ns.body {
            let inner = match member {
                Decl::Export(ExportDecl::Decl { declaration, .. }) => declaration.as_ref(),
                other => other,
            };
            let bound = self.lower_decl_inline(inner, scope, out)?;
            if matches!(
                inner,
                Decl::Function(_)
                    | Decl::Class(_)
                    | Decl::Variable(_)
                    | Decl::Namespace(_)
                    | Decl::Enum(_)
                    | Decl::SumType(_)
            ) {
                names.extend(bound);
            }
        }
        let properties = names
            .into_iter()
            .map(|name| {
                let binding = scope
                    .resolve_in_current_frame(&name)
                    .unwrap_or_else(|| self.global_binding(name.clone()));
                let value = HirExpr::Var(binding);
                HirObjectProp::Property {
                    key: HirPropKey::Static(name),
                    value,
                }
            })
            .collect();
        scope.pop_block();
        self.in_namespace = was_in_namespace;
        let value = HirExpr::Object { properties };
        if is_global {
            let target = self.global_binding(ns.id.clone());
            out.push(HirStmt::Assign { target, value });
        } else {
            let local = scope.alloc_local(ns.id.clone());
            out.push(HirStmt::Let {
                local,
                value,
                ty: HirType::Dynamic,
            });
        }
        Ok(())
    }

    pub(in crate::hir::lower) fn lower_extension(
        &mut self,
        ext: &ExtensionDecl,
        scope: &mut Scope,
        out: &mut Vec<HirStmt>,
    ) -> R<()> {
        let ty = extension_type_name(&ext.target);
        for member in &ext.members {
            match member {
                ExtensionMember::Method(m) => {
                    let mangled: Rc<str> = Rc::from(format!("__ext_{}_{}", ty, m.id));
                    self.push_global_closure(
                        out,
                        mangled,
                        &m.params,
                        &m.body,
                        m.modifiers.is_async,
                        m.modifiers.is_generator,
                        scope,
                    )?;
                }
                ExtensionMember::Getter { key, body, .. } => {
                    let mangled: Rc<str> = Rc::from(format!("__extget_{ty}_{key}"));
                    self.push_global_closure(out, mangled, &[], body, false, false, scope)?;
                }
                ExtensionMember::Setter {
                    key, param, body, ..
                } => {
                    let mangled: Rc<str> = Rc::from(format!("__extset_{ty}_{key}"));
                    self.push_global_closure(
                        out,
                        mangled,
                        std::slice::from_ref(param),
                        body,
                        false,
                        false,
                        scope,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn push_global_closure(
        &mut self,
        out: &mut Vec<HirStmt>,
        name: Rc<str>,
        params: &[Param],
        body: &Stmt,
        is_async: bool,
        is_generator: bool,
        scope: &mut Scope,
    ) -> R<()> {
        let return_ty = self.value_ty(AnnKey::decl(body.range.start.offset));
        let (func, upvalues) = self.lower_function_like(
            name.clone(),
            params,
            is_async,
            is_generator,
            false,
            true,
            body.range.start.line,
            BodyRef::Block(body),
            &[],
            return_ty,
            scope,
        )?;
        let target = self.global_binding(name);
        out.push(HirStmt::Assign {
            target,
            value: HirExpr::Closure {
                func: Box::new(func),
                upvalues,
            },
        });
        Ok(())
    }

    pub(in crate::hir::lower) fn lower_import(&self, decl: &ImportDecl) -> R<HirStmt> {
        let mut specs = Vec::new();
        for spec in &decl.specifiers {
            let (local, kind, off) = match spec {
                ImportSpecifier::Default { local, range } => {
                    (local.clone(), HirImportKind::Default, range.start.offset)
                }
                ImportSpecifier::Named {
                    local,
                    imported,
                    range,
                } => (
                    local.clone(),
                    HirImportKind::Named(imported.clone()),
                    range.start.offset,
                ),
                ImportSpecifier::Namespace { local, range } => {
                    (local.clone(), HirImportKind::Namespace, range.start.offset)
                }
            };
            let slot = self.ann.get_slot_idx(AnnKey::decl(off)).map(|s| s as u16);
            specs.push(HirImportSpec { local, kind, slot });
        }
        Ok(HirStmt::Import {
            source: decl.source.clone(),
            is_type: decl.is_type,
            specs,
        })
    }

    pub(in crate::hir::lower) fn lower_export(
        &mut self,
        decl: &ExportDecl,
        scope: &mut Scope,
        out: &mut Vec<HirStmt>,
    ) -> R<()> {
        match decl {
            ExportDecl::Decl { declaration, .. } => {
                let off = declaration.range().start.offset;
                let names = self.lower_decl_inline(declaration, scope, out)?;
                for name in names {
                    if let Some(slot) = self.export_slot(&name, off) {
                        out.push(HirStmt::StoreExport { name, slot });
                    }
                }
                Ok(())
            }
            ExportDecl::Default {
                declaration, range, ..
            } => match declaration.as_ref() {
                ExportDefaultDecl::Function(f) => {
                    if !f.modifiers.is_declare {
                        let local = scope.alloc_local(f.id.clone());
                        let (func, upvalues) = self.lower_function(f, scope)?;
                        out.push(HirStmt::Let {
                            local,
                            value: HirExpr::Closure {
                                func: Box::new(func),
                                upvalues,
                            },
                            ty: HirType::Ref,
                        });
                        let slot = self
                            .export_names
                            .iter()
                            .position(|n| &**n == "default")
                            .or_else(|| self.ann.get_slot_idx(AnnKey::decl(range.start.offset)))
                            .map(|p| p as u16);
                        if let Some(slot) = slot {
                            out.push(HirStmt::StoreExport {
                                name: f.id.clone(),
                                slot,
                            });
                        }
                    }
                    Ok(())
                }
                ExportDefaultDecl::Class(cl) => {
                    if !cl.modifiers.is_declare {
                        let name = cl.id.clone().unwrap_or_else(|| Rc::from("anonymous"));
                        let hir_class = self.lower_class(cl, scope)?;
                        let value = HirExpr::Class(Box::new(hir_class));
                        if scope.is_global() {
                            let target = self.global_binding(name.clone());
                            out.push(HirStmt::Assign { target, value });
                        } else {
                            let local = scope.alloc_local(name.clone());
                            out.push(HirStmt::Let {
                                local,
                                value,
                                ty: HirType::Ref,
                            });
                        }
                        let slot = self
                            .export_names
                            .iter()
                            .position(|n| &**n == "default")
                            .or_else(|| self.ann.get_slot_idx(AnnKey::decl(range.start.offset)))
                            .map(|p| p as u16);
                        if let Some(slot) = slot {
                            out.push(HirStmt::StoreExport { name, slot });
                        }
                    }
                    Ok(())
                }
                ExportDefaultDecl::Expr(e) => {
                    let value = self.lower_expr(e, scope)?;
                    let slot = self
                        .export_names
                        .iter()
                        .position(|n| &**n == "default")
                        .or_else(|| self.ann.get_slot_idx(AnnKey::decl(range.start.offset)))
                        .map(|p| p as u16);
                    out.push(HirStmt::ExportDefaultExpr { value, slot });
                    Ok(())
                }
            },
            ExportDecl::Named {
                specifiers,
                source,
                range: _,
                ..
            } => {
                let mut specs = Vec::new();
                for spec in specifiers {
                    let binding = self.resolve(&spec.local, scope);
                    let local_slot = self
                        .ann
                        .get_slot_idx(AnnKey::decl(spec.range.start.offset))
                        .map(|s| s as u16);
                    let exported_slot = self
                        .export_names
                        .iter()
                        .position(|n| &**n == &*spec.exported)
                        .or_else(|| self.ann.get_slot_idx(AnnKey::decl(spec.range.start.offset)))
                        .map(|s| s as u16);
                    specs.push(HirExportSpec {
                        binding,
                        local: spec.local.clone(),
                        exported: spec.exported.clone(),
                        local_slot,
                        exported_slot,
                    });
                }
                out.push(HirStmt::ExportNamed {
                    specifiers: specs,
                    source: source.clone(),
                });
                Ok(())
            }
            ExportDecl::All {
                source,
                alias,
                range,
                ..
            } => {
                let slot = alias.as_ref().and_then(|alias_name| {
                    self.export_names
                        .iter()
                        .position(|n| **n == **alias_name)
                        .or_else(|| self.ann.get_slot_idx(AnnKey::decl(range.start.offset)))
                        .map(|s| s as u16)
                });
                out.push(HirStmt::ExportAll {
                    source: source.clone(),
                    alias: alias.clone(),
                    slot,
                });
                Ok(())
            }
        }
    }

    fn export_slot(&self, name: &str, offset: u32) -> Option<u16> {
        self.export_names
            .iter()
            .position(|n| **n == *name)
            .map(|p| p as u16)
            .or_else(|| {
                self.ann
                    .get_slot_idx(AnnKey::decl(offset))
                    .map(|s| s as u16)
            })
    }
}

fn extension_type_name(target: &TypeNode) -> String {
    use varn_core::{IntrinsicType, TypeKind, TypeTag};
    match &target.kind {
        TypeKind::Intrinsic(TypeTag::Int) => IntrinsicType::Int.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Float) => IntrinsicType::Float.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Str) => IntrinsicType::Str.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Bool) => IntrinsicType::Bool.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Char) => IntrinsicType::Char.as_str().to_owned(),
        TypeKind::Named(n, _) => n.clone(),
        TypeKind::Generic(n, _, _) => n.clone(),
        TypeKind::Intrinsic(TypeTag::Array) => IntrinsicType::Array.as_str().to_owned(),
        _ => IntrinsicType::Dynamic.as_str().to_owned(),
    }
}
