pub fn find_member_receiver_type(
    program: &varn_core::ast::Program,
    db: &super::SemanticDB,
    offset: u32,
) -> Option<String> {
    for stmt in &program.body {
        if let Some(ty) = check_stmt_for_receiver(stmt, db, offset) {
            return Some(ty);
        }
    }
    None
}

fn check_stmt_for_receiver(
    stmt: &varn_core::ast::Stmt,
    db: &super::SemanticDB,
    offset: u32,
) -> Option<String> {
    match &stmt.kind {
        varn_core::ast::StmtKind::Expr { expression } => {
            check_expr_for_receiver(expression, db, offset)
        }
        varn_core::ast::StmtKind::Block { stmts } => {
            for s in stmts {
                if let Some(ty) = check_stmt_for_receiver(s, db, offset) {
                    return Some(ty);
                }
            }
            None
        }
        varn_core::ast::StmtKind::Decl(decl) => match decl.as_ref() {
            varn_core::ast::Decl::Variable(v) => {
                for d in &v.declarators {
                    if let Some(init) = &d.init {
                        if let Some(ty) = check_expr_for_receiver(init, db, offset) {
                            return Some(ty);
                        }
                    }
                }
                None
            }
            varn_core::ast::Decl::Function(f) => check_stmt_for_receiver(&f.body, db, offset),
            varn_core::ast::Decl::Class(c) => {
                for member in &c.body {
                    match member {
                        varn_core::ast::ClassMember::Method { body: Some(b), .. } => {
                            if let Some(ty) = check_stmt_for_receiver(b, db, offset) {
                                return Some(ty);
                            }
                        }
                        varn_core::ast::ClassMember::Constructor { body, .. } => {
                            if let Some(ty) = check_stmt_for_receiver(body, db, offset) {
                                return Some(ty);
                            }
                        }
                        varn_core::ast::ClassMember::Property { init: Some(v), .. } => {
                            if let Some(ty) = check_expr_for_receiver(v, db, offset) {
                                return Some(ty);
                            }
                        }
                        _ => {}
                    }
                }
                None
            }
            varn_core::ast::Decl::Extension(e) => {
                for m in &e.members {
                    match m {
                        varn_core::ast::ExtensionMember::Method(f) => {
                            if let Some(ty) = check_stmt_for_receiver(&f.body, db, offset) {
                                return Some(ty);
                            }
                        }
                        varn_core::ast::ExtensionMember::Getter { body, .. } => {
                            if let Some(ty) = check_stmt_for_receiver(body, db, offset) {
                                return Some(ty);
                            }
                        }
                        varn_core::ast::ExtensionMember::Setter { body, .. } => {
                            if let Some(ty) = check_stmt_for_receiver(body, db, offset) {
                                return Some(ty);
                            }
                        }
                    }
                }
                None
            }
            varn_core::ast::Decl::Export(e) => match e {
                varn_core::ast::ExportDecl::Decl { declaration, .. } => {
                    check_stmt_for_receiver(
                        &varn_core::ast::Stmt::new_with_range(
                            *e.range(),
                            varn_core::ast::StmtKind::Decl(declaration.clone()),
                        ),
                        db,
                        offset,
                    )
                }
                varn_core::ast::ExportDecl::Default { declaration, .. } => match declaration.as_ref() {
                    varn_core::ast::ExportDefaultDecl::Expr(expr) => {
                        check_expr_for_receiver(expr, db, offset)
                    }
                    varn_core::ast::ExportDefaultDecl::Function(f) => {
                        check_stmt_for_receiver(&f.body, db, offset)
                    }
                    varn_core::ast::ExportDefaultDecl::Class(_) => None,
                },
                _ => None,
            },
            _ => None,
        },
        varn_core::ast::StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            if let Some(ty) = check_expr_for_receiver(test, db, offset) {
                return Some(ty);
            }
            if let Some(ty) = check_stmt_for_receiver(consequent, db, offset) {
                return Some(ty);
            }
            if let Some(alt) = alternate {
                if let Some(ty) = check_stmt_for_receiver(alt, db, offset) {
                    return Some(ty);
                }
            }
            None
        }
        varn_core::ast::StmtKind::While { test, body }
        | varn_core::ast::StmtKind::DoWhile { test, body } => {
            if let Some(ty) = check_expr_for_receiver(test, db, offset) {
                return Some(ty);
            }
            check_stmt_for_receiver(body, db, offset)
        }
        varn_core::ast::StmtKind::For {
            init,
            test,
            update,
            body,
        } => {
            if let Some(init_box) = init {
                match init_box.as_ref() {
                    varn_core::ast::ForInit::Var { declarators, .. } => {
                        for d in declarators {
                            if let Some(i) = &d.init {
                                if let Some(ty) = check_expr_for_receiver(i, db, offset) {
                                    return Some(ty);
                                }
                            }
                        }
                    }
                    varn_core::ast::ForInit::Expr(e) => {
                        if let Some(ty) = check_expr_for_receiver(e, db, offset) {
                            return Some(ty);
                        }
                    }
                }
            }
            if let Some(t) = test {
                if let Some(ty) = check_expr_for_receiver(t, db, offset) {
                    return Some(ty);
                }
            }
            if let Some(u) = update {
                if let Some(ty) = check_expr_for_receiver(u, db, offset) {
                    return Some(ty);
                }
            }
            check_stmt_for_receiver(body, db, offset)
        }
        varn_core::ast::StmtKind::ForIn { right, body, .. }
        | varn_core::ast::StmtKind::ForOf { right, body, .. } => {
            if let Some(ty) = check_expr_for_receiver(right, db, offset) {
                return Some(ty);
            }
            check_stmt_for_receiver(body, db, offset)
        }
        varn_core::ast::StmtKind::Switch {
            discriminant,
            cases,
        } => {
            if let Some(ty) = check_expr_for_receiver(discriminant, db, offset) {
                return Some(ty);
            }
            for c in cases {
                if let Some(t) = &c.test {
                    if let Some(ty) = check_expr_for_receiver(t, db, offset) {
                        return Some(ty);
                    }
                }
                for s in &c.body {
                    if let Some(ty) = check_stmt_for_receiver(s, db, offset) {
                        return Some(ty);
                    }
                }
            }
            None
        }
        varn_core::ast::StmtKind::Return { argument: Some(e) }
        | varn_core::ast::StmtKind::Throw { argument: e } => {
            check_expr_for_receiver(e, db, offset)
        }
        varn_core::ast::StmtKind::Try {
            block,
            catch,
            finally,
        } => {
            if let Some(ty) = check_stmt_for_receiver(block, db, offset) {
                return Some(ty);
            }
            if let Some(c) = catch {
                if let Some(ty) = check_stmt_for_receiver(&c.body, db, offset) {
                    return Some(ty);
                }
            }
            if let Some(f) = finally {
                if let Some(ty) = check_stmt_for_receiver(f, db, offset) {
                    return Some(ty);
                }
            }
            None
        }
        varn_core::ast::StmtKind::Using { declarations, .. } => {
            for d in declarations {
                if let Some(i) = &d.init {
                    if let Some(ty) = check_expr_for_receiver(i, db, offset) {
                        return Some(ty);
                    }
                }
            }
            None
        }
        varn_core::ast::StmtKind::Labeled { body, .. } => {
            check_stmt_for_receiver(body, db, offset)
        }
        _ => None,
    }
}

