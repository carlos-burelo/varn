use varn_core::ast::{
    Arg, ArrayEl, ArrowBody, AstArena, AstId, ClassDecl, ClassMember, Decl, EnumDecl, ExportDecl,
    ExportDefaultDecl, ExprId, ExprKind, ExtensionDecl, ExtensionMember, ForInit, FunctionDecl,
    MatchBody, MatchCase, NamespaceDecl, ObjectProp, Program, PropKey, StmtId, StmtKind,
    StructDecl, SwitchCase, TemplatePart, VarDeclarator,
};

#[derive(Clone, Copy, Debug)]
pub struct SpatialEntry {
    pub start: u32,
    pub end: u32,
    pub expr: ExprId,
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
    pub fn build(program: &Program, a: &AstArena) -> Self {
        let mut entries = Vec::with_capacity(512);
        for stmt in &program.body {
            collect_stmt(a, stmt, &mut entries);
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
                    None => best = Some((span_len, entry.expr.index())),
                    Some((best_len, _)) if span_len < best_len => {
                        best = Some((span_len, entry.expr.index()));
                    }
                    _ => {}
                }
            }
        }

        best.map(|(_, id)| id)
    }

    /// Every expression of the program, in source order: the nodes its
    /// statements reach, never one the parser built and backtracked over.
    pub fn exprs(&self) -> impl Iterator<Item = ExprId> + '_ {
        self.entries.iter().map(|e| e.expr)
    }
}

