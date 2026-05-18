use crate::colors::{BLUE, BOLD, CYAN, C_AST, DIM, RESET, YELLOW};
use varn_core::ast::{
    Arg, ArrayEl, ArrowBody, ClassMember, Decl, ExportDecl, ExportDefaultDecl, Expr, ExprKind,
    ForInit, InterfaceMember, MatchBody, ObjectProp, Pattern, Program, PropKey, Stmt, StmtKind,
    TemplatePart, VarKind,
};

pub fn debug_ast(program: &Program) {
    use crate::colors::{footer, header};
    header(C_AST, "abstract syntax tree", &program.filename);

    for (i, stmt) in program.body.iter().enumerate() {
        let is_last = i == program.body.len() - 1;
        print_stmt(stmt, "", is_last);
    }

    footer(
        C_AST,
        &format!("{} top-level statements", program.body.len()),
    );
}

fn print_stmt(stmt: &Stmt, indent: &str, is_last: bool) {
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    match &stmt.kind {
        StmtKind::Block { stmts } => {
            eprintln!("{indent}{marker}{BOLD}BlockStmt{RESET}");
            for (i, s) in stmts.iter().enumerate() {
                print_stmt(s, &child_indent, i == stmts.len() - 1);
            }
        }
        StmtKind::Empty => eprintln!("{indent}{marker}{DIM}EmptyStmt{RESET}"),
        StmtKind::Expr { expression } => {
            eprintln!("{indent}{marker}{BOLD}ExprStmt{RESET}");
            print_expr(expression, &child_indent, true);
        }
        StmtKind::Decl(decl) => print_decl(decl, indent, is_last),
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            eprintln!("{indent}{marker}{BOLD}IfStmt{RESET}");
            print_expr(test, &child_indent, false);
            print_stmt(consequent, &child_indent, alternate.is_none());
            if let Some(alt) = alternate {
                print_stmt(alt, &child_indent, true);
            }
        }
        StmtKind::While { test, body } => {
            eprintln!("{indent}{marker}{BOLD}WhileStmt{RESET}");
            print_expr(test, &child_indent, false);
            print_stmt(body, &child_indent, true);
        }
        StmtKind::DoWhile { body, test } => {
            eprintln!("{indent}{marker}{BOLD}DoWhileStmt{RESET}");
            print_stmt(body, &child_indent, false);
            print_expr(test, &child_indent, true);
        }
        StmtKind::For {
            init,
            test,
            update,
            body,
        } => {
            eprintln!("{indent}{marker}{BOLD}ForStmt{RESET}");
            if let Some(i) = init {
                match &**i {
                    ForInit::Var { kind, declarators } => {
                        eprintln!("{child_indent}├── {BOLD}Init{RESET} ({:?})", kind);
                        for (idx, d) in declarators.iter().enumerate() {
                            let m = if idx == declarators.len() - 1 {
                                "└── "
                            } else {
                                "├── "
                            };
                            eprintln!(
                                "{child_indent}│   {m}{YELLOW}{}{RESET}",
                                format_pattern(&d.id)
                            );
                        }
                    }
                    ForInit::Expr(e) => print_expr(e, &child_indent, false),
                }
            }
            if let Some(t) = test {
                print_expr(t, &child_indent, false);
            }
            if let Some(u) = update {
                print_expr(u, &child_indent, false);
            }
            print_stmt(body, &child_indent, true);
        }
        StmtKind::ForIn {
            left, right, body, ..
        } => {
            eprintln!("{indent}{marker}{BOLD}ForInStmt{RESET}");
            eprintln!("{child_indent}├── {YELLOW}{}{RESET}", format_pattern(left));
            print_expr(right, &child_indent, false);
            print_stmt(body, &child_indent, true);
        }
        StmtKind::ForOf {
            left,
            right,
            body,
            is_await,
            ..
        } => {
            let await_str = if *is_await { " await" } else { "" };
            eprintln!("{indent}{marker}{BOLD}ForOfStmt{RESET}{DIM}{await_str}{RESET}");
            eprintln!("{child_indent}├── {YELLOW}{}{RESET}", format_pattern(left));
            print_expr(right, &child_indent, false);
            print_stmt(body, &child_indent, true);
        }
        StmtKind::Switch {
            discriminant,
            cases,
        } => {
            eprintln!("{indent}{marker}{BOLD}SwitchStmt{RESET}");
            print_expr(discriminant, &child_indent, cases.is_empty());
            for (i, case) in cases.iter().enumerate() {
                let is_l = i == cases.len() - 1;
                let m = if is_l { "└── " } else { "├── " };
                let label = if let Some(t) = &case.test {
                    format_expr_short(t)
                } else {
                    "default".to_owned()
                };
                eprintln!("{child_indent}{m}{BOLD}Case{RESET} {YELLOW}{label}{RESET}");
                let c_indent = format!("{child_indent}{}", if is_l { "    " } else { "│   " });
                for (j, s) in case.body.iter().enumerate() {
                    print_stmt(s, &c_indent, j == case.body.len() - 1);
                }
            }
        }
        StmtKind::Return { argument } => {
            eprintln!("{indent}{marker}{BOLD}ReturnStmt{RESET}");
            if let Some(arg) = argument {
                print_expr(arg, &child_indent, true);
            }
        }
        StmtKind::Break { label, .. } => {
            let l = label.as_ref().map(|s| format!(" {s}")).unwrap_or_default();
            eprintln!("{indent}{marker}{BOLD}Break{RESET}{l}");
        }
        StmtKind::Continue { label, .. } => {
            let l = label.as_ref().map(|s| format!(" {s}")).unwrap_or_default();
            eprintln!("{indent}{marker}{BOLD}Continue{RESET}{l}");
        }
        StmtKind::Throw { argument } => {
            eprintln!("{indent}{marker}{BOLD}ThrowStmt{RESET}");
            print_expr(argument, &child_indent, true);
        }
        StmtKind::Try {
            block,
            catch,
            finally,
        } => {
            eprintln!("{indent}{marker}{BOLD}TryStmt{RESET}");
            print_stmt(block, &child_indent, catch.is_none() && finally.is_none());
            if let Some(c) = catch {
                let m = if finally.is_none() {
                    "└── "
                } else {
                    "├── "
                };
                eprintln!(
                    "{child_indent}{m}{BOLD}Catch{RESET} {YELLOW}{}{RESET}",
                    c.param
                        .as_ref()
                        .map(format_pattern)
                        .unwrap_or("_".to_owned())
                );
                let c_ind = format!(
                    "{child_indent}{}",
                    if finally.is_none() { "    " } else { "│   " }
                );
                print_stmt(&c.body, &c_ind, true);
            }
            if let Some(f) = finally {
                eprintln!("{child_indent}└── {BOLD}Finally{RESET}");
                print_stmt(f, &format!("{child_indent}    "), true);
            }
        }
        StmtKind::Using {
            declarations,
            is_await,
        } => {
            let await_str = if *is_await { " await" } else { "" };
            eprintln!("{indent}{marker}{BOLD}UsingDecl{RESET}{DIM}{await_str}{RESET}");
            for (i, d) in declarations.iter().enumerate() {
                if let Some(init) = &d.init {
                    print_expr(init, &child_indent, i == declarations.len() - 1);
                }
            }
        }
        StmtKind::Labeled { label, body } => {
            eprintln!("{indent}{marker}{BOLD}Label{RESET} {CYAN}{label}{RESET}:");
            print_stmt(body, &child_indent, true);
        }
        StmtKind::Debugger => eprintln!("{indent}{marker}{BOLD}Debugger{RESET}"),
    }
}

