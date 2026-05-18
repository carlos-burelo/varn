use crate::colors::{C_TYPES, DIM, RESET};
use varn_core::ast::Program;

pub fn debug_expr(program: &Program, _range: Option<(u32, u32)>) {
    use crate::colors::{footer, header};
    header(C_TYPES, "expression mapping", &program.filename);

    eprintln!("  {DIM}(Not fully implemented in varn-debug migration yet){RESET}");

    footer(C_TYPES, "expression analysis complete");
}
