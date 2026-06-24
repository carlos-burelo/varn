use varn_core::ast::Program;
use varn_utilities::chalk::chalk;
use varn_utilities::terminal;
use varn_utilities::terminal::Section;

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