fn print_decl(decl: &Decl, indent: &str, is_last: bool) {
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    match decl {
        Decl::Variable(v) => {
            let kind = match v.kind {
                VarKind::Let => "Let",
                VarKind::Const => "Const",
            };
            eprintln!("{indent}{marker}{BOLD}VariableDecl ({kind}){RESET}");
            for (i, d) in v.declarators.iter().enumerate() {
                let d_is_last = i == v.declarators.len() - 1;
                let d_marker = if d_is_last {
                    "└── "
                } else {
                    "├── "
                };
                eprintln!(
                    "{child_indent}{d_marker}{BOLD}Var{RESET} {YELLOW}{}{RESET}",
                    format_pattern(&d.id)
                );
                if let Some(init) = &d.init {
                    let d_ind =
                        format!("{child_indent}{}", if d_is_last { "    " } else { "│   " });
                    print_expr(init, &d_ind, true);
                }
            }
        }
        Decl::Function(f) => {
            eprintln!(
                "{indent}{marker}{BOLD}FunctionDecl{RESET} {BLUE}{}{RESET}",
                f.id
            );
            print_stmt(&f.body, &child_indent, true);
        }
        Decl::Class(c) => {
            let name = c.id.as_deref().unwrap_or("<anonymous>");
            eprintln!("{indent}{marker}{BOLD}ClassDecl{RESET} {BLUE}{name}{RESET}");
            for (i, m) in c.body.iter().enumerate() {
                let is_l = i == c.body.len() - 1;
                let mk = if is_l { "└── " } else { "├── " };
                match m {
                    ClassMember::Method { key, .. } => {
                        eprintln!("{child_indent}{mk}{BOLD}Method{RESET} {BLUE}{key}{RESET}")
                    }
                    ClassMember::Property { key, .. } => {
                        eprintln!("{child_indent}{mk}{BOLD}Property{RESET} {CYAN}{key}{RESET}")
                    }
                    ClassMember::Constructor { .. } => {
                        eprintln!("{child_indent}{mk}{BOLD}Constructor{RESET}")
                    }
                    _ => eprintln!("{child_indent}{mk}{DIM}<other member>{RESET}"),
                }
            }
        }
        Decl::Interface(i_node) => {
            eprintln!(
                "{indent}{marker}{BOLD}InterfaceDecl{RESET} {BLUE}{}{RESET}",
                i_node.id
            );
            for (idx, m) in i_node.body.iter().enumerate() {
                let is_l = idx == i_node.body.len() - 1;
                let mk = if is_l { "└── " } else { "├── " };
                match m {
                    InterfaceMember::Property { key, .. } => {
                        eprintln!("{child_indent}{mk}{BOLD}Property{RESET} {CYAN}{key}{RESET}")
                    }
                    InterfaceMember::Method { key, .. } => {
                        eprintln!("{child_indent}{mk}{BOLD}Method{RESET} {BLUE}{key}{RESET}")
                    }
                    InterfaceMember::Callable { .. } => {
                        eprintln!("{child_indent}{mk}{BOLD}Callable{RESET}")
                    }
                    InterfaceMember::Index { .. } => {
                        eprintln!("{child_indent}{mk}{BOLD}Index{RESET}")
                    }
                }
            }
        }
        Decl::Enum(e) => {
            eprintln!(
                "{indent}{marker}{BOLD}EnumDecl{RESET} {BLUE}{}{RESET}",
                e.id
            );
            for (idx, m) in e.members.iter().enumerate() {
                let is_l = idx == e.members.len() - 1;
                let mk = if is_l { "└── " } else { "├── " };
                eprintln!("{child_indent}{mk}{YELLOW}{}{RESET}", m.id);
            }
        }
        Decl::Namespace(n) => {
            eprintln!(
                "{indent}{marker}{BOLD}NamespaceDecl{RESET} {BLUE}{}{RESET}",
                n.id
            );
            for (idx, d) in n.body.iter().enumerate() {
                print_decl(d, &child_indent, idx == n.body.len() - 1);
            }
        }
        Decl::TypeAlias(t) => {
            eprintln!(
                "{indent}{marker}{BOLD}TypeAlias{RESET} {BLUE}{}{RESET}",
                t.id
            );
        }
        Decl::Import(i) => {
            eprintln!(
                "{indent}{marker}{BOLD}Import{RESET} {YELLOW}{:?}{RESET}",
                i.source
            );
        }
        Decl::Export(e) => match e {
            ExportDecl::Decl { declaration, .. } => {
                eprintln!("{indent}{marker}{BOLD}ExportDecl{RESET}");
                print_decl(declaration, &child_indent, true);
            }
            ExportDecl::Default { declaration, .. } => {
                eprintln!("{indent}{marker}{BOLD}ExportDefault{RESET}");
                match &**declaration {
                    ExportDefaultDecl::Class(c) => {
                        print_decl(&Decl::Class(c.clone()), &child_indent, true)
                    }
                    ExportDefaultDecl::Function(f) => {
                        print_decl(&Decl::Function(f.clone()), &child_indent, true)
                    }
                    ExportDefaultDecl::Expr(e) => print_expr(e, &child_indent, true),
                }
            }
            ExportDecl::Named { source, .. } => {
                let s = source
                    .as_ref()
                    .map(|s| format!(" from {s:?}"))
                    .unwrap_or_default();
                eprintln!("{indent}{marker}{BOLD}ExportNamed{RESET}{s}");
            }
            ExportDecl::All { source, .. } => {
                eprintln!("{indent}{marker}{BOLD}ExportAll{RESET} from {source:?}");
            }
        },
        Decl::Extension(e) => {
            let name = e.id.as_deref().unwrap_or("<anonymous>");
            eprintln!("{indent}{marker}{BOLD}ExtensionDecl{RESET} {BLUE}{name}{RESET}");
        }
        Decl::Struct(s) => {
            eprintln!(
                "{indent}{marker}{BOLD}StructDecl{RESET} {BLUE}{}{RESET}",
                s.id
            );
            for (idx, f) in s.fields.iter().enumerate() {
                let mk = if idx == s.fields.len() - 1 {
                    "└── "
                } else {
                    "├── "
                };
                eprintln!("{child_indent}{mk}{CYAN}{}{RESET}", f.name);
            }
        }
        Decl::SumType(s) => {
            eprintln!(
                "{indent}{marker}{BOLD}SumTypeDecl{RESET} {BLUE}{}{RESET}",
                s.id
            );
            for (idx, v) in s.variants.iter().enumerate() {
                let mk = if idx == s.variants.len() - 1 {
                    "└── "
                } else {
                    "├── "
                };
                eprintln!("{child_indent}{mk}{YELLOW}{}{RESET}", v.name);
            }
        }
    }
}

