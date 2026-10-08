use varn_core::ast::{Arg, AstArena, ExprId, ExprKind};
use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_core::AtomInterner;

use super::print_expr;
use super::short::format_expr_short;

pub(super) fn try_print(
    expr_id: ExprId,
    arena: &AstArena,
    indent: &str,
    marker: &str,
    child_indent: &str,
    interner: &AtomInterner,
) -> bool {
    match &arena.expr(expr_id).kind {
        ExprKind::Member {
            object,
            property,
            computed,
            ..
        } => {
            let (object, property, computed) = (*object, *property, *computed);
            if !computed {
                if let ExprKind::Identifier { name } = &arena.expr(property).kind {
                    terminal::log(format!(
                        "{indent}{marker}{} {}",
                        chalk("Member").bold(),
                        chalk(format!(
                            "{}.{}",
                            format_expr_short(object, arena, interner),
                            interner.resolve(*name)
                        ))
                        .cyan()
                    ));
                    return true;
                }
            }
            terminal::log(format!(
                "{indent}{marker}{} (computed)",
                chalk("Member").bold()
            ));
            print_expr(object, arena, child_indent, false, interner);
            print_expr(property, arena, child_indent, true, interner);
            true
        }
        ExprKind::Call { callee, args, .. } => {
            let callee = *callee;
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Call").bold(),
                chalk(format_expr_short(callee, arena, interner)).blue()
            ));
            for (i, a) in args.iter().enumerate() {
                let e = match a {
                    Arg::Positional(e) | Arg::Spread(e) => *e,
                    Arg::Named { value, .. } => *value,
                };
                print_expr(e, arena, child_indent, i == args.len() - 1, interner);
            }
            true
        }
        ExprKind::New { callee, args, .. } => {
            let callee = *callee;
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("New").bold(),
                chalk(format_expr_short(callee, arena, interner)).blue()
            ));
            for (i, a) in args.iter().enumerate() {
                let e = match a {
                    Arg::Positional(e) | Arg::Spread(e) => *e,
                    Arg::Named { value, .. } => *value,
                };
                print_expr(e, arena, child_indent, i == args.len() - 1, interner);
            }
            true
        }
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
        | ExprKind::Identifier { .. }
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
