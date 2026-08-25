use std::collections::{HashMap, HashSet};
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, TextEdit, WorkspaceEdit,
};
use varn_core::ast::{Decl, Expr, ExprKind, MatchPattern, Program, Stmt, StmtKind};
use varn_core::TypeKind;

use crate::document::DocumentState;

pub fn generate_match_arms_action(
    state: &DocumentState,
    uri: &tower_lsp::lsp_types::Url,
    cursor_line: u32,
    _cursor_col: u32,
) -> Option<CodeActionOrCommand> {
    let program = state.ast.as_ref()?;
    let match_info = find_match_at_pos(program, cursor_line)?;

    // Retrieve subject type
    let subject_type = state
        .db
        .expr_types
        .get(&match_info.subject_id)
        .map(|info| &info.ty);

    // Collect all expected variant names based on type or enum declarations
    let (type_name, all_variants) = resolve_variants(state, program, subject_type, &match_info)?;

    // Collect already handled variant names in match cases
    let handled_variants = match_info.handled_cases;

    // Determine missing variants
    let missing: Vec<String> = all_variants
        .into_iter()
        .filter(|v| !handled_variants.contains(v))
        .collect();

    if missing.is_empty() {
        return None;
    }

    let indent = " ".repeat(match_info.indent_cols + 4);
    let mut new_cases = String::new();
    for variant in &missing {
        if variant == "true" || variant == "false" {
            new_cases.push_str(&format!("\n{}{} => {{\n{}    // TODO\n{}}}", indent, variant, indent, indent));
        } else if !type_name.is_empty() {
            new_cases.push_str(&format!("\n{}{}.{} => {{\n{}    // TODO\n{}}}", indent, type_name, variant, indent, indent));
        } else {
            new_cases.push_str(&format!("\n{}{} => {{\n{}    // TODO\n{}}}", indent, variant, indent, indent));
        }
    }

    let insert_pos = Position {
        line: match_info.insert_line,
        character: match_info.insert_col,
    };

    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: new_cases,
        }],
    );

    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("💡 Fill missing match arms ({})", missing.join(", ")),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    }))
}

struct MatchInfo {
    subject_id: u32,
    subject_ident: Option<String>,
    handled_cases: HashSet<String>,
    insert_line: u32,
    insert_col: u32,
    indent_cols: usize,
}

fn find_match_at_pos(program: &Program, line: u32) -> Option<MatchInfo> {
    for stmt in &program.body {
        if let Some(info) = check_stmt_for_match(stmt, line) {
            return Some(info);
        }
    }
    None
}

fn check_stmt_for_match(stmt: &Stmt, line: u32) -> Option<MatchInfo> {
    match &stmt.kind {
        StmtKind::Expr { expression } => check_expr_for_match(expression, line),
        StmtKind::Block { stmts } => {
            for s in stmts {
                if let Some(info) = check_stmt_for_match(s, line) {
                    return Some(info);
                }
            }
            None
        }
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            if let Some(info) = check_stmt_for_match(consequent, line) {
                return Some(info);
            }
            if let Some(alt) = alternate {
                if let Some(info) = check_stmt_for_match(alt, line) {
                    return Some(info);
                }
            }
            None
        }
        StmtKind::Decl(decl) => match decl.as_ref() {
            Decl::Function(f) => check_stmt_for_match(&f.body, line),
            Decl::Class(c) => {
                for member in &c.body {
                    if let varn_core::ast::ClassMember::Method { body: Some(b), .. } = member {
                        if let Some(info) = check_stmt_for_match(b, line) {
                            return Some(info);
                        }
                    }
                }
                None
            }
            _ => None,
        },
        _ => None,
    }
}

