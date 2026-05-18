use varn_core::ast::{Program, Stmt, Decl, Expr, Arg};
use crate::pipeline::colors::{BOLD, DIM, RESET, YELLOW, BLUE, C_AST, GREEN};

pub fn debug_ast(program: &Program) {
    use super::super::colors::{footer, header};
    header(C_AST, "abstract syntax tree", &program.filename);

    for (i, stmt) in program.body.iter().enumerate() {
        let is_last = i == program.body.len() - 1;
        print_stmt(stmt, "", is_last);
    }

    footer(C_AST, &format!("{} top-level statements", program.body.len()));
}

fn print_stmt(stmt: &Stmt, indent: &str, is_last: bool) {
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    match stmt {
        Stmt::Block { stmts, .. } => {
            eprintln!("{indent}{marker}{BOLD}BlockStmt{RESET}");
            for (i, s) in stmts.iter().enumerate() {
                print_stmt(s, &child_indent, i == stmts.len() - 1);
            }
        }
        Stmt::Decl(decl) => print_decl(decl, indent, is_last),
        Stmt::Expr { expression, .. } => {
            eprintln!("{indent}{marker}{BOLD}ExprStmt{RESET}");
            print_expr(expression, &child_indent, true);
        }
        Stmt::Return { argument, .. } => {
            eprintln!("{indent}{marker}{BOLD}ReturnStmt{RESET}");
            if let Some(ref e) = argument {
                print_expr(e, &child_indent, true);
            }
        }
        Stmt::If { test, consequent, alternate, .. } => {
            eprintln!("{indent}{marker}{BOLD}IfStmt{RESET}");
            print_expr(test, &child_indent, false);
            if let Some(alt) = alternate {
                print_stmt(consequent, &child_indent, false);
                print_stmt(alt, &child_indent, true);
            } else {
                print_stmt(consequent, &child_indent, true);
            }
        }
        _ => eprintln!("{indent}{marker}{DIM}{:?}{RESET}", stmt),
    }
}

fn print_decl(decl: &Decl, indent: &str, is_last: bool) {
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });
    
    match decl {
        Decl::Function(f) => {
            eprintln!("{indent}{marker}{BOLD}{BLUE}FunctionDecl{RESET} {YELLOW}{}{RESET}", f.id);
            print_stmt(&f.body, &child_indent, true);
        }
        Decl::Variable(v) => {
            eprintln!("{indent}{marker}{BOLD}{BLUE}VariableDecl{RESET} {DIM}({:?}){RESET}", v.kind);
            for (i, d) in v.declarators.iter().enumerate() {
                let d_is_last = i == v.declarators.len() - 1;
                let d_marker = if d_is_last { "└── " } else { "├── " };
                let d_indent = format!("{child_indent}{}", if d_is_last { "    " } else { "│   " });
                
                let id_name = match &d.id {
                    varn_core::ast::Pattern::Identifier { name, .. } => name.as_str(),
                    _ => "pattern",
                };
                eprintln!("{child_indent}{d_marker}{BOLD}Var{RESET} {YELLOW}{id_name}{RESET}");
                
                if let Some(init) = &d.init {
                    print_expr(init, &d_indent, true);
                }
            }
        }
        _ => eprintln!("{indent}{marker}{BOLD}{BLUE}Decl{RESET} {DIM}{:?}{RESET}", decl),
    }
}

fn print_expr(expr: &Expr, indent: &str, is_last: bool) {
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    match expr {
        Expr::Identifier { name, .. } => {
            eprintln!("{indent}{marker}{YELLOW}{name}{RESET} {DIM}(id){RESET}");
        }
        Expr::IntLiteral { value, .. } => {
            eprintln!("{indent}{marker}{BLUE}{value}{RESET} {DIM}(int){RESET}");
        }
        Expr::FloatLiteral { value, .. } => {
            eprintln!("{indent}{marker}{BLUE}{value}{RESET} {DIM}(float){RESET}");
        }
        Expr::StrLiteral { value, .. } => {
            eprintln!("{indent}{marker}{BLUE}\"{value}\"{RESET} {DIM}(str){RESET}");
        }
        Expr::BoolLiteral { value, .. } => {
            eprintln!("{indent}{marker}{BLUE}{value}{RESET} {DIM}(bool){RESET}");
        }
        Expr::NullLiteral { .. } => {
            eprintln!("{indent}{marker}{BLUE}null{RESET} {DIM}(null){RESET}");
        }
        Expr::Binary { left, op, right, .. } => {
            eprintln!("{indent}{marker}{GREEN}{:?}{RESET} {DIM}(binary){RESET}", op);
            print_expr(left, &child_indent, false);
            print_expr(right, &child_indent, true);
        }
        Expr::Unary { op, operand, .. } => {
            eprintln!("{indent}{marker}{GREEN}{:?}{RESET} {DIM}(unary){RESET}", op);
            print_expr(operand, &child_indent, true);
        }
        Expr::Call { callee, args, .. } => {
            eprintln!("{indent}{marker}{BOLD}Call{RESET}");
            print_expr(callee, &child_indent, args.is_empty());
            for (i, arg) in args.iter().enumerate() {
                let arg_is_last = i == args.len() - 1;
                match arg {
                    Arg::Positional(e) => print_expr(e, &child_indent, arg_is_last),
                    Arg::Named { label, value } => {
                        let a_marker = if arg_is_last { "└── " } else { "├── " };
                        eprintln!("{child_indent}{a_marker}{DIM}{label}:{RESET}");
                        let a_indent = format!("{child_indent}{}", if arg_is_last { "    " } else { "│   " });
                        print_expr(value, &a_indent, true);
                    }
                    Arg::Spread(e) => {
                        let a_marker = if arg_is_last { "└── " } else { "├── " };
                        eprintln!("{child_indent}{a_marker}{DIM}...{RESET}");
                        let a_indent = format!("{child_indent}{}", if arg_is_last { "    " } else { "│   " });
                        print_expr(e, &a_indent, true);
                    }
                }
            }
        }
        Expr::Member { object, property, .. } => {
            eprintln!("{indent}{marker}{BOLD}Member{RESET}");
            print_expr(object, &child_indent, false);
            print_expr(property, &child_indent, true);
        }
        _ => {
            eprintln!("{indent}{marker}{DIM}{:?}{RESET}", expr);
        }
    }
}
