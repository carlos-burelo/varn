use super::patterns::{pattern_names, walk_expr_children};
use rustc_hash::FxHashSet;
use varn_core::ast::{
    ArrowBody, AstArena, Decl, ExportDecl, ExprId, ExprKind, ForInit, MatchBody, MatchPattern,
    Param, StmtId, StmtKind, VarDeclarator,
};
use varn_core::Atom;

pub(super) enum Body<'a> {
    Stmt(StmtId),
    Arrow(&'a ArrowBody),
}

pub(super) fn free_reads(arena: &AstArena, params: &[Param], body: Body) -> FxHashSet<Atom> {
    let mut c = Collector {
        arena,
        reads: FxHashSet::default(),
        declared: FxHashSet::default(),
    };
    c.params(params);
    match body {
        Body::Stmt(s) => c.stmt(s),
        Body::Arrow(ArrowBody::Expr(e)) => c.expr(*e),
        Body::Arrow(ArrowBody::Block(s)) => c.stmt(*s),
    }
    c.reads.retain(|name| !c.declared.contains(name));
    c.reads
}

struct Collector<'a> {
    arena: &'a AstArena,
    reads: FxHashSet<Atom>,
    declared: FxHashSet<Atom>,
}

impl Collector<'_> {
    fn params(&mut self, params: &[Param]) {
        for p in params {
            self.declared.extend(pattern_names(&p.pattern));
            if let Some(d) = p.default {
                self.expr(d);
            }
        }
    }

    fn declarators(&mut self, declarators: &[VarDeclarator]) {
        for d in declarators {
            self.declared.extend(pattern_names(&d.id));
            if let Some(init) = d.init {
                self.expr(init);
            }
        }
    }

    fn match_bindings(&mut self, pattern: &MatchPattern) {
        match pattern {
            MatchPattern::Wildcard => {}
            MatchPattern::Literal(e) => self.expr(*e),
            MatchPattern::Identifier(name) => {
                self.declared.insert(*name);
            }
            MatchPattern::Record { fields, .. } => {
                for (name, inner) in fields {
                    match inner {
                        Some(p) => self.match_bindings(p),
                        None => {
                            self.declared.insert(*name);
                        }
                    }
                }
            }
            MatchPattern::Sequence(items) => items.iter().for_each(|p| self.match_bindings(p)),
            MatchPattern::Type { binding, .. } => self.declared.extend(*binding),
            MatchPattern::EnumVariant { bindings, .. } => {
                self.declared.extend(bindings.iter().map(|b| b.name));
            }
        }
    }

    fn expr(&mut self, e: ExprId) {
        let arena = self.arena;
        match &arena.expr(e).kind {
            ExprKind::Identifier { name } => {
                self.reads.insert(*name);
            }
            ExprKind::Member {
                object,
                property,
                computed,
                ..
            } => {
                self.expr(*object);
                if *computed {
                    self.expr(*property);
                }
            }
            ExprKind::Assign { target, value, .. } => {
                self.expr(*target);
                self.expr(*value);
            }
            ExprKind::Function { params, body, .. } => {
                self.params(params);
                self.stmt(*body);
            }
            ExprKind::Arrow { params, body, .. } => {
                self.params(params);
                match body.as_ref() {
                    ArrowBody::Expr(x) => self.expr(*x),
                    ArrowBody::Block(s) => self.stmt(*s),
                }
            }
            ExprKind::Match { subject, cases } => {
                self.expr(*subject);
                for case in cases {
                    self.match_bindings(&case.pattern);
                    if let Some(g) = case.guard {
                        self.expr(g);
                    }
                    match &case.body {
                        MatchBody::Expr(x) => self.expr(*x),
                        MatchBody::Block(s) => self.stmt(*s),
                    }
                }
            }
            ExprKind::ClassExpr { .. } => {}
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::ImportMeta(_)
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Call { .. }
            | ExprKind::New { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Paren { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::Try { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => walk_expr_children(e, arena, &mut |c| self.expr(c)),
        }
    }

    fn decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Variable(v) => self.declarators(&v.declarators),
            Decl::Function(f) => {
                self.declared.insert(f.id);
                self.params(&f.params);
                self.stmt(f.body);
            }
            Decl::Class(c) => self.declared.extend(c.id),
            Decl::Enum(e) => {
                self.declared.insert(e.id);
            }
            Decl::Export(ExportDecl::Decl { declaration, .. }) => self.decl(declaration),
            Decl::Interface(_)
            | Decl::TypeAlias(_)
            | Decl::Namespace(_)
            | Decl::Import(_)
            | Decl::Export(_)
            | Decl::Extension(_)
            | Decl::Struct(_)
            | Decl::SumType(_) => {}
        }
    }

    fn stmt(&mut self, s: StmtId) {
        let arena = self.arena;
        match &arena.stmt(s).kind {
            StmtKind::Block { stmts } => stmts.iter().for_each(|&x| self.stmt(x)),
            StmtKind::Expr { expression } => self.expr(*expression),
            StmtKind::Decl(d) => self.decl(d),
            StmtKind::Return { argument } => {
                if let Some(a) = argument {
                    self.expr(*a);
                }
            }
            StmtKind::Throw { argument } => self.expr(*argument),
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                self.expr(*test);
                self.stmt(*consequent);
                if let Some(a) = alternate {
                    self.stmt(*a);
                }
            }
            StmtKind::While { test, body } | StmtKind::DoWhile { body, test } => {
                self.expr(*test);
                self.stmt(*body);
            }
            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                match init.as_deref() {
                    Some(ForInit::Expr(e)) => self.expr(*e),
                    Some(ForInit::Var { declarators, .. }) => self.declarators(declarators),
                    None => {}
                }
                if let Some(t) = test {
                    self.expr(*t);
                }
                if let Some(u) = update {
                    self.expr(*u);
                }
                self.stmt(*body);
            }
            StmtKind::ForIn {
                left, right, body, ..
            }
            | StmtKind::ForOf {
                left, right, body, ..
            } => {
                self.declared.extend(pattern_names(left));
                self.expr(*right);
                self.stmt(*body);
            }
            StmtKind::Switch {
                discriminant,
                cases,
            } => {
                self.expr(*discriminant);
                for case in cases {
                    if let Some(t) = case.test {
                        self.expr(t);
                    }
                    case.body.iter().for_each(|&x| self.stmt(x));
                }
            }
            StmtKind::Try {
                block,
                catches,
                finally,
            } => {
                self.stmt(*block);
                for clause in catches {
                    if let Some(p) = &clause.param {
                        self.declared.extend(pattern_names(p));
                    }
                    self.stmt(clause.body);
                }
                if let Some(f) = finally {
                    self.stmt(*f);
                }
            }
            StmtKind::Using { declarations, .. } => self.declarators(declarations),
            StmtKind::Labeled { body, .. } => self.stmt(*body),
            StmtKind::Break { .. }
            | StmtKind::Continue { .. }
            | StmtKind::Empty
            | StmtKind::Debugger
            | StmtKind::Error => {}
        }
    }
}
