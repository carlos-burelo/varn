use varn_term::chalk::chalk;
use varn_term::terminal::Section;

pub fn debug_consts(filename: &str) {
    Section::new("constant folding")
        .subtitle(filename)
        .color(|c| c.yellow())
        .print();
    varn_term::terminal::log(format!(
        "  {}",
        chalk("(Constant evaluation trace not implemented)").dim()
    ));
    Section::new("constant folding").close();
}
