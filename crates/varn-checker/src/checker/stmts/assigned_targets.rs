use crate::checker::definite_assignment::walk_expr_children;
use rustc_hash::FxHashSet;
use varn_core::ast::{AstArena, Decl, ExprId, ExprKind, ForInit, StmtId, StmtKind};
use varn_core::Atom;

pub(crate) enum MemberRoot {
    This(ExprId),
    Name(ExprId, Atom),
}

pub(crate) struct AssignedTargets {
    pub vars: FxHashSet<Atom>,
    pub members: Vec<(MemberRoot, Vec<Atom>)>,
}

impl AssignedTargets {
    pub(crate) fn empty() -> Self {
        AssignedTargets {
            vars: FxHashSet::default(),
            members: Vec::new(),
        }
    }
}

pub(crate) fn assigned_in_stmt(stmt: StmtId, arena: &AstArena, out: &mut AssignedTargets) {
    match &arena.stmt(stmt).kind {
        StmtKind::Block { stmts } => {
            for s in stmts {
                assigned_in_stmt(*s, arena, out);
            }
        }
        StmtKind::Expr { expression } => assigned_in_expr(*expression, arena, out),
        StmtKind::Decl(decl) => match decl.as_ref() {
            Decl::Variable(v) => {
                for d in &v.declarators {
                    if let Some(init) = d.init {
                        assigned_in_expr(init, arena, out);
                    }
                }
            }
            Decl::Function(_)
            | Decl::Class(_)
            | Decl::Interface(_)
            | Decl::TypeAlias(_)
            | Decl::Enum(_)
            | Decl::Import(_)
            | Decl::Export(_)
            | Decl::Extension(_)
            | Decl::Namespace(_)
            | Decl::Struct(_)
            | Decl::SumType(_) => {}
        },
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            assigned_in_expr(*test, arena, out);
            assigned_in_stmt(*consequent, arena, out);
            if let Some(alt) = alternate {
                assigned_in_stmt(*alt, arena, out);
            }
        }
        StmtKind::While { test, body } | StmtKind::DoWhile { body, test } => {
            assigned_in_expr(*test, arena, out);
            assigned_in_stmt(*body, arena, out);
        }
        StmtKind::For {
            init,
            test,
            update,
            body,
        } => {
            match init.as_deref() {
                Some(ForInit::Var { declarators, .. }) => {
                    for d in declarators {
                        if let Some(init) = d.init {
                            assigned_in_expr(init, arena, out);
                        }
                    }
                }
                Some(ForInit::Expr(e)) => assigned_in_expr(*e, arena, out),
                None => {}
            }
            for e in [test, update].into_iter().flatten() {
                assigned_in_expr(*e, arena, out);
            }
            assigned_in_stmt(*body, arena, out);
        }
        StmtKind::ForIn { right, body, .. } | StmtKind::ForOf { right, body, .. } => {
            assigned_in_expr(*right, arena, out);
            assigned_in_stmt(*body, arena, out);
        }
        StmtKind::Switch {
            discriminant,
            cases,
        } => {
            assigned_in_expr(*discriminant, arena, out);
            for case in cases {
                if let Some(test) = case.test {
                    assigned_in_expr(test, arena, out);
                }
                for s in &case.body {
                    assigned_in_stmt(*s, arena, out);
                }
            }
        }
        StmtKind::Return { argument } => {
            if let Some(a) = argument {
                assigned_in_expr(*a, arena, out);
            }
        }
        StmtKind::Throw { argument } => assigned_in_expr(*argument, arena, out),
        StmtKind::Try {
            block,
            catches,
            finally,
        } => {
            assigned_in_stmt(*block, arena, out);
            for c in catches {
                assigned_in_stmt(c.body, arena, out);
            }
            if let Some(f) = finally {
                assigned_in_stmt(*f, arena, out);
            }
        }
        StmtKind::Using { declarations, .. } => {
            for d in declarations {
                if let Some(init) = d.init {
                    assigned_in_expr(init, arena, out);
                }
            }
        }
        StmtKind::Labeled { body, .. } => assigned_in_stmt(*body, arena, out),
        StmtKind::Empty
        | StmtKind::Error
        | StmtKind::Break { .. }
        | StmtKind::Continue { .. }
        | StmtKind::Debugger => {}
    }
}

pub(crate) fn assigned_in_expr(expr: ExprId, arena: &AstArena, out: &mut AssignedTargets) {
    if let ExprKind::Assign { target, .. } = &arena.expr(expr).kind {
        if let ExprKind::Identifier { name } = &arena.expr(*target).kind {
            out.vars.insert(*name);
        } else if let Some((root, path)) = static_path_atoms(*target, arena) {
            out.members.push((root, path));
        }
    }
    walk_expr_children(expr, arena, &mut |child| {
        assigned_in_expr(child, arena, out)
    });
}

pub(crate) fn static_path_atoms(expr: ExprId, arena: &AstArena) -> Option<(MemberRoot, Vec<Atom>)> {
    let mut names = Vec::new();
    let mut node = expr;
    let root = loop {
        match &arena.expr(node).kind {
            ExprKind::Member {
                object,
                property,
                computed: false,
                optional: false,
                ..
            } => {
                let ExprKind::Identifier { name } = &arena.expr(*property).kind else {
                    return None;
                };
                names.push(*name);
                node = *object;
            }
            ExprKind::Identifier { name } => {
                break MemberRoot::Name(node, *name);
            }
            ExprKind::This => break MemberRoot::This(node),
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::RegexLiteral { .. }
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Member { .. }
            | ExprKind::Call { .. }
            | ExprKind::New { .. }
            | ExprKind::Function { .. }
            | ExprKind::Arrow { .. }
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
            | ExprKind::ClassExpr { .. }
            | ExprKind::Match { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => return None,
        }
    };
    names.reverse();
    Some((root, names))
}
