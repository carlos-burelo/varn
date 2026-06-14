use varn_core::ast::{Program, Stmt, Decl, Expr, Arg};
use varn_utilities::chalk::chalk;
use varn_utilities::terminal::{self, Section};

pub fn debug_ast(program: &Program) {
    Section::new("abstract syntax tree").subtitle(&program.filename).color(|c| c.cyan()).print();

    for (i, stmt) in program.body.iter().enumerate() {
        let is_last = i == program.body.len() - 1;
        print_stmt(stmt, "", is_last);
    }

    varn_utilities::terminal::log(chalk(format!("── {} top-level statements ──", program.body.len())).dim());
}

fn print_stmt(stmt: &Stmt, indent: &str, is_last: bool) {
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    match &stmt.kind {
        varn_core::ast::StmtKind::Block { stmts, .. } => {
            terminal::log(format!("{indent}{marker}{} [L{}]", chalk("BlockStmt").bold(), stmt.range.start.line));
            for (i, s) in stmts.iter().enumerate() {
                print_stmt(s, &child_indent, i == stmts.len() - 1);
            }
        }
        varn_core::ast::StmtKind::Decl(decl) => print_decl(decl, indent, is_last),
        varn_core::ast::StmtKind::Expr { expression, .. } => {
            terminal::log(format!("{indent}{marker}{} [L{}]", chalk("ExprStmt").bold(), stmt.range.start.line));
            print_expr(expression, &child_indent, true);
        }
        varn_core::ast::StmtKind::Return { argument, .. } => {
            terminal::log(format!("{indent}{marker}{} [L{}]", chalk("ReturnStmt").bold(), stmt.range.start.line));
            if let Some(ref e) = argument {
                print_expr(e, &child_indent, true);
            }
        }
        varn_core::ast::StmtKind::If { test, consequent, alternate, .. } => {
            terminal::log(format!("{indent}{marker}{} [L{}]", chalk("IfStmt").bold(), stmt.range.start.line));
            print_expr(test, &child_indent, false);
            if let Some(alt) = alternate {
                print_stmt(consequent, &child_indent, false);
                print_stmt(alt, &child_indent, true);
            } else {
                print_stmt(consequent, &child_indent, true);
            }
        }
        _ => terminal::log(format!("{indent}{marker}{} [L{}]", chalk(format!("{:?}", stmt.kind)).dim(), stmt.range.start.line)),
    }
}

fn print_decl(decl: &Decl, indent: &str, is_last: bool) {
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    match decl {
        Decl::Function(f) => {
            terminal::log(format!("{indent}{marker}{} {} [L{}]", chalk("FunctionDecl").blue().bold(), chalk(&f.id).yellow(), decl.range().start.line));
            print_stmt(&f.body, &child_indent, true);
        }
        Decl::Variable(v) => {
            terminal::log(format!("{indent}{marker}{} {} [L{}]", chalk("VariableDecl").blue().bold(), chalk(format!("({:?})", v.kind)).dim(), decl.range().start.line));
            for (i, d) in v.declarators.iter().enumerate() {
                let d_is_last = i == v.declarators.len() - 1;
                let d_marker = if d_is_last { "└── " } else { "├── " };
                let d_indent = format!("{child_indent}{}", if d_is_last { "    " } else { "│   " });

                let id_name = match &d.id {
                    varn_core::ast::Pattern::Identifier { name, .. } => name.as_str(),
                    _ => "pattern",
                };
                terminal::log(format!("{child_indent}{d_marker}{} {} [L{}]", chalk("Var").bold(), chalk(id_name).yellow(), d.range.start.line));

                if let Some(init) = &d.init {
                    print_expr(init, &d_indent, true);
                }
            }
        }
        _ => terminal::log(format!("{indent}{marker}{} {} [L{}]", chalk("Decl").blue().bold(), chalk(format!("{:?}", decl)).dim(), decl.range().start.line)),
    }
}

