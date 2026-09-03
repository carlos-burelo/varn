use varn_core::ast::{
    Arg, ArrayEl, ArrowBody, AstId, ClassDecl, ClassMember, Decl, EnumDecl, ExportDecl,
    ExportDefaultDecl, Expr, ExprKind, ExtensionDecl, ExtensionMember, ForInit, FunctionDecl,
    MatchBody, MatchCase, NamespaceDecl, ObjectProp, Program, PropKey, Stmt, StmtKind, StructDecl,
    SwitchCase, TemplatePart, VarDeclarator,
};

#[derive(Clone, Copy, Debug)]
pub struct SpatialEntry {
    pub start: u32,
    pub end: u32,
    pub ast_id: AstId,
}

/// O(log N) Spatial Index over all AST nodes in a document.
///
/// Replaces the legacy offset-keying maps and linear scans.
/// Translates cursor byte offset into the most specific (innermost) `AstId`.
#[derive(Clone, Debug, Default)]
pub struct SpatialIndex {
    entries: Vec<SpatialEntry>,
}

impl SpatialIndex {
    pub fn build(program: &Program) -> Self {
        let mut entries = Vec::with_capacity(512);
        for stmt in &program.body {
            collect_stmt(stmt, &mut entries);
        }
        // Sort by start ASC; for identical start, sort by span length DESC (larger/outer spans first)
        entries.sort_by(|a, b| {
            a.start
                .cmp(&b.start)
                .then_with(|| (b.end.saturating_sub(b.start)).cmp(&a.end.saturating_sub(a.start)))
        });
        Self { entries }
    }

    /// Finds the innermost AST expression/node containing `offset`.
    pub fn innermost_at(&self, offset: u32) -> Option<AstId> {
        if self.entries.is_empty() {
            return None;
        }

        // Find boundary of entries where start <= offset
        let upper = match self.entries.binary_search_by(|e| e.start.cmp(&offset)) {
            Ok(idx) => {
                let mut i = idx;
                while i + 1 < self.entries.len() && self.entries[i + 1].start == offset {
                    i += 1;
                }
                i + 1
            }
            Err(idx) => idx,
        };

        let mut best: Option<(u32, AstId)> = None;
        for entry in &self.entries[..upper] {
            if entry.start <= offset && offset <= entry.end {
                let span_len = entry.end.saturating_sub(entry.start);
                match best {
                    None => best = Some((span_len, entry.ast_id)),
                    Some((best_len, _)) if span_len < best_len => {
                        best = Some((span_len, entry.ast_id));
                    }
                    _ => {}
                }
            }
        }

        best.map(|(_, id)| id)
    }
}

fn collect_stmt(stmt: &Stmt, out: &mut Vec<SpatialEntry>) {
    match &stmt.kind {
        StmtKind::Block { stmts } => {
            for s in stmts {
                collect_stmt(s, out);
            }
        }
        StmtKind::Empty
        | StmtKind::Error
        | StmtKind::Debugger
        | StmtKind::Break { .. }
        | StmtKind::Continue { .. } => {}
        StmtKind::Expr { expression } => {
            collect_expr(expression, out);
        }
        StmtKind::Decl(decl) => {
            collect_decl(decl, out);
        }
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            collect_expr(test, out);
            collect_stmt(consequent, out);
            if let Some(alt) = alternate {
                collect_stmt(alt, out);
            }
        }
        StmtKind::While { test, body } | StmtKind::DoWhile { test, body } => {
            collect_expr(test, out);
            collect_stmt(body, out);
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
                            collect_var_declarator(d, out);
                        }
                    }
                    ForInit::Expr(e) => collect_expr(e, out),
                }
            }
            if let Some(test) = test {
                collect_expr(test, out);
            }
            if let Some(update) = update {
                collect_expr(update, out);
            }
            collect_stmt(body, out);
        }
        StmtKind::ForIn { right, body, .. } | StmtKind::ForOf { right, body, .. } => {
            collect_expr(right, out);
            collect_stmt(body, out);
        }
        StmtKind::Switch {
            discriminant,
            cases,
        } => {
            collect_expr(discriminant, out);
            for case in cases {
                collect_switch_case(case, out);
            }
        }
        StmtKind::Return { argument } => {
            if let Some(arg) = argument {
                collect_expr(arg, out);
            }
        }
        StmtKind::Throw { argument } => {
            collect_expr(argument, out);
        }
        StmtKind::Try {
            block,
            catches,
            finally,
        } => {
            collect_stmt(block, out);
            for c in catches {
                collect_stmt(&c.body, out);
            }
            if let Some(f) = finally {
                collect_stmt(f, out);
            }
        }
        StmtKind::Using { declarations, .. } => {
            for d in declarations {
                collect_var_declarator(d, out);
            }
        }
        StmtKind::Labeled { body, .. } => {
            collect_stmt(body, out);
        }
    }
}

