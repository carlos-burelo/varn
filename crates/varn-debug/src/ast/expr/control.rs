use varn_core::ast::{ArrowBody, AstArena, Decl, ExprId, ExprKind, MatchBody, TemplatePart};
use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_core::AtomInterner;

use super::super::decl::print_decl;
use super::super::print_stmt;
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
        ExprKind::Match { subject, cases } => {
            let subject = *subject;
            terminal::log(format!("{indent}{marker}{}", chalk("MatchExpr").bold()));
            print_expr(subject, arena, child_indent, cases.is_empty(), interner);
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
            true
        }
        ExprKind::Arrow { body, .. } => {
            terminal::log(format!("{indent}{marker}{}", chalk("ArrowFunc").bold()));
            match body.as_ref() {
                ArrowBody::Block(s) => print_stmt(*s, arena, child_indent, true, interner),
                ArrowBody::Expr(e) => print_expr(*e, arena, child_indent, true, interner),
            }
            true
        }
        ExprKind::Function { fn_id, body, .. } => {
            let body = *body;
            let name = fn_id.map(|a| interner.resolve(a)).unwrap_or("<anonymous>");
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("FunctionExpr").bold(),
                chalk(name).blue()
            ));
            print_stmt(body, arena, child_indent, true, interner);
            true
        }
        ExprKind::Await { argument } => {
            let argument = *argument;
            terminal::log(format!("{indent}{marker}{}", chalk("Await").bold()));
            print_expr(argument, arena, child_indent, true, interner);
            true
        }
        ExprKind::Spawn { argument } => {
            let argument = *argument;
            terminal::log(format!("{indent}{marker}{}", chalk("Spawn").bold()));
            print_expr(argument, arena, child_indent, true, interner);
            true
        }
        ExprKind::Yield {
            argument, delegate, ..
        } => {
            let (argument, delegate) = (*argument, *delegate);
            let d = if delegate { "*" } else { "" };
            terminal::log(format!("{indent}{marker}{}{d}", chalk("Yield").bold()));
            if let Some(a) = argument {
                print_expr(a, arena, child_indent, true, interner);
            }
            true
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
                        print_expr(*e, arena, child_indent, is_l, interner)
                    }
                }
            }
            true
        }
        ExprKind::TaggedTemplate { tag, template, .. } => {
            let (tag, template) = (*tag, *template);
            terminal::log(format!(
                "{indent}{marker}{}",
                chalk("TaggedTemplate").bold()
            ));
            print_expr(tag, arena, child_indent, false, interner);
            print_expr(template, arena, child_indent, true, interner);
            true
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
        | ExprKind::Member { .. }
        | ExprKind::Call { .. }
        | ExprKind::New { .. }
        | ExprKind::Sequence { .. }
        | ExprKind::Paren { .. }
        | ExprKind::Spread { .. }
        | ExprKind::Pipeline { .. }
        | ExprKind::Range { .. }
        | ExprKind::NonNull { .. }
        | ExprKind::Try { .. }
        | ExprKind::As { .. }
        | ExprKind::Satisfies { .. }
        | ExprKind::Is { .. }
        | ExprKind::With { .. }
        | ExprKind::MetaAccess { .. } => false,
    }
}
