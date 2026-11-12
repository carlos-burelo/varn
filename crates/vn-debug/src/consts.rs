use vn_core::term::chalk::chalk;
use vn_core::term::terminal::Section;

pub fn debug_consts(filename: &str) {
    Section::new("constant folding")
        .subtitle(filename)
        .color(|c| c.yellow())
        .print();
    vn_core::term::terminal::log(format!(
        "  {}",
        chalk("(Constant evaluation trace not implemented)").dim()
    ));
    Section::new("constant folding").close();
}