fn collect_switch_case(case: &SwitchCase, out: &mut Vec<SpatialEntry>) {
    if let Some(test) = &case.test {
        collect_expr(test, out);
    }
    for s in &case.body {
        collect_stmt(s, out);
    }
}

fn collect_var_declarator(d: &VarDeclarator, out: &mut Vec<SpatialEntry>) {
    if let Some(init) = &d.init {
        collect_expr(init, out);
    }
}

fn collect_decl(decl: &Decl, out: &mut Vec<SpatialEntry>) {
    match decl {
        Decl::Variable(v) => {
            for d in &v.declarators {
                collect_var_declarator(d, out);
            }
        }
        Decl::Function(f) => collect_fn_decl(f, out),
        Decl::Class(c) => collect_class_decl(c, out),
        Decl::Enum(e) => collect_enum_decl(e, out),
        Decl::Namespace(n) => collect_namespace_decl(n, out),
        Decl::Export(exp) => collect_export_decl(exp, out),
        Decl::Extension(ext) => collect_extension_decl(ext, out),
        Decl::Struct(s) => collect_struct_decl(s, out),
        Decl::Interface(_) | Decl::TypeAlias(_) | Decl::Import(_) | Decl::SumType(_) => {}
    }
}

fn collect_fn_decl(f: &FunctionDecl, out: &mut Vec<SpatialEntry>) {
    for p in &f.params {
        if let Some(default) = &p.default {
            collect_expr(default, out);
        }
    }
    collect_stmt(&f.body, out);
}

fn collect_class_decl(c: &ClassDecl, out: &mut Vec<SpatialEntry>) {
    if let Some(super_cls) = &c.super_class {
        collect_expr(super_cls, out);
    }
    for member in &c.body {
        match member {
            ClassMember::Constructor { params, body, .. } => {
                for p in params {
                    if let Some(default) = &p.default {
                        collect_expr(default, out);
                    }
                }
                collect_stmt(body, out);
            }
            ClassMember::Destructor { body, .. } | ClassMember::StaticBlock { body, .. } => {
                collect_stmt(body, out);
            }
            ClassMember::Method {
                params,
                body: Some(body),
                ..
            } => {
                for p in params {
                    if let Some(default) = &p.default {
                        collect_expr(default, out);
                    }
                }
                collect_stmt(body, out);
            }
            ClassMember::Property {
                init: Some(init), ..
            } => {
                collect_expr(init, out);
            }
            ClassMember::Getter {
                body: Some(body), ..
            } => {
                collect_stmt(body, out);
            }
            ClassMember::Setter {
                param,
                body: Some(body),
                ..
            } => {
                if let Some(default) = &param.default {
                    collect_expr(default, out);
                }
                collect_stmt(body, out);
            }
            _ => {}
        }
    }
}

fn collect_enum_decl(e: &EnumDecl, out: &mut Vec<SpatialEntry>) {
    for m in &e.members {
        if let Some(init) = &m.init {
            collect_expr(init, out);
        }
        for f in &m.payload_fields {
            if let Some(init) = &f.init {
                collect_expr(init, out);
            }
        }
    }
}

fn collect_namespace_decl(n: &NamespaceDecl, out: &mut Vec<SpatialEntry>) {
    for d in &n.body {
        collect_decl(d, out);
    }
}

fn collect_export_decl(exp: &ExportDecl, out: &mut Vec<SpatialEntry>) {
    match exp {
        ExportDecl::Default { declaration, .. } => match declaration.as_ref() {
            ExportDefaultDecl::Function(f) => collect_fn_decl(f, out),
            ExportDefaultDecl::Class(c) => collect_class_decl(c, out),
            ExportDefaultDecl::Expr(e) => collect_expr(e, out),
        },
        ExportDecl::Decl { declaration, .. } => collect_decl(declaration, out),
        _ => {}
    }
}

fn collect_extension_decl(ext: &ExtensionDecl, out: &mut Vec<SpatialEntry>) {
    for m in &ext.members {
        match m {
            ExtensionMember::Method(f) => collect_fn_decl(f, out),
            ExtensionMember::Getter { body, .. } => collect_stmt(body, out),
            ExtensionMember::Setter { param, body, .. } => {
                if let Some(default) = &param.default {
                    collect_expr(default, out);
                }
                collect_stmt(body, out);
            }
        }
    }
}

fn collect_struct_decl(s: &StructDecl, out: &mut Vec<SpatialEntry>) {
    for f in &s.fields {
        if let Some(default) = &f.default {
            collect_expr(default, out);
        }
    }
}

