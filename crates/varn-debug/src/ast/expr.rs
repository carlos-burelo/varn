//! Expressions in the `vn debug ast` outline, and their one-line forms.

use varn_core::ast::{
    Arg, ArrayEl, ArrowBody, AstArena, Decl, ExprId, ExprKind, MatchBody, ObjectProp, Pattern,
    PropKey, TemplatePart,
};
use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_core::AtomInterner;

use super::decl::print_decl;
use super::print_stmt;

pub(super) fn print_expr(
    expr_id: ExprId,
    arena: &AstArena,
    indent: &str,
    is_last: bool,
    interner: &AtomInterner,
) {
    let expr = arena.expr(expr_id);
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    match &expr.kind {
        ExprKind::IntLiteral { value, .. } => terminal::log(format!(
            "{indent}{marker}{} {}",
            chalk(value).yellow(),
            chalk("(int)").dim()
        )),
        ExprKind::FloatLiteral { value, .. } => terminal::log(format!(
            "{indent}{marker}{} {}",
            chalk(value).yellow(),
            chalk("(float)").dim()
        )),
        ExprKind::BigIntLiteral { raw } => terminal::log(format!(
            "{indent}{marker}{} {}",
            chalk(interner.resolve(*raw)).yellow(),
            chalk("(bigint)").dim()
        )),
        ExprKind::DecimalLiteral { raw } => terminal::log(format!(
            "{indent}{marker}{} {}",
            chalk(interner.resolve(*raw)).yellow(),
            chalk("(decimal)").dim()
        )),
        ExprKind::StrLiteral { value } => terminal::log(format!(
            "{indent}{marker}{} {}",
            chalk(format!("{value:?}")).yellow(),
            chalk("(str)").dim()
        )),
        ExprKind::CharLiteral { value } => terminal::log(format!(
            "{indent}{marker}{} {}",
            chalk(format!("'{value}'")),
            chalk("(char)").dim()
        )),
        ExprKind::BoolLiteral { value } => terminal::log(format!(
            "{indent}{marker}{} {}",
            chalk(value).yellow(),
            chalk("(bool)").dim()
        )),
        ExprKind::NullLiteral => {
            terminal::log(format!("{indent}{marker}{}", chalk("null").yellow()))
        }
        ExprKind::RegexLiteral { pattern, flags } => terminal::log(format!(
            "{indent}{marker}{} {}",
            chalk(format!("/{pattern}/{flags}")).yellow(),
            chalk("(regex)").dim()
        )),
        ExprKind::Identifier { name } => terminal::log(format!(
            "{indent}{marker}{} {}",
            chalk(interner.resolve(*name)).cyan(),
            chalk("(id)").dim()
        )),
        ExprKind::This => terminal::log(format!("{indent}{marker}{}", chalk("this").cyan())),
        ExprKind::Super => terminal::log(format!("{indent}{marker}{}", chalk("super").cyan())),
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
                    return;
                }
            }
            terminal::log(format!(
                "{indent}{marker}{} (computed)",
                chalk("Member").bold()
            ));
            print_expr(object, arena, &child_indent, false, interner);
            print_expr(property, arena, &child_indent, true, interner);
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
                print_expr(e, arena, &child_indent, i == args.len() - 1, interner);
            }
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
                print_expr(e, arena, &child_indent, i == args.len() - 1, interner);
            }
        }
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
                        ArrayEl::Expr(e) => print_expr(*e, arena, &child_indent, is_l, interner),
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
                    _ => terminal::log(format!("{child_indent}{m}{}", chalk("<other prop>").dim())),
                }
            }
        }
        ExprKind::Unary { op, operand, .. } => {
            let operand = *operand;
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Unary").bold(),
                chalk(format!("{op:?}")).yellow()
            ));
            print_expr(operand, arena, &child_indent, true, interner);
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
            print_expr(operand, arena, &child_indent, true, interner);
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
            print_expr(left, arena, &child_indent, false, interner);
            print_expr(right, arena, &child_indent, true, interner);
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
            print_expr(left, arena, &child_indent, false, interner);
            print_expr(right, arena, &child_indent, true, interner);
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
            print_expr(target, arena, &child_indent, false, interner);
            print_expr(value, arena, &child_indent, true, interner);
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            let (test, consequent, alternate) = (*test, *consequent, *alternate);
            terminal::log(format!("{indent}{marker}{}", chalk("Ternary").bold()));
            print_expr(test, arena, &child_indent, false, interner);
            print_expr(consequent, arena, &child_indent, false, interner);
            print_expr(alternate, arena, &child_indent, true, interner);
        }
        ExprKind::Match { subject, cases } => {
            let subject = *subject;
            terminal::log(format!("{indent}{marker}{}", chalk("MatchExpr").bold()));
            print_expr(subject, arena, &child_indent, cases.is_empty(), interner);
            for (i, c) in cases.iter().enumerate() {
                let is_l = i == cases.len() - 1;
                terminal::log(format!(
                    "{child_indent}{} {}",
                    if is_l { "└── " } else { "├── " },
                    chalk("Case").bold()
                ));
                let c_ind = format!("{child_indent}{}", if is_l { "    " } else { "│   " });
                match &c.body {
                    MatchBody::Block(s) => print_stmt(*s, arena, &c_ind, true, interner),
                    MatchBody::Expr(e) => print_expr(*e, arena, &c_ind, true, interner),
                }
            }
        }
        ExprKind::Arrow { body, .. } => {
            terminal::log(format!("{indent}{marker}{}", chalk("ArrowFunc").bold()));
            match body.as_ref() {
                ArrowBody::Block(s) => print_stmt(*s, arena, &child_indent, true, interner),
                ArrowBody::Expr(e) => print_expr(*e, arena, &child_indent, true, interner),
            }
        }
        ExprKind::Function { fn_id, body, .. } => {
            let body = *body;
            let name = fn_id.map(|a| interner.resolve(a)).unwrap_or("<anonymous>");
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("FunctionExpr").bold(),
                chalk(name).blue()
            ));
            print_stmt(body, arena, &child_indent, true, interner);
        }
        ExprKind::Await { argument } => {
            let argument = *argument;
            terminal::log(format!("{indent}{marker}{}", chalk("Await").bold()));
            print_expr(argument, arena, &child_indent, true, interner);
        }
        ExprKind::Spawn { argument } => {
            let argument = *argument;
            terminal::log(format!("{indent}{marker}{}", chalk("Spawn").bold()));
            print_expr(argument, arena, &child_indent, true, interner);
        }
        ExprKind::Yield {
            argument, delegate, ..
        } => {
            let (argument, delegate) = (*argument, *delegate);
            let d = if delegate { "*" } else { "" };
            terminal::log(format!("{indent}{marker}{}{d}", chalk("Yield").bold()));
            if let Some(a) = argument {
                print_expr(a, arena, &child_indent, true, interner);
            }
        }
        ExprKind::Template { parts } => {
            terminal::log(format!("{indent}{marker}{}", chalk("Template").bold()));
            for (i, p) in parts.iter().enumerate() {
                let is_l = i == parts.len() - 1;
                let m = if is_l { "└── " } else { "├── " };
                match p {
                    TemplatePart::Literal(s) => terminal::log(format!(
                        "{child_indent}{m}{}",
                        chalk(format!("{s:?}")).yellow()
                    )),
                    TemplatePart::Interpolation(e) => {
                        print_expr(*e, arena, &child_indent, is_l, interner)
                    }
                }
            }
        }
        ExprKind::TaggedTemplate { tag, template, .. } => {
            let (tag, template) = (*tag, *template);
            terminal::log(format!(
                "{indent}{marker}{}",
                chalk("TaggedTemplate").bold()
            ));
            print_expr(tag, arena, &child_indent, false, interner);
            print_expr(template, arena, &child_indent, true, interner);
        }
        ExprKind::Pipeline { left, right } => {
            let (left, right) = (*left, *right);
            terminal::log(format!("{indent}{marker}{}", chalk("Pipeline").bold()));
            print_expr(left, arena, &child_indent, false, interner);
            print_expr(right, arena, &child_indent, true, interner);
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
            print_expr(start, arena, &child_indent, false, interner);
            print_expr(end, arena, &child_indent, true, interner);
        }
        ExprKind::NonNull { expression } => {
            let expression = *expression;
            terminal::log(format!("{indent}{marker}{} !", chalk("NonNull").bold()));
            print_expr(expression, arena, &child_indent, true, interner);
        }
        ExprKind::Try { expression } => {
            let expression = *expression;
            terminal::log(format!("{indent}{marker}{} ?", chalk("TryExpr").bold()));
            print_expr(expression, arena, &child_indent, true, interner);
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
            print_expr(expression, arena, &child_indent, true, interner);
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
            print_expr(expression, arena, &child_indent, true, interner);
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
            print_expr(expression, arena, &child_indent, true, interner);
        }
        ExprKind::Sequence { expressions } => {
            terminal::log(format!("{indent}{marker}{}", chalk("Sequence").bold()));
            for (i, &e) in expressions.iter().enumerate() {
                print_expr(
                    e,
                    arena,
                    &child_indent,
                    i == expressions.len() - 1,
                    interner,
                );
            }
        }
        ExprKind::Paren { expression } => {
            let expression = *expression;
            terminal::log(format!("{indent}{marker}{}", chalk("Paren").bold()));
            print_expr(expression, arena, &child_indent, true, interner);
        }
        ExprKind::ClassExpr { declaration } => {
            terminal::log(format!("{indent}{marker}{}", chalk("ClassExpr").bold()));
            print_decl(
                &Decl::Class(*declaration.clone()),
                arena,
                indent,
                true,
                interner,
            );
        }
        _ => {
            let label = format!("{:?}", expr.kind)
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
    }
}

pub(super) fn format_expr_short(
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
        _ => "...".to_owned(),
    }
}

pub(super) fn is_simple_array_el(el: &ArrayEl, arena: &AstArena) -> bool {
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
        _ => false,
    }
}

pub(super) fn format_array_el_short(
    el: &ArrayEl,
    arena: &AstArena,
    interner: &AtomInterner,
) -> String {
    match el {
        ArrayEl::Hole => "_".to_owned(),
        ArrayEl::Expr(e) => format_expr_short(*e, arena, interner),
        ArrayEl::Spread(e) => format!("...{}", format_expr_short(*e, arena, interner)),
    }
}

pub(super) fn format_prop_key(key: &PropKey, arena: &AstArena, interner: &AtomInterner) -> String {
    match key {
        PropKey::Identifier(s) | PropKey::Str(s) => s.clone(),
        PropKey::Int(i) => i.to_string(),
        PropKey::Computed(e) => format!("[{}]", format_expr_short(*e, arena, interner)),
    }
}

pub(super) fn format_pattern(pat: &Pattern, interner: &AtomInterner) -> String {
    match pat {
        Pattern::Identifier { name, .. } => interner.resolve(*name).to_owned(),
        _ => "{...}".to_owned(),
    }
}
