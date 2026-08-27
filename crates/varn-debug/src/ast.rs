use varn_core::ast::{
    Arg, ArrayEl, ArrowBody, ClassMember, Decl, ExportDecl, ExportDefaultDecl, Expr, ExprKind,
    ForInit, InterfaceMember, MatchBody, ObjectProp, Pattern, Program, PropKey, Stmt, StmtKind,
    TemplatePart, VarKind,
};
use varn_term::chalk::chalk;
use varn_term::terminal;
use varn_term::terminal::Section;

pub fn debug_ast(program: &Program) {
    Section::new("abstract syntax tree")
        .subtitle(&program.filename)
        .color(|c| c.cyan())
        .print();

    for (i, stmt) in program.body.iter().enumerate() {
        let is_last = i == program.body.len() - 1;
        print_stmt(stmt, "", is_last);
    }

    Section::new("abstract syntax tree")
        .subtitle(format!("{} top-level statements", program.body.len()))
        .close();
}

fn print_stmt(stmt: &Stmt, indent: &str, is_last: bool) {
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    match &stmt.kind {
        StmtKind::Block { stmts } => {
            terminal::log(format!("{indent}{marker}{}", chalk("BlockStmt").bold()));
            for (i, s) in stmts.iter().enumerate() {
                print_stmt(s, &child_indent, i == stmts.len() - 1);
            }
        }
        StmtKind::Empty => terminal::log(format!("{indent}{marker}{}", chalk("EmptyStmt").dim())),
        StmtKind::Error => terminal::log(format!(
            "{indent}{marker}{} {}",
            chalk("ErrorStmt").bold(),
            chalk(&format!(
                "{}:{}..{}:{}",
                stmt.range.start.line,
                stmt.range.start.column,
                stmt.range.end.line,
                stmt.range.end.column
            ))
            .dim()
        )),
        StmtKind::Expr { expression } => {
            terminal::log(format!("{indent}{marker}{}", chalk("ExprStmt").bold()));
            print_expr(expression, &child_indent, true);
        }
        StmtKind::Decl(decl) => print_decl(decl, indent, is_last),
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            terminal::log(format!("{indent}{marker}{}", chalk("IfStmt").bold()));
            print_expr(test, &child_indent, false);
            print_stmt(consequent, &child_indent, alternate.is_none());
            if let Some(alt) = alternate {
                print_stmt(alt, &child_indent, true);
            }
        }
        StmtKind::While { test, body } => {
            terminal::log(format!("{indent}{marker}{}", chalk("WhileStmt").bold()));
            print_expr(test, &child_indent, false);
            print_stmt(body, &child_indent, true);
        }
        StmtKind::DoWhile { body, test } => {
            terminal::log(format!("{indent}{marker}{}", chalk("DoWhileStmt").bold()));
            print_stmt(body, &child_indent, false);
            print_expr(test, &child_indent, true);
        }
        StmtKind::For {
            init,
            test,
            update,
            body,
        } => {
            terminal::log(format!("{indent}{marker}{}", chalk("ForStmt").bold()));
            if let Some(i) = init {
                match &**i {
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
                                chalk(format_pattern(&d.id)).yellow()
                            ));
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
            terminal::log(format!("{indent}{marker}{}", chalk("ForInStmt").bold()));
            terminal::log(format!(
                "{child_indent}├── {}",
                chalk(format_pattern(left)).yellow()
            ));
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
            terminal::log(format!(
                "{indent}{marker}{}{}",
                chalk("ForOfStmt").bold(),
                chalk(await_str).dim()
            ));
            terminal::log(format!(
                "{child_indent}├── {}",
                chalk(format_pattern(left)).yellow()
            ));
            print_expr(right, &child_indent, false);
            print_stmt(body, &child_indent, true);
        }
        StmtKind::Switch {
            discriminant,
            cases,
        } => {
            terminal::log(format!("{indent}{marker}{}", chalk("SwitchStmt").bold()));
            print_expr(discriminant, &child_indent, cases.is_empty());
            for (i, case) in cases.iter().enumerate() {
                let is_l = i == cases.len() - 1;
                let m = if is_l { "└── " } else { "├── " };
                let label = if let Some(t) = &case.test {
                    format_expr_short(t)
                } else {
                    "default".to_owned()
                };
                terminal::log(format!(
                    "{child_indent}{m}{} {}",
                    chalk("Case").bold(),
                    chalk(label).yellow()
                ));
                let c_indent = format!("{child_indent}{}", if is_l { "    " } else { "│   " });
                for (j, s) in case.body.iter().enumerate() {
                    print_stmt(s, &c_indent, j == case.body.len() - 1);
                }
            }
        }
        StmtKind::Return { argument } => {
            terminal::log(format!("{indent}{marker}{}", chalk("ReturnStmt").bold()));
            if let Some(arg) = argument {
                print_expr(arg, &child_indent, true);
            }
        }
        StmtKind::Break { label, .. } => {
            let l = label.as_ref().map(|s| format!(" {s}")).unwrap_or_default();
            terminal::log(format!("{indent}{marker}{}{l}", chalk("Break").bold()));
        }
        StmtKind::Continue { label, .. } => {
            let l = label.as_ref().map(|s| format!(" {s}")).unwrap_or_default();
            terminal::log(format!("{indent}{marker}{}{l}", chalk("Continue").bold()));
        }
        StmtKind::Throw { argument } => {
            terminal::log(format!("{indent}{marker}{}", chalk("ThrowStmt").bold()));
            print_expr(argument, &child_indent, true);
        }
        StmtKind::Try {
            block,
            catches,
            finally,
        } => {
            terminal::log(format!("{indent}{marker}{}", chalk("TryStmt").bold()));
            print_stmt(block, &child_indent, catches.is_empty() && finally.is_none());
            for (i, c) in catches.iter().enumerate() {
                let is_last = i == catches.len() - 1 && finally.is_none();
                let m = if is_last {
                    "└── "
                } else {
                    "├── "
                };
                let param_str = c
                    .param
                    .as_ref()
                    .map(format_pattern)
                    .unwrap_or("_".to_owned());
                terminal::log(format!(
                    "{child_indent}{m}{} {}",
                    chalk("Catch").bold(),
                    chalk(param_str).yellow()
                ));
                let c_ind = format!(
                    "{child_indent}{}",
                    if is_last { "    " } else { "│   " }
                );
                print_stmt(&c.body, &c_ind, true);
            }
            if let Some(f) = finally {
                terminal::log(format!("{child_indent}└── {}", chalk("Finally").bold()));
                let f_ind = format!("{child_indent}    ");
                print_stmt(f, &f_ind, true);
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
                if let Some(init) = &d.init {
                    print_expr(init, &child_indent, i == declarations.len() - 1);
                }
            }
        }
        StmtKind::Labeled { label, body } => {
            terminal::log(format!(
                "{indent}{marker}{} {}:",
                chalk("Label").bold(),
                chalk(label).cyan()
            ));
            print_stmt(body, &child_indent, true);
        }
        StmtKind::Debugger => {
            terminal::log(format!("{indent}{marker}{}", chalk("Debugger").bold()))
        }
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
            terminal::log(format!(
                "{indent}{marker}{}",
                chalk(format!("VariableDecl ({kind})")).bold()
            ));
            for (i, d) in v.declarators.iter().enumerate() {
                let d_is_last = i == v.declarators.len() - 1;
                let d_marker = if d_is_last {
                    "└── "
                } else {
                    "├── "
                };
                terminal::log(format!(
                    "{child_indent}{d_marker}{} {}",
                    chalk("Var").bold(),
                    chalk(format_pattern(&d.id)).yellow()
                ));
                if let Some(init) = &d.init {
                    let d_ind =
                        format!("{child_indent}{}", if d_is_last { "    " } else { "│   " });
                    print_expr(init, &d_ind, true);
                }
            }
        }
        Decl::Function(f) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("FunctionDecl").bold(),
                chalk(&f.id).blue()
            ));
            print_stmt(&f.body, &child_indent, true);
        }
        Decl::Class(c) => {
            let name = c.id.as_deref().unwrap_or("<anonymous>");
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("ClassDecl").bold(),
                chalk(name).blue()
            ));
            for (i, m) in c.body.iter().enumerate() {
                let is_l = i == c.body.len() - 1;
                let mk = if is_l { "└── " } else { "├── " };
                match m {
                    ClassMember::Method { key, .. } => terminal::log(format!(
                        "{child_indent}{mk}{} {}",
                        chalk("Method").bold(),
                        chalk(key).blue()
                    )),
                    ClassMember::Property { key, .. } => terminal::log(format!(
                        "{child_indent}{mk}{} {}",
                        chalk("Property").bold(),
                        chalk(key).cyan()
                    )),
                    ClassMember::Constructor { .. } => {
                        terminal::log(format!("{child_indent}{mk}{}", chalk("Constructor").bold()))
                    }
                    _ => terminal::log(format!(
                        "{child_indent}{mk}{}",
                        chalk("<other member>").dim()
                    )),
                }
            }
        }
        Decl::Interface(i_node) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("InterfaceDecl").bold(),
                chalk(&i_node.id).blue()
            ));
            for (idx, m) in i_node.body.iter().enumerate() {
                let is_l = idx == i_node.body.len() - 1;
                let mk = if is_l { "└── " } else { "├── " };
                match m {
                    InterfaceMember::Property { key, .. } => terminal::log(format!(
                        "{child_indent}{mk}{} {}",
                        chalk("Property").bold(),
                        chalk(key).cyan()
                    )),
                    InterfaceMember::Method { key, .. } => terminal::log(format!(
                        "{child_indent}{mk}{} {}",
                        chalk("Method").bold(),
                        chalk(key).blue()
                    )),
                    InterfaceMember::Callable { .. } => {
                        terminal::log(format!("{child_indent}{mk}{}", chalk("Callable").bold()))
                    }
                    InterfaceMember::Index { .. } => {
                        terminal::log(format!("{child_indent}{mk}{}", chalk("Index").bold()))
                    }
                }
            }
        }
        Decl::Enum(e) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("EnumDecl").bold(),
                chalk(&e.id).blue()
            ));
            for (idx, m) in e.members.iter().enumerate() {
                let is_l = idx == e.members.len() - 1;
                let mk = if is_l { "└── " } else { "├── " };
                terminal::log(format!("{child_indent}{mk}{}", chalk(&m.id).yellow()));
            }
        }
        Decl::Namespace(n) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("NamespaceDecl").bold(),
                chalk(&n.id).blue()
            ));
            for (idx, d) in n.body.iter().enumerate() {
                print_decl(d, &child_indent, idx == n.body.len() - 1);
            }
        }
        Decl::TypeAlias(t) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("TypeAlias").bold(),
                chalk(&t.id).blue()
            ));
        }
        Decl::Import(i) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Import").bold(),
                chalk(format!("{:?}", i.source)).yellow()
            ));
        }
        Decl::Export(e) => match e {
            ExportDecl::Decl { declaration, .. } => {
                terminal::log(format!("{indent}{marker}{}", chalk("ExportDecl").bold()));
                print_decl(declaration, &child_indent, true);
            }
            ExportDecl::Default { declaration, .. } => {
                terminal::log(format!("{indent}{marker}{}", chalk("ExportDefault").bold()));
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
                terminal::log(format!(
                    "{indent}{marker}{}{s}",
                    chalk("ExportNamed").bold()
                ));
            }
            ExportDecl::All { source, .. } => {
                terminal::log(format!(
                    "{indent}{marker}{} from {source:?}",
                    chalk("ExportAll").bold()
                ));
            }
        },
        Decl::Extension(e) => {
            let name = e.id.as_deref().unwrap_or("<anonymous>");
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("ExtensionDecl").bold(),
                chalk(name).blue()
            ));
        }
        Decl::Struct(s) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("StructDecl").bold(),
                chalk(&s.id).blue()
            ));
            for (idx, f) in s.fields.iter().enumerate() {
                let mk = if idx == s.fields.len() - 1 {
                    "└── "
                } else {
                    "├── "
                };
                terminal::log(format!("{child_indent}{mk}{}", chalk(&f.name).cyan()));
            }
        }
        Decl::SumType(s) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("SumTypeDecl").bold(),
                chalk(&s.id).blue()
            ));
            for (idx, v) in s.variants.iter().enumerate() {
                let mk = if idx == s.variants.len() - 1 {
                    "└── "
                } else {
                    "├── "
                };
                terminal::log(format!("{child_indent}{mk}{}", chalk(&v.name).yellow()));
            }
        }
    }
}

