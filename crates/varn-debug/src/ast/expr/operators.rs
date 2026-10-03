use varn_core::ast::{AstArena, ExprId, ExprKind};
use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_core::AtomInterner;

use super::print_expr;

pub(super) fn try_print(
    expr_id: ExprId,
    arena: &AstArena,
    indent: &str,
    marker: &str,
    child_indent: &str,
    interner: &AtomInterner,
) -> bool {
    match &arena.expr(expr_id).kind {
        ExprKind::Unary { op, operand, .. } => {
            let operand = *operand;
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Unary").bold(),
                chalk(format!("{op:?}")).yellow()
            ));
            print_expr(operand, arena, child_indent, true, interner);
            true
        }
        ExprKind::Update {
            op,
            operand,
            prefix,
        } => {
            let (operand, prefix) = (*operand, *prefix);
            let p = if prefix { "prefix " } else { "" };
            terminal::log(format!(
                "{indent}{marker}{} {}{}",
                chalk("Update").bold(),
                chalk(p).dim(),
                chalk(format!("{op:?}")).yellow()
            ));
            print_expr(operand, arena, child_indent, true, interner);
            true
        }
        ExprKind::Binary {
            op, left, right, ..
        } => {
            let (left, right) = (*left, *right);
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Binary").bold(),
                chalk(format!("{op:?}")).yellow()
            ));
            print_expr(left, arena, child_indent, false, interner);
            print_expr(right, arena, child_indent, true, interner);
            true
        }
        ExprKind::Logical {
            op, left, right, ..
        } => {
            let (left, right) = (*left, *right);
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Logical").bold(),
                chalk(format!("{op:?}")).yellow()
            ));
            print_expr(left, arena, child_indent, false, interner);
            print_expr(right, arena, child_indent, true, interner);
            true
        }
        ExprKind::Assign {
            op, target, value, ..
        } => {
            let (target, value) = (*target, *value);
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Assign").bold(),
                chalk(format!("{op:?}")).yellow()
            ));
            print_expr(target, arena, child_indent, false, interner);
            print_expr(value, arena, child_indent, true, interner);
            true
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            let (test, consequent, alternate) = (*test, *consequent, *alternate);
            terminal::log(format!("{indent}{marker}{}", chalk("Ternary").bold()));
            print_expr(test, arena, child_indent, false, interner);
            print_expr(consequent, arena, child_indent, false, interner);
            print_expr(alternate, arena, child_indent, true, interner);
            true
        }
        ExprKind::Pipeline { left, right } => {
            let (left, right) = (*left, *right);
            terminal::log(format!("{indent}{marker}{}", chalk("Pipeline").bold()));
            print_expr(left, arena, child_indent, false, interner);
            print_expr(right, arena, child_indent, true, interner);
            true
        }
        ExprKind::Range {
            start,
            end,
            inclusive,
        } => {
            let (start, end, inclusive) = (*start, *end, *inclusive);
            let op = if inclusive { "..=" } else { ".." };
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Range").bold(),
                chalk(op).yellow()
            ));
            print_expr(start, arena, child_indent, false, interner);
            print_expr(end, arena, child_indent, true, interner);
            true
        }
        ExprKind::NonNull { expression } => {
            let expression = *expression;
            terminal::log(format!("{indent}{marker}{} !", chalk("NonNull").bold()));
            print_expr(expression, arena, child_indent, true, interner);
            true
        }
        ExprKind::Try { expression } => {
            let expression = *expression;
            terminal::log(format!("{indent}{marker}{} ?", chalk("TryExpr").bold()));
            print_expr(expression, arena, child_indent, true, interner);
            true
        }
        ExprKind::As {
            expression,
            type_ann,
        } => {
            let expression = *expression;
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("As").bold(),
                chalk(format!("({type_ann:?})")).dim()
            ));
            print_expr(expression, arena, child_indent, true, interner);
            true
        }
        ExprKind::Satisfies {
            expression,
            type_ann,
        } => {
            let expression = *expression;
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Satisfies").bold(),
                chalk(format!("({type_ann:?})")).dim()
            ));
            print_expr(expression, arena, child_indent, true, interner);
            true
        }
        ExprKind::Is {
            expression,
            type_ann,
        } => {
            let expression = *expression;
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Is").bold(),
                chalk(format!("({type_ann:?})")).dim()
            ));
            print_expr(expression, arena, child_indent, true, interner);
            true
        }
        ExprKind::Sequence { expressions } => {
            terminal::log(format!("{indent}{marker}{}", chalk("Sequence").bold()));
            for (i, &e) in expressions.iter().enumerate() {
                print_expr(e, arena, child_indent, i == expressions.len() - 1, interner);
            }
            true
        }
        ExprKind::Paren { expression } => {
            let expression = *expression;
            terminal::log(format!("{indent}{marker}{}", chalk("Paren").bold()));
            print_expr(expression, arena, child_indent, true, interner);
            true
        }
        _ => false,
    }
}
