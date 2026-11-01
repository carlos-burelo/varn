use super::operators::AssignOp;
use super::{ArrayEl, AstArena, ExprId, ExprKind, ObjectProp, PropKey};

pub fn is_destructure_target(arena: &AstArena, target: ExprId) -> bool {
    match &arena.expr(target).kind {
        ExprKind::Array { .. } | ExprKind::Tuple { .. } | ExprKind::Object { .. } => true,
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
        | ExprKind::Identifier { .. }
        | ExprKind::Missing
        | ExprKind::This
        | ExprKind::Super
        | ExprKind::ImportMeta(_)
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
        | ExprKind::MetaAccess { .. } => false,
    }
}

pub fn is_destructuring_assign(arena: &AstArena, op: AssignOp, target: ExprId) -> bool {
    op == AssignOp::Assign && is_destructure_target(arena, target)
}

#[derive(Default)]
pub struct DestructureParts {
    pub leaves: Vec<ExprId>,
    pub reads: Vec<ExprId>,
}

pub fn destructure_parts(arena: &AstArena, target: ExprId) -> DestructureParts {
    let mut parts = DestructureParts::default();
    collect_parts(arena, target, &mut parts);
    parts
}

fn collect_parts(arena: &AstArena, target: ExprId, out: &mut DestructureParts) {
    match &arena.expr(target).kind {
        ExprKind::Array { elements } => {
            for element in elements {
                match element {
                    ArrayEl::Expr(e) | ArrayEl::Spread(e) => collect_parts(arena, *e, out),
                    ArrayEl::Hole => {}
                }
            }
        }
        ExprKind::Tuple { elements } => {
            for e in elements {
                collect_parts(arena, *e, out);
            }
        }
        ExprKind::Object { properties } => {
            for property in properties {
                match property {
                    ObjectProp::Property { key, value, .. } => {
                        if let PropKey::Computed(k) = key {
                            out.reads.push(*k);
                        }
                        collect_parts(arena, *value, out);
                    }
                    ObjectProp::Spread { argument, .. } => collect_parts(arena, *argument, out),
                    ObjectProp::Method { .. } => {}
                }
            }
        }
        ExprKind::Assign {
            op: AssignOp::Assign,
            target: inner,
            value,
        } => {
            out.reads.push(*value);
            collect_parts(arena, *inner, out);
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
        | ExprKind::TaggedTemplate { .. }
        | ExprKind::Identifier { .. }
        | ExprKind::Missing
        | ExprKind::This
        | ExprKind::Super
        | ExprKind::ImportMeta(_)
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
        | ExprKind::MetaAccess { .. } => out.leaves.push(target),
    }
}