fn check_expr_for_receiver(
    expr: &varn_core::ast::Expr,
    db: &super::SemanticDB,
    offset: u32,
) -> Option<String> {
    match &expr.kind {
        varn_core::ast::ExprKind::Member {
            object, property, ..
        } => {
            let p_start = property.range.start.offset;
            let p_end = property.range.end.offset;
            if offset >= p_start && offset <= p_end {
                let mut inner_obj = object.as_ref();
                while let varn_core::ast::ExprKind::Paren { expression }
                | varn_core::ast::ExprKind::NonNull { expression } = &inner_obj.kind
                {
                    inner_obj = expression.as_ref();
                }

                match &inner_obj.kind {
                    varn_core::ast::ExprKind::Range { .. } => return Some("Range".to_string()),
                    varn_core::ast::ExprKind::StrLiteral { .. } => {
                        return Some("str".to_string())
                    }
                    varn_core::ast::ExprKind::IntLiteral { .. } => {
                        return Some("int".to_string())
                    }
                    varn_core::ast::ExprKind::FloatLiteral { .. } => {
                        return Some("float".to_string())
                    }
                    varn_core::ast::ExprKind::BoolLiteral { .. } => {
                        return Some("bool".to_string())
                    }
                    _ => {}
                }

                if let Some(info) = db
                    .expr_types
                    .get(&inner_obj.range.start.offset)
                    .or_else(|| db.expr_types.get(&object.range.start.offset))
                {
                    let t_str = info.ty.to_string();
                    if !t_str.is_empty() && t_str != "unknown" && t_str != "dynamic" {
                        return Some(t_str);
                    }
                }
                match &inner_obj.kind {
                    varn_core::ast::ExprKind::Array { .. } => return Some("Array".to_string()),
                    varn_core::ast::ExprKind::Identifier { name } => {
                        if let Some((sid, ty)) = db.resolve_at(name, inner_obj.range.start.offset) {
                            if sid < db.arena.len() {
                                let sym = db.arena.get(sid);
                                if matches!(
                                    sym.kind,
                                    varn_checker::SymbolKind::Class
                                        | varn_checker::SymbolKind::Enum
                                        | varn_checker::SymbolKind::Interface
                                        | varn_checker::SymbolKind::Struct
                                        | varn_checker::SymbolKind::Namespace
                                ) {
                                    return Some(name.to_string());
                                }
                            }
                            let ty_str = ty.to_string();
                            if !ty_str.is_empty() && ty_str != "unknown" && ty_str != "dynamic" {
                                return Some(ty_str);
                            }
                        }
                    }
                    _ => {}
                }
            }
            check_expr_for_receiver(object, db, offset)
                .or_else(|| check_expr_for_receiver(property, db, offset))
        }
        varn_core::ast::ExprKind::Call { callee, args, .. } => {
            if let Some(ty) = check_expr_for_receiver(callee, db, offset) {
                return Some(ty);
            }
            for arg in args {
                let arg_expr = match arg {
                    varn_core::ast::Arg::Positional(e)
                    | varn_core::ast::Arg::Spread(e)
                    | varn_core::ast::Arg::Named { value: e, .. } => e,
                };
                if let Some(ty) = check_expr_for_receiver(arg_expr, db, offset) {
                    return Some(ty);
                }
            }
            None
        }
        varn_core::ast::ExprKind::Paren { expression }
        | varn_core::ast::ExprKind::NonNull { expression }
        | varn_core::ast::ExprKind::Await { argument: expression }
        | varn_core::ast::ExprKind::Spawn { argument: expression } => {
            check_expr_for_receiver(expression, db, offset)
        }
        varn_core::ast::ExprKind::Binary { left, right, .. }
        | varn_core::ast::ExprKind::Logical { left, right, .. }
        | varn_core::ast::ExprKind::Assign {
            target: left,
            value: right,
            ..
        }
        | varn_core::ast::ExprKind::Pipeline { left, right } => {
            check_expr_for_receiver(left, db, offset)
                .or_else(|| check_expr_for_receiver(right, db, offset))
        }
        varn_core::ast::ExprKind::Unary { operand, .. }
        | varn_core::ast::ExprKind::Update { operand, .. } => {
            check_expr_for_receiver(operand, db, offset)
        }
        varn_core::ast::ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            check_expr_for_receiver(test, db, offset)
                .or_else(|| check_expr_for_receiver(consequent, db, offset))
                .or_else(|| check_expr_for_receiver(alternate, db, offset))
        }
        _ => None,
    }
}
