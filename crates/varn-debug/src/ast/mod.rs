//! `vn debug ast`: the syntax tree as an indented outline.
//!
//! Statements here, declarations in [`decl`], expressions and the short
//! one-line forms in [`expr`].

mod decl;
mod expr;

use varn_core::ast::{AstArena, ForInit, Program, StmtId, StmtKind};
use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_core::term::terminal::Section;
use varn_core::AtomInterner;

use decl::print_decl;
use expr::{format_expr_short, format_pattern, print_expr};

pub fn debug_ast(program: &Program, arena: &AstArena, interner: &AtomInterner) {
    Section::new("abstract syntax tree")
        .subtitle(&program.filename)
        .color(|c| c.cyan())
        .print();

    for (i, &stmt) in program.body.iter().enumerate() {
        let is_last = i == program.body.len() - 1;
        print_stmt(stmt, arena, "", is_last, interner);
    }

    Section::new("abstract syntax tree")
        .subtitle(format!("{} top-level statements", program.body.len()))
        .close();
}

pub(super) fn print_stmt(
    stmt_id: StmtId,
    arena: &AstArena,
    indent: &str,
    is_last: bool,
    interner: &AtomInterner,
) {
    let stmt = arena.stmt(stmt_id);
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    match &stmt.kind {
        StmtKind::Block { stmts } => {
            terminal::log(format!("{indent}{marker}{}", chalk("BlockStmt").bold()));
            for (i, &s) in stmts.iter().enumerate() {
                print_stmt(s, arena, &child_indent, i == stmts.len() - 1, interner);
            }
        }
        StmtKind::Empty => terminal::log(format!("{indent}{marker}{}", chalk("EmptyStmt").dim())),
        StmtKind::Error => terminal::log(format!(
            "{indent}{marker}{} {}",
            chalk("ErrorStmt").bold(),
            chalk(format!(
                "{}:{}..{}:{}",
                stmt.range.start.line,
                stmt.range.start.column,
                stmt.range.end.line,
                stmt.range.end.column
            ))
            .dim()
        )),
        StmtKind::Expr { expression } => {
            let expression = *expression;
            terminal::log(format!("{indent}{marker}{}", chalk("ExprStmt").bold()));
            print_expr(expression, arena, &child_indent, true, interner);
        }
        StmtKind::Decl(decl) => print_decl(decl, arena, indent, is_last, interner),
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            let (test, consequent, alternate) = (*test, *consequent, *alternate);
            terminal::log(format!("{indent}{marker}{}", chalk("IfStmt").bold()));
            print_expr(test, arena, &child_indent, false, interner);
            print_stmt(
                consequent,
                arena,
                &child_indent,
                alternate.is_none(),
                interner,
            );
            if let Some(alt) = alternate {
                print_stmt(alt, arena, &child_indent, true, interner);
            }
        }
        StmtKind::While { test, body } => {
            let (test, body) = (*test, *body);
            terminal::log(format!("{indent}{marker}{}", chalk("WhileStmt").bold()));
            print_expr(test, arena, &child_indent, false, interner);
            print_stmt(body, arena, &child_indent, true, interner);
        }
        StmtKind::DoWhile { body, test } => {
            let (body, test) = (*body, *test);
            terminal::log(format!("{indent}{marker}{}", chalk("DoWhileStmt").bold()));
            print_stmt(body, arena, &child_indent, false, interner);
            print_expr(test, arena, &child_indent, true, interner);
        }
        StmtKind::For {
            init,
            test,
            update,
            body,
        } => {
            let (test, update, body) = (*test, *update, *body);
            terminal::log(format!("{indent}{marker}{}", chalk("ForStmt").bold()));
            if let Some(i) = init {
                match i.as_ref() {
                    ForInit::Var { kind, declarators } => {
                        terminal::log(format!(
                            "{child_indent}├── {} ({:?})",
                            chalk("Init").bold(),
                            kind
                        ));
                        for (idx, d) in declarators.iter().enumerate() {
                            let m = if idx == declarators.len() - 1 {
                                "└── "
                            } else {
                                "├── "
                            };
                            terminal::log(format!(
                                "{child_indent}│   {m}{}",
                                chalk(format_pattern(&d.id, interner)).yellow()
                            ));
                        }
                    }
                    ForInit::Expr(e) => print_expr(*e, arena, &child_indent, false, interner),
                }
            }
            if let Some(t) = test {
                print_expr(t, arena, &child_indent, false, interner);
            }
            if let Some(u) = update {
                print_expr(u, arena, &child_indent, false, interner);
            }
            print_stmt(body, arena, &child_indent, true, interner);
        }
        StmtKind::ForIn {
            left, right, body, ..
        } => {
            let (right, body) = (*right, *body);
            terminal::log(format!("{indent}{marker}{}", chalk("ForInStmt").bold()));
            terminal::log(format!(
                "{child_indent}├── {}",
                chalk(format_pattern(left, interner)).yellow()
            ));
            print_expr(right, arena, &child_indent, false, interner);
            print_stmt(body, arena, &child_indent, true, interner);
        }
        StmtKind::ForOf {
            left,
            right,
            body,
            is_await,
            ..
        } => {
            let (right, body, is_await) = (*right, *body, *is_await);
            let await_str = if is_await { " await" } else { "" };
            terminal::log(format!(
                "{indent}{marker}{}{}",
                chalk("ForOfStmt").bold(),
                chalk(await_str).dim()
            ));
            terminal::log(format!(
                "{child_indent}├── {}",
                chalk(format_pattern(left, interner)).yellow()
            ));
            print_expr(right, arena, &child_indent, false, interner);
            print_stmt(body, arena, &child_indent, true, interner);
        }
        StmtKind::Switch {
            discriminant,
            cases,
        } => {
            let discriminant = *discriminant;
            terminal::log(format!("{indent}{marker}{}", chalk("SwitchStmt").bold()));
            print_expr(
                discriminant,
                arena,
                &child_indent,
                cases.is_empty(),
                interner,
            );
            for (i, case) in cases.iter().enumerate() {
                let is_l = i == cases.len() - 1;
                let m = if is_l { "└── " } else { "├── " };
                let label = if let Some(t) = &case.test {
                    format_expr_short(*t, arena, interner)
                } else {
                    "default".to_owned()
                };
                terminal::log(format!(
                    "{child_indent}{m}{} {}",
                    chalk("Case").bold(),
                    chalk(label).yellow()
                ));
                let c_indent = format!("{child_indent}{}", if is_l { "    " } else { "│   " });
                for (j, &s) in case.body.iter().enumerate() {
                    print_stmt(s, arena, &c_indent, j == case.body.len() - 1, interner);
                }
            }
        }
        StmtKind::Return { argument } => {
            let argument = *argument;
            terminal::log(format!("{indent}{marker}{}", chalk("ReturnStmt").bold()));
            if let Some(arg) = argument {
                print_expr(arg, arena, &child_indent, true, interner);
            }
        }
        StmtKind::Break { label, .. } => {
            let l = label
                .map(|a| format!(" {}", interner.resolve(a)))
                .unwrap_or_default();
            terminal::log(format!("{indent}{marker}{}{l}", chalk("Break").bold()));
        }
        StmtKind::Continue { label, .. } => {
            let l = label
                .map(|a| format!(" {}", interner.resolve(a)))
                .unwrap_or_default();
            terminal::log(format!("{indent}{marker}{}{l}", chalk("Continue").bold()));
        }
        StmtKind::Throw { argument } => {
            let argument = *argument;
            terminal::log(format!("{indent}{marker}{}", chalk("ThrowStmt").bold()));
            print_expr(argument, arena, &child_indent, true, interner);
        }
        StmtKind::Try {
            block,
            catches,
            finally,
        } => {
            let (block, finally) = (*block, *finally);
            terminal::log(format!("{indent}{marker}{}", chalk("TryStmt").bold()));
            print_stmt(
                block,
                arena,
                &child_indent,
                catches.is_empty() && finally.is_none(),
                interner,
            );
            for (i, c) in catches.iter().enumerate() {
                let is_last = i == catches.len() - 1 && finally.is_none();
                let m = if is_last { "└── " } else { "├── " };
                let param_str = c
                    .param
                    .as_ref()
                    .map(|p| format_pattern(p, interner))
                    .unwrap_or("_".to_owned());
                terminal::log(format!(
                    "{child_indent}{m}{} {}",
                    chalk("Catch").bold(),
                    chalk(param_str).yellow()
                ));
                let c_ind = format!("{child_indent}{}", if is_last { "    " } else { "│   " });
                print_stmt(c.body, arena, &c_ind, true, interner);
            }
            if let Some(f) = finally {
                terminal::log(format!("{child_indent}└── {}", chalk("Finally").bold()));
                let f_ind = format!("{child_indent}    ");
                print_stmt(f, arena, &f_ind, true, interner);
            }
        }
        StmtKind::Using {
            declarations,
            is_await,
        } => {
            let await_str = if *is_await { " await" } else { "" };
            terminal::log(format!(
                "{indent}{marker}{}{}",
                chalk("UsingDecl").bold(),
                chalk(await_str).dim()
            ));
            for (i, d) in declarations.iter().enumerate() {
                if let Some(init) = d.init {
                    print_expr(
                        init,
                        arena,
                        &child_indent,
                        i == declarations.len() - 1,
                        interner,
                    );
                }
            }
        }
        StmtKind::Labeled { label, body } => {
            let body = *body;
            terminal::log(format!(
                "{indent}{marker}{} {}:",
                chalk("Label").bold(),
                chalk(interner.resolve(*label)).cyan()
            ));
            print_stmt(body, arena, &child_indent, true, interner);
        }
        StmtKind::Debugger => {
            terminal::log(format!("{indent}{marker}{}", chalk("Debugger").bold()))
        }
    }
}
