use std::fs;
use std::path::Path;
use varn_core::ast::*;
use varn_core::SourceRange;

fn check_range_sanity(node_desc: &str, range: &SourceRange, max_len: usize, filename: &str) {
    let end = range.end.offset as usize;

    assert!(
        range.start.offset <= range.end.offset,
        "[{filename}] {node_desc} has start offset ({}) > end offset ({})",
        range.start.offset,
        range.end.offset
    );

    assert!(
        end <= max_len,
        "[{filename}] {node_desc} end offset ({}) exceeds source length ({})",
        end,
        max_len
    );

    assert!(
        range.start.line <= range.end.line,
        "[{filename}] {node_desc} start line ({}) > end line ({})",
        range.start.line,
        range.end.line
    );
}

fn check_enclosure(parent_desc: &str, parent: &SourceRange, child_desc: &str, child: &SourceRange, filename: &str) {
    assert!(
        parent.start.offset <= child.start.offset,
        "[{filename}] Parent {parent_desc} (start: {}) does not enclose child {child_desc} (start: {})",
        parent.start.offset,
        child.start.offset
    );
    assert!(
        parent.end.offset >= child.end.offset,
        "[{filename}] Parent {parent_desc} (end: {}) does not enclose child {child_desc} (end: {})",
        parent.end.offset,
        child.end.offset
    );
}

fn validate_expr(expr: &Expr, max_len: usize, filename: &str) {
    check_range_sanity("Expr", &expr.range, max_len, filename);

    match &expr.kind {
        ExprKind::Binary { left, right, .. } | ExprKind::Logical { left, right, .. } => {
            check_enclosure("Binary/Logical Expr", &expr.range, "Left child", &left.range, filename);
            check_enclosure("Binary/Logical Expr", &expr.range, "Right child", &right.range, filename);
            validate_expr(left, max_len, filename);
            validate_expr(right, max_len, filename);
        }
        ExprKind::Unary { operand, .. } | ExprKind::Update { operand, .. } => {
            check_enclosure("Unary Expr", &expr.range, "Operand", &operand.range, filename);
            validate_expr(operand, max_len, filename);
        }
        ExprKind::Member { object, property, .. } => {
            check_enclosure("Member Expr", &expr.range, "Object", &object.range, filename);
            check_enclosure("Member Expr", &expr.range, "Property", &property.range, filename);
            validate_expr(object, max_len, filename);
            validate_expr(property, max_len, filename);
        }
        ExprKind::Call { callee, args, .. } => {
            check_enclosure("Call Expr", &expr.range, "Callee", &callee.range, filename);
            validate_expr(callee, max_len, filename);
            for arg in args {
                let arg_expr = match arg {
                    Arg::Positional(e) | Arg::Spread(e) | Arg::Named { value: e, .. } => e,
                };
                check_enclosure("Call Expr", &expr.range, "Arg", &arg_expr.range, filename);
                validate_expr(arg_expr, max_len, filename);
            }
        }
        ExprKind::Paren { expression }
        | ExprKind::NonNull { expression }
        | ExprKind::Await { argument: expression }
        | ExprKind::Spawn { argument: expression } => {
            check_enclosure("Enclosing Expr", &expr.range, "Inner", &expression.range, filename);
            validate_expr(expression, max_len, filename);
        }
        ExprKind::Array { elements } => {
            for el in elements {
                match el {
                    ArrayEl::Expr(e) | ArrayEl::Spread(e) => {
                        check_enclosure("Array Expr", &expr.range, "Element", &e.range, filename);
                        validate_expr(e, max_len, filename);
                    }
                    ArrayEl::Hole => {}
                }
            }
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            check_enclosure("Ternary Expr", &expr.range, "Test", &test.range, filename);
            check_enclosure("Ternary Expr", &expr.range, "Consequent", &consequent.range, filename);
            check_enclosure("Ternary Expr", &expr.range, "Alternate", &alternate.range, filename);
            validate_expr(test, max_len, filename);
            validate_expr(consequent, max_len, filename);
            validate_expr(alternate, max_len, filename);
        }
        _ => {}
    }
}

