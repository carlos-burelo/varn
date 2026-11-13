use super::definite_assignment::walk_expr_children;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use vn_core::ast::{AstArena, AstTypeKind};
use vn_core::ast::{
    Decl, ExportDecl, ExprId, ExprKind, ForInit, FunctionDecl, Pattern, StmtId, StmtKind, TypeNode,
    VarDeclarator,
};
use vn_core::lang_type::LangPrimitive;
use vn_core::{Atom, AtomInterner};
use vn_sem::symbol::{InlineDep, InlineSrc};

const NODE_BUDGET: usize = 96;
const MAX_DEPTH: usize = 4;

pub struct InlineAnalyzer<'a> {
    arena: &'a AstArena,
    interner: &'a AtomInterner,
    source: &'a str,
    module: Arc<str>,
    functions: FxHashMap<Atom, &'a FunctionDecl>,
    memo: FxHashMap<Atom, Option<Arc<InlineSrc>>>,
    active: Vec<Atom>,
}

impl<'a> InlineAnalyzer<'a> {
    pub fn new(
        body: &[StmtId],
        arena: &'a AstArena,
        interner: &'a AtomInterner,
        source: &'a str,
        module: &str,
    ) -> Self {
        let mut functions = FxHashMap::default();
        for &stmt in body {
            let StmtKind::Decl(decl) = &arena.stmt(stmt).kind else {
                continue;
            };
            match decl.as_ref() {
                Decl::Function(f) => {
                    functions.insert(f.id, f);
                }
                Decl::Export(ExportDecl::Decl { declaration, .. }) => {
                    if let Decl::Function(f) = declaration.as_ref() {
                        functions.insert(f.id, f);
                    }
                }
                Decl::Variable(_)
                | Decl::Class(_)
                | Decl::Interface(_)
                | Decl::TypeAlias(_)
                | Decl::Enum(_)
                | Decl::Namespace(_)
                | Decl::Import(_)
                | Decl::Export(_)
                | Decl::Extension(_)
                | Decl::SumType(_) => {}
            }
        }
        Self {
            arena,
            interner,
            source,
            module: Arc::from(module),
            functions,
            memo: FxHashMap::default(),
            active: Vec::new(),
        }
    }

    pub fn analyze(&mut self, name: Atom) -> Option<Arc<InlineSrc>> {
        if let Some(done) = self.memo.get(&name) {
            return done.clone();
        }
        if self.active.contains(&name) || self.active.len() >= MAX_DEPTH {
            return None;
        }
        let f = *self.functions.get(&name)?;
        self.active.push(name);
        let built = self.build(f);
        self.active.pop();
        self.memo.insert(name, built.clone());
        built
    }

    fn build(&mut self, f: &FunctionDecl) -> Option<Arc<InlineSrc>> {
        if f.modifiers.is_async
            || f.modifiers.is_generator
            || f.modifiers.is_declare
            || !f.decorators.is_empty()
            || !f.type_params.is_empty()
        {
            return None;
        }
        let mut census = Census {
            arena: self.arena,
            interner: self.interner,
            nodes: 0,
            portable: true,
            scopes: vec![FxHashSet::default()],
            refs: Vec::new(),
        };
        for param in &f.params {
            let Pattern::Identifier { name, .. } = &param.pattern else {
                return None;
            };
            if param.is_rest || param.is_optional || param.default.is_some() {
                return None;
            }
            if !param
                .type_ann
                .as_ref()
                .is_some_and(|ty| portable_type(ty, self.interner))
            {
                return None;
            }
            census.declare(*name);
        }
        if let Some(ret) = &f.return_type
            && !portable_return(ret, self.interner)
        {
            return None;
        }
        census.stmt(f.body);
        if !census.portable || census.nodes > NODE_BUDGET {
            return None;
        }

        let start = f.range.start.offset as usize;
        let end = f.range.end.offset as usize;
        let text = self.source.get(start..end)?;
        if !text.starts_with(vn_core::TokenKind::Function.as_str()) {
            return None;
        }
        let name_offset = (f.id_offset as usize).checked_sub(start)?;

        let mut grouped: Vec<(Atom, Vec<u32>)> = Vec::new();
        for (name, offset) in census.refs {
            let relative = (offset as usize).checked_sub(start)?;
            let written = self.interner.resolve(name);
            if text.get(relative..relative + written.len())? != written {
                return None;
            }
            match grouped.iter_mut().find(|(n, _)| *n == name) {
                Some((_, offsets)) => offsets.push(relative as u32),
                None => grouped.push((name, vec![relative as u32])),
            }
        }
        let mut deps = Vec::with_capacity(grouped.len());
        for (name, offsets) in grouped {
            let src = self.analyze(name)?;
            deps.push(InlineDep { offsets, src });
        }
        Some(Arc::new(InlineSrc {
            module: self.module.clone(),
            name: Arc::from(self.interner.resolve(f.id)),
            text: Arc::from(text),
            name_offset: name_offset as u32,
            deps,
        }))
    }
}