fn collect_expr(expr: &Expr, out: &mut Vec<SpatialEntry>) {
    out.push(SpatialEntry {
        start: expr.range.start.offset,
        end: expr.range.end.offset,
        ast_id: expr.id,
    });

    match &expr.kind {
        ExprKind::TaggedTemplate { tag, template } => {
            collect_expr(tag, out);
            collect_expr(template, out);
        }
        ExprKind::Template { parts } => {
            for part in parts {
                if let TemplatePart::Interpolation(e) = part {
                    collect_expr(e, out);
                }
            }
        }
        ExprKind::Array { elements } => {
            for el in elements {
                match el {
                    ArrayEl::Expr(e) | ArrayEl::Spread(e) => collect_expr(e, out),
                    ArrayEl::Hole => {}
                }
            }
        }
        ExprKind::Object { properties } | ExprKind::Record { properties } => {
            for prop in properties {
                collect_object_prop(prop, out);
            }
        }
        ExprKind::Tuple { elements }
        | ExprKind::Sequence {
            expressions: elements,
        } => {
            for el in elements {
                collect_expr(el, out);
            }
        }
        ExprKind::Unary { operand, .. }
        | ExprKind::Update { operand, .. }
        | ExprKind::Paren {
            expression: operand,
        }
        | ExprKind::Await { argument: operand }
        | ExprKind::Spawn { argument: operand }
        | ExprKind::Spread { argument: operand }
        | ExprKind::NonNull {
            expression: operand,
        }
        | ExprKind::Try {
            expression: operand,
        }
        | ExprKind::As {
            expression: operand,
            ..
        }
        | ExprKind::Satisfies {
            expression: operand,
            ..
        }
        | ExprKind::Is {
            expression: operand,
            ..
        } => {
            collect_expr(operand, out);
        }
        ExprKind::Binary { left, right, .. }
        | ExprKind::Logical { left, right, .. }
        | ExprKind::Assign {
            target: left,
            value: right,
            ..
        }
        | ExprKind::Pipeline { left, right }
        | ExprKind::Range {
            start: left,
            end: right,
            ..
        } => {
            collect_expr(left, out);
            collect_expr(right, out);
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_expr(test, out);
            collect_expr(consequent, out);
            collect_expr(alternate, out);
        }
        ExprKind::Member {
            object, property, ..
        } => {
            collect_expr(object, out);
            collect_expr(property, out);
        }
        ExprKind::Call { callee, args, .. } | ExprKind::New { callee, args, .. } => {
            collect_expr(callee, out);
            for arg in args {
                match arg {
                    Arg::Positional(e) | Arg::Spread(e) | Arg::Named { value: e, .. } => {
                        collect_expr(e, out);
                    }
                }
            }
        }
        ExprKind::Function { body, params, .. } => {
            for p in params {
                if let Some(default) = &p.default {
                    collect_expr(default, out);
                }
            }
            collect_stmt(body, out);
        }
        ExprKind::Arrow { params, body, .. } => {
            for p in params {
                if let Some(default) = &p.default {
                    collect_expr(default, out);
                }
            }
            match body.as_ref() {
                ArrowBody::Expr(e) => collect_expr(e, out),
                ArrowBody::Block(s) => collect_stmt(s, out),
            }
        }
        ExprKind::Yield {
            argument: Some(arg),
            ..
        } => {
            collect_expr(arg, out);
        }
        ExprKind::ClassExpr { declaration } => {
            collect_class_decl(declaration, out);
        }
        ExprKind::Match { subject, cases } => {
            collect_expr(subject, out);
            for c in cases {
                collect_match_case(c, out);
            }
        }
        ExprKind::With { object, properties } => {
            collect_expr(object, out);
            for p in properties {
                collect_object_prop(p, out);
            }
        }
        ExprKind::MetaAccess { target, .. } => {
            collect_expr(target, out);
        }
        _ => {}
    }
}

fn collect_object_prop(prop: &ObjectProp, out: &mut Vec<SpatialEntry>) {
    match prop {
        ObjectProp::Property { key, value, .. } => {
            if let PropKey::Computed(e) = key {
                collect_expr(e, out);
            }
            collect_expr(value, out);
        }
        ObjectProp::Method {
            key, params, body, ..
        } => {
            if let PropKey::Computed(e) = key {
                collect_expr(e, out);
            }
            for p in params {
                if let Some(default) = &p.default {
                    collect_expr(default, out);
                }
            }
            collect_stmt(body, out);
        }
        ObjectProp::Getter { key, body, .. } => {
            if let PropKey::Computed(e) = key {
                collect_expr(e, out);
            }
            collect_stmt(body, out);
        }
        ObjectProp::Setter {
            key, param, body, ..
        } => {
            if let PropKey::Computed(e) = key {
                collect_expr(e, out);
            }
            if let Some(default) = &param.default {
                collect_expr(default, out);
            }
            collect_stmt(body, out);
        }
        ObjectProp::Spread { argument, .. } => {
            collect_expr(argument, out);
        }
    }
}

fn collect_match_case(case: &MatchCase, out: &mut Vec<SpatialEntry>) {
    if let Some(guard) = &case.guard {
        collect_expr(guard, out);
    }
    match &case.body {
        MatchBody::Expr(e) => collect_expr(e, out),
        MatchBody::Block(s) => collect_stmt(s, out),
    }
}