fn validate_stmt(stmt: &Stmt, max_len: usize, filename: &str) {
    check_range_sanity("Stmt", &stmt.range, max_len, filename);

    match &stmt.kind {
        StmtKind::Expr { expression } => {
            check_enclosure("Expr Stmt", &stmt.range, "Expression", &expression.range, filename);
            validate_expr(expression, max_len, filename);
        }
        StmtKind::Block { stmts } => {
            for s in stmts {
                check_enclosure("Block Stmt", &stmt.range, "Inner Stmt", &s.range, filename);
                validate_stmt(s, max_len, filename);
            }
        }
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            check_enclosure("If Stmt", &stmt.range, "Test", &test.range, filename);
            check_enclosure("If Stmt", &stmt.range, "Consequent", &consequent.range, filename);
            validate_expr(test, max_len, filename);
            validate_stmt(consequent, max_len, filename);
            if let Some(alt) = alternate {
                check_enclosure("If Stmt", &stmt.range, "Alternate", &alt.range, filename);
                validate_stmt(alt, max_len, filename);
            }
        }
        StmtKind::Return { argument: Some(e) } | StmtKind::Throw { argument: e } => {
            check_enclosure("Return/Throw Stmt", &stmt.range, "Argument", &e.range, filename);
            validate_expr(e, max_len, filename);
        }
        StmtKind::Decl(decl) => match decl.as_ref() {
            Decl::Function(f) => {
                check_range_sanity("FunctionDecl", &f.range, max_len, filename);
                check_enclosure("FunctionDecl", &f.range, "Body", &f.body.range, filename);
                validate_stmt(&f.body, max_len, filename);
            }
            Decl::Class(c) => {
                check_range_sanity("ClassDecl", &c.range, max_len, filename);
                for member in &c.body {
                    match member {
                        ClassMember::Method { body: Some(b), range, .. } => {
                            check_range_sanity("Method", range, max_len, filename);
                            validate_stmt(b, max_len, filename);
                        }
                        ClassMember::Constructor { body, range, .. } => {
                            check_range_sanity("Constructor", range, max_len, filename);
                            validate_stmt(body, max_len, filename);
                        }
                        ClassMember::Property { init: Some(i), range, .. } => {
                            check_range_sanity("Property", range, max_len, filename);
                            validate_expr(i, max_len, filename);
                        }
                        _ => {}
                    }
                }
            }
            Decl::Variable(v) => {
                check_range_sanity("VariableDecl", &v.range, max_len, filename);
                for d in &v.declarators {
                    check_range_sanity("VariableDeclarator", &d.range, max_len, filename);
                    if let Some(init) = &d.init {
                        check_enclosure("VariableDeclarator", &d.range, "Init", &init.range, filename);
                        validate_expr(init, max_len, filename);
                    }
                }
            }
            _ => {}
        },
        _ => {}
    }
}

fn verify_parser_positions(filename: &str, source: &str) {
    let (tokens, lexemes, diags) = varn_lexer::scan(source, filename);
    assert!(diags.is_empty(), "[{filename}] Lexing failed: {:?}", diags);

    let program = varn_parser::parse(tokens, lexemes, filename)
        .unwrap_or_else(|e| panic!("[{filename}] Parsing produced errors: {:?}", e));

    let max_len = source.len();
    for stmt in &program.body {
        validate_stmt(stmt, max_len, filename);
    }
}

#[test]
fn test_all_vn_files_parser_position_invariants() {
    let tests_dir = Path::new("../../tests").canonicalize().or_else(|_| Path::new("tests").canonicalize()).unwrap();
    let mut count = 0;

    for entry in fs::read_dir(tests_dir).expect("failed to read tests directory") {
        let entry = entry.expect("valid entry");
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "vn") {
            let filename = path.file_name().unwrap().to_str().unwrap().to_string();
            let source = fs::read_to_string(&path).expect("read file");
            verify_parser_positions(&filename, &source);
            count += 1;
        }
    }

    assert!(count >= 80, "Expected to test >= 80 files, tested {count}");
    println!("Verified {count} .vn files for strict parser position/range enclosure invariants.");
}