fn portable_return(ty: &TypeNode, interner: &AtomInterner) -> bool {
    matches!(&ty.kind, AstTypeKind::Primitive(LangPrimitive::Void)) || portable_type(ty, interner)
}

fn portable_type(ty: &TypeNode, interner: &AtomInterner) -> bool {
    match &ty.kind {
        AstTypeKind::Named(name, None) => matches!(
            LangPrimitive::from_str(interner.resolve(*name)),
            Some(
                LangPrimitive::Bool
                    | LangPrimitive::Int
                    | LangPrimitive::Float
                    | LangPrimitive::Char
                    | LangPrimitive::Str
            )
        ),
        AstTypeKind::Array(inner) => portable_type(inner, interner),
        AstTypeKind::Primitive(
            LangPrimitive::Bool
            | LangPrimitive::Int
            | LangPrimitive::Float
            | LangPrimitive::Char
            | LangPrimitive::Str,
        ) => true,
        AstTypeKind::Primitive(
            LangPrimitive::Null
            | LangPrimitive::BigInt
            | LangPrimitive::Decimal
            | LangPrimitive::Void
            | LangPrimitive::Never
            | LangPrimitive::Dynamic
            | LangPrimitive::Error,
        )
        | AstTypeKind::Literal(_)
        | AstTypeKind::This
        | AstTypeKind::Union(_)
        | AstTypeKind::Intersection(_)
        | AstTypeKind::Tuple(_)
        | AstTypeKind::Named(_, Some(_))
        | AstTypeKind::Generic(..)
        | AstTypeKind::TemplateLiteral(_)
        | AstTypeKind::Fn(_)
        | AstTypeKind::Record(_)
        | AstTypeKind::Interface(_)
        | AstTypeKind::Typeof(_)
        | AstTypeKind::KeyOf(_)
        | AstTypeKind::IndexedAccess { .. }
        | AstTypeKind::Mapped { .. }
        | AstTypeKind::Conditional { .. }
        | AstTypeKind::Infer(_)
        | AstTypeKind::EnumVariant { .. }
        | AstTypeKind::TypePredicate { .. } => false,
    }
}

struct Census<'a> {
    arena: &'a AstArena,
    interner: &'a AtomInterner,
    nodes: usize,
    portable: bool,
    scopes: Vec<FxHashSet<Atom>>,
    refs: Vec<(Atom, u32)>,
}

