use varn_core::ast::{AstArena, ExprId, ExprKind, Pattern};
use varn_core::AtomInterner;

pub(crate) fn format_expr_short(
    expr_id: ExprId,
    arena: &AstArena,
    interner: &AtomInterner,
) -> String {
    match &arena.expr(expr_id).kind {
        ExprKind::Identifier { name } => interner.resolve(*name).to_owned(),
        ExprKind::IntLiteral { value, .. } => value.to_string(),
        ExprKind::FloatLiteral { value, .. } => value.to_string(),
        ExprKind::StrLiteral { value } => format!("{value:?}"),
        ExprKind::BoolLiteral { value } => value.to_string(),
        ExprKind::Member {
            object,
            property,
            computed,
            ..
        } => {
            let (object, property, computed) = (*object, *property, *computed);
            if !computed {
                if let ExprKind::Identifier { name } = &arena.expr(property).kind {
                    return format!(
                        "{}.{}",
                        format_expr_short(object, arena, interner),
                        interner.resolve(*name)
                    );
                }
            }
            format!("{}[...]", format_expr_short(object, arena, interner))
        }
        ExprKind::BigIntLiteral { .. }
        | ExprKind::DecimalLiteral { .. }
        | ExprKind::CharLiteral { .. }
        | ExprKind::NullLiteral
        | ExprKind::RegexLiteral { .. }
        | ExprKind::Template { .. }
        | ExprKind::TaggedTemplate { .. }
        | ExprKind::Missing
        | ExprKind::This
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
        | ExprKind::MetaAccess { .. } => "...".to_owned(),
    }
}

pub(crate) fn format_pattern(pat: &Pattern, interner: &AtomInterner) -> String {
    match pat {
        Pattern::Identifier { name, .. } => interner.resolve(*name).to_owned(),
        Pattern::Array { .. }
        | Pattern::Object { .. }
        | Pattern::Assignment { .. }
        | Pattern::Rest { .. } => "{...}".to_owned(),
    }
}
