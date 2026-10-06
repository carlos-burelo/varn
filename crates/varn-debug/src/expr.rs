

















use std::fmt::Write as _;

use varn_checker::CheckResult;
use varn_core::ast::Program;






pub(crate) fn line_col(source: &str, offset: u32) -> (u32, u32) {
    let offset = offset as usize;
    let mut line = 1u32;
    let mut col = 1u32;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}













pub fn render_check_types(program: &Program, source: &str, check: &CheckResult) -> String {
    let mut out = String::new();

    
    
    
    let name = program
        .filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&program.filename);
    let _ = writeln!(out, "# check:types {name}");

    let _ = writeln!(out, "## checker types (key = expr id)");
    let mut by_id: Vec<(&u32, &varn_checker::TypeEntry)> = check.expr_table.iter().collect();
    by_id.sort_by_key(|(id, _)| **id);
    for (id, entry) in by_id {
        let (line, col) = line_col(source, entry.start);
        let ty = entry.ty.display(&check.bind.ty_table, &check.bind.interner);
        let _ = writeln!(out, "{id} | {line}:{col} | {ty}");
    }

    out
}


pub use crate::phases::check_types::debug_check_types;
