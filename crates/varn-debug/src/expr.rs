use varn_core::ast::Program;
use varn_term::chalk::chalk;
use varn_term::terminal;
use varn_term::terminal::Section;

pub fn debug_expr(program: &Program, _range: Option<(u32, u32)>) {
    Section::new("expression mapping")
        .subtitle(&program.filename)
        .color(|c| c.blue())
        .print();

    terminal::log(format!(
        "  {}",
        chalk("(Not fully implemented in varn-debug migration yet)").dim()
    ));

    Section::new("expression mapping").close();
}
