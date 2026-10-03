use varn_core::ast::{AstArena, ExprId, ExprKind};
use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_core::AtomInterner;

pub(super) fn try_print(
    expr_id: ExprId,
    arena: &AstArena,
    indent: &str,
    marker: &str,
    interner: &AtomInterner,
) -> bool {
    match &arena.expr(expr_id).kind {
        ExprKind::IntLiteral { value, .. } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk(value).yellow(),
                chalk("(int)").dim()
            ));
            true
        }
        ExprKind::FloatLiteral { value, .. } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk(value).yellow(),
                chalk("(float)").dim()
            ));
            true
        }
        ExprKind::BigIntLiteral { raw } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk(interner.resolve(*raw)).yellow(),
                chalk("(bigint)").dim()
            ));
            true
        }
        ExprKind::DecimalLiteral { raw } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk(interner.resolve(*raw)).yellow(),
                chalk("(decimal)").dim()
            ));
            true
        }
        ExprKind::StrLiteral { value } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk(format!("{value:?}")).yellow(),
                chalk("(str)").dim()
            ));
            true
        }
        ExprKind::CharLiteral { value } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk(format!("'{value}'")),
                chalk("(char)").dim()
            ));
            true
        }
        ExprKind::BoolLiteral { value } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk(value).yellow(),
                chalk("(bool)").dim()
            ));
            true
        }
        ExprKind::NullLiteral => {
            terminal::log(format!("{indent}{marker}{}", chalk("null").yellow()));
            true
        }
        ExprKind::RegexLiteral { pattern, flags } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk(format!("/{pattern}/{flags}")).yellow(),
                chalk("(regex)").dim()
            ));
            true
        }
        ExprKind::Identifier { name } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk(interner.resolve(*name)).cyan(),
                chalk("(id)").dim()
            ));
            true
        }
        ExprKind::This => {
            terminal::log(format!("{indent}{marker}{}", chalk("this").cyan()));
            true
        }
        ExprKind::Super => {
            terminal::log(format!("{indent}{marker}{}", chalk("super").cyan()));
            true
        }
        _ => false,
    }
}