fn print_expr(expr: &Expr, indent: &str, is_last: bool) {
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    match &expr.kind {
        varn_core::ast::ExprKind::Identifier { name, .. } => {
            terminal::log(format!("{indent}{marker}{} {} [L{}]", chalk(name).yellow(), chalk("(id)").dim(), expr.range.start.line));
        }
        varn_core::ast::ExprKind::IntLiteral { value, .. } => {
            terminal::log(format!("{indent}{marker}{} {} [L{}]", chalk(value).blue(), chalk("(int)").dim(), expr.range.start.line));
        }
        varn_core::ast::ExprKind::FloatLiteral { value, .. } => {
            terminal::log(format!("{indent}{marker}{} {} [L{}]", chalk(value).blue(), chalk("(float)").dim(), expr.range.start.line));
        }
        varn_core::ast::ExprKind::StrLiteral { value, .. } => {
            terminal::log(format!("{indent}{marker}{} {} [L{}]", chalk(format!("\"{value}\"")), chalk("(str)").dim(), expr.range.start.line));
        }
        varn_core::ast::ExprKind::BoolLiteral { value, .. } => {
            terminal::log(format!("{indent}{marker}{} {} [L{}]", chalk(value).blue(), chalk("(bool)").dim(), expr.range.start.line));
        }
        varn_core::ast::ExprKind::NullLiteral { .. } => {
            terminal::log(format!("{indent}{marker}{} {} [L{}]", chalk("null").blue(), chalk("(null)").dim(), expr.range.start.line));
        }
        varn_core::ast::ExprKind::Binary { left, op, right, .. } => {
            terminal::log(format!("{indent}{marker}{} {} [L{}]", chalk(format!("{:?}", op)).green(), chalk("(binary)").dim(), expr.range.start.line));
            print_expr(left, &child_indent, false);
            print_expr(right, &child_indent, true);
        }
        varn_core::ast::ExprKind::Unary { op, operand, .. } => {
            terminal::log(format!("{indent}{marker}{} {} [L{}]", chalk(format!("{:?}", op)).green(), chalk("(unary)").dim(), expr.range.start.line));
            print_expr(operand, &child_indent, true);
        }
        varn_core::ast::ExprKind::Call { callee, args, .. } => {
            terminal::log(format!("{indent}{marker}{} [L{}]", chalk("Call").bold(), expr.range.start.line));
            print_expr(callee, &child_indent, args.is_empty());
            for (i, arg) in args.iter().enumerate() {
                let arg_is_last = i == args.len() - 1;
                match arg {
                    Arg::Positional(e) => print_expr(e, &child_indent, arg_is_last),
                    Arg::Named { label, value } => {
                        let a_marker = if arg_is_last { "└── " } else { "├── " };
                        terminal::log(format!("{child_indent}{a_marker}{} [L{}]", chalk(format!("{label}:")).dim(), value.range.start.line));
                        let a_indent = format!("{child_indent}{}", if arg_is_last { "    " } else { "│   " });
                        print_expr(value, &a_indent, true);
                    }
                    Arg::Spread(e) => {
                        let a_marker = if arg_is_last { "└── " } else { "├── " };
                        terminal::log(format!("{child_indent}{a_marker}{} [L{}]", chalk("...").dim(), e.range.start.line));
                        let a_indent = format!("{child_indent}{}", if arg_is_last { "    " } else { "│   " });
                        print_expr(e, &a_indent, true);
                    }
                }
            }
        }
        varn_core::ast::ExprKind::Member { object, property, .. } => {
            terminal::log(format!("{indent}{marker}{} [L{}]", chalk("Member").bold(), expr.range.start.line));
            print_expr(object, &child_indent, false);
            print_expr(property, &child_indent, true);
        }
        _ => {
            terminal::log(format!("{indent}{marker}{} [L{}]", chalk(format!("{:?}", expr.kind)).dim(), expr.range.start.line));
        }
    }
}
