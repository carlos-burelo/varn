use varn_core::ast::arena::{AstArena, ExprId, StmtId};
use varn_core::ast::decl::{Decl, ExportDecl};
use varn_core::ast::expr::{
    Arg, ArrayEl, ArrowBody, ExprKind, MatchBody, MatchCase, ObjectProp, PropKey, TemplatePart,
};
use varn_core::ast::pattern::{MatchPattern, Param, Pattern};
use varn_core::ast::stmt::{ForInit, StmtKind};

pub fn method_uses_receiver(arena: &AstArena, params: &[Param], body: StmtId) -> bool {
    params.iter().any(|p| {
        pattern_default_uses_receiver(arena, &p.pattern)
            || p.default.is_some_and(|e| expr_uses_receiver(arena, e))
    }) || stmt_uses_receiver(arena, body)
}

fn stmt_uses_receiver(arena: &AstArena, id: StmtId) -> bool {
    match &arena.stmt(id).kind {
        StmtKind::Block { stmts } => stmts.iter().any(|s| stmt_uses_receiver(arena, *s)),
        StmtKind::Empty => false,
        StmtKind::Error => false,
        StmtKind::Debugger => false,
        StmtKind::Expr { expression } => expr_uses_receiver(arena, *expression),
        StmtKind::Decl(d) => decl_uses_receiver(arena, d),
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            expr_uses_receiver(arena, *test)
                || stmt_uses_receiver(arena, *consequent)
                || alternate.is_some_and(|a| stmt_uses_receiver(arena, a))
        }
        StmtKind::While { test, body } => {
            expr_uses_receiver(arena, *test) || stmt_uses_receiver(arena, *body)
        }
        StmtKind::DoWhile { body, test } => {
            stmt_uses_receiver(arena, *body) || expr_uses_receiver(arena, *test)
        }
        StmtKind::For {
            init,
            test,
            update,
            body,
        } => {
            init.as_ref().is_some_and(|i| match i.as_ref() {
                ForInit::Var { declarators, .. } => declarators
                    .iter()
                    .any(|d| d.init.is_some_and(|e| expr_uses_receiver(arena, e))),
                ForInit::Expr(e) => expr_uses_receiver(arena, *e),
            }) || test.is_some_and(|t| expr_uses_receiver(arena, t))
                || update.is_some_and(|u| expr_uses_receiver(arena, u))
                || stmt_uses_receiver(arena, *body)
        }
        StmtKind::ForIn {
            left, right, body, ..
        } => {
            pattern_default_uses_receiver(arena, left)
                || expr_uses_receiver(arena, *right)
                || stmt_uses_receiver(arena, *body)
        }
        StmtKind::ForOf {
            left, right, body, ..
        } => {
            pattern_default_uses_receiver(arena, left)
                || expr_uses_receiver(arena, *right)
                || stmt_uses_receiver(arena, *body)
        }
        StmtKind::Switch {
            discriminant,
            cases,
        } => {
            expr_uses_receiver(arena, *discriminant)
                || cases.iter().any(|c| {
                    c.test.is_some_and(|t| expr_uses_receiver(arena, t))
                        || c.body.iter().any(|s| stmt_uses_receiver(arena, *s))
                })
        }
        StmtKind::Return { argument } => argument.is_some_and(|e| expr_uses_receiver(arena, e)),
        StmtKind::Break { .. } => false,
        StmtKind::Continue { .. } => false,
        StmtKind::Throw { argument } => expr_uses_receiver(arena, *argument),
        StmtKind::Try {
            block,
            catches,
            finally,
        } => {
            stmt_uses_receiver(arena, *block)
                || catches.iter().any(|c| {
                    c.param
                        .as_ref()
                        .is_some_and(|p| pattern_default_uses_receiver(arena, p))
                        || stmt_uses_receiver(arena, c.body)
                })
                || finally.is_some_and(|f| stmt_uses_receiver(arena, f))
        }
        StmtKind::Using { declarations, .. } => declarations.iter().any(|d| {
            pattern_default_uses_receiver(arena, &d.id)
                || d.init.is_some_and(|e| expr_uses_receiver(arena, e))
        }),
        StmtKind::Labeled { body, .. } => stmt_uses_receiver(arena, *body),
    }
}

