use super::exprs::collect_expr;
use super::index::SpatialEntry;
use varn_core::ast::{
    AstArena, ClassDecl, ClassMember, Decl, EnumDecl, ExportDecl, ExportDefaultDecl, ExtensionDecl,
    ExtensionMember, ForInit, FunctionDecl, NamespaceDecl, StmtId, StmtKind, StructDecl,
    SwitchCase, VarDeclarator,
};

pub(super) fn collect_stmt(a: &AstArena, stmt: &StmtId, out: &mut Vec<SpatialEntry>) {
    let stmt = a.stmt(*stmt);
    match &stmt.kind {
        StmtKind::Block { stmts } => {
            for s in stmts {
                collect_stmt(a, s, out);
            }
        }
        StmtKind::Empty
        | StmtKind::Error
        | StmtKind::Debugger
        | StmtKind::Break { .. }
        | StmtKind::Continue { .. } => {}
        StmtKind::Expr { expression } => {
            collect_expr(a, expression, out);
        }
        StmtKind::Decl(decl) => {
            collect_decl(a, decl, out);
        }
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            collect_expr(a, test, out);
            collect_stmt(a, consequent, out);
            if let Some(alt) = alternate {
                collect_stmt(a, alt, out);
            }
        }
        StmtKind::While { test, body } | StmtKind::DoWhile { test, body } => {
            collect_expr(a, test, out);
            collect_stmt(a, body, out);
        }
        StmtKind::For {
            init,
            test,
            update,
            body,
        } => {
            if let Some(init) = init {
                match init.as_ref() {
                    ForInit::Var { declarators, .. } => {
                        for d in declarators {
                            collect_var_declarator(a, d, out);
                        }
                    }
                    ForInit::Expr(e) => collect_expr(a, e, out),
                }
            }
            if let Some(test) = test {
                collect_expr(a, test, out);
            }
            if let Some(update) = update {
                collect_expr(a, update, out);
            }
            collect_stmt(a, body, out);
        }
        StmtKind::ForIn { right, body, .. } | StmtKind::ForOf { right, body, .. } => {
            collect_expr(a, right, out);
            collect_stmt(a, body, out);
        }
        StmtKind::Switch {
            discriminant,
            cases,
        } => {
            collect_expr(a, discriminant, out);
            for case in cases {
                collect_switch_case(a, case, out);
            }
        }
        StmtKind::Return { argument } => {
            if let Some(arg) = argument {
                collect_expr(a, arg, out);
            }
        }
        StmtKind::Throw { argument } => {
            collect_expr(a, argument, out);
        }
        StmtKind::Try {
            block,
            catches,
            finally,
        } => {
            collect_stmt(a, block, out);
            for c in catches {
                collect_stmt(a, &c.body, out);
            }
            if let Some(f) = finally {
                collect_stmt(a, f, out);
            }
        }
        StmtKind::Using { declarations, .. } => {
            for d in declarations {
                collect_var_declarator(a, d, out);
            }
        }
        StmtKind::Labeled { body, .. } => {
            collect_stmt(a, body, out);
        }
    }
}

pub(super) fn collect_switch_case(a: &AstArena, case: &SwitchCase, out: &mut Vec<SpatialEntry>) {
    if let Some(test) = &case.test {
        collect_expr(a, test, out);
    }
    for s in &case.body {
        collect_stmt(a, s, out);
    }
}

pub(super) fn collect_var_declarator(a: &AstArena, d: &VarDeclarator, out: &mut Vec<SpatialEntry>) {
    if let Some(init) = &d.init {
        collect_expr(a, init, out);
    }
}

pub(super) fn collect_decl(a: &AstArena, decl: &Decl, out: &mut Vec<SpatialEntry>) {
    match decl {
        Decl::Variable(v) => {
            for d in &v.declarators {
                collect_var_declarator(a, d, out);
            }
        }
        Decl::Function(f) => collect_fn_decl(a, f, out),
        Decl::Class(c) => collect_class_decl(a, c, out),
        Decl::Enum(e) => collect_enum_decl(a, e, out),
        Decl::Namespace(n) => collect_namespace_decl(a, n, out),
        Decl::Export(exp) => collect_export_decl(a, exp, out),
        Decl::Extension(ext) => collect_extension_decl(a, ext, out),
        Decl::Struct(s) => collect_struct_decl(a, s, out),
        Decl::Interface(_) | Decl::TypeAlias(_) | Decl::Import(_) | Decl::SumType(_) => {}
    }
}