fn print_expr(expr: &Expr, indent: &str, is_last: bool) {
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
            chalk(raw).yellow(),
            chalk("(bigint)").dim()
        )),
        ExprKind::DecimalLiteral { raw } => terminal::log(format!(
            "{indent}{marker}{} {}",
            chalk(raw).yellow(),
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
            chalk(name).cyan(),
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
            if !*computed {
                if let ExprKind::Identifier { name } = &property.kind {
                    terminal::log(format!(
                        "{indent}{marker}{} {}",
                        chalk("Member").bold(),
                        chalk(format!("{}.{name}", format_expr_short(object))).cyan()
                    ));
                    return;
                }
            }
            terminal::log(format!(
                "{indent}{marker}{} (computed)",
                chalk("Member").bold()
            ));
            print_expr(object, &child_indent, false);
            print_expr(property, &child_indent, true);
        }
        ExprKind::Call { callee, args, .. } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Call").bold(),
                chalk(format_expr_short(callee)).blue()
            ));
            for (i, a) in args.iter().enumerate() {
                let e = match a {
                    Arg::Positional(e) | Arg::Spread(e) => e,
                    Arg::Named { value, .. } => value,
                };
                print_expr(e, &child_indent, i == args.len() - 1);
            }
        }
        ExprKind::New { callee, args, .. } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("New").bold(),
                chalk(format_expr_short(callee)).blue()
            ));
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
                        ArrayEl::Expr(e) => print_expr(e, &child_indent, is_l),
                        ArrayEl::Spread(e) => {
                            terminal::log(format!(
                                "{child_indent}{} {}",
                                if is_l { "└── " } else { "├── " },
                                chalk("...").bold()
                            ));
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
                        let k = format_prop_key(key);
                        if *shorthand {
                            terminal::log(format!(
                                "{child_indent}{m}{} {}",
                                chalk(&k).cyan(),
                                chalk("(shorthand)").dim()
                            ));
                        } else {
                            terminal::log(format!("{child_indent}{m}{}:", chalk(&k).cyan()));
                            print_expr(
                                value,
                                &format!("{child_indent}{}", if is_l { "    " } else { "│   " }),
                                true,
                            );
                        }
                    }
                    ObjectProp::Spread { argument, .. } => {
                        terminal::log(format!("{child_indent}{m}{}", chalk("...").bold()));
                        print_expr(
                            argument,
                            &format!("{child_indent}{}", if is_l { "    " } else { "│   " }),
                            true,
                        );
                    }
                    _ => terminal::log(format!("{child_indent}{m}{}", chalk("<other prop>").dim())),
                }
            }
        }
        ExprKind::Unary { op, operand, .. } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Unary").bold(),
                chalk(format!("{op:?}")).yellow()
            ));
            print_expr(operand, &child_indent, true);
        }
        ExprKind::Update {
            op,
            operand,
            prefix,
        } => {
            let p = if *prefix { "prefix " } else { "" };
            terminal::log(format!(
                "{indent}{marker}{} {}{}",
                chalk("Update").bold(),
                chalk(p).dim(),
                chalk(format!("{op:?}")).yellow()
            ));
            print_expr(operand, &child_indent, true);
        }
        ExprKind::Binary {
            op, left, right, ..
        } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Binary").bold(),
                chalk(format!("{op:?}")).yellow()
            ));
            print_expr(left, &child_indent, false);
            print_expr(right, &child_indent, true);
        }
        ExprKind::Logical {
            op, left, right, ..
        } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Logical").bold(),
                chalk(format!("{op:?}")).yellow()
            ));
            print_expr(left, &child_indent, false);
            print_expr(right, &child_indent, true);
        }
        ExprKind::Assign {
            op, target, value, ..
        } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Assign").bold(),
                chalk(format!("{op:?}")).yellow()
            ));
            print_expr(target, &child_indent, false);
            print_expr(value, &child_indent, true);
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            terminal::log(format!("{indent}{marker}{}", chalk("Ternary").bold()));
            print_expr(test, &child_indent, false);
            print_expr(consequent, &child_indent, false);
            print_expr(alternate, &child_indent, true);
        }
        ExprKind::Match { subject, cases } => {
            terminal::log(format!("{indent}{marker}{}", chalk("MatchExpr").bold()));
            print_expr(subject, &child_indent, cases.is_empty());
            for (i, c) in cases.iter().enumerate() {
                let is_l = i == cases.len() - 1;
                terminal::log(format!(
                    "{child_indent}{} {}",
                    if is_l { "└── " } else { "├── " },
                    chalk("Case").bold()
                ));
                let c_ind = format!("{child_indent}{}", if is_l { "    " } else { "│   " });
                match &c.body {
                    MatchBody::Block(s) => print_stmt(s, &c_ind, true),
                    MatchBody::Expr(e) => print_expr(e, &c_ind, true),
                }
            }
        }
        ExprKind::Arrow { body, .. } => {
            terminal::log(format!("{indent}{marker}{}", chalk("ArrowFunc").bold()));
            match body.as_ref() {
                ArrowBody::Block(s) => print_stmt(s, &child_indent, true),
                ArrowBody::Expr(e) => print_expr(e, &child_indent, true),
            }
        }
        ExprKind::Function { fn_id, body, .. } => {
            let name = fn_id.as_deref().unwrap_or("<anonymous>");
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("FunctionExpr").bold(),
                chalk(name).blue()
            ));
            print_stmt(body, &child_indent, true);
        }
        ExprKind::Await { argument } => {
            terminal::log(format!("{indent}{marker}{}", chalk("Await").bold()));
            print_expr(argument, &child_indent, true);
        }
        ExprKind::Spawn { argument } => {
            terminal::log(format!("{indent}{marker}{}", chalk("Spawn").bold()));
            print_expr(argument, &child_indent, true);
        }
        ExprKind::Yield {
            argument, delegate, ..
        } => {
            let d = if *delegate { "*" } else { "" };
            terminal::log(format!("{indent}{marker}{}{d}", chalk("Yield").bold()));
            if let Some(a) = argument {
                print_expr(a, &child_indent, true);
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
                    TemplatePart::Interpolation(e) => print_expr(e, &child_indent, is_l),
                }
            }
        }
        ExprKind::TaggedTemplate { tag, template, .. } => {
            terminal::log(format!(
                "{indent}{marker}{}",
                chalk("TaggedTemplate").bold()
            ));
            print_expr(tag, &child_indent, false);
            print_expr(template, &child_indent, true);
        }
        ExprKind::Pipeline { left, right } => {
            terminal::log(format!("{indent}{marker}{}", chalk("Pipeline").bold()));
            print_expr(left, &child_indent, false);
            print_expr(right, &child_indent, true);
        }
        ExprKind::Range {
            start,
            end,
            inclusive,
        } => {
            let op = if *inclusive { "..=" } else { ".." };
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Range").bold(),
                chalk(op).yellow()
            ));
            print_expr(start, &child_indent, false);
            print_expr(end, &child_indent, true);
        }
        ExprKind::NonNull { expression } => {
            terminal::log(format!("{indent}{marker}{} !", chalk("NonNull").bold()));
            print_expr(expression, &child_indent, true);
        }
        ExprKind::Try { expression } => {
            terminal::log(format!("{indent}{marker}{} ?", chalk("TryExpr").bold()));
            print_expr(expression, &child_indent, true);
        }
        ExprKind::As {
            expression,
            type_ann,
        } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("As").bold(),
                chalk(format!("({type_ann:?})")).dim()
            ));
            print_expr(expression, &child_indent, true);
        }
        ExprKind::Satisfies {
            expression,
            type_ann,
        } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Satisfies").bold(),
                chalk(format!("({type_ann:?})")).dim()
            ));
            print_expr(expression, &child_indent, true);
        }
        ExprKind::Is {
            expression,
            type_ann,
        } => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Is").bold(),
                chalk(format!("({type_ann:?})")).dim()
            ));
            print_expr(expression, &child_indent, true);
        }
        ExprKind::Sequence { expressions } => {
            terminal::log(format!("{indent}{marker}{}", chalk("Sequence").bold()));
            for (i, e) in expressions.iter().enumerate() {
                print_expr(e, &child_indent, i == expressions.len() - 1);
            }
        }
        ExprKind::Paren { expression } => {
            terminal::log(format!("{indent}{marker}{}", chalk("Paren").bold()));
            print_expr(expression, &child_indent, true);
        }
        ExprKind::ClassExpr { declaration } => {
            terminal::log(format!("{indent}{marker}{}", chalk("ClassExpr").bold()));
            print_decl(&Decl::Class(*declaration.clone()), indent, true);
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