fn print_expr(expr: &Expr, indent: &str, is_last: bool) {
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    match &expr.kind {
        ExprKind::IntLiteral { value, .. } => {
            eprintln!("{indent}{marker}{YELLOW}{value}{RESET} {DIM}(int){RESET}")
        }
        ExprKind::FloatLiteral { value, .. } => {
            eprintln!("{indent}{marker}{YELLOW}{value}{RESET} {DIM}(float){RESET}")
        }
        ExprKind::BigIntLiteral { raw } => {
            eprintln!("{indent}{marker}{YELLOW}{raw}{RESET} {DIM}(bigint){RESET}")
        }
        ExprKind::DecimalLiteral { raw } => {
            eprintln!("{indent}{marker}{YELLOW}{raw}{RESET} {DIM}(decimal){RESET}")
        }
        ExprKind::StrLiteral { value } => {
            eprintln!("{indent}{marker}{YELLOW}{value:?}{RESET} {DIM}(str){RESET}")
        }
        ExprKind::CharLiteral { value } => {
            eprintln!("{indent}{marker}{YELLOW}'{value}'{RESET} {DIM}(char){RESET}")
        }
        ExprKind::BoolLiteral { value } => {
            eprintln!("{indent}{marker}{YELLOW}{value}{RESET} {DIM}(bool){RESET}")
        }
        ExprKind::NullLiteral => eprintln!("{indent}{marker}{YELLOW}null{RESET}"),
        ExprKind::RegexLiteral { pattern, flags } => {
            eprintln!("{indent}{marker}{YELLOW}/{pattern}/{flags}{RESET} {DIM}(regex){RESET}")
        }
        ExprKind::Identifier { name } => {
            eprintln!("{indent}{marker}{CYAN}{name}{RESET} {DIM}(id){RESET}")
        }
        ExprKind::This => eprintln!("{indent}{marker}{CYAN}this{RESET}"),
        ExprKind::Super => eprintln!("{indent}{marker}{CYAN}super{RESET}"),
        ExprKind::Member {
            object,
            property,
            computed,
            ..
        } => {
            if !*computed {
                if let ExprKind::Identifier { name } = &property.kind {
                    eprintln!(
                        "{indent}{marker}{BOLD}Member{RESET} {CYAN}{}.{name}{RESET}",
                        format_expr_short(object)
                    );
                    return;
                }
            }
            eprintln!("{indent}{marker}{BOLD}Member{RESET} (computed)");
            print_expr(object, &child_indent, false);
            print_expr(property, &child_indent, true);
        }
        ExprKind::Call { callee, args, .. } => {
            eprintln!(
                "{indent}{marker}{BOLD}Call{RESET} {BLUE}{}{RESET}",
                format_expr_short(callee)
            );
            for (i, a) in args.iter().enumerate() {
                let e = match a {
                    Arg::Positional(e) | Arg::Spread(e) => e,
                    Arg::Named { value, .. } => value,
                };
                print_expr(e, &child_indent, i == args.len() - 1);
            }
        }
        ExprKind::New { callee, args, .. } => {
            eprintln!(
                "{indent}{marker}{BOLD}New{RESET} {BLUE}{}{RESET}",
                format_expr_short(callee)
            );
            for (i, a) in args.iter().enumerate() {
                let e = match a {
                    Arg::Positional(e) | Arg::Spread(e) => e,
                    Arg::Named { value, .. } => value,
                };
                print_expr(e, &child_indent, i == args.len() - 1);
            }
        }
        ExprKind::Array { elements } => {
            if elements.iter().all(is_simple_array_el) && elements.len() <= 10 {
                let items: Vec<String> = elements.iter().map(format_array_el_short).collect();
                eprintln!("{indent}{marker}{BOLD}Array{RESET} [{}]", items.join(", "));
            } else {
                eprintln!("{indent}{marker}{BOLD}Array{RESET}");
                for (i, el) in elements.iter().enumerate() {
                    let is_l = i == elements.len() - 1;
                    match el {
                        ArrayEl::Hole => eprintln!(
                            "{child_indent}{} {DIM}<hole>{RESET}",
                            if is_l { "└── " } else { "├── " }
                        ),
                        ArrayEl::Expr(e) => print_expr(e, &child_indent, is_l),
                        ArrayEl::Spread(e) => {
                            eprintln!(
                                "{child_indent}{} {BOLD}...{RESET}",
                                if is_l { "└── " } else { "├── " }
                            );
                            print_expr(
                                e,
                                &format!("{child_indent}{}", if is_l { "    " } else { "│   " }),
                                true,
                            );
                        }
                    }
                }
            }
        }
        ExprKind::Object { properties } => {
            eprintln!("{indent}{marker}{BOLD}Object{RESET}");
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
                        let k = format_prop_key(key);
                        if *shorthand {
                            eprintln!("{child_indent}{m}{CYAN}{k}{RESET} {DIM}(shorthand){RESET}");
                        } else {
                            eprintln!("{child_indent}{m}{CYAN}{k}{RESET}:");
                            print_expr(
                                value,
                                &format!("{child_indent}{}", if is_l { "    " } else { "│   " }),
                                true,
                            );
                        }
                    }
                    ObjectProp::Spread { argument, .. } => {
                        eprintln!("{child_indent}{m}{BOLD}...{RESET}");
                        print_expr(
                            argument,
                            &format!("{child_indent}{}", if is_l { "    " } else { "│   " }),
                            true,
                        );
                    }
                    _ => eprintln!("{child_indent}{m}{DIM}<other prop>{RESET}"),
                }
            }
        }
        ExprKind::Unary { op, operand, .. } => {
            eprintln!("{indent}{marker}{BOLD}Unary{RESET} {YELLOW}{op:?}{RESET}");
            print_expr(operand, &child_indent, true);
        }
        ExprKind::Update {
            op,
            operand,
            prefix,
        } => {
            let p = if *prefix { "prefix " } else { "" };
            eprintln!("{indent}{marker}{BOLD}Update{RESET} {DIM}{p}{RESET}{YELLOW}{op:?}{RESET}");
            print_expr(operand, &child_indent, true);
        }
        ExprKind::Binary {
            op, left, right, ..
        } => {
            eprintln!("{indent}{marker}{BOLD}Binary{RESET} {YELLOW}{op:?}{RESET}");
            print_expr(left, &child_indent, false);
            print_expr(right, &child_indent, true);
        }
        ExprKind::Logical {
            op, left, right, ..
        } => {
            eprintln!("{indent}{marker}{BOLD}Logical{RESET} {YELLOW}{op:?}{RESET}");
            print_expr(left, &child_indent, false);
            print_expr(right, &child_indent, true);
        }
        ExprKind::Assign {
            op, target, value, ..
        } => {
            eprintln!("{indent}{marker}{BOLD}Assign{RESET} {YELLOW}{op:?}{RESET}");
            print_expr(target, &child_indent, false);
            print_expr(value, &child_indent, true);
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            eprintln!("{indent}{marker}{BOLD}Ternary{RESET}");
            print_expr(test, &child_indent, false);
            print_expr(consequent, &child_indent, false);
            print_expr(alternate, &child_indent, true);
        }
        ExprKind::Match { subject, cases } => {
            eprintln!("{indent}{marker}{BOLD}MatchExpr{RESET}");
            print_expr(subject, &child_indent, cases.is_empty());
            for (i, c) in cases.iter().enumerate() {
                let is_l = i == cases.len() - 1;
                eprintln!(
                    "{child_indent}{} {BOLD}Case{RESET}",
                    if is_l { "└── " } else { "├── " }
                );
                let c_ind = format!("{child_indent}{}", if is_l { "    " } else { "│   " });
                match &c.body {
                    MatchBody::Block(s) => print_stmt(s, &c_ind, true),
                    MatchBody::Expr(e) => print_expr(e, &c_ind, true),
                }
            }
        }
        ExprKind::Arrow { body, .. } => {
            eprintln!("{indent}{marker}{BOLD}ArrowFunc{RESET}");
            match body.as_ref() {
                ArrowBody::Block(s) => print_stmt(s, &child_indent, true),
                ArrowBody::Expr(e) => print_expr(e, &child_indent, true),
            }
        }
        ExprKind::Function { fn_id, body, .. } => {
            let name = fn_id.as_deref().unwrap_or("<anonymous>");
            eprintln!("{indent}{marker}{BOLD}FunctionExpr{RESET} {BLUE}{name}{RESET}");
            print_stmt(body, &child_indent, true);
        }
        ExprKind::Await { argument } => {
            eprintln!("{indent}{marker}{BOLD}Await{RESET}");
            print_expr(argument, &child_indent, true);
        }
        ExprKind::Spawn { argument } => {
            eprintln!("{indent}{marker}{BOLD}Spawn{RESET}");
            print_expr(argument, &child_indent, true);
        }
        ExprKind::Yield {
            argument, delegate, ..
        } => {
            let d = if *delegate { "*" } else { "" };
            eprintln!("{indent}{marker}{BOLD}Yield{RESET}{d}");
            if let Some(a) = argument {
                print_expr(a, &child_indent, true);
            }
        }
        ExprKind::Template { parts } => {
            eprintln!("{indent}{marker}{BOLD}Template{RESET}");
            for (i, p) in parts.iter().enumerate() {
                let is_l = i == parts.len() - 1;
                let m = if is_l { "└── " } else { "├── " };
                match p {
                    TemplatePart::Literal(s) => eprintln!("{child_indent}{m}{YELLOW}{s:?}{RESET}"),
                    TemplatePart::Interpolation(e) => print_expr(e, &child_indent, is_l),
                }
            }
        }
        ExprKind::TaggedTemplate { tag, template, .. } => {
            eprintln!("{indent}{marker}{BOLD}TaggedTemplate{RESET}");
            print_expr(tag, &child_indent, false);
            print_expr(template, &child_indent, true);
        }
        ExprKind::Pipeline { left, right } => {
            eprintln!("{indent}{marker}{BOLD}Pipeline{RESET}");
            print_expr(left, &child_indent, false);
            print_expr(right, &child_indent, true);
        }
        ExprKind::Range {
            start,
            end,
            inclusive,
        } => {
            let op = if *inclusive { "..=" } else { ".." };
            eprintln!("{indent}{marker}{BOLD}Range{RESET} {YELLOW}{op}{RESET}");
            print_expr(start, &child_indent, false);
            print_expr(end, &child_indent, true);
        }
        ExprKind::NonNull { expression } => {
            eprintln!("{indent}{marker}{BOLD}NonNull{RESET} !");
            print_expr(expression, &child_indent, true);
        }
        ExprKind::Try { expression } => {
            eprintln!("{indent}{marker}{BOLD}TryExpr{RESET} ?");
            print_expr(expression, &child_indent, true);
        }
        ExprKind::As {
            expression,
            type_ann,
        } => {
            eprintln!("{indent}{marker}{BOLD}As{RESET} {DIM}({type_ann:?}){RESET}");
            print_expr(expression, &child_indent, true);
        }
        ExprKind::Satisfies {
            expression,
            type_ann,
        } => {
            eprintln!("{indent}{marker}{BOLD}Satisfies{RESET} {DIM}({type_ann:?}){RESET}");
            print_expr(expression, &child_indent, true);
        }
        ExprKind::Is {
            expression,
            type_ann,
        } => {
            eprintln!("{indent}{marker}{BOLD}Is{RESET} {DIM}({type_ann:?}){RESET}");
            print_expr(expression, &child_indent, true);
        }
        ExprKind::Sequence { expressions } => {
            eprintln!("{indent}{marker}{BOLD}Sequence{RESET}");
            for (i, e) in expressions.iter().enumerate() {
                print_expr(e, &child_indent, i == expressions.len() - 1);
            }
        }
        ExprKind::Paren { expression } => {
            eprintln!("{indent}{marker}{BOLD}Paren{RESET}");
            print_expr(expression, &child_indent, true);
        }
        ExprKind::ClassExpr { declaration } => {
            eprintln!("{indent}{marker}{BOLD}ClassExpr{RESET}");
            print_decl(&Decl::Class(*declaration.clone()), indent, true);
        }
        _ => {
            let label = format!("{:?}", expr.kind)
                .split('{')
                .next()
                .unwrap_or("Expr")
                .trim()
                .to_owned();
            eprintln!("{indent}{marker}{DIM}<{label}>{RESET}");
        }
    }
}

fn format_expr_short(expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Identifier { name } => name.to_string(),
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
            if !*computed {
                if let ExprKind::Identifier { name } = &property.kind {
                    return format!("{}.{name}", format_expr_short(object));
                }
            }
            format!("{}[...]", format_expr_short(object))
        }
        _ => "...".to_owned(),
    }
}

fn is_simple_array_el(el: &ArrayEl) -> bool {
    match el {
        ArrayEl::Expr(e) => matches!(
            &e.kind,
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

fn format_array_el_short(el: &ArrayEl) -> String {
    match el {
        ArrayEl::Hole => "_".to_owned(),
        ArrayEl::Expr(e) => format_expr_short(e),
        ArrayEl::Spread(e) => format!("...{}", format_expr_short(e)),
    }
}

fn format_prop_key(key: &PropKey) -> String {
    match key {
        PropKey::Identifier(s) | PropKey::Str(s) => s.clone(),
        PropKey::Int(i) => i.to_string(),
        PropKey::Computed(e) => format!("[{}]", format_expr_short(e)),
    }
}

fn format_pattern(pat: &Pattern) -> String {
    match pat {
        Pattern::Identifier { name, .. } => name.to_string(),
        _ => "{...}".to_owned(),
    }
}