pub(super) fn collect_fn_decl(a: &AstArena, f: &FunctionDecl, out: &mut Vec<SpatialEntry>) {
    for p in &f.params {
        if let Some(default) = &p.default {
            collect_expr(a, default, out);
        }
    }
    collect_stmt(a, &f.body, out);
}

pub(super) fn collect_class_decl(a: &AstArena, c: &ClassDecl, out: &mut Vec<SpatialEntry>) {
    if let Some(super_cls) = &c.super_class {
        collect_expr(a, super_cls, out);
    }
    for member in &c.body {
        match member {
            ClassMember::Constructor { params, body, .. } => {
                for p in params {
                    if let Some(default) = &p.default {
                        collect_expr(a, default, out);
                    }
                }
                collect_stmt(a, body, out);
            }
            ClassMember::Destructor { body, .. } | ClassMember::StaticBlock { body, .. } => {
                collect_stmt(a, body, out);
            }
            ClassMember::Method {
                params,
                body: Some(body),
                ..
            } => {
                for p in params {
                    if let Some(default) = &p.default {
                        collect_expr(a, default, out);
                    }
                }
                collect_stmt(a, body, out);
            }
            ClassMember::Property {
                init: Some(init), ..
            } => {
                collect_expr(a, init, out);
            }
            ClassMember::Getter {
                body: Some(body), ..
            } => {
                collect_stmt(a, body, out);
            }
            ClassMember::Setter {
                param,
                body: Some(body),
                ..
            } => {
                if let Some(default) = &param.default {
                    collect_expr(a, default, out);
                }
                collect_stmt(a, body, out);
            }
            _ => {}
        }
    }
}

pub(super) fn collect_enum_decl(a: &AstArena, e: &EnumDecl, out: &mut Vec<SpatialEntry>) {
    for m in &e.members {
        if let Some(init) = &m.init {
            collect_expr(a, init, out);
        }
        for f in &m.payload_fields {
            if let Some(init) = &f.init {
                collect_expr(a, init, out);
            }
        }
    }
}

pub(super) fn collect_namespace_decl(a: &AstArena, n: &NamespaceDecl, out: &mut Vec<SpatialEntry>) {
    for d in &n.body {
        collect_decl(a, d, out);
    }
}

pub(super) fn collect_export_decl(a: &AstArena, exp: &ExportDecl, out: &mut Vec<SpatialEntry>) {
    match exp {
        ExportDecl::Default { declaration, .. } => match declaration.as_ref() {
            ExportDefaultDecl::Function(f) => collect_fn_decl(a, f, out),
            ExportDefaultDecl::Class(c) => collect_class_decl(a, c, out),
            ExportDefaultDecl::Expr(e) => collect_expr(a, e, out),
        },
        ExportDecl::Decl { declaration, .. } => collect_decl(a, declaration, out),
        _ => {}
    }
}

pub(super) fn collect_extension_decl(
    a: &AstArena,
    ext: &ExtensionDecl,
    out: &mut Vec<SpatialEntry>,
) {
    for m in &ext.members {
        match m {
            ExtensionMember::Method(f) => collect_fn_decl(a, f, out),
            ExtensionMember::Getter { body, .. } => collect_stmt(a, body, out),
            ExtensionMember::Setter { param, body, .. } => {
                if let Some(default) = &param.default {
                    collect_expr(a, default, out);
                }
                collect_stmt(a, body, out);
            }
        }
    }
}

pub(super) fn collect_struct_decl(a: &AstArena, s: &StructDecl, out: &mut Vec<SpatialEntry>) {
    for f in &s.fields {
        if let Some(default) = &f.default {
            collect_expr(a, default, out);
        }
    }
}