fn decl_uses_receiver(arena: &AstArena, d: &Decl) -> bool {
    match d {
        Decl::Variable(v) => v.declarators.iter().any(|x| {
            pattern_default_uses_receiver(arena, &x.id)
                || x.init.is_some_and(|e| expr_uses_receiver(arena, e))
        }),
        Decl::Export(e) => match e {
            ExportDecl::Decl { declaration, .. } => decl_uses_receiver(arena, declaration),
            ExportDecl::Named { .. } | ExportDecl::Default { .. } | ExportDecl::All { .. } => false,
        },
        Decl::Function(_)
        | Decl::Class(_)
        | Decl::Interface(_)
        | Decl::TypeAlias(_)
        | Decl::Enum(_)
        | Decl::Namespace(_)
        | Decl::Import(_)
        | Decl::Extension(_)
        | Decl::Struct(_)
        | Decl::SumType(_) => false,
    }
}

fn expr_uses_receiver(arena: &AstArena, id: ExprId) -> bool {
    match &arena.expr(id).kind {
        ExprKind::IntLiteral { .. }
        | ExprKind::FloatLiteral { .. }
        | ExprKind::BigIntLiteral { .. }
        | ExprKind::DecimalLiteral { .. }
        | ExprKind::StrLiteral { .. }
        | ExprKind::CharLiteral { .. }
        | ExprKind::BoolLiteral { .. }
        | ExprKind::NullLiteral
        | ExprKind::RegexLiteral { .. }
        | ExprKind::Identifier { .. }
        | ExprKind::Missing => false,
        ExprKind::This => true,
        ExprKind::Super => true,
        ExprKind::Template { parts } => parts.iter().any(|p| match p {
            TemplatePart::Literal(_) => false,
            TemplatePart::Interpolation(e) => expr_uses_receiver(arena, *e),
        }),
        ExprKind::TaggedTemplate { tag, template } => {
            expr_uses_receiver(arena, *tag) || expr_uses_receiver(arena, *template)
        }
        ExprKind::Array { elements } => elements.iter().any(|el| match el {
            ArrayEl::Expr(e) | ArrayEl::Spread(e) => expr_uses_receiver(arena, *e),
            ArrayEl::Hole => false,
        }),
        ExprKind::Object { properties } | ExprKind::Record { properties } => {
            properties.iter().any(|p| prop_uses_receiver(arena, p))
        }
        ExprKind::Tuple { elements } => elements.iter().any(|e| expr_uses_receiver(arena, *e)),
        ExprKind::Unary { operand, .. } | ExprKind::Update { operand, .. } => {
            expr_uses_receiver(arena, *operand)
        }
        ExprKind::Binary { left, right, .. } | ExprKind::Logical { left, right, .. } => {
            expr_uses_receiver(arena, *left) || expr_uses_receiver(arena, *right)
        }
        ExprKind::Assign { target, value, .. } => {
            expr_uses_receiver(arena, *target) || expr_uses_receiver(arena, *value)
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            expr_uses_receiver(arena, *test)
                || expr_uses_receiver(arena, *consequent)
                || expr_uses_receiver(arena, *alternate)
        }
        ExprKind::Member {
            object, property, ..
        } => expr_uses_receiver(arena, *object) || expr_uses_receiver(arena, *property),
        ExprKind::Call { callee, args, .. } | ExprKind::New { callee, args, .. } => {
            expr_uses_receiver(arena, *callee)
                || args.iter().any(|a| match a {
                    Arg::Positional(e) | Arg::Spread(e) => expr_uses_receiver(arena, *e),
                    Arg::Named { value, .. } => expr_uses_receiver(arena, *value),
                })
        }
        ExprKind::Function { .. } => false,
        ExprKind::Arrow { body, .. } => match body.as_ref() {
            ArrowBody::Expr(e) => expr_uses_receiver(arena, *e),
            ArrowBody::Block(s) => stmt_uses_receiver(arena, *s),
        },
        ExprKind::Sequence { expressions } => {
            expressions.iter().any(|e| expr_uses_receiver(arena, *e))
        }
        ExprKind::Paren { expression }
        | ExprKind::Await {
            argument: expression,
        }
        | ExprKind::Spawn {
            argument: expression,
        }
        | ExprKind::Spread {
            argument: expression,
        }
        | ExprKind::NonNull { expression }
        | ExprKind::Try { expression }
        | ExprKind::As { expression, .. }
        | ExprKind::Satisfies { expression, .. }
        | ExprKind::Is { expression, .. } => expr_uses_receiver(arena, *expression),
        ExprKind::Yield { argument, .. } => argument.is_some_and(|e| expr_uses_receiver(arena, e)),
        ExprKind::Pipeline { left, right }
        | ExprKind::Range {
            start: left,
            end: right,
            ..
        } => expr_uses_receiver(arena, *left) || expr_uses_receiver(arena, *right),
        ExprKind::ClassExpr { .. } => false,
        ExprKind::Match { subject, cases } => {
            expr_uses_receiver(arena, *subject)
                || cases.iter().any(|c| match_case_uses_receiver(arena, c))
        }
        ExprKind::With { object, properties } => {
            expr_uses_receiver(arena, *object)
                || properties.iter().any(|p| prop_uses_receiver(arena, p))
        }
        ExprKind::MetaAccess { target, .. } => expr_uses_receiver(arena, *target),
    }
}

