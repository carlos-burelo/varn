use tower_lsp::lsp_types::{Position, Range, SelectionRange};
use varn_core::ast::{Arg, Expr, ExprKind, Program, Stmt, StmtKind};
use varn_core::source::SourceRange;

use crate::document::DocumentState;

pub fn build_selection_ranges(
    state: &DocumentState,
    positions: &[Position],
) -> Vec<SelectionRange> {
    positions
        .iter()
        .map(|pos| build_single_selection_range(state, *pos))
        .collect()
}

fn build_single_selection_range(state: &DocumentState, pos: Position) -> SelectionRange {
    let mut ranges = Vec::new();

    // 1. Innermost token range
    if let Some(tok) = state
        .tokens
        .iter()
        .find(|t| t.line == pos.line && t.col <= pos.character && pos.character <= t.col + t.length)
    {
        ranges.push(Range {
            start: Position {
                line: tok.line,
                character: tok.col,
            },
            end: Position {
                line: tok.line,
                character: tok.col + tok.length,
            },
        });
    }

    // 2. AST expression & statement hierarchy
    if let Some(program) = &state.ast {
        collect_enclosing_ranges(program, pos.line, pos.character, &mut ranges);
    }

    // 3. Whole file fallback
    if let Some(last_tok) = state.tokens.last() {
        let full_range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: last_tok.line + 1,
                character: 0,
            },
        };
        ranges.push(full_range);
    }

    // Deduplicate identical consecutive ranges
    ranges.dedup();

    // Fold ranges into nested SelectionRange
    let mut current: Option<SelectionRange> = None;
    for r in ranges.into_iter().rev() {
        current = Some(SelectionRange {
            range: r,
            parent: current.map(Box::new),
        });
    }

    current.unwrap_or(SelectionRange {
        range: Range {
            start: pos,
            end: pos,
        },
        parent: None,
    })
}

fn collect_enclosing_ranges(
    program: &Program,
    line: u32,
    col: u32,
    ranges: &mut Vec<Range>,
) {
    for stmt in &program.body {
        collect_in_stmt(stmt, line, col, ranges);
    }
}

fn collect_in_stmt(stmt: &Stmt, line: u32, col: u32, ranges: &mut Vec<Range>) {
    if !contains_pos(stmt.range(), line, col) {
        return;
    }

    match &stmt.kind {
        StmtKind::Expr { expression } => collect_in_expr(expression, line, col, ranges),
        StmtKind::Block { stmts } => {
            for s in stmts {
                collect_in_stmt(s, line, col, ranges);
            }
        }
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            collect_in_expr(test, line, col, ranges);
            collect_in_stmt(consequent, line, col, ranges);
            if let Some(alt) = alternate {
                collect_in_stmt(alt, line, col, ranges);
            }
        }
        StmtKind::Decl(decl) => match decl.as_ref() {
            varn_core::ast::Decl::Function(f) => {
                collect_in_stmt(&f.body, line, col, ranges);
            }
            varn_core::ast::Decl::Class(c) => {
                for member in &c.body {
                    if let varn_core::ast::ClassMember::Method { body: Some(b), .. } = member {
                        collect_in_stmt(b, line, col, ranges);
                    }
                }
            }
            _ => {}
        },
        StmtKind::Return { argument: Some(e) } => collect_in_expr(e, line, col, ranges),
        _ => {}
    }

    ranges.push(to_lsp_range(stmt.range()));
}

fn collect_in_expr(expr: &Expr, line: u32, col: u32, ranges: &mut Vec<Range>) {
    if !contains_pos(&expr.range, line, col) {
        return;
    }

    match &expr.kind {
        ExprKind::Binary { left, right, .. } => {
            collect_in_expr(left, line, col, ranges);
            collect_in_expr(right, line, col, ranges);
        }
        ExprKind::Unary { operand, .. } => collect_in_expr(operand, line, col, ranges),
        ExprKind::Call { callee, args, .. } => {
            collect_in_expr(callee, line, col, ranges);
            for arg in args {
                let arg_expr = match arg {
                    Arg::Positional(e) | Arg::Spread(e) | Arg::Named { value: e, .. } => e,
                };
                collect_in_expr(arg_expr, line, col, ranges);
            }
        }
        ExprKind::Pipeline { left, right } => {
            collect_in_expr(left, line, col, ranges);
            collect_in_expr(right, line, col, ranges);
        }
        ExprKind::Match { subject, cases } => {
            collect_in_expr(subject, line, col, ranges);
            for case in cases {
                if contains_pos(&case.range, line, col) {
                    ranges.push(to_lsp_range(&case.range));
                }
            }
        }
        _ => {}
    }

    ranges.push(to_lsp_range(&expr.range));
}

fn contains_pos(r: &SourceRange, line: u32, col: u32) -> bool {
    let s_line = r.start.line.saturating_sub(1);
    let e_line = r.end.line.saturating_sub(1);
    if line < s_line || line > e_line {
        return false;
    }
    if line == s_line && col < r.start.column {
        return false;
    }
    if line == e_line && col > r.end.column {
        return false;
    }
    true
}

fn to_lsp_range(r: &SourceRange) -> Range {
    Range {
        start: Position {
            line: r.start.line.saturating_sub(1),
            character: r.start.column,
        },
        end: Position {
            line: r.end.line.saturating_sub(1),
            character: r.end.column,
        },
    }
}
