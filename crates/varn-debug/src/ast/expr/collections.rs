use varn_core::ast::{ArrayEl, AstArena, ExprId, ExprKind, ObjectProp, PropKey};
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
        ExprKind::Array { elements } => {
            if elements.iter().all(|el| is_simple_array_el(el, arena)) && elements.len() <= 10 {
                let items: Vec<String> = elements
                    .iter()
                    .map(|el| format_array_el_short(el, arena, interner))
                    .collect();
                terminal::log(format!(
                    "{indent}{marker}{} [{}]",
                    chalk("Array").bold(),
                    items.join(", ")
                ));
            } else {
                terminal::log(format!("{indent}{marker}{}", chalk("Array").bold()));
                for (i, el) in elements.iter().enumerate() {
                    let is_l = i == elements.len() - 1;
                    match el {
                        ArrayEl::Hole => terminal::log(format!(
                            "{child_indent}{} {}",
                            if is_l { "└── " } else { "├── " },
                            chalk("<hole>").dim()
                        )),
                        ArrayEl::Expr(e) => print_expr(*e, arena, child_indent, is_l, interner),
                        ArrayEl::Spread(e) => {
                            terminal::log(format!(
                                "{child_indent}{} {}",
                                if is_l { "└── " } else { "├── " },
                                chalk("...").bold()
                            ));
                            print_expr(
                                *e,
                                arena,
                                &format!("{child_indent}{}", if is_l { "    " } else { "│   " }),
                                true,
                                interner,
                            );
                        }
                    }
                }
            }
            true
        }
        ExprKind::Object { properties } => {
            terminal::log(format!("{indent}{marker}{}", chalk("Object").bold()));
            for (i, p) in properties.iter().enumerate() {
                let is_l = i == properties.len() - 1;
                let m = if is_l { "└── " } else { "├── " };
                match p {
                    ObjectProp::Property {
                        key,
                        value,
                        shorthand,
                        ..
                    } => {
                        let k = format_prop_key(key, arena, interner);
                        if *shorthand {
                            terminal::log(format!(
                                "{child_indent}{m}{} {}",
                                chalk(&k).cyan(),
                                chalk("(shorthand)").dim()
                            ));
                        } else {
                            terminal::log(format!("{child_indent}{m}{}:", chalk(&k).cyan()));
                            print_expr(
                                *value,
                                arena,
                                &format!("{child_indent}{}", if is_l { "    " } else { "│   " }),
                                true,
                                interner,
                            );
                        }
                    }
                    ObjectProp::Spread { argument, .. } => {
                        terminal::log(format!("{child_indent}{m}{}", chalk("...").bold()));
                        print_expr(
                            *argument,
                            arena,
                            &format!("{child_indent}{}", if is_l { "    " } else { "│   " }),
                            true,
                            interner,
                        );
                    }
                    ObjectProp::Method { .. }
                    | ObjectProp::Getter { .. }
                    | ObjectProp::Setter { .. } => {
                        terminal::log(format!("{child_indent}{m}{}", chalk("<other prop>").dim()))
                    }
                }
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
        | ExprKind::MetaAccess { .. } => false,
    }
}

fn is_simple_array_el(el: &ArrayEl, arena: &AstArena) -> bool {
    match el {
        ArrayEl::Expr(e) => matches!(
            &arena.expr(*e).kind,
            ExprKind::IntLiteral { .. }
                | ExprKind::FloatLiteral { .. }
                | ExprKind::StrLiteral { .. }
                | ExprKind::BoolLiteral { .. }
                | ExprKind::Identifier { .. }
        ),
        ArrayEl::Hole => true,
        ArrayEl::Spread(_) => false,
    }
}

fn format_array_el_short(el: &ArrayEl, arena: &AstArena, interner: &AtomInterner) -> String {
    match el {
        ArrayEl::Hole => "_".to_owned(),
        ArrayEl::Expr(e) => format_expr_short(*e, arena, interner),
        ArrayEl::Spread(e) => format!("...{}", format_expr_short(*e, arena, interner)),
    }
}

fn format_prop_key(key: &PropKey, arena: &AstArena, interner: &AtomInterner) -> String {
    match key {
        PropKey::Identifier(s) | PropKey::Str(s) => s.clone(),
        PropKey::Int(i) => i.to_string(),
        PropKey::Computed(e) => format!("[{}]", format_expr_short(*e, arena, interner)),
    }
}
