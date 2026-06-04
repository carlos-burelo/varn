use varn_core::ast::{Program, Stmt, Decl, Expr, Arg};
use varn_utilities::chalk::chalk;
use varn_utilities::terminal::Section;

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

    match stmt {
        Stmt::Block { stmts, .. } => {
            varn_utilities::terminal::log(format!("{indent}{marker}{}", chalk("BlockStmt").bold()));
            for (i, s) in stmts.iter().enumerate() {
                print_stmt(s, &child_indent, i == stmts.len() - 1);
            }
        }
        Stmt::Decl(decl) => print_decl(decl, indent, is_last),
        Stmt::Expr { expression, .. } => {
            varn_utilities::terminal::log(format!("{indent}{marker}{}", chalk("ExprStmt").bold()));
            print_expr(expression, &child_indent, true);
        }
        Stmt::Return { argument, .. } => {
            varn_utilities::terminal::log(format!("{indent}{marker}{}", chalk("ReturnStmt").bold()));
            if let Some(ref e) = argument {
                print_expr(e, &child_indent, true);
            }
        }
        Stmt::If { test, consequent, alternate, .. } => {
            varn_utilities::terminal::log(format!("{indent}{marker}{}", chalk("IfStmt").bold()));
            print_expr(test, &child_indent, false);
            if let Some(alt) = alternate {
                print_stmt(consequent, &child_indent, false);
                print_stmt(alt, &child_indent, true);
            } else {
                print_stmt(consequent, &child_indent, true);
            }
        }
        _ => varn_utilities::terminal::log(format!("{indent}{marker}{}", chalk(format!("{:?}", stmt)).dim())),
    }
}

fn print_decl(decl: &Decl, indent: &str, is_last: bool) {
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    match decl {
        Decl::Function(f) => {
            varn_utilities::terminal::log(format!("{indent}{marker}{} {}", chalk("FunctionDecl").blue().bold(), chalk(&f.id).yellow()));
            print_stmt(&f.body, &child_indent, true);
        }
        Decl::Variable(v) => {
            varn_utilities::terminal::log(format!("{indent}{marker}{} {}", chalk("VariableDecl").blue().bold(), chalk(format!("({:?})", v.kind)).dim()));
            for (i, d) in v.declarators.iter().enumerate() {
                let d_is_last = i == v.declarators.len() - 1;
                let d_marker = if d_is_last { "└── " } else { "├── " };
                let d_indent = format!("{child_indent}{}", if d_is_last { "    " } else { "│   " });

                let id_name = match &d.id {
                    varn_core::ast::Pattern::Identifier { name, .. } => name.as_str(),
                    _ => "pattern",
                };
                varn_utilities::terminal::log(format!("{child_indent}{d_marker}{} {}", chalk("Var").bold(), chalk(id_name).yellow()));

                if let Some(init) = &d.init {
                    print_expr(init, &d_indent, true);
                }
            }
        }
        _ => varn_utilities::terminal::log(format!("{indent}{marker}{} {}", chalk("Decl").blue().bold(), chalk(format!("{:?}", decl)).dim())),
    }
}

fn print_expr(expr: &Expr, indent: &str, is_last: bool) {
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    match expr {
        Expr::Identifier { name, .. } => {
            varn_utilities::terminal::log(format!("{indent}{marker}{} {}", chalk(name).yellow(), chalk("(id)").dim()));
        }
        Expr::IntLiteral { value, .. } => {
            varn_utilities::terminal::log(format!("{indent}{marker}{} {}", chalk(value).blue(), chalk("(int)").dim()));
        }
        Expr::FloatLiteral { value, .. } => {
            varn_utilities::terminal::log(format!("{indent}{marker}{} {}", chalk(value).blue(), chalk("(float)").dim()));
        }
        Expr::StrLiteral { value, .. } => {
            varn_utilities::terminal::log(format!("{indent}{marker}{} {}", chalk(format!("\"{value}\"")), chalk("(str)").dim()));
        }
        Expr::BoolLiteral { value, .. } => {
            varn_utilities::terminal::log(format!("{indent}{marker}{} {}", chalk(value).blue(), chalk("(bool)").dim()));
        }
        Expr::NullLiteral { .. } => {
            varn_utilities::terminal::log(format!("{indent}{marker}{} {}", chalk("null").blue(), chalk("(null)").dim()));
        }
        Expr::Binary { left, op, right, .. } => {
            varn_utilities::terminal::log(format!("{indent}{marker}{} {}", chalk(format!("{:?}", op)).green(), chalk("(binary)").dim()));
            print_expr(left, &child_indent, false);
            print_expr(right, &child_indent, true);
        }
        Expr::Unary { op, operand, .. } => {
            varn_utilities::terminal::log(format!("{indent}{marker}{} {}", chalk(format!("{:?}", op)).green(), chalk("(unary)").dim()));
            print_expr(operand, &child_indent, true);
        }
        Expr::Call { callee, args, .. } => {
            varn_utilities::terminal::log(format!("{indent}{marker}{}", chalk("Call").bold()));
            print_expr(callee, &child_indent, args.is_empty());
            for (i, arg) in args.iter().enumerate() {
                let arg_is_last = i == args.len() - 1;
                match arg {
                    Arg::Positional(e) => print_expr(e, &child_indent, arg_is_last),
                    Arg::Named { label, value } => {
                        let a_marker = if arg_is_last { "└── " } else { "├── " };
                        varn_utilities::terminal::log(format!("{child_indent}{a_marker}{}", chalk(format!("{label}:")).dim()));
                        let a_indent = format!("{child_indent}{}", if arg_is_last { "    " } else { "│   " });
                        print_expr(value, &a_indent, true);
                    }
                    Arg::Spread(e) => {
                        let a_marker = if arg_is_last { "└── " } else { "├── " };
                        varn_utilities::terminal::log(format!("{child_indent}{a_marker}{}", chalk("...").dim()));
                        let a_indent = format!("{child_indent}{}", if arg_is_last { "    " } else { "│   " });
                        print_expr(e, &a_indent, true);
                    }
                }
            }
        }
        Expr::Member { object, property, .. } => {
            varn_utilities::terminal::log(format!("{indent}{marker}{}", chalk("Member").bold()));
            print_expr(object, &child_indent, false);
            print_expr(property, &child_indent, true);
        }
        _ => {
            varn_utilities::terminal::log(format!("{indent}{marker}{}", chalk(format!("{:?}", expr)).dim()));
        }
    }
}