fn check_expr_for_match(expr: &Expr, line: u32) -> Option<MatchInfo> {
    if let ExprKind::Match { subject, cases } = &expr.kind {
        let range = &expr.range;

        let mut handled = HashSet::new();
        let mut last_case_end_line = range.start.line.saturating_sub(1);
        let mut last_case_end_col = range.start.column;

        for case in cases {
            let c_range = &case.range;
            last_case_end_line = c_range.end.line.saturating_sub(1);
            last_case_end_col = c_range.end.column;

            match &case.pattern {
                MatchPattern::Identifier(name) => {
                    handled.insert(name.to_string());
                }
                MatchPattern::EnumVariant { variant_name, .. } => {
                    handled.insert(variant_name.to_string());
                }
                MatchPattern::Literal(e) => {
                    if let ExprKind::BoolLiteral { value } = &e.kind {
                        handled.insert(value.to_string());
                    } else if let ExprKind::Member { property, .. } = &e.kind {
                        if let ExprKind::Identifier { name } = &property.kind {
                            handled.insert(name.to_string());
                        }
                    }
                }
                _ => {}
            }
        }

        let subject_ident = match &subject.kind {
            ExprKind::Identifier { name } => Some(name.to_string()),
            _ => None,
        };

        return Some(MatchInfo {
            subject_id: subject.id,
            subject_ident,
            handled_cases: handled,
            insert_line: last_case_end_line,
            insert_col: last_case_end_col,
            indent_cols: range.start.column as usize,
        });
    }

    match &expr.kind {
        ExprKind::Binary { left, right, .. } => {
            check_expr_for_match(left, line).or_else(|| check_expr_for_match(right, line))
        }
        ExprKind::Unary { operand, .. } => check_expr_for_match(operand, line),
        ExprKind::Call { callee, args, .. } => {
            if let Some(info) = check_expr_for_match(callee, line) {
                return Some(info);
            }
            for arg in args {
                let arg_expr = match arg {
                    varn_core::ast::Arg::Positional(e)
                    | varn_core::ast::Arg::Spread(e)
                    | varn_core::ast::Arg::Named { value: e, .. } => e,
                };
                if let Some(info) = check_expr_for_match(arg_expr, line) {
                    return Some(info);
                }
            }
            None
        }
        _ => None,
    }
}

fn resolve_variants(
    state: &DocumentState,
    program: &Program,
    subject_ty: Option<&varn_checker::Type>,
    match_info: &MatchInfo,
) -> Option<(String, Vec<String>)> {
    if let Some(ty) = subject_ty {
        match &ty.0 {
            TypeKind::Intrinsic(varn_core::TypeTag::Bool) => {
                return Some(("".to_string(), vec!["true".to_string(), "false".to_string()]));
            }
            TypeKind::Named(name, _) => {
                // Check if enum declaration exists in AST for this name
                for stmt in &program.body {
                    if let StmtKind::Decl(decl) = &stmt.kind {
                        if let Decl::Enum(e) = decl.as_ref() {
                            if e.id.as_ref() == name.as_ref() {
                                let variants = e.members.iter().map(|m| m.id.to_string()).collect();
                                return Some((name.to_string(), variants));
                            }
                        }
                    }
                }
            }
            TypeKind::Union(types) => {
                let variants: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                return Some(("".to_string(), variants));
            }
            _ => {}
        }
    }

    // Search in AST for Enum declarations matching subject identifier
    if let Some(ident) = &match_info.subject_ident {
        for s in state.symbols() {
            if s.name() == *ident {
                let ty_str = s.ty().to_string();
                for stmt in &program.body {
                    if let StmtKind::Decl(decl) = &stmt.kind {
                        if let Decl::Enum(e) = decl.as_ref() {
                            if e.id.as_ref() == ty_str || e.id.as_ref() == ident {
                                let variants = e.members.iter().map(|m| m.id.to_string()).collect();
                                return Some((e.id.to_string(), variants));
                            }
                        }
                    }
                }
            }
        }
    }

    // Scan program declarations for any enum
    for stmt in &program.body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            if let Decl::Enum(e) = decl.as_ref() {
                let variants: Vec<String> = e.members.iter().map(|m| m.id.to_string()).collect();
                return Some((e.id.to_string(), variants));
            }
        }
    }

    None
}
