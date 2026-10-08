use rustc_hash::FxHashSet;
use std::sync::Arc;
use varn_core::ast::operators::AssignOp;
use varn_core::ast::{AstArena, ExprId, ExprKind, StmtId, StmtKind};
use varn_core::AtomInterner;

pub(super) fn fields_assigned_on_every_path(
    body: StmtId,
    arena: &AstArena,
    interner: &AtomInterner,
) -> FxHashSet<Arc<str>> {
    let mut out = FxHashSet::default();
    walk_stmt(body, arena, interner, &mut out);
    out
}

fn this_field_target(e: ExprId, arena: &AstArena, interner: &AtomInterner) -> Option<Arc<str>> {
    match &arena.expr(e).kind {
        ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } => {
            if !matches!(arena.expr(*object).kind, ExprKind::This) {
                return None;
            }
            match &arena.expr(*property).kind {
                ExprKind::Identifier { name } => Some(Arc::from(interner.resolve(*name))),
                ExprKind::IntLiteral { .. } | ExprKind::FloatLiteral { .. } | ExprKind::BigIntLiteral { .. } | ExprKind::DecimalLiteral { .. } | ExprKind::StrLiteral { .. } | ExprKind::CharLiteral { .. } | ExprKind::BoolLiteral { .. } | ExprKind::NullLiteral | ExprKind::RegexLiteral { .. } | ExprKind::Template { .. } | ExprKind::TaggedTemplate { .. } | ExprKind::Missing | ExprKind::This | ExprKind::Super | ExprKind::Array { .. } | ExprKind::Object { .. } | ExprKind::Tuple { .. } | ExprKind::Record { .. } | ExprKind::Unary { .. } | ExprKind::Update { .. } | ExprKind::Binary { .. } | ExprKind::Logical { .. } | ExprKind::Assign { .. } | ExprKind::Conditional { .. } | ExprKind::Member { .. } | ExprKind::Call { .. } | ExprKind::New { .. } | ExprKind::Function { .. } | ExprKind::Arrow { .. } | ExprKind::Sequence { .. } | ExprKind::Paren { .. } | ExprKind::Await { .. } | ExprKind::Spawn { .. } | ExprKind::Yield { .. } | ExprKind::Spread { .. } | ExprKind::Pipeline { .. } | ExprKind::Range { .. } | ExprKind::NonNull { .. } | ExprKind::Try { .. } | ExprKind::As { .. } | ExprKind::Satisfies { .. } | ExprKind::ClassExpr { .. } | ExprKind::Match { .. } | ExprKind::Is { .. } | ExprKind::With { .. } | ExprKind::MetaAccess { .. } => None,
            }
        }
        ExprKind::IntLiteral { .. } | ExprKind::FloatLiteral { .. } | ExprKind::BigIntLiteral { .. } | ExprKind::DecimalLiteral { .. } | ExprKind::StrLiteral { .. } | ExprKind::CharLiteral { .. } | ExprKind::BoolLiteral { .. } | ExprKind::NullLiteral | ExprKind::RegexLiteral { .. } | ExprKind::Template { .. } | ExprKind::TaggedTemplate { .. } | ExprKind::Identifier { .. } | ExprKind::Missing | ExprKind::This | ExprKind::Super | ExprKind::Array { .. } | ExprKind::Object { .. } | ExprKind::Tuple { .. } | ExprKind::Record { .. } | ExprKind::Unary { .. } | ExprKind::Update { .. } | ExprKind::Binary { .. } | ExprKind::Logical { .. } | ExprKind::Assign { .. } | ExprKind::Conditional { .. } | ExprKind::Member { .. } | ExprKind::Call { .. } | ExprKind::New { .. } | ExprKind::Function { .. } | ExprKind::Arrow { .. } | ExprKind::Sequence { .. } | ExprKind::Paren { .. } | ExprKind::Await { .. } | ExprKind::Spawn { .. } | ExprKind::Yield { .. } | ExprKind::Spread { .. } | ExprKind::Pipeline { .. } | ExprKind::Range { .. } | ExprKind::NonNull { .. } | ExprKind::Try { .. } | ExprKind::As { .. } | ExprKind::Satisfies { .. } | ExprKind::ClassExpr { .. } | ExprKind::Match { .. } | ExprKind::Is { .. } | ExprKind::With { .. } | ExprKind::MetaAccess { .. } => None,
    }
}

fn walk_expr(e: ExprId, arena: &AstArena, interner: &AtomInterner, out: &mut FxHashSet<Arc<str>>) {
    if let ExprKind::Assign {
        op: AssignOp::Assign,
        target,
        ..
    } = &arena.expr(e).kind
    {
        if let Some(name) = this_field_target(*target, arena, interner) {
            out.insert(name);
        }
    }
}

fn walk_stmt(s: StmtId, arena: &AstArena, interner: &AtomInterner, out: &mut FxHashSet<Arc<str>>) {
    match &arena.stmt(s).kind {
        StmtKind::Block { stmts } => {
            for inner in stmts {
                walk_stmt(*inner, arena, interner, out);
            }
        }
        StmtKind::Expr { expression } => walk_expr(*expression, arena, interner, out),
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            let mut then_set = FxHashSet::default();
            walk_stmt(*consequent, arena, interner, &mut then_set);
            let else_set = match alternate {
                Some(alt) => {
                    let mut s = FxHashSet::default();
                    walk_stmt(*alt, arena, interner, &mut s);
                    s
                }

                None => FxHashSet::default(),
            };
            out.extend(then_set.intersection(&else_set).cloned());
        }

        StmtKind::Empty | StmtKind::Decl(_) | StmtKind::Error | StmtKind::While { .. } | StmtKind::DoWhile { .. } | StmtKind::For { .. } | StmtKind::ForIn { .. } | StmtKind::ForOf { .. } | StmtKind::Switch { .. } | StmtKind::Return { .. } | StmtKind::Break { .. } | StmtKind::Continue { .. } | StmtKind::Throw { .. } | StmtKind::Try { .. } | StmtKind::Using { .. } | StmtKind::Labeled { .. } | StmtKind::Debugger => {}
    }
}