fn prop_uses_receiver(arena: &AstArena, p: &ObjectProp) -> bool {
    match p {
        ObjectProp::Property { key, value, .. } => {
            key_uses_receiver(arena, key) || expr_uses_receiver(arena, *value)
        }
        ObjectProp::Method { .. } | ObjectProp::Getter { .. } | ObjectProp::Setter { .. } => false,
        ObjectProp::Spread { argument, .. } => expr_uses_receiver(arena, *argument),
    }
}

fn key_uses_receiver(arena: &AstArena, k: &PropKey) -> bool {
    match k {
        PropKey::Computed(e) => expr_uses_receiver(arena, *e),
        PropKey::Identifier(_) | PropKey::Str(_) | PropKey::Int(_) => false,
    }
}

fn match_case_uses_receiver(arena: &AstArena, c: &MatchCase) -> bool {
    (match &c.pattern {
        MatchPattern::Literal(e) => expr_uses_receiver(arena, *e),
        MatchPattern::Wildcard
        | MatchPattern::Identifier(_)
        | MatchPattern::Record { .. }
        | MatchPattern::Sequence(_)
        | MatchPattern::Type { .. }
        | MatchPattern::EnumVariant { .. } => false,
    }) || c.guard.is_some_and(|g| expr_uses_receiver(arena, g))
        || match &c.body {
            MatchBody::Block(s) => stmt_uses_receiver(arena, *s),
            MatchBody::Expr(e) => expr_uses_receiver(arena, *e),
        }
}

fn pattern_default_uses_receiver(arena: &AstArena, p: &Pattern) -> bool {
    match p {
        Pattern::Identifier { .. } => false,
        Pattern::Array { elements, rest, .. } => {
            elements.iter().any(|el| {
                el.as_ref()
                    .is_some_and(|e| pattern_default_uses_receiver(arena, &e.pattern))
            }) || rest
                .as_ref()
                .is_some_and(|r| pattern_default_uses_receiver(arena, r.as_ref()))
        }
        Pattern::Object {
            properties, rest, ..
        } => {
            properties
                .iter()
                .any(|pr| pattern_default_uses_receiver(arena, &pr.value))
                || rest
                    .as_ref()
                    .is_some_and(|r| pattern_default_uses_receiver(arena, r.as_ref()))
        }
        Pattern::Assignment { left, right, .. } => {
            pattern_default_uses_receiver(arena, left) || expr_uses_receiver(arena, *right)
        }
        Pattern::Rest { argument, .. } => pattern_default_uses_receiver(arena, argument),
    }
}
