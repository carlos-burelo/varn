use crate::stream::TokenStream;
use varn_core::ast::TypeNode;
use varn_core::TypeKind;

pub(crate) fn parse_literal_type(
    s: &mut TokenStream,
    range: varn_core::SourceRange,
) -> Result<TypeNode, String> {
    use varn_core::ast::{operators::UnaryOp, ExprKind};
    use varn_core::TypeLiteral;
    let expr = crate::expressions::parse_unary_expr(s)?;
    let literal = match &s.arena.expr(expr).kind {
        ExprKind::IntLiteral { value, .. } => TypeLiteral::Int(*value),
        ExprKind::Unary {
            op: UnaryOp::Minus,
            operand,
            ..
        } => match &s.arena.expr(*operand).kind {
            ExprKind::IntLiteral { value, .. } => TypeLiteral::Int(-*value),
            _ => return Err(literal_type_error(range)),
        },
        ExprKind::StrLiteral { value } => {
            let value = value.clone();
            TypeLiteral::Str(s.interner.intern(&value))
        }
        ExprKind::BoolLiteral { value } => TypeLiteral::Bool(*value),
        ExprKind::CharLiteral { value } => TypeLiteral::Char(*value),
        _ => return Err(literal_type_error(range)),
    };
    let full_range = s.span_from(range);
    Ok(s.type_node(full_range, TypeKind::Literal(literal)))
}

pub(crate) fn literal_type_error(range: varn_core::SourceRange) -> String {
    format!(
        "only int, str, bool and char literals can be types at {}:{}",
        range.start.line, range.start.column
    )
}
