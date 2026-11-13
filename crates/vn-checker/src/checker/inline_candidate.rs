use super::definite_assignment::free_names::{free_reads, Body};
use super::definite_assignment::walk_expr_children;
use vn_core::ast::{AstArena, AstTypeKind};
use vn_core::ast::{
    Decl, ExprId, ExprKind, ForInit, FunctionDecl, Pattern, StmtId, StmtKind, TypeNode,
};
use vn_core::lang_type::LangPrimitive;
use vn_core::AtomInterner;

const NODE_BUDGET: usize = 96;

pub struct InlineSource {
    pub text: String,
    pub name_offset: usize,
}

pub fn inline_source(
    f: &FunctionDecl,
    arena: &AstArena,
    interner: &AtomInterner,
    source: &str,
) -> Option<InlineSource> {
    if f.modifiers.is_async
        || f.modifiers.is_generator
        || f.modifiers.is_declare
        || !f.decorators.is_empty()
        || !f.type_params.is_empty()
    {
        return None;
    }
    for param in &f.params {
        let Pattern::Identifier { .. } = &param.pattern else {
            return None;
        };
        if param.is_rest || param.is_optional || param.default.is_some() {
            return None;
        }
        if !param
            .type_ann
            .as_ref()
            .is_some_and(|ty| portable_type(ty, interner))
        {
            return None;
        }
    }
    if let Some(ret) = &f.return_type
        && !portable_return(ret, interner)
    {
        return None;
    }
    let mut census = Census {
        arena,
        nodes: 0,
        portable: true,
    };
    census.stmt(f.body);
    if !census.portable || census.nodes > NODE_BUDGET {
        return None;
    }
    if !free_reads(arena, &f.params, Body::Stmt(f.body)).is_empty() {
        return None;
    }
    let start = f.range.start.offset as usize;
    let end = f.range.end.offset as usize;
    let text = source.get(start..end)?;
    let name_offset = (f.id_offset as usize).checked_sub(start)?;
    text.starts_with(vn_core::TokenKind::Function.as_str())
        .then(|| InlineSource {
            text: text.to_owned(),
            name_offset,
        })
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
    nodes: usize,
    portable: bool,
}

impl Census<'_> {
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
            ExprKind::Assign { target, value, .. } => {
                self.expr(*target);
                self.expr(*value);
            }
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::Identifier { .. }
            | ExprKind::Template { .. }
            | ExprKind::Array { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Member { .. }
            | ExprKind::Call { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Paren { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::Is { .. } => walk_expr_children(e, arena, &mut |c| self.expr(c)),
        }
    }

    fn stmt(&mut self, s: StmtId) {
        self.nodes += 1;
        let arena = self.arena;
        match &arena.stmt(s).kind {
            StmtKind::Block { stmts } => stmts.iter().for_each(|&x| self.stmt(x)),
            StmtKind::Expr { expression } => self.expr(*expression),
            StmtKind::Decl(d) => match d.as_ref() {
                Decl::Variable(v) => {
                    for declarator in &v.declarators {
                        if let Some(init) = declarator.init {
                            self.expr(init);
                        }
                    }
                }
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
            } => {
                match init.as_deref() {
                    Some(ForInit::Expr(e)) => self.expr(*e),
                    Some(ForInit::Var { declarators, .. }) => {
                        for declarator in declarators {
                            if let Some(init) = declarator.init {
                                self.expr(init);
                            }
                        }
                    }
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
            StmtKind::ForIn { right, body, .. } | StmtKind::ForOf { right, body, .. } => {
                self.expr(*right);
                self.stmt(*body);
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
