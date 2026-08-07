use super::exprs::{annotate_expr, get_expr_type, record_cg_ty_at};
use super::AnnotateCtx;
use crate::binder::resolve_type_node;
use crate::symbol::SymbolKind;
use crate::types::{Type, TypeContext};
use varn_core::ast::{Decl, ExportDecl, ForInit, ImportSpecifier, Stmt, StmtKind};
use varn_core::TypeAnnotations;

pub(crate) fn annotate_stmt(stmt: &Stmt, ann: &mut TypeAnnotations, ctx: &mut AnnotateCtx) {
    match &stmt.kind {
        StmtKind::Expr { expression } => annotate_expr(expression, ann, ctx),
        StmtKind::Return {
            argument: Some(arg),
        } => annotate_expr(arg, ann, ctx),
        StmtKind::Return { .. } => {}
        StmtKind::Block { stmts } => {
            let old_locals = ctx.locals.clone();
            let old_evolved = ctx.evolved_locals.clone();
            for s in stmts {
                annotate_stmt(s, ann, ctx);
            }
            ctx.locals = old_locals;
            ctx.evolved_locals = old_evolved;
        }
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            annotate_expr(test, ann, ctx);
            annotate_stmt(consequent, ann, ctx);
            if let Some(alt) = alternate {
                annotate_stmt(alt, ann, ctx);
            }
        }
        StmtKind::While { test, body } | StmtKind::DoWhile { test, body } => {
            annotate_expr(test, ann, ctx);
            annotate_stmt(body, ann, ctx);
        }
        StmtKind::For {
            init,
            test,
            update,
            body,
        } => {
            let old_locals = ctx.locals.clone();
            let old_evolved = ctx.evolved_locals.clone();
            if let Some(init_box) = init {
                match init_box.as_ref() {
                    ForInit::Expr(e) => annotate_expr(e, ann, ctx),
                    ForInit::Var { declarators, .. } => {
                        for d in declarators {
                            if let Some(init_expr) = &d.init {
                                annotate_expr(init_expr, ann, ctx);
                            }
                            let name_opt = match &d.id {
                                varn_core::ast::Pattern::Identifier { name, .. } => {
                                    Some(name.clone())
                                }
                                _ => None,
                            };
                            if let Some(name) = name_opt {
                                let type_ann = d.type_ann.as_ref().or(match &d.id {
                                    varn_core::ast::Pattern::Identifier { type_ann, .. } => {
                                        type_ann.as_ref()
                                    }
                                    _ => None,
                                });
                                ctx.evolved_locals.remove(name.as_ref());
                                if let Some(ann_node) = type_ann {
                                    let ty = resolve_type_node(ann_node, Some(ctx.bind));
                                    ctx.locals.insert(name, ty);
                                } else if let Some(init_expr) = &d.init {
                                    let ty = get_expr_type(init_expr, ctx);
                                    ctx.locals.insert(name, ty);
                                }
                            } else {
                                ungovern_pattern_names(&d.id, ctx);
                            }
                        }
                    }
                }
            }
            if let Some(t) = test {
                annotate_expr(t, ann, ctx);
            }
            if let Some(u) = update {
                annotate_expr(u, ann, ctx);
            }
            annotate_stmt(body, ann, ctx);
            ctx.locals = old_locals;
            ctx.evolved_locals = old_evolved;
        }
        StmtKind::Decl(decl) => annotate_decl(decl, ann, ctx),
        _ => {}
    }
}