impl Census<'_> {
    fn declare(&mut self, name: Atom) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name);
        }
    }

    fn bound(&self, name: Atom) -> bool {
        self.scopes.iter().any(|scope| scope.contains(&name))
    }

    fn free_identifier(&self, e: ExprId) -> bool {
        let mut current = e;
        loop {
            let kind = &self.arena.expr(current).kind;
            if let ExprKind::Paren { expression } = kind {
                current = *expression;
                continue;
            }
            let ExprKind::Identifier { name } = kind else {
                return false;
            };
            return !self.bound(*name);
        }
    }

    fn typed(&mut self, ty: &TypeNode) {
        self.portable &= portable_type(ty, self.interner);
    }

    fn declarator(&mut self, d: &VarDeclarator) {
        let Pattern::Identifier { name, .. } = &d.id else {
            self.portable = false;
            return;
        };
        if let Some(ty) = &d.type_ann {
            self.typed(ty);
        }
        if let Some(init) = d.init {
            self.expr(init);
        }
        self.declare(*name);
    }

    fn expr(&mut self, e: ExprId) {
        self.nodes += 1;
        let arena = self.arena;
        match &arena.expr(e).kind {
            ExprKind::Function { .. }
            | ExprKind::Arrow { .. }
            | ExprKind::ClassExpr { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::New { .. }
            | ExprKind::Try { .. }
            | ExprKind::ImportMeta(_)
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. }
            | ExprKind::Object { .. }
            | ExprKind::Match { .. }
            | ExprKind::Missing => self.portable = false,
            ExprKind::Identifier { name } => {
                if !self.bound(*name) {
                    self.refs.push((*name, arena.expr(e).range.start.offset));
                }
            }
            ExprKind::Assign { target, value, .. } => {
                if self.free_identifier(*target) {
                    self.portable = false;
                }
                self.expr(*target);
                self.expr(*value);
            }
            ExprKind::Update { operand, .. } => {
                if self.free_identifier(*operand) {
                    self.portable = false;
                }
                self.expr(*operand);
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
            ExprKind::As {
                expression,
                type_ann,
            }
            | ExprKind::Satisfies {
                expression,
                type_ann,
            }
            | ExprKind::Is {
                expression,
                type_ann,
            } => {
                self.typed(type_ann);
                self.expr(*expression);
            }
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::Template { .. }
            | ExprKind::Array { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Call { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Paren { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. } => walk_expr_children(e, arena, &mut |c| self.expr(c)),
        }
    }

    fn scoped(&mut self, body: impl FnOnce(&mut Self)) {
        self.scopes.push(FxHashSet::default());
        body(self);
        self.scopes.pop();
    }

    fn stmt(&mut self, s: StmtId) {
        self.nodes += 1;
        let arena = self.arena;
        match &arena.stmt(s).kind {
            StmtKind::Block { stmts } => self.scoped(|c| stmts.iter().for_each(|&x| c.stmt(x))),
            StmtKind::Expr { expression } => self.expr(*expression),
            StmtKind::Decl(d) => match d.as_ref() {
                Decl::Variable(v) => v.declarators.iter().for_each(|d| self.declarator(d)),
                Decl::Function(_)
                | Decl::Class(_)
                | Decl::Interface(_)
                | Decl::TypeAlias(_)
                | Decl::Enum(_)
                | Decl::Namespace(_)
                | Decl::Import(_)
                | Decl::Export(_)
                | Decl::Extension(_)
                | Decl::SumType(_) => self.portable = false,
            },
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
            } => self.scoped(|c| {
                match init.as_deref() {
                    Some(ForInit::Expr(e)) => c.expr(*e),
                    Some(ForInit::Var { declarators, .. }) => {
                        declarators.iter().for_each(|d| c.declarator(d));
                    }
                    None => {}
                }
                if let Some(t) = test {
                    c.expr(*t);
                }
                if let Some(u) = update {
                    c.expr(*u);
                }
                c.stmt(*body);
            }),
            StmtKind::ForIn {
                left, right, body, ..
            }
            | StmtKind::ForOf {
                left, right, body, ..
            } => {
                self.expr(*right);
                self.scoped(|c| {
                    for name in left.bound_names() {
                        c.declare(name);
                    }
                    c.stmt(*body);
                });
            }
            StmtKind::Break { .. } | StmtKind::Continue { .. } | StmtKind::Empty => {}
            StmtKind::Try { .. }
            | StmtKind::Using { .. }
            | StmtKind::Labeled { .. }
            | StmtKind::Debugger
            | StmtKind::Error => self.portable = false,
        }
    }
}
