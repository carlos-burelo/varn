pub(super) mod access;
pub(super) mod collections;
pub(super) mod control;
pub(super) mod literals;
pub(super) mod operators;
pub(super) mod short;

pub(crate) use short::format_pattern;

use vn_core::ast::{AstArena, ExprId};
use vn_core::term::chalk::chalk;
use vn_core::term::terminal;
use vn_core::AtomInterner;

pub(super) fn print_expr(
    expr_id: ExprId,
    arena: &AstArena,
    indent: &str,
    is_last: bool,
    interner: &AtomInterner,
) {
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    if literals::try_print(expr_id, arena, indent, marker, interner) {
        return;
    }
    if access::try_print(expr_id, arena, indent, marker, &child_indent, interner) {
        return;
    }
    if collections::try_print(expr_id, arena, indent, marker, &child_indent, interner) {
        return;
    }
    if operators::try_print(expr_id, arena, indent, marker, &child_indent, interner) {
        return;
    }
    if control::try_print(expr_id, arena, indent, marker, &child_indent, interner) {
        return;
    }

    let label = format!("{:?}", arena.expr(expr_id).kind)
        .split('{')
        .next()
        .unwrap_or("Expr")
        .trim()
        .to_owned();
    terminal::log(format!(
        "{indent}{marker}{}",
        chalk(format!("<{label}>")).dim()
    ));
}
