use varn_core::ast::{
    Arg, ArrayEl, ArrowBody, AstArena, ClassMember, Decl, ExportDecl, ExportDefaultDecl, ExprId,
    ExprKind, ForInit, InterfaceMember, MatchBody, ObjectProp, Pattern, Program, PropKey, StmtId,
    StmtKind, TemplatePart, VarKind,
};
use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_core::term::terminal::Section;
use varn_core::AtomInterner;

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

fn print_stmt(
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

fn print_decl(decl: &Decl, arena: &AstArena, indent: &str, is_last: bool, interner: &AtomInterner) {
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
                    chalk(format_pattern(&d.id, interner)).yellow()
                ));
                if let Some(init) = d.init {
                    let d_ind =
                        format!("{child_indent}{}", if d_is_last { "    " } else { "│   " });
                    print_expr(init, arena, &d_ind, true, interner);
                }
            }
        }
        Decl::Function(f) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("FunctionDecl").bold(),
                chalk(interner.resolve(f.id)).blue()
            ));
            print_stmt(f.body, arena, &child_indent, true, interner);
        }
        Decl::Class(c) => {
            let name = c.id.map(|a| interner.resolve(a)).unwrap_or("<anonymous>");
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
                        chalk(interner.resolve(*key)).blue()
                    )),
                    ClassMember::Property { key, .. } => terminal::log(format!(
                        "{child_indent}{mk}{} {}",
                        chalk("Property").bold(),
                        chalk(interner.resolve(*key)).cyan()
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
                chalk(interner.resolve(i_node.id)).blue()
            ));
            for (idx, m) in i_node.body.iter().enumerate() {
                let is_l = idx == i_node.body.len() - 1;
                let mk = if is_l { "└── " } else { "├── " };
                match m {
                    InterfaceMember::Property { key, .. } => terminal::log(format!(
                        "{child_indent}{mk}{} {}",
                        chalk("Property").bold(),
                        chalk(interner.resolve(*key)).cyan()
                    )),
                    InterfaceMember::Method { key, .. } => terminal::log(format!(
                        "{child_indent}{mk}{} {}",
                        chalk("Method").bold(),
                        chalk(interner.resolve(*key)).blue()
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
                chalk(interner.resolve(e.id)).blue()
            ));
            for (idx, m) in e.members.iter().enumerate() {
                let is_l = idx == e.members.len() - 1;
                let mk = if is_l { "└── " } else { "├── " };
                terminal::log(format!(
                    "{child_indent}{mk}{}",
                    chalk(interner.resolve(m.id)).yellow()
                ));
            }
        }
        Decl::Namespace(n) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("NamespaceDecl").bold(),
                chalk(interner.resolve(n.id)).blue()
            ));
            for (idx, d) in n.body.iter().enumerate() {
                print_decl(d, arena, &child_indent, idx == n.body.len() - 1, interner);
            }
        }
        Decl::TypeAlias(t) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("TypeAlias").bold(),
                chalk(interner.resolve(t.id)).blue()
            ));
        }
        Decl::Import(i) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Import").bold(),
                chalk(format!("{:?}", interner.resolve(i.source))).yellow()
            ));
        }
        Decl::Export(e) => match e {
            ExportDecl::Decl { declaration, .. } => {
                terminal::log(format!("{indent}{marker}{}", chalk("ExportDecl").bold()));
                print_decl(declaration, arena, &child_indent, true, interner);
            }
            ExportDecl::Default { declaration, .. } => {
                terminal::log(format!("{indent}{marker}{}", chalk("ExportDefault").bold()));
                match &**declaration {
                    ExportDefaultDecl::Class(c) => print_decl(
                        &Decl::Class(c.clone()),
                        arena,
                        &child_indent,
                        true,
                        interner,
                    ),
                    ExportDefaultDecl::Function(f) => print_decl(
                        &Decl::Function(f.clone()),
                        arena,
                        &child_indent,
                        true,
                        interner,
                    ),
                    ExportDefaultDecl::Expr(e) => {
                        print_expr(*e, arena, &child_indent, true, interner)
                    }
                }
            }
            ExportDecl::Named { source, .. } => {
                let s = source
                    .map(|s| format!(" from {:?}", interner.resolve(s)))
                    .unwrap_or_default();
                terminal::log(format!(
                    "{indent}{marker}{}{s}",
                    chalk("ExportNamed").bold()
                ));
            }
            ExportDecl::All { source, .. } => {
                terminal::log(format!(
                    "{indent}{marker}{} from {:?}",
                    chalk("ExportAll").bold(),
                    interner.resolve(*source)
                ));
            }
        },
        Decl::Extension(e) => {
            let name = e.id.map(|a| interner.resolve(a)).unwrap_or("<anonymous>");
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
                chalk(interner.resolve(s.id)).blue()
            ));
            for (idx, f) in s.fields.iter().enumerate() {
                let mk = if idx == s.fields.len() - 1 {
                    "└── "
                } else {
                    "├── "
                };
                terminal::log(format!(
                    "{child_indent}{mk}{}",
                    chalk(interner.resolve(f.name)).cyan()
                ));
            }
        }
        Decl::SumType(s) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("SumTypeDecl").bold(),
                chalk(interner.resolve(s.id)).blue()
            ));
            for (idx, v) in s.variants.iter().enumerate() {
                let mk = if idx == s.variants.len() - 1 {
                    "└── "
                } else {
                    "├── "
                };
                terminal::log(format!(
                    "{child_indent}{mk}{}",
                    chalk(interner.resolve(v.name)).yellow()
                ));
            }
        }
    }
}

fn print_expr(
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

fn format_expr_short(expr_id: ExprId, arena: &AstArena, interner: &AtomInterner) -> String {
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
        _ => false,
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

fn format_pattern(pat: &Pattern, interner: &AtomInterner) -> String {
    match pat {
        Pattern::Identifier { name, .. } => interner.resolve(*name).to_owned(),
        _ => "{...}".to_owned(),
    }
}