fn collect_stmt(a: &AstArena, stmt: &StmtId, out: &mut Vec<SpatialEntry>) {
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

fn collect_switch_case(a: &AstArena, case: &SwitchCase, out: &mut Vec<SpatialEntry>) {
    if let Some(test) = &case.test {
        collect_expr(a, test, out);
    }
    for s in &case.body {
        collect_stmt(a, s, out);
    }
}

fn collect_var_declarator(a: &AstArena, d: &VarDeclarator, out: &mut Vec<SpatialEntry>) {
    if let Some(init) = &d.init {
        collect_expr(a, init, out);
    }
}

fn collect_decl(a: &AstArena, decl: &Decl, out: &mut Vec<SpatialEntry>) {
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

fn collect_fn_decl(a: &AstArena, f: &FunctionDecl, out: &mut Vec<SpatialEntry>) {
    for p in &f.params {
        if let Some(default) = &p.default {
            collect_expr(a, default, out);
        }
    }
    collect_stmt(a, &f.body, out);
}

fn collect_class_decl(a: &AstArena, c: &ClassDecl, out: &mut Vec<SpatialEntry>) {
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

fn collect_enum_decl(a: &AstArena, e: &EnumDecl, out: &mut Vec<SpatialEntry>) {
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

fn collect_namespace_decl(a: &AstArena, n: &NamespaceDecl, out: &mut Vec<SpatialEntry>) {
    for d in &n.body {
        collect_decl(a, d, out);
    }
}

fn collect_export_decl(a: &AstArena, exp: &ExportDecl, out: &mut Vec<SpatialEntry>) {
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

fn collect_extension_decl(a: &AstArena, ext: &ExtensionDecl, out: &mut Vec<SpatialEntry>) {
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

fn collect_struct_decl(a: &AstArena, s: &StructDecl, out: &mut Vec<SpatialEntry>) {
    for f in &s.fields {
        if let Some(default) = &f.default {
            collect_expr(a, default, out);
        }
    }
}

fn collect_expr(a: &AstArena, id: &ExprId, out: &mut Vec<SpatialEntry>) {
    let expr = a.expr(*id);
    out.push(SpatialEntry {
        start: expr.range.start.offset,
        end: expr.range.end.offset,
        expr: *id,
    });

    match &expr.kind {
        ExprKind::TaggedTemplate { tag, template } => {
            collect_expr(a, tag, out);
            collect_expr(a, template, out);
        }
        ExprKind::Template { parts } => {
            for part in parts {
                if let TemplatePart::Interpolation(e) = part {
                    collect_expr(a, e, out);
                }
            }
        }
        ExprKind::Array { elements } => {
            for el in elements {
                match el {
                    ArrayEl::Expr(e) | ArrayEl::Spread(e) => collect_expr(a, e, out),
                    ArrayEl::Hole => {}
                }
            }
        }
        ExprKind::Object { properties } | ExprKind::Record { properties } => {
            for prop in properties {
                collect_object_prop(a, prop, out);
            }
        }
        ExprKind::Tuple { elements }
        | ExprKind::Sequence {
            expressions: elements,
        } => {
            for el in elements {
                collect_expr(a, el, out);
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
            collect_expr(a, operand, out);
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
            collect_expr(a, left, out);
            collect_expr(a, right, out);
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_expr(a, test, out);
            collect_expr(a, consequent, out);
            collect_expr(a, alternate, out);
        }
        ExprKind::Member {
            object, property, ..
        } => {
            collect_expr(a, object, out);
            collect_expr(a, property, out);
        }
        ExprKind::Call { callee, args, .. } | ExprKind::New { callee, args, .. } => {
            collect_expr(a, callee, out);
            for arg in args {
                match arg {
                    Arg::Positional(e) | Arg::Spread(e) | Arg::Named { value: e, .. } => {
                        collect_expr(a, e, out);
                    }
                }
            }
        }
        ExprKind::Function { body, params, .. } => {
            for p in params {
                if let Some(default) = &p.default {
                    collect_expr(a, default, out);
                }
            }
            collect_stmt(a, body, out);
        }
        ExprKind::Arrow { params, body, .. } => {
            for p in params {
                if let Some(default) = &p.default {
                    collect_expr(a, default, out);
                }
            }
            match body.as_ref() {
                ArrowBody::Expr(e) => collect_expr(a, e, out),
                ArrowBody::Block(s) => collect_stmt(a, s, out),
            }
        }
        ExprKind::Yield {
            argument: Some(arg),
            ..
        } => {
            collect_expr(a, arg, out);
        }
        ExprKind::ClassExpr { declaration } => {
            collect_class_decl(a, declaration, out);
        }
        ExprKind::Match { subject, cases } => {
            collect_expr(a, subject, out);
            for c in cases {
                collect_match_case(a, c, out);
            }
        }
        ExprKind::With { object, properties } => {
            collect_expr(a, object, out);
            for p in properties {
                collect_object_prop(a, p, out);
            }
        }
        ExprKind::MetaAccess { target, .. } => {
            collect_expr(a, target, out);
        }
        _ => {}
    }
}

fn collect_object_prop(a: &AstArena, prop: &ObjectProp, out: &mut Vec<SpatialEntry>) {
    match prop {
        ObjectProp::Property { key, value, .. } => {
            if let PropKey::Computed(e) = key {
                collect_expr(a, e, out);
            }
            collect_expr(a, value, out);
        }
        ObjectProp::Method {
            key, params, body, ..
        } => {
            if let PropKey::Computed(e) = key {
                collect_expr(a, e, out);
            }
            for p in params {
                if let Some(default) = &p.default {
                    collect_expr(a, default, out);
                }
            }
            collect_stmt(a, body, out);
        }
        ObjectProp::Getter { key, body, .. } => {
            if let PropKey::Computed(e) = key {
                collect_expr(a, e, out);
            }
            collect_stmt(a, body, out);
        }
        ObjectProp::Setter {
            key, param, body, ..
        } => {
            if let PropKey::Computed(e) = key {
                collect_expr(a, e, out);
            }
            if let Some(default) = &param.default {
                collect_expr(a, default, out);
            }
            collect_stmt(a, body, out);
        }
        ObjectProp::Spread { argument, .. } => {
            collect_expr(a, argument, out);
        }
    }
}

fn collect_match_case(a: &AstArena, case: &MatchCase, out: &mut Vec<SpatialEntry>) {
    if let Some(guard) = &case.guard {
        collect_expr(a, guard, out);
    }
    match &case.body {
        MatchBody::Expr(e) => collect_expr(a, e, out),
        MatchBody::Block(s) => collect_stmt(a, s, out),
    }
}