pub(crate) fn annotate_decl(decl: &Decl, ann: &mut TypeAnnotations, ctx: &mut AnnotateCtx) {
    match decl {
        Decl::Variable(v) => {
            for d in &v.declarators {
                if let Some(init) = &d.init {
                    annotate_expr(init, ann, ctx);
                }
                let id_info = match &d.id {
                    varn_core::ast::Pattern::Identifier { name, range, .. } => {
                        Some((name.clone(), range.start.offset))
                    }
                    _ => None,
                };
                if let Some((name, id_offset)) = id_info {
                    let type_ann = d.type_ann.as_ref().or(match &d.id {
                        varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                        _ => None,
                    });
                    if let Some(ann_node) = type_ann {
                        let ty = resolve_type_node(ann_node, Some(ctx.bind));
                        ctx.evolved_locals.remove(name.as_ref());
                        ctx.locals.insert(name, ty);
                    } else if let Some(evolved) =
                        ctx.bind.evolved_array_types.get(&id_offset).cloned()
                    {
                        ctx.evolved_locals.insert(name.clone());
                        ctx.locals.insert(name, evolved);
                    } else if let Some(init) = &d.init {
                        let ty = get_expr_type(init, ctx);
                        ctx.evolved_locals.remove(name.as_ref());
                        ctx.locals.insert(name, ty);
                    }
                } else {
                    ungovern_pattern_names(&d.id, ctx);
                }
            }
        }
        Decl::Function(f) => {
            if !f.modifiers.is_declare {
                if let Some(rt) = &f.return_type {
                    let ty = resolve_type_node(rt, Some(ctx.bind));
                    record_cg_ty_at(f.id_offset, &ty, ann, ctx);
                }
                let mut local_ctx = ctx.clone();
                local_ctx.evolved_locals.clear();
                for p in &f.params {
                    let name_opt = match &p.pattern {
                        varn_core::ast::Pattern::Identifier { name, .. } => Some(name.clone()),
                        _ => None,
                    };
                    if let Some(name) = name_opt {
                        let type_ann = p.type_ann.as_ref().or(match &p.pattern {
                            varn_core::ast::Pattern::Identifier { type_ann, .. } => {
                                type_ann.as_ref()
                            }
                            _ => None,
                        });
                        let ty = if let Some(ann_node) = type_ann {
                            resolve_type_node(ann_node, Some(ctx.bind))
                        } else {
                            Type::Dynamic
                        };
                        record_cg_ty_at(p.range.start.offset, &ty, ann, ctx);
                        local_ctx.locals.insert(name, ty);
                    }
                }
                annotate_stmt(&f.body, ann, &mut local_ctx);
            }
        }
        Decl::Import(i) => {
            for spec in &i.specifiers {
                let name = match spec {
                    ImportSpecifier::Default { local, .. } => local,
                    ImportSpecifier::Named { local, .. } => local,
                    ImportSpecifier::Namespace { local, .. } => local,
                };
                let scope = ctx.bind.scopes.get(ctx.bind.global_scope);
                if let Some(id) = scope.resolve(name, &ctx.bind.scopes) {
                    let sym = ctx.bind.arena.get(id);
                    if let Some(slot_idx) = sym.slot_idx {
                        ann.record_slot_idx(spec.range().start.offset, slot_idx);
                    }
                    if matches!(
                        sym.kind,
                        SymbolKind::Interface
                            | SymbolKind::TypeAlias
                            | SymbolKind::Struct
                            | SymbolKind::Extension
                    ) {
                        ann.record_type_only(spec.range().start.offset);
                    }
                }
            }
        }
        Decl::Export(e) => {
            let exports = if ctx.bind.source_file.starts_with("std:")
                || ctx.bind.source_file.starts_with("core:")
            {
                crate::module_resolver::resolve_stdlib_module_exports_ref(&ctx.bind.source_file)
            } else {
                crate::module_resolver::resolve_module_exports_ref(
                    &ctx.bind.source_file,
                    &mut vec![],
                )
            };
            match e {
                ExportDecl::Decl { declaration, .. } => {
                    annotate_decl(declaration, ann, ctx);
                    if let Some(name) = decl_primary_name(declaration) {
                        if let Some(sym) = exports.get(name.as_ref()) {
                            if let Some(slot_idx) = sym.slot_idx {
                                ann.record_slot_idx(declaration.range().start.offset, slot_idx);
                            }
                        }
                    }
                }
                ExportDecl::Default {
                    declaration, range, ..
                } => {
                    match declaration.as_ref() {
                        varn_core::ast::ExportDefaultDecl::Function(f) => {
                            annotate_decl(&Decl::Function(f.clone()), ann, ctx);
                        }
                        varn_core::ast::ExportDefaultDecl::Class(c) => {
                            annotate_decl(&Decl::Class(c.clone()), ann, ctx);
                        }
                        varn_core::ast::ExportDefaultDecl::Expr(ex) => {
                            annotate_expr(ex, ann, ctx);
                        }
                    }
                    if let Some(sym) = exports.get("default") {
                        if let Some(slot_idx) = sym.slot_idx {
                            ann.record_slot_idx(range.start.offset, slot_idx);
                        }
                    }
                }
                ExportDecl::Named { specifiers, .. } => {
                    for spec in specifiers {
                        if let Some(sym) = exports.get(&spec.exported.to_string()) {
                            if let Some(slot_idx) = sym.slot_idx {
                                ann.record_slot_idx(spec.range.start.offset, slot_idx);
                            }
                        }
                    }
                }
                ExportDecl::All { alias, range, .. } => {
                    if let Some(ns) = alias {
                        if let Some(sym) = exports.get(&ns.to_string()) {
                            if let Some(slot_idx) = sym.slot_idx {
                                ann.record_slot_idx(range.start.offset, slot_idx);
                            }
                        }
                    }
                }
            }
        }
        Decl::Class(c) => {
            let Some(class_name) = &c.id else {
                return;
            };
            let this_ty = Type::named_with_origin(
                class_name.clone(),
                ctx.bind.source_file().map(std::rc::Rc::from),
            );
            for member in &c.body {
                match member {
                    varn_core::ast::ClassMember::Constructor { params, body, .. } => {
                        annotate_method_body(params, body, Some(&this_ty), ann, ctx);
                    }
                    varn_core::ast::ClassMember::Method {
                        params,
                        body: Some(body),
                        modifiers,
                        ..
                    } => {
                        let this = (!modifiers.is_static).then_some(&this_ty);
                        annotate_method_body(params, body, this, ann, ctx);
                    }
                    varn_core::ast::ClassMember::Getter {
                        body: Some(body),
                        modifiers,
                        ..
                    } => {
                        let this = (!modifiers.is_static).then_some(&this_ty);
                        annotate_method_body(&[], body, this, ann, ctx);
                    }
                    varn_core::ast::ClassMember::Setter {
                        param,
                        body: Some(body),
                        modifiers,
                        ..
                    } => {
                        let this = (!modifiers.is_static).then_some(&this_ty);
                        annotate_method_body(std::slice::from_ref(param), body, this, ann, ctx);
                    }
                    varn_core::ast::ClassMember::Destructor { body, .. } => {
                        annotate_method_body(&[], body, Some(&this_ty), ann, ctx);
                    }
                    varn_core::ast::ClassMember::StaticBlock { body, .. } => {
                        annotate_method_body(&[], body, None, ann, ctx);
                    }
                    varn_core::ast::ClassMember::Property {
                        init: Some(init), ..
                    } => {
                        annotate_expr(init, ann, ctx);
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn annotate_method_body(
    params: &[varn_core::ast::Param],
    body: &Stmt,
    this_ty: Option<&Type>,
    ann: &mut TypeAnnotations,
    ctx: &AnnotateCtx,
) {
    let mut local_ctx = ctx.clone();
    local_ctx.evolved_locals.clear();
    if let Some(ty) = this_ty {
        local_ctx
            .locals
            .insert(std::rc::Rc::from("this"), ty.clone());
    }
    for p in params {
        let name_opt = match &p.pattern {
            varn_core::ast::Pattern::Identifier { name, .. } => Some(name.clone()),
            _ => None,
        };
        if let Some(name) = name_opt {
            let type_ann = p.type_ann.as_ref().or(match &p.pattern {
                varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                _ => None,
            });
            let ty = if let Some(ann_node) = type_ann {
                resolve_type_node(ann_node, Some(ctx.bind))
            } else {
                Type::Dynamic
            };
            record_cg_ty_at(p.range.start.offset, &ty, ann, ctx);
            local_ctx.locals.insert(name, ty);
        }
    }
    annotate_stmt(body, ann, &mut local_ctx);
}

fn decl_primary_name(decl: &Decl) -> Option<std::rc::Rc<str>> {
    match decl {
        Decl::Variable(v) => v.declarators.first().and_then(|d| match &d.id {
            varn_core::ast::Pattern::Identifier { name, .. } => Some(name.clone()),
            _ => None,
        }),
        Decl::Function(f) => Some(f.id.clone()),
        Decl::Class(c) => c.id.clone(),
        Decl::Enum(e) => Some(e.id.clone()),
        Decl::Interface(i) => Some(i.id.clone()),
        Decl::TypeAlias(t) => Some(t.id.clone()),
        Decl::Namespace(n) => Some(n.id.clone()),
        Decl::Struct(s) => Some(s.id.clone()),
        Decl::SumType(s) => Some(s.id.clone()),
        _ => None,
    }
}

fn collect_pattern_names(pattern: &varn_core::ast::Pattern, out: &mut Vec<std::rc::Rc<str>>) {
    use varn_core::ast::Pattern;
    match pattern {
        Pattern::Identifier { name, .. } => out.push(name.clone()),
        Pattern::Array { elements, rest, .. } => {
            for el in elements.iter().flatten() {
                collect_pattern_names(&el.pattern, out);
            }
            if let Some(r) = rest {
                collect_pattern_names(r, out);
            }
        }
        Pattern::Object {
            properties, rest, ..
        } => {
            for prop in properties {
                collect_pattern_names(&prop.value, out);
            }
            if let Some(r) = rest {
                collect_pattern_names(r, out);
            }
        }
        Pattern::Assignment { left, .. } => collect_pattern_names(left, out),
        Pattern::Rest { argument, .. } => collect_pattern_names(argument, out),
    }
}

fn ungovern_pattern_names(pattern: &varn_core::ast::Pattern, ctx: &mut AnnotateCtx) {
    let mut bound = Vec::new();
    collect_pattern_names(pattern, &mut bound);
    for name in bound {
        ctx.evolved_locals.remove(name.as_ref());
        ctx.locals.remove(name.as_ref());
    }
}
